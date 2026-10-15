//! Slash commands (framework poise) + handlers globaux.
//!
//! Les données globales ([`Data`]) sont partagées entre TOUTES les
//! commandes via `Arc` (géré par poise) : config immuable, couche API
//! (cache + dédup), état (users/verrous/sessions) et catalogue i18n.

use poise::serenity_prelude as serenity;
use rand::RngCore;
use serenity::{
    ButtonStyle, CreateActionRow, CreateButton, CreateEmbed, CreateInteractionResponseFollowup,
    MessageId,
};
use std::sync::Arc;

use crate::plugin::PluginHost;
use std::time::Duration;

use crate::api::Api;
use crate::config::Config;
use crate::confirm;
use crate::embeds::{card, dashboard, Tone};
use crate::errors::{error_text, ErrorContext};
use crate::i18n::{fill, Catalog};
use crate::manage;
use crate::members;
use crate::power::{self, Action};
use crate::state::State;
use crate::targets::{pick_service, pick_target};
use crate::watchdog;
use futures::StreamExt;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

/// Données globales du bot.
pub struct Data {
    pub cfg: Config,
    pub api: Arc<Api>,
    pub state: Arc<State>,
    pub catalog: &'static Catalog,
    /// Hôte de plugins dynamiques (dossier `plugins/`, ABI versionné).
    pub plugins: Arc<PluginHost>,
}

/// Réponse standard du bot : éphémère par défaut, mentions désactivées.
pub fn reply(embed: CreateEmbed) -> poise::CreateReply {
    poise::CreateReply::default()
        .embed(embed)
        .allowed_mentions(serenity::CreateAllowedMentions::new())
}

// ── Aides partagées par les commandes ──────────────────────────────────────

/// Contrôle d'accès opérateur.
pub(crate) async fn ensure_operator(ctx: &Context<'_>) -> Result<bool, Error> {
    let data = ctx.data();
    if data.state.is_allowed(&ctx.author().id.to_string()) {
        return Ok(true);
    }
    let c = data.catalog.common.access_denied_member;
    ctx.send(reply(card(c.title, c.body, Tone::Bad, None)))
        .await?;
    Ok(false)
}

/// Résolution de la cible : option explicite → session utilisateur →
/// cible par défaut. Gère la session OBSOLÈTE (purge + message).
pub async fn resolve_target_opt(
    ctx: &Context<'_>,
    target: Option<&str>,
) -> Result<Option<(String, crate::targets::TargetSpec)>, Error> {
    let data = ctx.data();
    let wanted = target
        .map(str::to_string)
        .or_else(|| data.state.get_target_session(&ctx.author().id.to_string()));
    if let Some(alias) = &wanted {
        if !data.cfg.targets.iter().any(|t| &t.alias == alias) {
            data.state
                .clear_target_session(&ctx.author().id.to_string());
            let c = data.catalog.common.session_reset_target;
            let body = fill(c.body, &[("alias", alias)]);
            ctx.send(reply(card(c.title, &body, Tone::Wait, None)))
                .await?;
            return Ok(None);
        }
    }
    match pick_target(&data.cfg.targets, wanted.as_deref()) {
        Ok((alias, spec)) => Ok(Some((alias.to_string(), spec.clone()))),
        Err(message) => {
            let c = data.catalog.common.unknown_target;
            ctx.send(reply(card(c.title, &message, Tone::Bad, None)))
                .await?;
            Ok(None)
        }
    }
}

/// Résolution du service : option explicite → session utilisateur →
/// service par défaut. Gère la session OBSOLÈTE (purge + message).
pub async fn resolve_service_opt(
    ctx: &Context<'_>,
    service: Option<&str>,
) -> Result<Option<(String, String)>, Error> {
    let data = ctx.data();
    let wanted = service
        .map(str::to_string)
        .or_else(|| data.state.get_service_session(&ctx.author().id.to_string()));
    if let Some(alias) = &wanted {
        if !data.cfg.services.iter().any(|(a, _)| a == alias) {
            data.state
                .clear_service_session(&ctx.author().id.to_string());
            let c = data.catalog.common.session_reset_service;
            let body = fill(c.body, &[("alias", alias)]);
            ctx.send(reply(card(c.title, &body, Tone::Wait, None)))
                .await?;
            return Ok(None);
        }
    }
    match pick_service(&data.cfg.services, wanted.as_deref()) {
        Ok((alias, unit)) => Ok(Some((alias.to_string(), unit.to_string()))),
        Err(message) => {
            let c = data.catalog.common.unknown_service;
            ctx.send(reply(card(c.title, &message, Tone::Bad, None)))
                .await?;
            Ok(None)
        }
    }
}

