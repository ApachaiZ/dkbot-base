//! Module `backups` : gestion des archives de sauvegarde depuis Discord.
//!
//! - `/backups list` — liste des archives (opérateurs) ;
//! - `/backups restore <archive>` — restauration (OWNER, double
//!   confirmation). Le script côté cible porte les gardes mécaniques :
//!   service arrêté (sinon refus), nom d'archive STRICTEMENT validé
//!   (`*.tar`, sans `/` — pas de traversée), copie de sûreté de l'état
//!   actuel AVANT restauration (`pre-restore-<horodatage>.tar`).
//!
//! Le module est désactivable à la volée (`/modules disable backups`).

use crate::commands::{reply, resolve_service_opt, resolve_target_opt, Context, Error};
use crate::confirm;
use crate::embeds::{card, Tone};
use crate::i18n::fill;
use crate::logger;
use crate::modules::{require, MODULE_BACKUPS};

/// `/backups list` — liste des archives de sauvegarde.
pub async fn list(
    ctx: Context<'_>,
    target: Option<String>,
    service: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    if !crate::commands::ensure_operator(&ctx).await? {
        return Ok(());
    }
    if !require(&ctx, MODULE_BACKUPS).await? {
        return Ok(());
    }
    let catalog = ctx.data().catalog;
    let b = &catalog.backups;

    let Some((target_alias, _)) = resolve_target_opt(&ctx, target.as_deref()).await? else {
        return Ok(());
    };
    let Some((alias, unit)) = resolve_service_opt(&ctx, service.as_deref()).await? else {
        return Ok(());
    };
    if ctx.data().cfg.saves_path.is_none() {
        ctx.send(reply(card(b.list_title, b.no_saves, Tone::Info, None)))
            .await?;
        return Ok(());
    }

    match ctx.data().api.backups_list(&target_alias, &alias).await {
        Ok(out) if out.trim() == "NOBACKUPS" => {
            ctx.send(reply(card(b.list_title, b.empty, Tone::Info, None)))
                .await?;
        }
        Ok(out) => {
            // Dernières lignes (les plus récentes — tri par nom) seulement.
            let tail: Vec<&str> = out.lines().rev().take(15).collect();
            let listing = format!(
                "```\n{}\n```",
                tail.into_iter().rev().collect::<Vec<_>>().join("\n")
            );
            let body = format!("{}\n{listing}", fill(b.listing, &[("service", &unit)]));
            ctx.send(reply(card(b.list_title, &body, Tone::Good, None)))
                .await?;
        }
        Err(e) => {
            ctx.send(reply(card(b.list_title, &e.message, Tone::Bad, None)))
                .await?;
        }
    }
    Ok(())
}

/// `/backups restore` — restaure une archive (owner, double confirmation).
pub async fn restore(
    ctx: Context<'_>,
    archive: String,
    target: Option<String>,
    service: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let catalog = ctx.data().catalog;
    let b = &catalog.backups;

    if !ctx.data().state.is_owner(&ctx.author().id.to_string()) {
        ctx.send(reply(card(
            b.access_denied.title,
            b.access_denied.body,
            Tone::Bad,
            None,
        )))
        .await?;
        return Ok(());
    }
    if !require(&ctx, MODULE_BACKUPS).await? {
        return Ok(());
    }
    let Some((target_alias, _)) = resolve_target_opt(&ctx, target.as_deref()).await? else {
        return Ok(());
    };
    let Some((alias, _)) = resolve_service_opt(&ctx, service.as_deref()).await? else {
        return Ok(());
    };
    if ctx.data().cfg.saves_path.is_none() {
        ctx.send(reply(card(b.list_title, b.no_saves, Tone::Info, None)))
            .await?;
        return Ok(());
    }

    // Double confirmation renforcée : la restauration ÉCRASE l'état actuel
    // (une copie de sûreté est faite côté cible, mais autant être explicite).
    let (_, confirmed) = confirm::confirm(
        &ctx,
        card(
            b.confirm_title,
            &fill(
                b.confirm_body,
                &[
                    ("archive", &archive),
                    ("target", &target_alias),
                    ("service", &alias),
                ],
            ),
            Tone::Wait,
            None,
        ),
    )
    .await?;
    if !confirmed {
        return Ok(());
    }

    match ctx
        .data()
        .api
        .backups_restore(&target_alias, &alias, &archive)
        .await
    {
        Ok(dest) => {
            logger::info(format!(
                "backups: restored {dest} on {target_alias}/{alias}"
            ));
            let body = fill(b.restored, &[("archive", &dest)]);
            ctx.send(reply(card(b.list_title, &body, Tone::Good, None)))
                .await?;
            crate::notify::audit(
                ctx.http(),
                &ctx.data().cfg,
                catalog,
                b.list_title,
                &body,
                Tone::Good,
            )
            .await;
        }
        Err(e) => {
            logger::error(format!(
                "backups: restore failed on {target_alias}/{alias}: {}",
                e.message
            ));
            ctx.send(reply(card(b.list_title, &e.message, Tone::Bad, None)))
                .await?;
            crate::notify::audit(
                ctx.http(),
                &ctx.data().cfg,
                catalog,
                b.list_title,
                &e.message,
                Tone::Bad,
            )
            .await;
        }
    }
    Ok(())
}
