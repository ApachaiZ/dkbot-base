//! Backends de connexion aux machines cibles.
//!
//! Un [`Backend`] est construit par cible : [`BackendKind::Local`] exécute
//! les commandes sur la machine du bot, [`BackendKind::Ssh`] les exécute à
//! distance via une session SSH (russh, pur Rust, asynchrone). Les deux
//! backends passent par le MÊME script de collecte ([`script`]) : le statut
//! rapporté est identique quel que soit le chemin.

pub mod local;
pub mod script;
pub mod ssh;

use std::time::Duration;

use crate::config::Config;
use crate::errors::{BackendError, ExitError};
use crate::status::{Address, Status};
use crate::targets::{TargetKind, TargetSpec};

#[derive(Debug, Clone)]
pub enum BackendKind {
    Local,
    Ssh(ssh::SshConn),
}

/// Issue d'une copie de sauvegarde sur la machine cible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyOutcome {
    /// Copie terminée alors que le service était (resté) arrêté.
    Copied { dest: String },
    /// Le service était actif : aucune copie (garde « arrêt avéré »).
    Running,
    /// Une autre copie est en cours (lock de sauvegarde détenu).
    Locked,
    /// Une copie réussie date de moins d'une heure : non répétée.
    Fresh,
}

/// Backend prêt à l'emploi : type de connexion + réglages communs.
#[derive(Debug, Clone)]
pub struct Backend {
    pub kind: BackendKind,
    /// Préfixe systemctl (`sudo`, `sudo -n`…), validé à la config.
    prefix: String,
    /// Timeout de chaque commande.
    timeout: Duration,
}

impl Backend {
    /// Construit le backend d'une cible à partir de la configuration.
    pub fn build(cfg: &Config, target: &TargetSpec) -> Result<Self, ExitError> {
        let kind = match &target.kind {
            TargetKind::Local => BackendKind::Local,
            TargetKind::Ssh { host, port, user } => {
                let user = user.clone().unwrap_or_else(|| cfg.ssh_user.clone());
                let key_path = cfg
                    .ssh_key_path
                    .clone()
                    .unwrap_or_else(|| crate::config::effective_key_path(cfg));
                BackendKind::Ssh(ssh::SshConn::new(
                    host.clone(),
                    *port,
                    user,
                    cfg.ssh_password.clone(),
                    Some(key_path),
                    std::path::PathBuf::from(&cfg.ssh_known_hosts),
                    cfg.ssh_timeout,
                ))
            }
        };
        Ok(Self {
            kind,
            prefix: cfg.systemctl_prefix.clone(),
            timeout: cfg.ssh_timeout,
        })
    }

    /// Type de connexion : `local` | `ssh`.
    pub fn label(&self) -> &'static str {
        match &self.kind {
            BackendKind::Local => "local",
            BackendKind::Ssh(_) => "ssh",
        }
    }

    /// Adresse d'affichage de la machine.
    pub fn address(&self) -> Address {
        match &self.kind {
            BackendKind::Local => Address {
                ip: "local".to_string(),
                port: None,
            },
            BackendKind::Ssh(conn) => Address {
                ip: conn.host.clone(),
                port: Some(conn.port as u32),
            },
        }
    }

    /// Collecte le statut réel de la machine + du service.
    pub async fn collect(&self, service: &str) -> Result<Status, BackendError> {
        let mut status = match &self.kind {
            BackendKind::Local => local::collect_local(service, self.timeout).await?,
            BackendKind::Ssh(conn) => conn.collect(service).await?,
        };
        status.address = Some(self.address());
        status.backend = Some(self.label().to_string());
        Ok(status)
    }

    /// Action power applicative : `start` | `stop` | `restart` d'une unité.
    pub async fn systemctl(&self, action: &str, unit: &str) -> Result<(), BackendError> {
        match &self.kind {
            BackendKind::Local => {
                local::systemctl_local(&self.prefix, action, Some(unit), self.timeout).await
            }
            BackendKind::Ssh(conn) => conn.systemctl(&self.prefix, action, unit).await,
        }
    }

    /// Redémarrage de la MACHINE (commande unique, sans retry).
    pub async fn reboot(&self) -> Result<(), BackendError> {
        match &self.kind {
            BackendKind::Local => {
                local::systemctl_local(&self.prefix, "reboot", None, self.timeout).await
            }
            BackendKind::Ssh(conn) => conn.reboot(&self.prefix).await,
        }
    }

    /// Lignes de log du service (journald ou fichier).
    pub async fn logs(
        &self,
        unit: &str,
        lines: u32,
        file: Option<&str>,
    ) -> Result<String, BackendError> {
        match &self.kind {
            BackendKind::Local => {
                local::logs_local(&self.prefix, unit, lines, file, self.timeout).await
            }
            BackendKind::Ssh(conn) => conn.logs(&self.prefix, unit, lines, file).await,
        }
    }

    /// Latence aller-retour (exécution de `true`).
    pub async fn ping(&self) -> Result<(), BackendError> {
        match &self.kind {
            BackendKind::Local => local::ping_local(self.timeout).await,
            BackendKind::Ssh(conn) => conn.ping().await,
        }
    }

    /// Copie des sauvegardes du jeu. Le script embarque la garde « arrêt
    /// avéré », un lock anti-concurrence et la règle « pas de recopie dans
    /// l'heure ». `timeout` borne la copie — l'appelant le dérive du
    /// budget restant du flux power.
    pub async fn copy_saves(
        &self,
        unit: &str,
        saves: &str,
        timeout: Duration,
    ) -> Result<CopyOutcome, BackendError> {
        match &self.kind {
            BackendKind::Local => local::copy_saves_local(unit, saves, timeout).await,
            BackendKind::Ssh(conn) => conn.copy_saves(unit, saves, timeout).await,
        }
    }

    /// Liste les archives de sauvegarde (module `backups`).
    pub async fn backups_list(&self, unit: &str, saves: &str) -> Result<String, BackendError> {
        match &self.kind {
            BackendKind::Local => local::backups_list_local(unit, saves, self.timeout).await,
            BackendKind::Ssh(conn) => conn.backups_list(unit, saves, self.timeout).await,
        }
    }

    /// Restaure une archive de sauvegarde (module `backups`).
    pub async fn backups_restore(
        &self,
        unit: &str,
        saves: &str,
        archive: &str,
    ) -> Result<String, BackendError> {
        match &self.kind {
            BackendKind::Local => {
                local::backups_restore_local(unit, saves, archive, self.timeout).await
            }
            BackendKind::Ssh(conn) => {
                conn.backups_restore(unit, saves, archive, self.timeout)
                    .await
            }
        }
    }

    /// Diagnostic en lecture seule (module `exec`) — arguments validés
    /// côté Rust avant l'appel.
    pub async fn exec_diagnostic(&self, args: &[String]) -> Result<String, BackendError> {
        match &self.kind {
            BackendKind::Local => local::exec_diagnostic_local(args, self.timeout).await,
            BackendKind::Ssh(conn) => conn.exec_diagnostic(args, self.timeout).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::targets::parse_spec;

    #[test]
    fn local_backend_labels_and_address() {
        let target = TargetSpec {
            alias: "main".into(),
            kind: parse_spec("local").unwrap(),
        };
        // Backend::build lit la config : on teste seulement la structure.
        let backend = Backend {
            kind: BackendKind::Local,
            prefix: String::new(),
            timeout: Duration::from_secs(20),
        };
        assert_eq!(backend.label(), "local");
        assert_eq!(backend.address().ip, "local");
        let _ = target;
    }
}
