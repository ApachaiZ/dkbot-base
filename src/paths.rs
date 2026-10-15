//! Chemins conventionnels : tout l'état vit dans `.config.d/` (config,
//! secrets, verrous, sessions, roster, hash des commandes) et les logs
//! dans `logs/` — les deux RELATIFS au dossier de données dans lequel le
//! processus se `chdir` au démarrage : `~/.config/dakkabot/` par défaut
//! (installation cargo standard), le dossier passé à `--root <dir>`, ou
//! la racine du registre `install.json` à côté du binaire.

use std::path::PathBuf;

/// Répertoire de configuration (créé à la demande par les écrivains).
pub fn config_dir() -> PathBuf {
    PathBuf::from(".config.d")
}

/// Fichier d'environnement (`.config.d/.env`).
pub fn env_file() -> PathBuf {
    config_dir().join(".env")
}

/// Fichier des opérateurs (chiffré).
pub fn users_file() -> PathBuf {
    config_dir().join("users.json")
}

/// Fichier des sessions par utilisateur (chiffré) : cible + service.
pub fn sessions_file() -> PathBuf {
    config_dir().join("sessions.json")
}

/// Roster des membres de la guilde (chiffré) — autocomplétion /users.
pub fn members_roster_file() -> PathBuf {
    config_dir().join("members.roster.json")
}

/// Jeton de date du dernier chargement du roster (en clair, sans secret).
pub fn members_state_file() -> PathBuf {
    config_dir().join("members.state.json")
}

/// Verrous persistants des actions power (en clair, format JSON).
pub fn active_lock_file() -> PathBuf {
    config_dir().join("active.lock")
}

/// États des MODULES (en clair, pas de secret).
pub fn modules_file() -> PathBuf {
    config_dir().join("modules.json")
}

/// Entrées du planificateur (en clair, pas de secret).
pub fn schedule_file() -> PathBuf {
    config_dir().join("schedule.json")
}

/// Sortie métriques Prometheus (format textfile node_exporter).
pub fn metrics_file() -> PathBuf {
    config_dir().join("metrics.prom")
}

/// Hash des slash commands enregistrées (guilde + application + commandes).
pub fn commands_hash_file() -> PathBuf {
    config_dir().join("commands.hash")
}

/// Statistiques de durée des actions (moyennes persistées).
pub fn stats_file() -> PathBuf {
    config_dir().join("stats.json")
}

/// Fichier de log partagé du bot.
pub fn log_file() -> PathBuf {
    PathBuf::from("logs").join("dakkabot.log")
}

/// Dossier des GIFs d'emojis sur disque (personnalisables sans recompiler).
pub fn gif_dir(kind: &str) -> PathBuf {
    PathBuf::from("assets").join("emojis").join(kind)
}