// ── Autocomplétions ────────────────────────────────────────────────────────

async fn target_autocomplete(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    let partial = partial.to_lowercase();
    ctx.data()
        .cfg
        .targets
        .iter()
        .filter(|t| t.alias.to_lowercase().contains(&partial))
        .map(|t| {
            serenity::AutocompleteChoice::new(
                format!("{} ({})", t.alias, t.display()),
                t.alias.clone(),
            )
        })
        .collect()
}

async fn service_autocomplete(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    let partial = partial.to_lowercase();
    ctx.data()
        .cfg
        .services
        .iter()
        .filter(|(alias, _)| alias.to_lowercase().contains(&partial))
        .map(|(alias, unit)| {
            serenity::AutocompleteChoice::new(format!("{alias} ({unit})"), alias.clone())
        })
        .collect()
}

// ── Commandes ──────────────────────────────────────────────────────────────

/// 📊 Dashboard live de la machine + du service.
#[poise::command(slash_command, ephemeral)]
pub async fn status(
    ctx: Context<'_>,
    #[description = "Target machine (defaults to your session target, then the primary one)"]
    #[autocomplete = "target_autocomplete"]
    target: Option<String>,
    #[description = "Application service (defaults to your session service, then the primary one)"]
    #[autocomplete = "service_autocomplete"]
    service: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    if !ensure_operator(&ctx).await? {
        return Ok(());
    }
    let Some((target_alias, _)) = resolve_target_opt(&ctx, target.as_deref()).await? else {
        return Ok(());
    };
    let Some((alias, unit)) = resolve_service_opt(&ctx, service.as_deref()).await? else {
        return Ok(());
    };
    // Lecture non-fresh : le cache 5 s partagé avec le watchdog évite une
    // collecte redondante si un scan vient de passer.
    let status = ctx.data().api.get(&target_alias, &alias, false).await?;
    let embed = dashboard(&status, &alias, &unit, ctx.data().catalog);

    // Bouton « Rafraîchir » : re-lit l'état et réédite CE message.
    let refresh_id = random_hex(8);
    let row = CreateActionRow::Buttons(vec![CreateButton::new(&refresh_id)
        .label(ctx.data().catalog.status.refresh)
        .style(ButtonStyle::Secondary)]);
    let handle = ctx.send(reply(embed).components(vec![row])).await?;
    let message_id = handle.message().await?.id;
    spawn_refresh_button(&ctx, message_id, refresh_id, target_alias, alias, unit);
    Ok(())
}

/// Fenêtre de vie du bouton Rafraîchir : 10 min, sous la limite des 15 min
/// du token d'interaction.
const REFRESH_TIMEOUT: Duration = Duration::from_secs(600);

/// Identifiant aléatoire de bouton (16 octets hex).
fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// Tâche détachée du bouton Rafraîchir : chaque clic relit l'état et
/// RÉÉDITE le message éphémère via le TOKEN D'INTERACTION.
fn spawn_refresh_button(
    ctx: &Context<'_>,
    message_id: MessageId,
    refresh_id: String,
    target_alias: String,
    alias: String,
    unit: String,
) {
    let http = ctx.serenity_context().http.clone();
    let shard = ctx.serenity_context().shard.clone();
    let interaction = match ctx {
        poise::Context::Application(app) => app.interaction.clone(),
        poise::Context::Prefix(_) => return,
    };
    let api = ctx.data().api.clone();
    let catalog = ctx.data().catalog;
    let author_id = ctx.author().id;

    tokio::spawn(async move {
        let mut collector = Box::pin(
            serenity::collector::ComponentInteractionCollector::new(&shard)
                .message_id(message_id)
                .author_id(author_id)
                .timeout(REFRESH_TIMEOUT)
                .stream(),
        );
        while let Some(click) = collector.next().await {
            let _ = click.defer(&http).await;
            let row = CreateActionRow::Buttons(vec![CreateButton::new(&refresh_id)
                .label(catalog.status.refresh)
                .style(ButtonStyle::Secondary)]);
            let builder = match api.get(&target_alias, &alias, true).await {
                Ok(status) => CreateInteractionResponseFollowup::new()
                    .embed(dashboard(&status, &alias, &unit, catalog))
                    .components(vec![row]),
                Err(err) => CreateInteractionResponseFollowup::new()
                    .embed(card(
                        catalog.common.generic_error.title,
                        &error_text(&err, ErrorContext::Other, catalog),
                        Tone::Bad,
                        None,
                    ))
                    .components(Vec::new()),
            };
            let _ = interaction.edit_followup(&http, message_id, builder).await;
        }
        let _ = interaction
            .edit_followup(
                &http,
                message_id,
                CreateInteractionResponseFollowup::new().components(Vec::new()),
            )
            .await;
    });
}

