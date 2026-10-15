//! Commande `/logs` (owner uniquement) : dernières lignes de log du
//! service (journald par défaut, fichier si `LOG_FILE`), avec filtre.

use crate::commands::{Context, Error};
use crate::embeds::{card, Tone};
use crate::i18n::fill;
use crate::manage;

/// Taille maximale du corps d'embed (les logs sont tronqués au-delà).
const MAX_BODY_CHARS: usize = 1800;

/// Garde le début + la fin des lignes pour tenir dans un embed.
fn truncate_tail(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let head: String = text.chars().take(max_chars / 2).collect();
    let tail: String = text
        .chars()
        .rev()
        .take(max_chars / 2)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{head}\n…\n{tail}")
}

/// `/logs` — résolution cible/service, lecture, filtre, carte.
pub async fn logs(
    ctx: &Context<'_>,
    target: Option<String>,
    service: Option<String>,
    filter: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    if !manage::ensure_owner(ctx).await? {
        return Ok(());
    }
    let catalog = ctx.data().catalog;
    let Some((target_alias, _)) =
        crate::commands::resolve_target_opt(ctx, target.as_deref()).await?
    else {
        return Ok(());
    };
    let Some((alias, _)) = crate::commands::resolve_service_opt(ctx, service.as_deref()).await?
    else {
        return Ok(());
    };

    let raw = match ctx.data().api.logs(&target_alias, &alias).await {
        Ok(raw) => raw,
        Err(err) => {
            let c = catalog.logs;
            // `journalctl` absent / commande refusée / réseau : carte dédiée.
            let body = if err.kind == crate::errors::BackendErrorKind::Rejected
                && raw_looks_like_no_systemd(&err.message)
            {
                c.no_systemd.to_string()
            } else {
                fill(c.read_failed, &[("message", &err.message)])
            };
            ctx.send(crate::commands::reply(card(
                c.card_title,
                &body,
                Tone::Bad,
                None,
            )))
            .await?;
            return Ok(());
        }
    };

    let lines: Vec<&str> = raw.lines().collect();
    if lines.is_empty() || (lines.len() == 1 && lines[0].trim().is_empty()) {
        ctx.send(crate::commands::reply(card(
            catalog.logs.card_title,
            catalog.logs.empty,
            Tone::Info,
            None,
        )))
        .await?;
        return Ok(());
    }

    let (title, body): (String, String) =
        match filter.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
            Some(f) => {
                let needle = f.to_lowercase();
                let matching: Vec<&str> = lines
                    .iter()
                    .filter(|l| l.to_lowercase().contains(&needle))
                    .copied()
                    .collect();
                if matching.is_empty() {
                    let body = fill(catalog.logs.nothing_filtered, &[("filter", f)]);
                    (catalog.logs.card_title.to_string(), body)
                } else {
                    let count = matching.len();
                    let title = fill(
                        catalog.logs.title_filtered,
                        &[("count", &count.to_string()), ("filter", f)],
                    );
                    (title, format_log_body(&matching))
                }
            }
            None => {
                let count = lines.len();
                let title = fill(catalog.logs.title_last, &[("count", &count.to_string())]);
                (title, format_log_body(&lines))
            }
        };

    ctx.send(crate::commands::reply(card(
        &title,
        &body,
        Tone::Info,
        None,
    )))
    .await?;
    Ok(())
}

/// Corps d'embed : lignes en bloc code, tronquées au besoin.
fn format_log_body(lines: &[&str]) -> String {
    let joined = lines.join("\n");
    let truncated = truncate_tail(&joined, MAX_BODY_CHARS);
    let note = if truncated != joined {
        "\n*(sortie tronquée)*"
    } else {
        ""
    };
    format!("```\n{truncated}\n```{note}")
}

/// Détecte une erreur « journalctl : commande introuvable ».
fn raw_looks_like_no_systemd(message: &str) -> bool {
    let m = message.to_lowercase();
    m.contains("not found") || m.contains("no such file") || m.contains("command not found")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_keeps_short_text() {
        let s = "ligne\nligne";
        assert_eq!(truncate_tail(s, 100), s);
    }

    #[test]
    fn truncate_cuts_long_text() {
        let s: String = (0..100).map(|i| format!("ligne {i}\n")).collect();
        let out = truncate_tail(&s, 100);
        assert!(out.chars().count() <= 100 + 3);
        assert!(out.contains('…'));
    }

    #[test]
    fn detects_missing_journalctl() {
        assert!(raw_looks_like_no_systemd(
            "sh: journalctl: command not found"
        ));
        assert!(raw_looks_like_no_systemd("No such file or directory"));
        assert!(!raw_looks_like_no_systemd("Permission denied"));
    }
}
