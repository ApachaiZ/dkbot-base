//! Chargement et validation de la configuration (`.config.d/.env` >
//! environnement).
//!
//! Le bot lit `.config.d/.env` en priorité, puis les variables
//! d'environnement système si une valeur est absente (déploiement
//! systemd/Docker). Toute valeur manquante ou placeholder (`YOUR_…`) est
//! une erreur de CONFIGURATION → sortie avec le code 78 (EX_CONFIG) :
//! systemd et tout superviseur ne redémarrent PAS en boucle sur une config
//! invalide.

use std::sync::OnceLock;
use std::time::Duration;

use crate::errors::ExitError;
use crate::i18n::{fill, Catalog};
use crate::paths;
use crate::targets::{is_unset, parse_services, parse_targets, TargetSpec};

/// Lecture d'une variable d'environnement (le `.env` est déjà chargé par
/// dotenvy au démarrage).
fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().to_string())
}

/// Charge `.config.d/.env` UNE seule fois (dotenvy n'écrase jamais les
/// vraies variables d'environnement).
pub fn load_dotenv() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = dotenvy::from_path(paths::env_file());
    });
}

/// Mode de lecture des logs du service.
#[derive(Debug, Clone)]
pub enum LogMode {
    /// `journalctl -u <unité>` (défaut).
    Journald,
    /// `tail -n <n>` d'un fichier de log (`LOG_FILE`).
    File(String),
}

/// La configuration figée du bot (immuable une fois construite).
#[derive(Clone)]
pub struct Config {
    pub token: String,
    pub client_id: String,
    pub owner_id: String,
    /// Guilde PRINCIPALE (compat, première de `guild_ids`).
    pub guild_id: String,
    /// Guildes autorisées (DISCORD_GUILD_IDS, repli : DISCORD_GUILD_ID).
    pub guild_ids: Vec<String>,
    pub alert_channel_id: Option<String>,
    /// Canal d'AUDIT des événements (actions confirmées, planificateur,
    /// sauvegardes, reboot). Repli : le canal du watchdog.
    pub audit_channel_id: Option<String>,
    pub alert_interval_ms: Option<u64>,
    /// Cibles `(alias, spec)` dans l'ordre de déclaration — la première est
    /// la cible par défaut.
    pub targets: Vec<TargetSpec>,
    /// Services `(alias, unité systemd)` — le premier est le défaut.
    pub services: Vec<(String, String)>,
    pub language: &'static str,
    /// Utilisateur SSH par défaut (cibles sans `user@`).
    pub ssh_user: String,
    pub ssh_password: Option<String>,
    pub ssh_key_path: Option<String>,
    pub ssh_known_hosts: String,
    pub ssh_timeout: Duration,
    /// Préfixe de `systemctl` si root est requis (`sudo`, `sudo -n`…) —
    /// appliqué LOCALEMENT et À DISTANCE, validé strictement.
    pub systemctl_prefix: String,
    pub log_mode: LogMode,
    pub log_lines: u32,
    /// Chemin ABSOLU des sauvegardes du jeu sur la machine cible
    /// (`SAVES_PATH`). `None` = pas de copie de sauvegarde.
    pub saves_path: Option<String>,
    /// Délai maximal d'une copie de sauvegarde (les saves peuvent être
    /// volumineux — indépendant du timeout SSH des commandes ordinaires).
    pub saves_copy_timeout: Duration,
    pub watchdog_cpu_pct: f64,
    pub watchdog_ram_pct: f64,
    pub watchdog_disk_pct: f64,
}

impl Config {
    /// Catalogue i18n associé à la langue de la config — pratique pour les
    /// tâches détachées (watchdog) qui ne reçoivent pas le catalogue.
    pub fn language_catalog(&self) -> &'static Catalog {
        crate::i18n::catalog(self.language)
    }
}

/// Préfixe systemctl sûr : lettres, chiffres, espaces, `-`, `_`, `/`, `.`,
/// `=`, `:`. Tout autre caractère (métacaractères shell) est refusé.
fn valid_prefix(prefix: &str) -> bool {
    prefix
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '/' | '.' | '=' | ':'))
}

/// Chemin de clé SSH par défaut : `~/.ssh/id_ed25519` sinon `~/.ssh/id_rsa`.
fn default_key_path() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    for candidate in ["id_ed25519", "id_rsa"] {
        let path = format!("{home}/.ssh/{candidate}");
        if std::path::Path::new(&path).is_file() {
            return path;
        }
    }
    format!("{home}/.ssh/id_ed25519")
}

fn parse_u64(value: Option<&str>, default: u64, min: u64) -> u64 {
    value
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v >= min)
        .unwrap_or(default)
}

/// Chemin de sauvegarde acceptable : absolu, sans caractère de contrôle
/// (pas de newline/NUL — le chemin passe en ARGUMENT du shell, jamais
/// interpolé dans une ligne de commande).
pub fn valid_saves_path(path: &str) -> bool {
    path.starts_with('/') && !path.chars().any(|c| c.is_control())
}