/// ▶️ Démarre le service applicatif (avec confirmation et suivi).
#[poise::command(slash_command, ephemeral)]
pub async fn start(
    ctx: Context<'_>,
    #[description = "Target machine"]
    #[autocomplete = "target_autocomplete"]
    target: Option<String>,
    #[description = "Application service"]
    #[autocomplete = "service_autocomplete"]
    service: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    if !ensure_operator(&ctx).await? {
        return Ok(());
    }
    power::power(ctx, Action::Start, target, service, false).await
}

/// ⏹️ Arrête le service applicatif (avec confirmation et suivi).
#[poise::command(slash_command, ephemeral)]
pub async fn stop(
    ctx: Context<'_>,
    #[description = "Target machine"]
    #[autocomplete = "target_autocomplete"]
    target: Option<String>,
    #[description = "Application service"]
    #[autocomplete = "service_autocomplete"]
    service: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    if !ensure_operator(&ctx).await? {
        return Ok(());
    }
    power::power(ctx, Action::Stop, target, service, false).await
}

/// 🔁 Redémarre le service applicatif (avec confirmation et suivi).
#[poise::command(slash_command, ephemeral)]
pub async fn restart(
    ctx: Context<'_>,
    #[description = "Target machine"]
    #[autocomplete = "target_autocomplete"]
    target: Option<String>,
    #[description = "Application service"]
    #[autocomplete = "service_autocomplete"]
    service: Option<String>,
    #[description = "Back up during the stop (stop → copy → start)"] with_backup: Option<bool>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    if !ensure_operator(&ctx).await? {
        return Ok(());
    }
    power::power(
        ctx,
        Action::Restart,
        target,
        service,
        with_backup.unwrap_or(false),
    )
    .await
}

/// 🔌 Redémarre la MACHINE elle-même (owner uniquement).
#[poise::command(slash_command, ephemeral)]
pub async fn reboot(
    ctx: Context<'_>,
    #[description = "Target machine"]
    #[autocomplete = "target_autocomplete"]
    target: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    power::reboot(ctx, target).await
}

/// 🏓 Mesure la latence aller-retour vers la machine.
#[poise::command(slash_command, ephemeral)]
pub async fn ping(
    ctx: Context<'_>,
    #[description = "Target machine"]
    #[autocomplete = "target_autocomplete"]
    target: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    if !ensure_operator(&ctx).await? {
        return Ok(());
    }
    let Some((target_alias, target_spec)) = resolve_target_opt(&ctx, target.as_deref()).await?
    else {
        return Ok(());
    };
    let catalog = ctx.data().catalog;
    let start = std::time::Instant::now();
    match ctx.data().api.ping(&target_alias).await {
        Ok(()) => {
            let ms = start.elapsed().as_millis();
            let tone = if ms < 300 {
                Tone::Good
            } else if ms < 1000 {
                Tone::Wait
            } else {
                Tone::Bad
            };
            let body = fill(
                catalog.ping.pong.body,
                &[("name", &target_spec.display()), ("ms", &ms.to_string())],
            );
            ctx.send(reply(card(catalog.ping.pong.title, &body, tone, None)))
                .await?;
        }
        Err(err) => {
            let body = fill(
                catalog.ping.failed.body,
                &[
                    ("name", &target_spec.display()),
                    ("message", &error_text(&err, ErrorContext::Other, catalog)),
                ],
            );
            ctx.send(reply(card(
                catalog.ping.failed.title,
                &body,
                Tone::Bad,
                None,
            )))
            .await?;
        }
    }
    Ok(())
}

