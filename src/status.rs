//! État canonique d'une machine + de son service, tel que rapporté par le
//! script de collecte exécuté sur la cible (locale ou SSH).
//!
//! Le champ `state` décrit le SERVICE applicatif (unité systemd) et est
//! TOUJOURS ramené au vocabulaire commun : `running` | `stopped` |
//! `starting` | `stopping` | `failed` | `unknown`. Les métriques valent
//! `None` quand la machine ne les fournit pas (le dashboard masque alors
//! les lignes correspondantes).

use serde::{Deserialize, Serialize};

/// Adresse de connexion normalisée (`{ip, port}`).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Address {
    pub ip: String,
    pub port: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Status {
    /// Nom d'hôte de la machine (affiché comme titre du dashboard).
    #[serde(default)]
    pub name: Option<String>,
    /// État canonique du service (`running`, `stopped`…).
    #[serde(default)]
    pub state: String,
    /// État brut de systemd (`active`, `inactive`, `failed`…).
    #[serde(default)]
    pub raw_state: String,
    /// Uptime du service (depuis son dernier démarrage), en secondes.
    #[serde(default)]
    pub service_uptime_seconds: Option<f64>,
    /// Usage CPU instantané (mesuré sur ~0,5 s par la collecte), en %.
    #[serde(default)]
    pub cpu_pct: Option<f64>,
    /// Modèle du CPU (`Intel(R) Xeon(R)…`).
    #[serde(default)]
    pub cpu_model: Option<String>,
    /// Nombre de cœurs logiques.
    #[serde(default)]
    pub cpu_count: Option<f64>,
    /// Charge moyenne 1 minute.
    #[serde(default)]
    pub load1: Option<f64>,
    /// Charge moyenne 5 minutes.
    #[serde(default)]
    pub load5: Option<f64>,
    /// Charge moyenne 15 minutes.
    #[serde(default)]
    pub load15: Option<f64>,
    /// RAM utilisée (MB).
    #[serde(default)]
    pub ram_mb: Option<f64>,
    /// RAM totale (MB).
    #[serde(default)]
    pub ram_max_mb: Option<f64>,
    /// Disque racine utilisé (MB).
    #[serde(default)]
    pub disk_mb: Option<f64>,
    /// Disque racine total (MB).
    #[serde(default)]
    pub disk_max_mb: Option<f64>,
    /// Uptime SYSTÈME (la machine), en secondes.
    #[serde(default)]
    pub uptime_seconds: Option<f64>,
    /// Nom « joli » de l'OS (`Ubuntu 24.04.1 LTS`).
    #[serde(default)]
    pub os: Option<String>,
    /// Version du noyau.
    #[serde(default)]
    pub kernel: Option<String>,
    /// Adresse de la machine (hôte SSH, ou « local »).
    #[serde(default)]
    pub address: Option<Address>,
    /// Type de connexion : `local` | `ssh`.
    #[serde(default)]
    pub backend: Option<String>,
}

/// Nom d'affichage du service : le nom d'hôte rapporté s'il existe,
/// sinon l'alias (pseudonyme configurable).
pub fn display_name(status: &Status, alias: &str) -> String {
    match status.name.as_deref() {
        Some(n) if !n.trim().is_empty() => n.trim().to_string(),
        _ => alias.to_string(),
    }
}

/// Table d'états systemd → vocabulaire canonique.
pub fn map_state(raw_state: &str) -> String {
    match raw_state.trim().to_lowercase().as_str() {
        "active" => "running".to_string(),
        "reloading" => "running".to_string(),
        "inactive" => "stopped".to_string(),
        "dead" => "stopped".to_string(),
        "failed" => "failed".to_string(),
        "activating" => "starting".to_string(),
        "deactivating" => "stopping".to_string(),
        "nosystemd" | "unknown" | "" => "unknown".to_string(),
        other => other.to_string(),
    }
}
