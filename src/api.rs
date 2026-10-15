//! Couche API : cache 5 s + déduplication des requêtes en vol.
//!
//! Clé de cache : `cible/service` (les deux alias). Le cache partagé évite
//! un aller-retour machine redondant quand le watchdog vient de scanner,
//! et la cellule `OnceCell` partagée garantit qu'une seule collecte part
//! par clé (tous les concurrents attendent la même réponse).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, OnceCell};

use crate::backends::{Backend, CopyOutcome};
use crate::config::{Config, LogMode};
use crate::errors::{BackendError, ExitError};
use crate::status::Status;

/// Durée de validité du cache (5 s, comme le rythme maximal du watchdog).
pub const CACHE_TTL: Duration = Duration::from_secs(5);

/// Issue d'une tentative de copie de sauvegarde, vue de la couche API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupOutcome {
    /// `SAVES_PATH` absent de la configuration : pas de copie.
    NotConfigured,
    /// Copie terminée, service resté arrêté.
    Copied { dest: String },
    /// Le service était actif : aucune copie (garde « arrêt avéré »).
    SkippedRunning,
    /// Une autre copie est en cours (lock de sauvegarde détenu).
    SkippedLocked,
    /// Une copie réussie date de moins d'une heure : non répétée.
    SkippedFresh,
    /// La copie a échoué (répertoire absent, disque plein, timeout…).
    Failed(String),
}

type SharedFetch = Arc<OnceCell<Result<Status, BackendError>>>;

/// Clé `cible/service` (les deux alias).
pub fn key(target: &str, service: &str) -> String {
    format!("{target}\u{0}{service}")
}

pub struct Api {
    /// Config figée (log_lines, log_mode…).
    cfg: Config,
    /// Backend construit par cible.
    backends: HashMap<String, Backend>,
    /// Cache `clé → (horodatage, statut)`.
    cache: Mutex<HashMap<String, (Instant, Status)>>,
    /// Requêtes en vol `clé → cellule partagée`.
    inflight: Mutex<HashMap<String, SharedFetch>>,
}

impl Api {
    pub fn new(cfg: &Config) -> Result<Self, ExitError> {
        let mut backends = HashMap::new();
        for target in &cfg.targets {
            backends.insert(target.alias.clone(), Backend::build(cfg, target)?);
        }
        Ok(Self {
            cfg: cfg.clone(),
            backends,
            cache: Mutex::new(HashMap::new()),
            inflight: Mutex::new(HashMap::new()),
        })
    }

    /// Backend d'une cible (l'alias a été validé par `pick_target`).
    fn backend(&self, target: &str) -> &Backend {
        self.backends
            .get(target)
            .expect("backend construit pour chaque cible de la config")
    }

    /// Statut en cache pur (sans réseau) — pour nommer une cible en repli.
    #[allow(dead_code)]
    pub async fn get_cached(&self, target: &str, service: &str) -> Option<Status> {
        let cache = self.cache.lock().await;
        cache.get(&key(target, service)).map(|(_, s)| s.clone())
    }

    /// Vide le cache d'une clé (après une commande power) ou tout le cache.
    pub async fn clear_cache(&self, target: Option<&str>, service: Option<&str>) {
        let mut cache = self.cache.lock().await;
        match (target, service) {
            (Some(t), Some(s)) => {
                cache.remove(&key(t, s));
            }
            _ => cache.clear(),
        }
    }

    /// Statut frais ou en cache (`fresh = true` force la collecte).
    pub async fn get(
        &self,
        target: &str,
        service: &str,
        fresh: bool,
    ) -> Result<Status, BackendError> {
        let k = key(target, service);
        // Une collecte est-elle déjà en vol pour cette clé ? Tous les
        // concurrents — même `fresh` — partagent la même réponse.
        let inflight = self.inflight.lock().await.get(&k).cloned();
        if let Some(shared) = inflight {
            return shared
                .get_or_init(|| self.fetch_normalized(target, service))
                .await
                .clone();
        }
        if !fresh {
            if let Some((at, status)) = self.cache.lock().await.get(&k).cloned() {
                if at.elapsed() < CACHE_TTL {
                    return Ok(status);
                }
            }
        }
        let shared = Arc::new(OnceCell::new());
        self.inflight.lock().await.insert(k.clone(), shared.clone());
        let result = shared
            .get_or_init(|| self.fetch_normalized(target, service))
            .await
            .clone();
        self.inflight.lock().await.remove(&k);
        result
    }

    async fn fetch_normalized(&self, target: &str, service: &str) -> Result<Status, BackendError> {
        let unit = self
            .cfg
            .services
            .iter()
            .find(|(a, _)| a == service)
            .map(|(_, u)| u.as_str())
            .ok_or_else(|| BackendError::other(format!("unknown service \"{service}\"")))?;
        let status = self.backend(target).collect(unit).await?;
        self.cache
            .lock()
            .await
            .insert(key(target, service), (Instant::now(), status.clone()));
        Ok(status)
    }