/// 🎯 Définit la machine par défaut pour la session de l'utilisateur.
#[poise::command(slash_command, ephemeral)]
pub async fn target(
    ctx: Context<'_>,
    #[description = "Machine to use by default"]
    #[autocomplete = "target_autocomplete"]
    target: String,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    if !ensure_operator(&ctx).await? {
        return Ok(());
    }
    let data = ctx.data();
    if !data.cfg.targets.iter().any(|t| t.alias == target) {
        let c = data.catalog.common.unknown_target;
        let body = fill(c.body, &[("alias", &target)]);
        ctx.send(reply(card(c.title, &body, Tone::Bad, None)))
            .await?;
        return Ok(());
    }
    data.state
        .set_target_session(&ctx.author().id.to_string(), &target);
    let c = data.catalog.common.default_target;
    let body = fill(c.body, &[("alias", &target)]);
    ctx.send(reply(card(c.title, &body, Tone::Info, None)))
        .await?;
    Ok(())
}

/// ⚙️ Définit le service par défaut pour la session de l'utilisateur.
#[poise::command(slash_command, ephemeral)]
pub async fn server(
    ctx: Context<'_>,
    #[description = "Application service to use by default"]
    #[autocomplete = "service_autocomplete"]
    service: String,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    if !ensure_operator(&ctx).await? {
        return Ok(());
    }
    let data = ctx.data();
    if !data.cfg.services.iter().any(|(alias, _)| alias == &service) {
        let c = data.catalog.common.unknown_service;
        let body = fill(c.body, &[("alias", &service)]);
        ctx.send(reply(card(c.title, &body, Tone::Bad, None)))
            .await?;
        return Ok(());
    }
    data.state
        .set_service_session(&ctx.author().id.to_string(), &service);
    let c = data.catalog.common.default_service;
    let body = fill(c.body, &[("alias", &service)]);
    ctx.send(reply(card(c.title, &body, Tone::Info, None)))
        .await?;
    Ok(())
}

