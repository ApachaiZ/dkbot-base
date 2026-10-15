//! Construction des embeds Discord (cartes + dashboard modulaire).
//!
//! Le dashboard est MODULAIRE : chaque ligne (CPU, RAM, disque, charge,
//! uptime, OS…) n'apparaît que si la machine la renseigne. Les « puces »
//! (charge, uptimes) et les infos (OS, noyau, adresse, connexion) se
//! placent côte à côte quand elles vont par paire, sinon pleine largeur.

use poise::serenity_prelude as serenity;
use serenity::{CreateEmbed, CreateEmbedFooter, Timestamp};

use crate::i18n::{fill, Catalog};
use crate::status::{display_name, Status};

/// Ton de couleur d'une carte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Good,
    Bad,
    Info,
    Wait,
}

impl Tone {
    pub fn color(self) -> u32 {
        match self {
            Tone::Good => 0x2ecc71,
            Tone::Bad => 0xe74c3c,
            Tone::Info => 0x5865f2,
            Tone::Wait => 0xf1c40f,
        }
    }
}

/// Carte standard : titre + corps + couleur + pied de page + horodatage.
pub fn card(title: &str, text: &str, tone: Tone, footer: Option<&str>) -> CreateEmbed {
    CreateEmbed::new()
        .color(tone.color())
        .title(title)
        .description(text)
        .footer(CreateEmbedFooter::new(
            footer.unwrap_or("DAKKABOT • VPS CONTROL"),
        ))
        .timestamp(Timestamp::now())
}

/// État d'un service, en minuscules.
pub fn state(status: &Status) -> String {
    status.state.trim().to_lowercase()
}

pub fn online(status: &Status) -> bool {
    matches!(state(status).as_str(), "running" | "online")
}

pub fn offline(status: &Status) -> bool {
    matches!(state(status).as_str(), "offline" | "stopped")
}

/// Formatage d'un nombre façon `toLocaleString("en-US")` : `2048` → `2,048`.
pub fn fmt_num(n: f64) -> String {
    let rounded = (n * 100.0).round() / 100.0;
    let (int_part, frac_part) = if rounded.fract().abs() < 1e-9 {
        (format!("{rounded:.0}"), String::new())
    } else {
        let s = format!("{rounded:.2}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        match s.split_once('.') {
            Some((int, frac)) => (int.to_string(), format!(".{frac}")),
            None => (s.to_string(), String::new()),
        }
    };
    let mut grouped = String::new();
    for (i, c) in int_part.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{}{}", grouped.chars().rev().collect::<String>(), frac_part)
}

