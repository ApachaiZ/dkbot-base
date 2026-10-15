//! Roster des membres de la guilde (autocomplétion de `/users`).
//!
//! Politique de cooldown opcode 8 de Discord (un chargement complet de
//! membres par guilde par tranche de 10 min), persistée : le jeton de date
//! survit aux redémarrages (`members.state.json`). Le roster est chiffré
//! (`members.roster.json`, AES-256-GCM) et rejeté au-delà de 24 h.

use std::time::Duration;

use poise::serenity_prelude as serenity;

use crate::commands::{Context, Error};
use crate::crypto;
use crate::logger;
use crate::paths;

/// Cooldown d'un chargement complet des membres (opcode 8).
const COOLDOWN: Duration = Duration::from_secs(10 * 60);

/// Un roster persisté plus vieux n'est plus proposé.
const ROSTER_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        * 1000.0
}

#[derive(serde::Deserialize, serde::Serialize)]
struct Roster {
    #[serde(rename = "savedAt")]
    saved_at: f64,
    members: Vec<RosterMember>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct RosterMember {
    id: String,
    username: String,
}

fn read_state() -> serde_json::Value {
    std::fs::read_to_string(paths::members_state_file())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or(serde_json::Value::Null)
}

fn last_fetch_at() -> f64 {
    read_state()
        .get("fetchedAt")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
}

fn persist_roster(token: &str, mut members: Vec<(String, String)>) {
    members.retain(|(_, name)| !name.trim().is_empty());
    members.sort_by_key(|(_, name)| name.to_lowercase());
    let roster = Roster {
        saved_at: now_ms(),
        members: members
            .into_iter()
            .map(|(id, username)| RosterMember { id, username })
            .collect(),
    };
    if let Err(e) = crypto::write_json(
        token,
        paths::members_roster_file().as_path(),
        &serde_json::to_value(&roster).unwrap_or(serde_json::Value::Null),
    ) {
        logger::warn(format!("Could not persist member roster: {e}"));
    }
}

fn read_persisted_roster(token: &str) -> Vec<(String, String)> {
    let Some(value) = crypto::read_json(token, paths::members_roster_file().as_path()) else {
        return Vec::new();
    };
    let Ok(roster) = serde_json::from_value::<Roster>(value) else {
        return Vec::new();
    };
    if now_ms() - roster.saved_at > ROSTER_MAX_AGE.as_millis() as f64 {
        return Vec::new();
    }
    roster
        .members
        .into_iter()
        .map(|m| (m.id, m.username))
        .collect()
}

fn write_state_file() -> bool {
    crate::fsutil::atomic_write(
        paths::members_state_file().as_path(),
        serde_json::json!({ "fetchedAt": now_ms() })
            .to_string()
            .as_bytes(),
    )
    .is_ok()
}

/// Rafraîchit le roster des membres en respectant le cooldown opcode 8.
/// Retourne `true` si un chargement gateway a eu lieu.
pub async fn refresh_members(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    token: &str,
) -> bool {
    // 1. Le cache Discord en mémoire a déjà des membres → persistance
    //    gratuite, sans toucher au quota gateway.
    let cached: Vec<(String, String)> = guild_id
        .to_guild_cached(ctx)
        .map(|g| {
            g.members
                .iter()
                .filter(|(_, m)| !m.user.bot)
                .map(|(_, m)| (m.user.id.to_string(), m.user.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    if !cached.is_empty() {
        persist_roster(token, cached);
        return false;
    }

    // 2. Cooldown persisté.
    if now_ms() - last_fetch_at() < COOLDOWN.as_millis() as f64 {
        return false;
    }

    // 3. Chargement gateway (RequestGuildMembers).
    match guild_id.members(&ctx.http, None, None).await {
        Ok(members) => {
            let humans: Vec<(String, String)> = members
                .iter()
                .filter(|m| !m.user.bot)
                .map(|m| (m.user.id.to_string(), m.user.name.clone()))
                .collect();
            persist_roster(token, humans);
            write_state_file();
            true
        }
        Err(e) => {
            logger::warn(format!("Could not load guild members (rate limit?): {e}"));
            false
        }
    }
}

#[derive(Clone)]
struct RosterEntry {
    id: String,
    username: String,
    bot: bool,
}

async fn roster_entries(ctx: &Context<'_>, guild_id: serenity::GuildId) -> Vec<RosterEntry> {
    let data = ctx.data();
    // Cache Discord chaud : vérité immédiate.
    if let Some(guild) = guild_id.to_guild_cached(ctx.serenity_context()) {
        let entries: Vec<RosterEntry> = guild
            .members
            .values()
            .map(|m| RosterEntry {
                id: m.user.id.to_string(),
                username: m.user.name.clone(),
                bot: m.user.bot,
            })
            .collect();
        if !entries.is_empty() {
            return entries;
        }
    }
    // Repli : roster persisté (pseudo-membres, jamais des bots).
    read_persisted_roster(&data.cfg.token)
        .into_iter()
        .map(|(id, username)| RosterEntry {
            id,
            username,
            bot: false,
        })
        .collect()
}

/// Autocomplétion de l'option « membre » de `/users add|remove`.
pub async fn member_choices(
    ctx: &Context<'_>,
    partial: &str,
    subcommand: &str,
) -> Result<Vec<serenity::AutocompleteChoice>, Error> {
    let data = ctx.data();
    let guild_id = ctx.guild_id().ok_or_else(|| anyhow::anyhow!("no guild"))?;
    let users = data.state.get_users();
    let valid = |entry: &RosterEntry| {
        !entry.bot
            && !data.state.is_owner(&entry.id)
            && match subcommand {
                "add" => !users.contains(&entry.id),
                _ => users.contains(&entry.id),
            }
    };

    let typed = partial.trim().to_lowercase();
    // Recherche REST (aucun quota gateway) si l'utilisateur tape.
    if !typed.is_empty() {
        if let Ok(members) = guild_id.search_members(ctx.http(), &typed, Some(25)).await {
            let choices: Vec<serenity::AutocompleteChoice> = members
                .into_iter()
                .filter(|m| !m.user.bot)
                .filter(|m| {
                    valid(&RosterEntry {
                        id: m.user.id.to_string(),
                        username: m.user.name.clone(),
                        bot: m.user.bot,
                    })
                })
                .map(|m| {
                    serenity::AutocompleteChoice::new(
                        format!("{} ({})", m.user.name, m.user.id),
                        m.user.id.to_string(),
                    )
                })
                .collect();
            if !choices.is_empty() {
                return Ok(choices);
            }
        }
    }

    // Repli roster local.
    let roster = roster_entries(ctx, guild_id).await;
    let mut entries: Vec<&RosterEntry> = roster
        .iter()
        .filter(|e| valid(e))
        .filter(|e| typed.is_empty() || e.username.to_lowercase().contains(&typed))
        .collect();
    entries.sort_by_key(|e| e.username.to_lowercase());
    Ok(entries
        .into_iter()
        .take(25)
        .map(|e| {
            serenity::AutocompleteChoice::new(format!("{} ({})", e.username, e.id), e.id.clone())
        })
        .collect())
}