/// 👑 Gestion des opérateurs (sous-commandes dans `manage.rs`).
#[poise::command(
    slash_command,
    ephemeral,
    subcommands("add", "remove", "list", "clear")
)]
pub async fn users(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// 📜 Exporte les derniers journaux du BOT lui-même (owner).
#[poise::command(slash_command, ephemeral)]
pub async fn botlogs(
    ctx: Context<'_>,
    #[description = "Number of lines (1-200)"] lines: Option<u64>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let catalog = ctx.data().catalog;
    if !ctx.data().state.is_owner(&ctx.author().id.to_string()) {
        let c = catalog.bot_logs.access_denied;
        ctx.send(reply(card(c.title, c.body, Tone::Bad, None)))
            .await?;
        return Ok(());
    }
    let lines = lines.unwrap_or(50).clamp(1, 200) as usize;
    let path = crate::paths::log_file();
    let raw = match std::fs::read_to_string(&path) {
        Ok(r) => r,
        Err(_) => {
            ctx.send(reply(card(
                catalog.bot_logs.title,
                catalog.bot_logs.empty,
                Tone::Info,
                None,
            )))
            .await?;
            return Ok(());
        }
    };
    let mut tail: Vec<&str> = raw.lines().rev().take(lines).collect();
    tail.reverse();
    if tail.is_empty() {
        ctx.send(reply(card(
            catalog.bot_logs.title,
            catalog.bot_logs.empty,
            Tone::Info,
            None,
        )))
        .await?;
        return Ok(());
    }
    let body = cap_embed(&format!("```\n{}\n```", tail.join("\n")), 3900);
    ctx.send(reply(card(catalog.bot_logs.title, &body, Tone::Good, None)))
        .await?;
    Ok(())
}

/// 🧩 Gère les modules optionnels (owner).
#[poise::command(
    slash_command,
    ephemeral,
    subcommands("module_cmds::enable", "module_cmds::disable", "module_cmds::list")
)]
pub async fn modules(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// 💾 Gère les archives de sauvegarde.
#[poise::command(
    slash_command,
    ephemeral,
    subcommands("backup_cmds::list", "backup_cmds::restore")
)]
pub async fn backups(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Sous-commandes de `/backups`.
pub mod backup_cmds {
    use super::*;

    /// `/backups list` — liste les archives.
    #[poise::command(slash_command, ephemeral)]
    pub async fn list(
        ctx: Context<'_>,
        #[description = "Target machine"]
        #[autocomplete = "target_autocomplete"]
        target: Option<String>,
        #[description = "Application service"]
        #[autocomplete = "service_autocomplete"]
        service: Option<String>,
    ) -> Result<(), Error> {
        crate::backups::list(ctx, target, service).await
    }

    /// `/backups restore` — restaure une archive (owner).
    #[poise::command(slash_command, ephemeral)]
    pub async fn restore(
        ctx: Context<'_>,
        #[description = "Archive name (from /backups list)"] archive: String,
        #[description = "Target machine"]
        #[autocomplete = "target_autocomplete"]
        target: Option<String>,
        #[description = "Application service"]
        #[autocomplete = "service_autocomplete"]
        service: Option<String>,
    ) -> Result<(), Error> {
        crate::backups::restore(ctx, archive, target, service).await
    }
}

/// 🔎 Diagnostics en lecture seule (module `exec`, owner).
#[poise::command(slash_command, ephemeral)]
pub async fn exec(
    ctx: Context<'_>,
    #[description = "Read-only builtin"]
    #[autocomplete = "exec_builtin_autocomplete"]
    builtin: String,
    #[description = "Target machine"]
    #[autocomplete = "target_autocomplete"]
    target: Option<String>,
    #[description = "Systemd unit (journal only)"] unit: Option<String>,
    #[description = "Lines (journal only, 1-500)"] lines: Option<u64>,
) -> Result<(), Error> {
    crate::exec::run(ctx, builtin, target, unit, lines).await
}

async fn exec_builtin_autocomplete(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    crate::exec::builtin_autocomplete(ctx, partial)
}

/// 📅 Planifie des actions power (owner).
#[poise::command(
    slash_command,
    ephemeral,
    subcommands(
        "schedule_cmds::add",
        "schedule_cmds::remove",
        "schedule_cmds::list",
        "schedule_cmds::clear"
    )
)]
pub async fn schedule(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Sous-commandes de `/schedule`.
pub mod schedule_cmds {
    use super::*;

    /// `/schedule add` — ajoute une action planifiée.
    #[poise::command(slash_command, ephemeral)]
    pub async fn add(
        ctx: Context<'_>,
        #[description = "When: daily HH:MM, weekly D HH:MM (1=Monday) or once YYYY-MM-DD HH:MM"]
        when: String,
        #[description = "Action to run"]
        #[autocomplete = "schedule_action_autocomplete"]
        action: String,
        #[description = "Target machine"]
        #[autocomplete = "target_autocomplete"]
        target: Option<String>,
        #[description = "Application service"]
        #[autocomplete = "service_autocomplete"]
        service: Option<String>,
    ) -> Result<(), Error> {
        crate::schedule::add(ctx, when, action, target, service).await
    }

    /// `/schedule list` — liste les actions planifiées.
    #[poise::command(slash_command, ephemeral)]
    pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
        crate::schedule::list(ctx).await
    }

    /// `/schedule remove` — retire une action planifiée.
    #[poise::command(slash_command, ephemeral)]
    pub async fn remove(
        ctx: Context<'_>,
        #[description = "Entry id"] id: u64,
    ) -> Result<(), Error> {
        crate::schedule::remove(ctx, id).await
    }

    /// `/schedule clear` — vide le planificateur.
    #[poise::command(slash_command, ephemeral)]
    pub async fn clear(ctx: Context<'_>) -> Result<(), Error> {
        crate::schedule::clear(ctx).await
    }
}

async fn schedule_action_autocomplete(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    crate::schedule::action_autocomplete(ctx, partial)
}

/// Sous-commandes de `/modules` (module dédié : noms libres de collision).
pub mod module_cmds {
    use super::*;

