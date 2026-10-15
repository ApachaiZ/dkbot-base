//! Canal d'AUDIT : les événements notables (action confirmée, exécution du
//! planificateur, sauvegarde, reboot, changements de modules) partent dans
//! `DISCORD_AUDIT_CHANNEL_ID` — repli : le salon du watchdog. Aucun secret
//! n'y transite jamais ; l'absence de canal = silence (jamais d'erreur).

use poise::serenity_prelude as serenity;
use serenity::Http;

use crate::config::Config;
use crate::embeds::{card, Tone};
use crate::i18n::fill;
use crate::logger;

pub const AUDIT_FOOTER: &str = "DAKKABOT • AUDIT";

/// Envoie une carte d'audit (silencieuse sans canal configuré).
pub async fn audit(
    http: &Http,
    cfg: &Config,
    catalog: &crate::i18n::Catalog,
    title: &str,
    body: &str,
    tone: Tone,
) {
    let Some(channel_id) = cfg.audit_channel_id.as_deref() else {
        return;
    };
    let Ok(channel) = channel_id.parse::<u64>() else {
        return;
    };
    let message = serenity::CreateMessage::new()
        .add_embed(card(title, body, tone, Some(AUDIT_FOOTER)))
        .allowed_mentions(serenity::CreateAllowedMentions::new());
    match serenity::ChannelId::new(channel)
        .send_message(http, message)
        .await
    {
        Ok(_) => logger::info(fill(catalog.audit.notify_sent, &[("title", title)])),
        Err(e) => logger::error(fill(
            catalog.audit.notify_failed,
            &[("error", &e.to_string())],
        )),
    }
}
