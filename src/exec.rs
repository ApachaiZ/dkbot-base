//! Module `exec` : diagnostics EN LECTURE SEULE, sur liste blanche FIXE.
//!
//! - builtins : `journal` (journalctl d'une unité, lignes bornées 1-500),
//!   `ss` (ports en écoute), `df` (disques), `ip` (adresses), `uptime`,
//!   `who` ;
//! - OWNER uniquement, module DÉSACTIVÉ par défaut (`/modules enable exec`) ;
//! - chaque valeur est validée côté Rust AVANT d'atteindre le script
//!   embarqué : builtin ∈ liste blanche, unité `[A-Za-z0-9_.@:\-]` (256
//!   max), lignes bornées — aucune entrée libre, aucun shell ;
//! - toute exécution est loguée + notifiée dans le canal d'audit.

use crate::commands::{reply, resolve_target_opt, Context, Error};
use crate::embeds::{card, Tone};
use crate::i18n::fill;
use crate::logger;
use crate::modules::{require, MODULE_EXEC};

pub const BUILTINS: [&str; 6] = ["journal", "ss", "df", "ip", "uptime", "who"];

/// Nom d'unité systemd STRICTEMENT validé (même alphabet que les services
/// configurés — aucune métacaractère de shell).
pub fn valid_unit(unit: &str) -> bool {
    !unit.is_empty()
        && unit.len() <= 256
        && unit
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '@' | ':' | '-'))
}

/// `/exec <builtin> [unité] [lignes]` — diagnostic en lecture seule (owner).
pub async fn run(
    ctx: Context<'_>,
    builtin: String,
    target: Option<String>,
    unit: Option<String>,
    lines: Option<u64>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let catalog = ctx.data().catalog;
    let e = &catalog.exec;

    if !ctx.data().state.is_owner(&ctx.author().id.to_string()) {
        ctx.send(reply(card(
            e.access_denied.title,
            e.access_denied.body,
            Tone::Bad,
            None,
        )))
        .await?;
        return Ok(());
    }
    if !require(&ctx, MODULE_EXEC).await? {
        return Ok(());
    }

    // ── Validation STRICTE avant tout envoi ──
    if !BUILTINS.contains(&builtin.as_str()) {
        let mut body = fill(e.unknown, &[("builtin", &builtin)]);
        body.push('\n');
        body.push_str(e.hint);
        ctx.send(reply(card(e.title, &body, Tone::Bad, None)))
            .await?;
        return Ok(());
    }
    let unit = unit.unwrap_or_default();
    if builtin == "journal" && !valid_unit(&unit) {
        ctx.send(reply(card(e.title, e.invalid_unit, Tone::Bad, None)))
            .await?;
        return Ok(());
    }
    if !unit.is_empty() && !valid_unit(&unit) {
        ctx.send(reply(card(e.title, e.invalid_unit, Tone::Bad, None)))
            .await?;
        return Ok(());
    }
    let lines = lines.unwrap_or(50).clamp(1, 500);

    let Some((target_alias, _)) = resolve_target_opt(&ctx, target.as_deref()).await? else {
        return Ok(());
    };

    let args = vec![builtin.clone(), unit.clone(), lines.to_string()];
    match ctx.data().api.exec_diagnostic(&target_alias, &args).await {
        Ok(out) => {
            logger::info(format!("exec: {builtin} on {target_alias} (unit={unit:?})"));
            let tail: Vec<&str> = out.lines().rev().take(30).collect();
            let body = format!(
                "```\n{}\n```",
                tail.into_iter().rev().collect::<Vec<_>>().join("\n")
            );
            ctx.send(reply(card(e.title, &body, Tone::Good, None)))
                .await?;
            crate::notify::audit(
                ctx.http(),
                &ctx.data().cfg,
                catalog,
                e.title,
                &format!("`{builtin}` sur `{target_alias}`"),
                Tone::Info,
            )
            .await;
        }
        Err(err) => {
            logger::error(format!(
                "exec: {builtin} failed on {target_alias}: {}",
                err.message
            ));
            ctx.send(reply(card(e.title, &err.message, Tone::Bad, None)))
                .await?;
        }
    }
    Ok(())
}

/// Autocomplétion des builtins.
pub fn builtin_autocomplete(
    _ctx: Context<'_>,
    partial: &str,
) -> Vec<poise::serenity_prelude::AutocompleteChoice> {
    BUILTINS
        .iter()
        .filter(|b| b.contains(partial))
        .map(|b| poise::serenity_prelude::AutocompleteChoice::new((*b).to_string(), *b))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_validation_is_strict() {
        assert!(valid_unit("nginx"));
        assert!(valid_unit("unit-a.b@c:d"));
        assert!(!valid_unit("rm -rf /"));
        assert!(!valid_unit("a;b"));
        assert!(!valid_unit(""));
        assert!(!valid_unit("$(id)"));
    }

    #[test]
    fn builtins_are_the_whitelist() {
        assert_eq!(BUILTINS.len(), 6);
        assert!(BUILTINS.contains(&"journal"));
    }
}