    /// `/modules list` — état de tous les modules.
    #[poise::command(slash_command, ephemeral)]
    pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
        crate::modules::list(ctx).await
    }

    /// `/modules enable` — active un module.
    #[poise::command(slash_command, ephemeral)]
    pub async fn enable(
        ctx: Context<'_>,
        #[description = "Module to enable"]
        #[autocomplete = "module_autocomplete"]
        name: String,
    ) -> Result<(), Error> {
        crate::modules::enable(ctx, name).await
    }

    /// `/modules disable` — désactive un module.
    #[poise::command(slash_command, ephemeral)]
    pub async fn disable(
        ctx: Context<'_>,
        #[description = "Module to disable"]
        #[autocomplete = "module_autocomplete"]
        name: String,
    ) -> Result<(), Error> {
        crate::modules::disable(ctx, name).await
    }
}

async fn module_autocomplete(ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    crate::modules::autocomplete(ctx, partial)
}

/// `/users add` — autorise un opérateur.
#[poise::command(slash_command, ephemeral)]
async fn add(
    ctx: Context<'_>,
    #[description = "Member to authorize"]
    #[autocomplete = "member_autocomplete_add"]
    member: String,
) -> Result<(), Error> {
    manage::add(ctx, member).await
}

/// `/users remove` — révoque un opérateur.
#[poise::command(slash_command, ephemeral)]
async fn remove(
    ctx: Context<'_>,
    #[description = "Member to revoke"]
    #[autocomplete = "member_autocomplete_remove"]
    member: String,
) -> Result<(), Error> {
    manage::remove(ctx, member).await
}

/// `/users list` — liste paginée des opérateurs.
#[poise::command(slash_command, ephemeral)]
async fn list(
    ctx: Context<'_>,
    #[description = "Page (0-based)"] page: Option<usize>,
) -> Result<(), Error> {
    manage::list(ctx, page).await
}

/// `/users clear` — révoque tous les opérateurs.
#[poise::command(slash_command, ephemeral)]
async fn clear(ctx: Context<'_>) -> Result<(), Error> {
    manage::clear(ctx).await
}

async fn member_autocomplete_add(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    manage::member_autocomplete(ctx, partial, "add").await
}

async fn member_autocomplete_remove(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    manage::member_autocomplete(ctx, partial, "remove").await
}

/// 📜 Dernières lignes de log du service (owner uniquement).
#[poise::command(slash_command, ephemeral)]
pub async fn logs(
    ctx: Context<'_>,
    #[description = "Target machine"]
    #[autocomplete = "target_autocomplete"]
    target: Option<String>,
    #[description = "Application service"]
    #[autocomplete = "service_autocomplete"]
    service: Option<String>,
    #[description = "Only show lines containing this text"] filter: Option<String>,
) -> Result<(), Error> {
    crate::logs_cmd::logs(&ctx, target, service, filter).await
}

/// ❓ Rappel des commandes et permissions.
#[poise::command(slash_command, ephemeral)]
pub async fn help(ctx: Context<'_>) -> Result<(), Error> {
    let c = ctx.data().catalog.help;
    ctx.send(reply(card(c.title, c.body, Tone::Info, None)))
        .await?;
    Ok(())
}

// ── Liste des commandes (ordre d'enregistrement Discord) ───────────────────

/// Parse « k=v k=v » (ou JSON brut) en objet JSON d'arguments.
fn parse_args(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') {
        return trimmed.to_string();
    }
    let mut map = serde_json::Map::new();
    for token in trimmed.split_whitespace() {
        let Some((key, value)) = token.split_once('=') else { continue };
        let value = value.trim();
        let parsed: serde_json::Value = if let Ok(b) = value.parse::<bool>() {
            serde_json::json!(b)
        } else if let Ok(n) = value.parse::<f64>() {
            serde_json::json!(n)
        } else {
            serde_json::json!(value)
        };
        map.insert(key.to_string(), parsed);
    }
    serde_json::Value::Object(map).to_string()
}

/// Tronque un corps long : les embeds Discord sont limités à 4096 caractères.
pub(crate) fn cap_embed(body: &str, max: usize) -> String {
    if body.chars().count() <= max {
        return body.to_string();
    }
    let mut out: String = body.chars().take(max - 40).collect();
    out.push_str("\n\n… (réponse tronquée, utilisez un filtre)");
    out
}