    /// Action power applicative : UNE commande, jamais de retry interne.
    pub async fn send_power(
        &self,
        target: &str,
        service: &str,
        action: &str,
    ) -> Result<(), BackendError> {
        let unit = self
            .cfg
            .services
            .iter()
            .find(|(a, _)| a == service)
            .map(|(_, u)| u.as_str())
            .ok_or_else(|| BackendError::other(format!("unknown service \"{service}\"")))?;
        self.backend(target).systemctl(action, unit).await
    }

    /// Redémarrage de la MACHINE : une commande, jamais de retry.
    pub async fn reboot(&self, target: &str) -> Result<(), BackendError> {
        self.backend(target).reboot().await
    }

    /// Copie des sauvegardes du jeu sur la machine cible. L'appelant doit
    /// avoir établi que le service est arrêté : le script de copie garde
    /// cette garantie lui-même (aucune copie si l'unité est active).
    ///
    /// La copie est BORNÉE au budget restant du flux power (`deadline`
    /// moins une marge de finalisation) : elle ne peut jamais faire
    /// déborder le suivi (un restart réussi annoncé « non confirmé »
    /// serait un mensonge).
    ///
    /// Ne renvoie JAMAIS d'erreur : l'échec éventuel est une variante de
    /// [`BackupOutcome`] (l'action power, elle, doit continuer).
    pub async fn backup_saves(
        &self,
        target: &str,
        service: &str,
        deadline: Instant,
    ) -> BackupOutcome {
        let Some(saves) = self.cfg.saves_path.clone() else {
            return BackupOutcome::NotConfigured;
        };
        let Some(unit) = self
            .cfg
            .services
            .iter()
            .find(|(a, _)| a == service)
            .map(|(_, u)| u.as_str())
        else {
            return BackupOutcome::Failed(format!("unknown service \"{service}\""));
        };
        // Marge réservée à la finalisation du flux (carte finale, suivi
        // restant) après la copie.
        const MARGIN: Duration = Duration::from_secs(60);
        // Sous ce seuil, une copie ne peut pas aboutir : échec propre et
        // immédiat plutôt qu'un essai condamné.
        const MIN_TIMEOUT: Duration = Duration::from_secs(10);
        let budget = deadline.saturating_duration_since(Instant::now());
        let timeout = self
            .cfg
            .saves_copy_timeout
            .min(budget.saturating_sub(MARGIN));
        if timeout < MIN_TIMEOUT {
            return BackupOutcome::Failed("insufficient time budget for backup".to_string());
        }
        match self.backend(target).copy_saves(unit, &saves, timeout).await {
            Ok(CopyOutcome::Copied { dest }) => BackupOutcome::Copied { dest },
            Ok(CopyOutcome::Running) => BackupOutcome::SkippedRunning,
            Ok(CopyOutcome::Locked) => BackupOutcome::SkippedLocked,
            Ok(CopyOutcome::Fresh) => BackupOutcome::SkippedFresh,
            Err(e) => BackupOutcome::Failed(e.message),
        }
    }

    /// Latence aller-retour vers la machine.
    pub async fn ping(&self, target: &str) -> Result<(), BackendError> {
        self.backend(target).ping().await
    }

    /// Liste des archives de sauvegarde d'un service (module `backups`).
    pub async fn backups_list(&self, target: &str, service: &str) -> Result<String, BackendError> {
        let saves = self
            .cfg
            .saves_path
            .clone()
            .ok_or_else(|| BackendError::other("SAVES_PATH not configured"))?;
        let unit = self
            .cfg
            .services
            .iter()
            .find(|(a, _)| a == service)
            .map(|(_, u)| u.as_str())
            .ok_or_else(|| BackendError::other(format!("unknown service \"{service}\"")))?;
        self.backend(target).backups_list(unit, &saves).await
    }

    /// Restaure une archive de sauvegarde (module `backups`).
    pub async fn backups_restore(
        &self,
        target: &str,
        service: &str,
        archive: &str,
    ) -> Result<String, BackendError> {
        let saves = self
            .cfg
            .saves_path
            .clone()
            .ok_or_else(|| BackendError::other("SAVES_PATH not configured"))?;
        let unit = self
            .cfg
            .services
            .iter()
            .find(|(a, _)| a == service)
            .map(|(_, u)| u.as_str())
            .ok_or_else(|| BackendError::other(format!("unknown service \"{service}\"")))?;
        self.backend(target)
            .backups_restore(unit, &saves, archive)
            .await
    }

    /// Diagnostic en lecture seule (module `exec`) — `args` validés côté
    /// commande (builtin ∈ liste blanche, unité et lignes bornées).
    pub async fn exec_diagnostic(
        &self,
        target: &str,
        args: &[String],
    ) -> Result<String, BackendError> {
        self.backend(target).exec_diagnostic(args).await
    }

    /// Lignes de log du service (journald par défaut, fichier si `LOG_FILE`).
    pub async fn logs(&self, target: &str, service: &str) -> Result<String, BackendError> {
        let unit = self
            .cfg
            .services
            .iter()
            .find(|(a, _)| a == service)
            .map(|(_, u)| u.as_str())
            .ok_or_else(|| BackendError::other(format!("unknown service \"{service}\"")))?;
        let file = match &self.cfg.log_mode {
            LogMode::File(path) => Some(path.as_str()),
            LogMode::Journald => None,
        };
        self.backend(target)
            .logs(unit, self.cfg.log_lines, file)
            .await
    }
}