/// Durée lisible : `3h 24m` / `1d 2h`. Unités volontairement neutres (d/h/m).
pub fn fmt_duration(total_seconds: f64) -> String {
    let s = total_seconds.max(0.0).floor() as u64;
    let d = s / 86400;
    let h = s % 86400 / 3600;
    let m = s % 3600 / 60;
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

/// Barre visuelle sur 10 segments : `▰▰▰▰▱▱▱▱▱▱ 42.0%`.
pub fn bar(used: Option<f64>, max: Option<f64>) -> Option<String> {
    let (used, max) = (used?, max?);
    if max <= 0.0 {
        return None;
    }
    let pct = (used / max * 100.0).clamp(0.0, 100.0);
    let filled = (pct / 10.0).round() as usize;
    Some(format!(
        "{}{} {:.1}%",
        "▰".repeat(filled),
        "▱".repeat(10 - filled),
        pct
    ))
}

/// Dashboard `/status` modulaire (voir doc en tête de module).
///
/// La carte sépare la MACHINE (corps de la carte : elle est joignable dès
/// que le dashboard s'affiche) du SERVICE applicatif (premier champ, avec
/// le nom de l'unité systemd) : l'état « hors ligne » du serveur de jeu
/// n'est plus confondu avec l'état du VPS.
pub fn dashboard(status: &Status, alias: &str, unit: &str, catalog: &Catalog) -> CreateEmbed {
    let tone = if online(status) {
        Tone::Good
    } else if offline(status) {
        Tone::Bad
    } else {
        Tone::Wait
    };
    let service_state = if online(status) {
        catalog.status.state_running
    } else if offline(status) {
        catalog.status.state_offline
    } else {
        catalog.status.state_unknown
    };
    let embed = card(
        &fill(
            catalog.status.title,
            &[("name", &display_name(status, alias))],
        ),
        catalog.status.machine_online,
        tone,
        None,
    );

    // Champ « serveur » : l'état du service applicatif, toujours en tête,
    // étiqueté avec le nom de l'unité systemd.
    let mut fields: Vec<(String, String, bool)> = vec![(
        fill(catalog.status.service, &[("unit", unit)]),
        service_state.to_string(),
        false,
    )];
    if let Some(pct) = status.cpu_pct {
        if let Some(bar) = bar(Some(pct), Some(100.0)) {
            let mut value = bar;
            if let Some(model) = &status.cpu_model {
                value.push_str(&format!("\n{model}"));
            }
            if let Some(count) = status.cpu_count {
                value.push_str(&format!("\n{} {}", fmt_num(count), catalog.status.cores));
            }
            fields.push((catalog.status.cpu.to_string(), value, false));
        }
    }
    if let (Some(used), Some(max)) = (status.ram_mb, status.ram_max_mb) {
        if let Some(bar) = bar(Some(used), Some(max)) {
            fields.push((
                catalog.status.memory.to_string(),
                format!("{} / {} MB\n{}", fmt_num(used), fmt_num(max), bar),
                false,
            ));
        }
    }
    if let (Some(used), Some(max)) = (status.disk_mb, status.disk_max_mb) {
        if let Some(bar) = bar(Some(used), Some(max)) {
            fields.push((
                catalog.status.storage.to_string(),
                format!("{} / {} MB\n{}", fmt_num(used), fmt_num(max), bar),
                false,
            ));
        }
    }

    // ── « Puces » : charge et uptimes, côte à côte dès qu'elles sont ≥ 2 ──
    let mut chips: Vec<(String, String, bool)> = Vec::new();
    if let (Some(l1), Some(l5), Some(l15)) = (status.load1, status.load5, status.load15) {
        chips.push((
            catalog.status.load.to_string(),
            format!("1m {l1:.2} · 5m {l5:.2} · 15m {l15:.2}"),
            false,
        ));
    }
    if let Some(uptime) = status.uptime_seconds {
        chips.push((
            catalog.status.uptime.to_string(),
            fmt_duration(uptime),
            false,
        ));
    }
    if let Some(service_uptime) = status.service_uptime_seconds {
        chips.push((
            catalog.status.service_uptime.to_string(),
            fmt_duration(service_uptime),
            false,
        ));
    }
    let chips_inline = chips.len() > 1;
    for mut chip in chips {
        chip.2 = chips_inline;
        fields.push(chip);
    }

    // ── Infos : OS, noyau, adresse, connexion ──
    let mut info: Vec<(String, String, bool)> = Vec::new();
    if let Some(os) = &status.os {
        info.push((catalog.status.os.to_string(), os.clone(), false));
    }
    if let Some(kernel) = &status.kernel {
        info.push((catalog.status.kernel.to_string(), kernel.clone(), false));
    }
    if let Some(addr) = &status.address {
        let value = match addr.port {
            Some(port) => format!("{}:{port}", addr.ip),
            None => addr.ip.clone(),
        };
        info.push((catalog.status.address.to_string(), value, false));
    }
    if let Some(backend) = &status.backend {
        let label = match backend.as_str() {
            "ssh" => catalog.status.ssh_label,
            _ => catalog.status.local_label,
        };
        info.push((catalog.status.backend.to_string(), label.to_string(), false));
    }
    let info_inline = info.len() > 1;
    // Séparateur invisible : sans lui, deux paires inline consécutives
    // seraient regroupées par Discord en une grille 3+1.
    if chips_inline && info_inline {
        fields.push(("\u{200b}".to_string(), "\u{200b}".to_string(), false));
    }
    for mut i in info {
        i.2 = info_inline;
        fields.push(i);
    }

    if fields.is_empty() {
        fields.push((
            catalog.status.metrics.to_string(),
            catalog.status.not_reported.to_string(),
            false,
        ));
    }

    let fields: Vec<(String, String, bool)> = fields;
    let mut embed = embed;
    embed = embed.fields(fields);
    embed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_num_groups_thousands() {
        assert_eq!(fmt_num(2048.0), "2,048");
        assert_eq!(fmt_num(42.5), "42.5");
        assert_eq!(fmt_num(42.05), "42.05");
        assert_eq!(fmt_num(0.0), "0");
    }

    #[test]
    fn fmt_duration_uses_neutral_units() {
        assert_eq!(fmt_duration(3.0 * 3600.0 + 24.0 * 60.0), "3h 24m");
        assert_eq!(fmt_duration(2.0 * 86400.0 + 3600.0), "2d 1h");
        assert_eq!(fmt_duration(59.0), "0m");
    }

    #[test]
    fn bar_clamps_and_rounds() {
        assert_eq!(bar(Some(42.0), Some(100.0)).unwrap(), "▰▰▰▰▱▱▱▱▱▱ 42.0%");
        assert_eq!(bar(Some(150.0), Some(100.0)).unwrap(), "▰▰▰▰▰▰▰▰▰▰ 100.0%");
        assert_eq!(bar(None, Some(100.0)), None);
        assert_eq!(bar(Some(10.0), Some(0.0)), None);
    }

    #[test]
    fn online_offline_detection() {
        let s: Status = serde_json::from_value(serde_json::json!({"state": "running"})).unwrap();
        assert!(online(&s));
        let s: Status = serde_json::from_value(serde_json::json!({"state": "stopped"})).unwrap();
        assert!(offline(&s));
        let s: Status = serde_json::from_value(serde_json::json!({"state": "starting"})).unwrap();
        assert!(!online(&s) && !offline(&s));
    }
}