/// Construit le JSON d'appel : contexte appelant + arguments parsés.
async fn plugin_call_args(ctx: &Context<'_>, raw_args: &str) -> String {
    let mut caller = serde_json::json!({
        "id": ctx.author().id.to_string(),
        "is_owner": ctx.data().state.is_owner(&ctx.author().id.to_string()),
        "roles": [],
    });
    if let Some(guild) = ctx.guild_id() {
        if let Ok(member) = ctx
            .serenity_context()
            .http
            .get_member(guild, ctx.author().id)
            .await
        {
            let roles: Vec<String> = member.roles.iter().map(|r| r.get().to_string()).collect();
            caller["roles"] = serde_json::json!(roles);
        }
    }
    serde_json::json!({ "caller": caller, "args": serde_json::from_str::<serde_json::Value>(&parse_args(raw_args)).unwrap_or(serde_json::json!({})) }).to_string()
}

/// 🔌 Plugins dynamiques (ABI versionné) — owner.
#[poise::command(slash_command, ephemeral)]
pub async fn plugin_command(
    ctx: Context<'_>,
    #[description = "Nom du plugin (optionnel)"] nom: Option<String>,
    #[description = "Commande (optionnel)"] commande: Option<String>,
    #[description = "Arguments cle=valeur ou JSON (optionnel)"] args: Option<String>,
) -> Result<(), Error> {
    // Défer IMMÉDIAT : l'exécution d'un plugin peut prendre plusieurs
    // secondes (HTTP vers le mod) — sans defer, Discord coupe à 3 s et
    // l'utilisateur re-clique (double exécution des actions !).
    ctx.defer_ephemeral().await?;
    // L'autorisation fine appartient aux plugins (contexte appelant) ;
    // /plugin est ouvert aux opérateurs du bot.
    if !ensure_operator(&ctx).await? {
        return Ok(());
    }
    let host = &ctx.data().plugins;
    let names = host.names();
    let Some(nom) = nom else {
        let body = if names.is_empty() {
            "Aucun plugin chargé (dossier `plugins/` vide).".to_string()
        } else {
            format!("**Plugins chargés**\n{}", names.join("\n"))
        };
        ctx.send(reply(card("🔌 Plugins", &body, Tone::Good, None)))
            .await?;
        return Ok(());
    };
    let Some(commande) = commande else {
        match host.list_commands(&nom) {
            Ok(cmds) => {
                let body = if cmds.is_empty() {
                    format!("**{nom}** n'expose aucune commande.")
                } else {
                    format!("**{nom}** — commandes : {}", cmds.join(", "))
                };
                ctx.send(reply(card("🔌 Plugins", &body, Tone::Good, None)))
                    .await?;
            }
            Err(e) => {
                ctx.send(reply(card("🔌 Plugins", &e, Tone::Bad, None)))
                    .await?;
            }
        }
        return Ok(());
    };
    let args = plugin_call_args(&ctx, &args.unwrap_or_else(|| "{}".to_string())).await;
    // L'appel du plugin est BLOQUANT (client HTTP synchrone côté plugin) :
    // on le sort du runtime async pour ne pas geler la gateway Discord.
    let host = ctx.data().plugins.clone();
    let nom_run = nom.clone();
    let commande_run = commande.clone();
    let args_run = args.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        host.run(&nom_run, &commande_run, &args_run)
    })
    .await;
    match outcome {
        Ok(Ok(result)) => {
            let body = cap_embed(
                &format!("**{nom}** `{commande}` →\n```json\n{result}\n```"),
                3900,
            );
            ctx.send(reply(card("🔌 Plugins", &body, Tone::Good, None)))
                .await?;
        }
        Ok(Err(e)) => {
            ctx.send(reply(card("🔌 Plugins", &e, Tone::Bad, None)))
                .await?;
        }
        Err(join) => {
            ctx.send(reply(card(
                "🔌 Plugins",
                &format!("tâche interrompue : {join}"),
                Tone::Bad,
                None,
            )))
            .await?;
        }
    }
    Ok(())
}


pub fn commands() -> Vec<poise::Command<Data, Error>> {
    vec![
        status(),
        start(),
        stop(),
        restart(),
        reboot(),
        ping(),
        plugin_command(),
        target(),
        server(),
        users(),
        modules(),
        botlogs(),
        schedule(),
        backups(),
        exec(),
        logs(),
        help(),
    ]
}

