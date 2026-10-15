//! Framework de MODULES : fonctionnalités optionnelles, activées ou
//! désactivées À LA VOLÉE depuis Discord (`/modules`) sans redémarrage —
//! l'état est persisté dans `modules.json` (aucun secret : fichier en
//! clair, mais écrit en atomique 0600 comme le reste de l'état).
//!
//! Règles :
//! - les noms de modules sont des IDENTIFIANTS stables (jamais traduits) ;
//! - une commande de module vérifie l'état du module à l'entrée
//!   (`require_module`) et répond « module désactivé » sinon ;
//! - une boucle d'arrière-plan (watchdog, scheduler, métriques) vérifie
//!   l'état à CHAQUE tick : désactiver un module arrête son travail au
//!   tick suivant, sans redémarrage ;
//! - valeurs par défaut : watchdog/scheduler/backups actifs, `exec`
//!   (exécution de commandes) et `metrics` INACTIFS (surfaces à activer
//!   explicitement).

use std::collections::HashMap;

use poise::serenity_prelude as serenity;
use serde::{Deserialize, Serialize};

use crate::commands::{reply, Context, Error};
use crate::embeds::{card, Tone};
use crate::i18n::fill;

pub const MODULE_WATCHDOG: &str = "watchdog";
pub const MODULE_SCHEDULER: &str = "scheduler";
pub const MODULE_BACKUPS: &str = "backups";
pub const MODULE_EXEC: &str = "exec";
pub const MODULE_METRICS: &str = "metrics";

/// Tous les modules connus (ordre d'affichage).
pub const ALL_MODULES: [&str; 5] = [
    MODULE_WATCHDOG,
    MODULE_SCHEDULER,
    MODULE_BACKUPS,
    MODULE_EXEC,
    MODULE_METRICS,
];

/// Un module inconnu est refusé (jamais de création silencieuse).
pub fn is_known(name: &str) -> bool {
    ALL_MODULES.contains(&name)
}

/// État par défaut d'un module (sans `modules.json`). Surfaces sensibles
/// ou optionnelles : désactivées.
pub fn default_enabled(name: &str) -> bool {
    match name {
        MODULE_WATCHDOG | MODULE_SCHEDULER | MODULE_BACKUPS => true,
        MODULE_EXEC | MODULE_METRICS => false,
        _ => false,
    }
}

/// Registre des états de modules (persisté).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Modules {
    /// Uniquement les états EXPLICITES — l'absence = défaut.
    states: HashMap<String, bool>,
}

impl Modules {
    /// Charge `modules.json` ; entrées inconnues ou mal formées ignorées
    /// (le fichier n'est pas un secret : tolérance, jamais d'échec au
    /// démarrage).
    pub fn load() -> Self {
        Self::load_from(&crate::paths::modules_file())
    }

    fn load_from(path: &std::path::Path) -> Self {
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let Ok(mut parsed) = serde_json::from_str::<Modules>(&raw) else {
            return Self::default();
        };
        parsed.states.retain(|name, _| is_known(name));
        parsed
    }

    fn save_to(&self, path: &std::path::Path) {
        let Ok(json) = serde_json::to_string(self) else {
            return;
        };
        let _ = crate::fsutil::secure_write(path, json.as_bytes());
    }

    /// Persiste l'état (appelé par l'appelant après une bascule).
    pub fn save(&self) {
        self.save_to(&crate::paths::modules_file());
    }

    /// Le module est-il actif ? (état explicite, sinon défaut)
    pub fn enabled(&self, name: &str) -> bool {
        self.states
            .get(name)
            .copied()
            .unwrap_or_else(|| default_enabled(name))
    }

    /// Change l'état d'un module KNOWN (en mémoire ; persistez avec
    /// [`Self::save`]). `Err` = nom inconnu.
    pub fn set(&mut self, name: &str, on: bool) -> Result<(), ()> {
        if !is_known(name) {
            return Err(());
        }
        self.states.insert(name.to_string(), on);
        Ok(())
    }

    /// États de tous les modules connus (nom, actif) — pour `/modules`.
    pub fn entries(&self) -> Vec<(String, bool)> {
        ALL_MODULES
            .iter()
            .map(|name| ((*name).to_string(), self.enabled(name)))
            .collect()
    }

    /// Description localisée d'un module (liste `/modules`).
    fn description(name: &str, catalog: &crate::i18n::Catalog) -> &'static str {
        let m = &catalog.modules;
        match name {
            MODULE_WATCHDOG => m.desc_watchdog,
            MODULE_SCHEDULER => m.desc_scheduler,
            MODULE_BACKUPS => m.desc_backups,
            MODULE_EXEC => m.desc_exec,
            MODULE_METRICS => m.desc_metrics,
            _ => "",
        }
    }
}

// ── Commandes Discord ─────────────────────────────────────────────────────