/// Charge la configuration. Le provider n'existe plus : les backends
/// (local/SSH) sont construits par la couche API à partir des cibles.
pub fn load(catalog: &Catalog, language: &'static str) -> Result<Config, ExitError> {
    load_dotenv();

    // ── Validation bloquante (messages localisés, exit 78) ──
    for (value, key) in [
        (env("DISCORD_TOKEN"), "DISCORD_TOKEN"),
        (env("DISCORD_CLIENT_ID"), "DISCORD_CLIENT_ID"),
        (env("DISCORD_OWNER_ID"), "DISCORD_OWNER_ID"),
        (env("DISCORD_GUILD_ID"), "DISCORD_GUILD_ID"),
    ] {
        if is_unset(value.as_deref()) {
            return Err(ExitError::config(fill(
                catalog.errors.missing_config,
                &[("key", key)],
            )));
        }
    }

    if parse_guild_ids(
        env("DISCORD_GUILD_IDS").as_deref(),
        env("DISCORD_GUILD_ID").as_deref(),
    )
    .is_empty()
    {
        return Err(ExitError::config(crate::i18n::fill(
            catalog.errors.missing_config,
            &[("key", "DISCORD_GUILD_IDS")],
        )));
    }

    let targets = parse_targets(env("TARGETS").as_deref(), env("TARGET_HOST").as_deref());
    let services = parse_services(env("SERVICES").as_deref(), env("SERVICE").as_deref());
    if services.is_empty() {
        return Err(ExitError::config(catalog.errors.invalid_service));
    }

    let prefix = env("SYSTEMCTL_PREFIX").unwrap_or_default();
    if !valid_prefix(&prefix) {
        return Err(ExitError::config(format!(
            "SYSTEMCTL_PREFIX contains forbidden characters (shell metacharacters): \"{prefix}\""
        )));
    }

    let log_mode = match env("LOG_FILE") {
        Some(file) => LogMode::File(file),
        None => LogMode::Journald,
    };

    let saves_path = match env("SAVES_PATH") {
        Some(p) if !valid_saves_path(&p) => {
            return Err(ExitError::config(format!(
                "SAVES_PATH must be an absolute path without control characters: \"{p}\""
            )))
        }
        p => p,
    };

    let ssh_user = env("SSH_USER")
        .or_else(|| std::env::var("USER").ok().filter(|v| !v.is_empty()))
        .unwrap_or_else(|| "root".to_string());

    let cfg = Config {
        token: env("DISCORD_TOKEN").unwrap_or_default(),
        client_id: env("DISCORD_CLIENT_ID").unwrap_or_default(),
        owner_id: env("DISCORD_OWNER_ID").unwrap_or_default(),
        guild_id: env("DISCORD_GUILD_ID").unwrap_or_default(),
        guild_ids: parse_guild_ids(
            env("DISCORD_GUILD_IDS").as_deref(),
            env("DISCORD_GUILD_ID").as_deref(),
        ),
        alert_channel_id: env("DISCORD_ALERT_CHANNEL_ID"),
        audit_channel_id: env("DISCORD_AUDIT_CHANNEL_ID").or(env("DISCORD_ALERT_CHANNEL_ID")),
        alert_interval_ms: env("ALERT_INTERVAL_MS").and_then(|v| v.parse::<u64>().ok()),
        targets,
        services,
        language,
        ssh_user,
        ssh_password: env("SSH_PASSWORD"),
        ssh_key_path: env("SSH_KEY_PATH"),
        ssh_known_hosts: env("SSH_KNOWN_HOSTS_FILE").unwrap_or_else(|| {
            format!(
                "{}/.ssh/known_hosts",
                std::env::var("HOME").unwrap_or_default()
            )
        }),
        ssh_timeout: Duration::from_secs(parse_u64(env("SSH_TIMEOUT_SECS").as_deref(), 20, 1)),
        systemctl_prefix: prefix,
        log_mode,
        log_lines: parse_u64(env("LOG_LINES").as_deref(), 50, 1) as u32,
        saves_path,
        saves_copy_timeout: Duration::from_secs(parse_u64(
            env("SAVES_COPY_TIMEOUT_SECS").as_deref(),
            600,
            10,
        )),
        watchdog_cpu_pct: env("WATCHDOG_CPU_PCT")
            .and_then(|v| v.parse().ok())
            .unwrap_or(90.0),
        watchdog_ram_pct: env("WATCHDOG_RAM_PCT")
            .and_then(|v| v.parse().ok())
            .unwrap_or(90.0),
        watchdog_disk_pct: env("WATCHDOG_DISK_PCT")
            .and_then(|v| v.parse().ok())
            .unwrap_or(90.0),
    };
    Ok(cfg)
}

/// Guildes configurées : `DISCORD_GUILD_IDS` (liste à virgules), repli sur
/// `DISCORD_GUILD_ID`. Entrées vides ignorées, doublons retirés.
pub fn parse_guild_ids(list: Option<&str>, single: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = list
        .map(|s| {
            s.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if out.is_empty() {
        if let Some(one) = single {
            if !one.trim().is_empty() {
                out.push(one.trim().to_string());
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|x| seen.insert(x.clone()));
    out
}

pub fn effective_key_path(cfg: &Config) -> String {
    match &cfg.ssh_key_path {
        Some(p) => p.clone(),
        None => default_key_path(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guild_ids_parse_list_fallback_and_dedup() {
        assert_eq!(
            parse_guild_ids(Some(" 1 , 2,1, ,3 "), None),
            vec!["1".to_string(), "2".to_string(), "3".to_string()]
        );
        assert_eq!(parse_guild_ids(None, Some("42")), vec!["42".to_string()]);
        assert_eq!(parse_guild_ids(None, Some(" ")), Vec::<String>::new());
    }

    #[test]
    fn saves_path_must_be_absolute_and_clean() {
        assert!(valid_saves_path("/opt/game/saves"));
        assert!(valid_saves_path("/opt/game/my saves")); // espaces acceptés
        assert!(!valid_saves_path("relative/saves"));
        assert!(!valid_saves_path(""));
        assert!(!valid_saves_path("/opt/game\nsaves"));
        assert!(!valid_saves_path("/opt/game/saves\x00"));
    }
}