/// Événement `Ready` : bannière de démarrage, préchargement des membres et
/// démarrage du watchdog.
pub async fn on_ready(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Error>,
    data: &Data,
) {
    let serenity::FullEvent::Ready { data_about_bot } = event else {
        return;
    };
    crate::logger::banner(&[
        "🎛  DAKKABOT • VPS CONTROL".to_string(),
        format!("👤  {}", data_about_bot.user.tag()),
        format!(
            "🎯  targets: {}",
            data.cfg
                .targets
                .iter()
                .map(|t| t.alias.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        format!(
            "⚙   services: {}",
            data.cfg
                .services
                .iter()
                .map(|(alias, _)| alias.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        format!("🌐  language: {}", data.cfg.language),
        format!(
            "💻  rust {}  •  pid {}",
            env!("CARGO_PKG_VERSION"),
            std::process::id()
        ),
        format!("📅  {}", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S")),
    ]);

    for guild in &data.cfg.guild_ids {
        if let Ok(guild_id) = guild.parse::<u64>() {
            let _ =
                members::refresh_members(ctx, serenity::GuildId::new(guild_id), &data.cfg.token)
                    .await;
        }
    }

    watchdog::start(
        ctx.http.clone(),
        data.cfg.clone(),
        data.api.clone(),
        data.state.clone(),
    );
    crate::schedule::start(
        ctx.http.clone(),
        data.cfg.clone(),
        data.api.clone(),
        data.state.clone(),
    );
    crate::metrics::start(data.cfg.clone(), data.api.clone(), data.state.clone());
}

// ── Hook global : guilde configurée uniquement ─────────────────────────────

/// Refuse toute interaction hors des guildes configurées (multi-guilde :
/// `DISCORD_GUILD_IDS`).
pub fn command_check(
    ctx: Context<'_>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool, Error>> + Send + '_>> {
    let guilds = ctx.data().cfg.guild_ids.clone();
    let catalog = ctx.data().catalog;
    Box::pin(async move {
        let ok = ctx
            .guild_id()
            .map(|g| guilds.iter().any(|gid| gid == &g.to_string()))
            .unwrap_or(false);
        if !ok {
            let c = catalog.common.access_denied_guild;
            let _ = ctx
                .send(reply(card(c.title, c.body, Tone::Bad, None)))
                .await;
        }
        Ok(ok)
    })
}

// ── Gestion d'erreurs centralisée ──────────────────────────────────────────

/// Chaque cause produit un message DÉDIÉ (auth SSH, réseau, commande
/// refusée, timeout…) au lieu d'un fourre-tout.
pub async fn handle_error(err: poise::FrameworkError<'_, Data, Error>) {
    match err {
        poise::FrameworkError::Command { error, ctx, .. } => {
            let catalog = ctx.data().catalog;
            let context = match ctx.command().name.as_str() {
                "start" | "stop" | "restart" => ErrorContext::Power,
                _ => ErrorContext::Other,
            };
            let (title, body) = match error.downcast_ref::<crate::errors::BackendError>() {
                Some(backend_error) => (
                    catalog.common.generic_error.title,
                    error_text(backend_error, context, catalog),
                ),
                None => (catalog.common.generic_error.title, error.to_string()),
            };
            crate::logger::error(format!("Request failure: {body}"));
            let embed = card(title, &body, Tone::Bad, None);
            let edit = serenity::EditInteractionResponse::new()
                .embed(embed.clone())
                .clear_attachments();
            if let Err(edit_error) = confirm::edit_original(&ctx, edit).await {
                crate::logger::warn(format!("Could not edit original response: {edit_error}"));
                if let Err(reply_error) = ctx.send(reply(embed)).await {
                    crate::logger::error(format!(
                        "Could not update Discord response: {reply_error}"
                    ));
                }
            }
        }
        poise::FrameworkError::CommandPanic { payload, ctx, .. } => {
            let catalog = ctx.data().catalog;
            let detail = payload.as_deref().unwrap_or("non-string panic payload");
            crate::logger::error(format!("Command panic: {detail}"));
            let _ = ctx
                .send(reply(card(
                    catalog.common.generic_error.title,
                    catalog.errors.internal_panic,
                    Tone::Bad,
                    None,
                )))
                .await;
        }
        other => {
            crate::logger::error(format!("Framework error: {other}"));
        }
    }
}