/// Garde d'entrée des commandes de module : si le module est désactivé,
/// répond une carte explicative et retourne `Ok(false)`.
pub async fn require(ctx: &Context<'_>, name: &str) -> Result<bool, Error> {
    if ctx.data().state.module_enabled(name) {
        return Ok(true);
    }
    let c = ctx.data().catalog.modules.module_disabled;
    ctx.send(reply(card(
        c.title,
        &fill(c.body, &[("module", name)]),
        Tone::Info,
        None,
    )))
    .await?;
    Ok(false)
}

/// `/modules` — liste l'état des modules (owner uniquement).
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    if !ctx.data().state.is_owner(&ctx.author().id.to_string()) {
        let c = ctx.data().catalog.modules.access_denied;
        ctx.send(reply(card(c.title, c.body, Tone::Bad, None)))
            .await?;
        return Ok(());
    }
    let catalog = ctx.data().catalog;
    let entries = ctx.data().state.modules_entries();
    let mut body = String::new();
    for (name, enabled) in entries {
        let state = if enabled {
            catalog.modules.on
        } else {
            catalog.modules.off
        };
        let desc = Modules::description(&name, catalog);
        body.push_str(&format!("**{name}** — {state}\n{desc}\n\n"));
    }
    body.push_str(catalog.modules.list_hint);
    ctx.send(reply(card(
        catalog.modules.list_title,
        &body,
        Tone::Good,
        None,
    )))
    .await?;
    Ok(())
}

/// `/modules enable <module>` — active un module (owner).
pub async fn enable(ctx: Context<'_>, name: String) -> Result<(), Error> {
    toggle(ctx, &name, true).await
}

/// `/modules disable <module>` — désactive un module (owner).
pub async fn disable(ctx: Context<'_>, name: String) -> Result<(), Error> {
    toggle(ctx, &name, false).await
}

async fn toggle(ctx: Context<'_>, name: &str, on: bool) -> Result<(), Error> {
    let catalog = ctx.data().catalog;
    if !ctx.data().state.is_owner(&ctx.author().id.to_string()) {
        let c = catalog.modules.access_denied;
        ctx.send(reply(card(c.title, c.body, Tone::Bad, None)))
            .await?;
        return Ok(());
    }
    match ctx.data().state.set_module(name, on) {
        Ok(()) => {
            let text = if on {
                catalog.modules.toggled_on
            } else {
                catalog.modules.toggled_off
            };
            ctx.send(reply(card(
                catalog.modules.list_title,
                &fill(text, &[("module", name)]),
                Tone::Good,
                None,
            )))
            .await?;
            crate::logger::info(format!(
                "module '{name}' {}",
                if on { "enabled" } else { "disabled" }
            ));
            crate::notify::audit(
                ctx.http(),
                &ctx.data().cfg,
                ctx.data().catalog,
                catalog.modules.list_title,
                &fill(text, &[("module", name)]),
                Tone::Info,
            )
            .await;
        }
        Err(()) => {
            ctx.send(reply(card(
                catalog.modules.list_title,
                &fill(catalog.modules.unknown, &[("module", name)]),
                Tone::Bad,
                None,
            )))
            .await?;
        }
    }
    Ok(())
}

/// Autocomplétion des noms de modules.
pub fn autocomplete(_ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let partial = partial.to_lowercase();
    ALL_MODULES
        .iter()
        .filter(|name| name.contains(&partial))
        .map(|name| serenity::AutocompleteChoice::new((*name).to_string(), *name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_module_is_refused() {
        assert!(!is_known("nimporte"));
        assert!(is_known(MODULE_WATCHDOG));
    }

    #[test]
    fn defaults_are_safe() {
        assert!(default_enabled(MODULE_WATCHDOG));
        assert!(default_enabled(MODULE_SCHEDULER));
        assert!(default_enabled(MODULE_BACKUPS));
        assert!(!default_enabled(MODULE_EXEC));
        assert!(!default_enabled(MODULE_METRICS));
        assert!(!default_enabled("inconnu"));
    }

    #[test]
    fn explicit_state_overrides_default_and_persists() {
        let mut m = Modules::default();
        // Défaut appliqué quand aucun état explicite.
        assert!(m.enabled(MODULE_WATCHDOG));
        assert!(!m.enabled(MODULE_EXEC));
        // Bascule explicite + relecture depuis un fichier TEMPORAIRE.
        m.set(MODULE_EXEC, true).unwrap();
        assert!(m.enabled(MODULE_EXEC));
        let dir =
            std::env::temp_dir().join(format!("dakkabot_modules_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("modules.json");
        m.save_to(&path);
        let reloaded = Modules::load_from(&path);
        assert!(reloaded.enabled(MODULE_EXEC));
        assert!(!reloaded.enabled(MODULE_METRICS));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
