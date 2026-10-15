//! Enregistrement des slash commands (MULTI-GUILDE, hash + purge).
//!
//! Politique :
//! - enregistrement dans CHAQUE guilde configurée (DISCORD_GUILD_IDS),
//!   une requête PUT groupée par guilde ;
//! - un hash (guildes + application + commandes) persisté dans
//!   `.config.d/commands.hash` évite tout ré-enregistrement inutile ;
//! - les commandes GLOBALES résiduelles sont purgées à CHAQUE démarrage
//!   (répare les doublons d'une version antérieure), best-effort ;
//! - une guilde retirée de la config voit ses commandes purgées ;
//! - les erreurs Discord 50001 (Missing Access) et 10002 (Unknown
//!   Application) sont traduites en messages actionnables avec exit 78.

use poise::serenity_prelude as serenity;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::commands::{Data, Error};
use crate::config::Config;
use crate::errors::ExitError;
use crate::fsutil;
use crate::i18n::{fill, Catalog};
use crate::logger;
use crate::paths;

/// Empreinte SHA-256 (hex) de {guildes, application, commandes}.
pub fn commands_hash(cfg: &Config, commands_json: &[Value]) -> String {
    let payload = serde_json::json!({
        "guilds": cfg.guild_ids,
        "app": cfg.client_id,
        "cmds": commands_json,
    });
    format!("{:x}", Sha256::digest(payload.to_string().as_bytes()))
}

#[derive(serde::Deserialize)]
struct DeployState {
    guild: Option<String>,
    guilds: Option<Vec<String>>,
    hash: String,
}

/// État persisté du déploiement : JSON `{guilds, hash}` (ou `{guild, hash}`
/// hérité), ou hex seul hérité.
pub fn read_deploy_state(default_guild: &str) -> Option<(Vec<String>, String)> {
    let raw = std::fs::read_to_string(paths::commands_hash_file()).ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with('{') {
        let parsed: DeployState = serde_json::from_str(raw).ok()?;
        let guilds = parsed
            .guilds
            .or_else(|| parsed.guild.map(|g| vec![g]))
            .unwrap_or_else(|| vec![default_guild.to_string()]);
        return Some((guilds, parsed.hash));
    }
    if raw.len() == 64 && raw.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some((vec![default_guild.to_string()], raw.to_string()));
    }
    None
}

pub fn write_deploy_state(guilds: &[String], hash: &str) {
    if let Err(e) = fsutil::secure_write(
        &paths::commands_hash_file(),
        serde_json::json!({ "guilds": guilds, "hash": hash })
            .to_string()
            .as_bytes(),
    ) {
        logger::error(format!("Could not persist commands hash: {e}"));
    }
}

/// Code d'erreur Discord (`50001`, `10002`…) d'une erreur HTTP.
fn discord_error_code(err: &serenity::Error) -> Option<u64> {
    match err {
        serenity::Error::Http(serenity::http::HttpError::UnsuccessfulRequest(response)) => {
            Some(response.error.code as u64)
        }
        _ => None,
    }
}

/// Enregistre les commandes si nécessaire (avant la connexion gateway).
pub async fn deploy_commands_if_needed(
    http: &serenity::Http,
    cfg: &Config,
    catalog: &Catalog,
    commands: &[poise::Command<Data, Error>],
) -> Result<(), Error> {
    let built = poise::builtins::create_application_commands(commands);
    let commands_json: Vec<Value> = built
        .iter()
        .map(|c| serde_json::to_value(c).unwrap_or(Value::Null))
        .collect();

    // ── Purge des commandes GLOBALES (à chaque démarrage) ──
    match http
        .create_global_commands(&Vec::<serenity::CreateCommand>::new())
        .await
    {
        Ok(_) => logger::info(catalog.audit.commands_global_purged),
        Err(e) => logger::warn(fill(
            catalog.audit.commands_global_purge_failed,
            &[("error", &e.to_string())],
        )),
    }

    // ── Skip si rien n'a changé ──
    let hash = commands_hash(cfg, &commands_json);
    let previous = read_deploy_state(&cfg.guild_id);
    if previous.as_ref().map(|(_, h)| h == &hash).unwrap_or(false) {
        logger::info(catalog.audit.commands_skipped);
        return Ok(());
    }

    // ── Guildes retirées de la config : purge de leurs commandes ──
    if let Some((old_guilds, _)) = &previous {
        for old_guild in old_guilds {
            if cfg.guild_ids.contains(old_guild) {
                continue;
            }
            if let Ok(old_id) = old_guild.parse::<u64>() {
                let purge = serenity::GuildId::new(old_id)
                    .set_commands(http, Vec::<serenity::CreateCommand>::new())
                    .await;
                match purge {
                    Ok(_) => logger::info(fill(
                        catalog.audit.commands_old_guild_purged,
                        &[("guild", old_guild.as_str())],
                    )),
                    Err(e) => logger::warn(fill(
                        catalog.audit.commands_old_guild_purge_failed,
                        &[("guild", old_guild), ("error", &e.to_string())],
                    )),
                }
            }
        }
    }

    // ── Enregistrement groupé dans CHAQUE guilde configurée ──
    for guild in &cfg.guild_ids {
        let guild_id = match guild.parse::<u64>() {
            Ok(id) => serenity::GuildId::new(id),
            Err(e) => {
                return Err(ExitError::config(format!(
                    "DISCORD_GUILD_IDS : guilde invalide {guild} ({e})"
                ))
                .into());
            }
        };
        if let Err(e) = guild_id.set_commands(http, built.clone()).await {
            match discord_error_code(&e) {
                Some(50001) => {
                    return Err(ExitError::config(fill(
                        catalog.errors.deploy_missing_access,
                        &[
                            ("guild", guild.as_str()),
                            ("client", cfg.client_id.as_str()),
                        ],
                    ))
                    .into());
                }
                Some(10002) => {
                    return Err(ExitError::config(fill(
                        catalog.errors.deploy_unknown_application,
                        &[("client", cfg.client_id.as_str())],
                    ))
                    .into());
                }
                _ => return Err(e.into()),
            }
        }
    }
    write_deploy_state(&cfg.guild_ids, &hash);
    logger::info(catalog.audit.commands_registered);
    Ok(())
}
