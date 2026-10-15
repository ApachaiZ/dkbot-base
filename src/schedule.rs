//! Planificateur (module `scheduler`) : actions power récurrentes ou
//! ponctuelles exécutées SANS interaction Discord.
//!
//! - `/schedule add <when> <action> [target] [service]` (owner) ;
//! - formats de `when` : `daily HH:MM`, `weekly D HH:MM` (D : 1=lundi …
//!   7=dimanche), `once AAAA-MM-JJ HH:MM` (retirée après exécution) ;
//! - l'exécution réutilise [`crate::power::run_headless`] : verrou
//!   anti-collision, UNE commande jamais répétée, suivi 2 états stables,
//!   sauvegarde à l'arrêt avéré — puis notification dans le canal d'audit ;
//! - « à l'heure dite » : l'entrée court si le bot était arrêté à l'heure
//!   prévue (rattrapage le même jour), jamais après minuit ;
//! - module désactivable À LA VOLÉE : le tick suivant est sauté.
//!
//! Stockage : `schedule.json` (pas de secret, écriture atomique 0600).

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Utc, Weekday};
use poise::serenity_prelude as serenity;
use serde::{Deserialize, Serialize};

use crate::commands::{reply, Context, Error};
use crate::embeds::{card, Tone};
use crate::i18n::{fill, Catalog};
use crate::modules::MODULE_SCHEDULER;
use crate::power::{self, Action};
use crate::state::State;

const TICK: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: u64,
    /// Spécification textuelle (`daily 04:00`, `weekly 2 04:00`,
    /// `once 2026-10-20 04:00`).
    pub when: String,
    /// `start` | `stop` | `restart`.
    pub action: String,
    /// Alias de cible.
    pub target: String,
    /// Alias de service.
    pub service: String,
    /// Dernier jour d'exécution (`AAAA-MM-JJ`) — anti-répétition.
    pub last_run: Option<String>,
    /// Création (RFC3339) — une entrée créée aujourd'hui APRÈS son heure
    /// ne court pas le jour même (compat : `None` = comportement hérité).
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Spec {
    Daily(NaiveTime),
    Weekly(Weekday, NaiveTime),
    Once(DateTime<Utc>),
}

/// Parse et valide une spécification `when`. `Err` = message de refus.
pub(crate) fn parse_when(raw: &str) -> Result<Spec, String> {
    let parts: Vec<&str> = raw.split_whitespace().collect();
    match parts.as_slice() {
        ["daily", hm] => parse_hm(hm).map(Spec::Daily),
        ["weekly", d, hm] => {
            let day: u32 = d
                .parse()
                .map_err(|_| format!("jour invalide : {d} (attendu 1-7)"))?;
            let weekday = match day {
                1 => Weekday::Mon,
                2 => Weekday::Tue,
                3 => Weekday::Wed,
                4 => Weekday::Thu,
                5 => Weekday::Fri,
                6 => Weekday::Sat,
                7 => Weekday::Sun,
                _ => return Err(format!("jour invalide : {d} (attendu 1-7)")),
            };
            Ok(Spec::Weekly(weekday, parse_hm(hm)?))
        }
        ["once", date, hm] => {
            let date = NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .map_err(|_| format!("date invalide : {date} (attendu AAAA-MM-JJ)"))?;
            let time = parse_hm(hm)?;
            let dt = date.and_time(time).and_utc();
            Ok(Spec::Once(dt))
        }
        _ => Err("format attendu : daily HH:MM, weekly D HH:MM ou once AAAA-MM-JJ HH:MM".into()),
    }
}

fn parse_hm(hm: &str) -> Result<NaiveTime, String> {
    NaiveTime::parse_from_str(hm, "%H:%M")
        .map_err(|_| format!("heure invalide : {hm} (attendu HH:MM)"))
}

/// L'entrée doit-elle s'exécuter MAINTENANT ? (fonction pure, testée)
///
/// - `once` : UNIQUEMENT le jour prévu (jamais de rattrapage après la
///   date — une entrée manquée expire) ;
/// - `daily`/`weekly` : à partir de l'heure prévue, si l'entrée n'a pas
///   déjà couru aujourd'hui ET si elle a été créée AVANT l'occurrence du
///   jour (une entrée créée aujourd'hui après son heure attend demain) ;
///   le rattrapage couvre le jour du retour après une panne.
pub(crate) fn is_due(entry: &Entry, now: DateTime<Utc>) -> bool {
    let Ok(spec) = parse_when(&entry.when) else {
        return false;
    };
    let today = now.format("%Y-%m-%d").to_string();
    if entry.last_run.as_deref() == Some(today.as_str()) {
        return false;
    }
    match spec {
        Spec::Once(dt) => now >= dt && now.date_naive() == dt.date_naive(),
        Spec::Daily(time) => {
            if now.time() < time {
                return false;
            }
            created_before_today(entry, now, time)
        }
        Spec::Weekly(day, time) => {
            if now.weekday() != day || now.time() < time {
                return false;
            }
            created_before_today(entry, now, time)
        }
    }
}

/// L'entrée a-t-elle été créée AVANT l'occurrence d'aujourd'hui ?
fn created_before_today(entry: &Entry, now: DateTime<Utc>, time: chrono::NaiveTime) -> bool {
    match entry.created_at.as_deref() {
        None => true, // ancien format : comportement hérité (rattrapage)
        Some(raw) => match DateTime::parse_from_rfc3339(raw) {
            Ok(created) => {
                created.date_naive() < now.date_naive()
                    || (created.date_naive() == now.date_naive() && created.time() < time)
            }
            Err(_) => true,
        },
    }
}

/// Une entrée `once` dont la date est passée est EXPIRÉE (retirée au tick,
/// jamais exécutée en retard).
pub(crate) fn is_expired(entry: &Entry, now: DateTime<Utc>) -> bool {
    matches!(parse_when(&entry.when), Ok(Spec::Once(dt)) if now.date_naive() > dt.date_naive())
}

/// Charge les entrées persistées (tolérant : jamais d'échec au démarrage).
pub fn load() -> Vec<Entry> {
    let path = crate::paths::schedule_file();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Persiste les entrées (appelé par [`crate::state::State`] après
/// mutation).
pub(crate) fn save(entries: &[Entry]) {
    let Ok(json) = serde_json::to_string(entries) else {
        return;
    };
    let _ = crate::fsutil::secure_write(&crate::paths::schedule_file(), json.as_bytes());
}

fn parse_action(raw: &str) -> Option<Action> {
    match raw {
        "start" => Some(Action::Start),
        "stop" => Some(Action::Stop),
        "restart" => Some(Action::Restart),
        _ => None,
    }
}

/// Boucle du planificateur (tâche détachée, premier tick 30 s après Ready).
pub fn start(
    http: Arc<serenity::Http>,
    cfg: crate::config::Config,
    api: Arc<crate::api::Api>,
    st: Arc<State>,
) {
    tokio::spawn(async move {
        let catalog = cfg.language_catalog();
        let mut ticker = tokio::time::interval(TICK);
        loop {
            ticker.tick().await;
            if !st.module_enabled(MODULE_SCHEDULER) {
                continue; // désactivé à la volée : tick sauté
            }
            let now = Utc::now();
            // Entrées `once` expirées : retirées, jamais exécutées en retard.
            let snapshot = st.schedule_snapshot();
            for entry in snapshot.iter().filter(|e| is_expired(e, now)) {
                st.schedule_remove(entry.id);
                crate::logger::info(format!(
                    "scheduler: #{} expired ({}) — removed",
                    entry.id, entry.when
                ));
            }
            let due: Vec<Entry> = {
                let mut entries = st.schedule_snapshot();
                entries.retain(|e| is_due(e, now));
                entries
            };
            for entry in due {
                // Marquage AVANT l'exécution : une exécution longue n'est
                // jamais re-dispatchée au tick suivant (pas de faux échec
                // « verrou détenu », pas de double commande).
                let once = matches!(parse_when(&entry.when), Ok(Spec::Once(_)));
                st.schedule_mark_run(entry.id);
                execute_one(&http, &cfg, &api, &st, catalog, &entry).await;
                if once {
                    st.schedule_remove(entry.id);
                }
            }
        }
    });
}

async fn execute_one(
    http: &serenity::Http,
    cfg: &crate::config::Config,
    api: &Arc<crate::api::Api>,
    st: &Arc<State>,
    catalog: &Catalog,
    entry: &Entry,
) {
    let Some(action) = parse_action(&entry.action) else {
        return;
    };
    crate::logger::info(format!(
        "scheduler: executing #{} ({action} on {}:{})",
        entry.id,
        entry.target,
        entry.service,
        action = action.as_str(),
    ));
    let result = power::run_headless(api, st, catalog, action, &entry.target, &entry.service).await;
    let title = catalog.schedule.list_title;
    match result {
        Ok(summary) => {
            crate::logger::info(format!("scheduler: #{} ok — {summary}", entry.id));
            crate::notify::audit(
                http,
                cfg,
                catalog,
                title,
                &fill(
                    catalog.schedule.audit_ok,
                    &[
                        ("action", &entry.action),
                        ("target", &entry.target),
                        ("service", &entry.service),
                        ("summary", &summary),
                    ],
                ),
                Tone::Good,
            )
            .await;
        }
        Err(error) => {
            crate::logger::error(format!("scheduler: #{} failed — {error}", entry.id));
            crate::notify::audit(
                http,
                cfg,
                catalog,
                title,
                &fill(
                    catalog.schedule.audit_fail,
                    &[
                        ("action", &entry.action),
                        ("target", &entry.target),
                        ("service", &entry.service),
                        ("error", &error),
                    ],
                ),
                Tone::Bad,
            )
            .await;
        }
    }
}

// ── Commandes Discord (owner) ─────────────────────────────────────────────

async fn require_owner(ctx: &Context<'_>) -> Result<bool, Error> {
    if ctx.data().state.is_owner(&ctx.author().id.to_string()) {
        return Ok(true);
    }
    let c = ctx.data().catalog.schedule.access_denied;
    ctx.send(reply(card(c.title, c.body, Tone::Bad, None)))
        .await?;
    Ok(false)
}

/// `/schedule` — liste les actions planifiées.
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    if !require_owner(&ctx).await? {
        return Ok(());
    }
    let catalog = ctx.data().catalog;
    let entries = ctx.data().state.schedule_snapshot();
    let mut body = String::new();
    if entries.is_empty() {
        body.push_str(catalog.schedule.empty);
    } else {
        for e in entries {
            body.push_str(&format!(
                "**#{id}** `{when}` → **{action}** sur `{target}/{service}`{done}\n",
                id = e.id,
                when = e.when,
                action = e.action,
                target = e.target,
                service = e.service,
                done = e
                    .last_run
                    .as_ref()
                    .map(|d| format!(" — dernier run : {d}"))
                    .unwrap_or_default(),
            ));
        }
    }
    ctx.send(reply(card(
        catalog.schedule.list_title,
        &body,
        Tone::Good,
        None,
    )))
    .await?;
    Ok(())
}

/// `/schedule add` — ajoute une entrée.
pub async fn add(
    ctx: Context<'_>,
    when: String,
    action: String,
    target: Option<String>,
    service: Option<String>,
) -> Result<(), Error> {
    if !require_owner(&ctx).await? {
        return Ok(());
    }
    let catalog = ctx.data().catalog;
    let title = catalog.schedule.list_title;
    // Validation AVANT toute écriture : une seule carte d'erreur possible.
    let error: Option<String> = if parse_when(&when).is_err() {
        Some(catalog.schedule.invalid_when.to_string())
    } else if matches!(parse_when(&when), Ok(Spec::Once(dt)) if dt <= Utc::now()) {
        Some(fill(catalog.schedule.invalid_past, &[("when", &when)]))
    } else if parse_action(&action).is_none() {
        Some(fill(
            catalog.schedule.invalid_action,
            &[("action", &action)],
        ))
    } else {
        None
    };
    let target = target.unwrap_or_else(|| {
        ctx.data()
            .cfg
            .targets
            .first()
            .map(|t| t.alias.clone())
            .unwrap_or_default()
    });
    let service = service.unwrap_or_else(|| {
        ctx.data()
            .cfg
            .services
            .first()
            .map(|(a, _)| a.clone())
            .unwrap_or_default()
    });
    let error = error.or_else(|| {
        if !ctx.data().cfg.targets.iter().any(|t| t.alias == target) {
            Some(fill(
                catalog.schedule.unknown_target,
                &[("target", &target)],
            ))
        } else if !ctx.data().cfg.services.iter().any(|(a, _)| *a == service) {
            Some(fill(
                catalog.schedule.unknown_service,
                &[("service", &service)],
            ))
        } else {
            None
        }
    });
    if let Some(msg) = error {
        ctx.send(reply(card(title, &msg, Tone::Bad, None))).await?;
        return Ok(());
    }

    let id = ctx.data().state.schedule_add(Entry {
        id: 0, // recalculé par schedule_add
        when: when.clone(),
        action: action.clone(),
        target: target.clone(),
        service: service.clone(),
        last_run: None,
        created_at: Some(Utc::now().to_rfc3339()),
    });
    let c = ctx.data().catalog.schedule;
    ctx.send(reply(card(
        c.list_title,
        &fill(
            c.added,
            &[
                ("id", &id.to_string()),
                ("when", &when),
                ("action", &action),
                ("target", &target),
                ("service", &service),
            ],
        ),
        Tone::Good,
        None,
    )))
    .await?;
    Ok(())
}

/// `/schedule remove` — retire une entrée.
pub async fn remove(ctx: Context<'_>, id: u64) -> Result<(), Error> {
    if !require_owner(&ctx).await? {
        return Ok(());
    }
    let catalog = ctx.data().catalog;
    let removed = ctx.data().state.schedule_remove(id);
    let text = if removed {
        catalog.schedule.removed
    } else {
        catalog.schedule.unknown_id
    };
    ctx.send(reply(card(
        catalog.schedule.list_title,
        &fill(text, &[("id", &id.to_string())]),
        if removed { Tone::Good } else { Tone::Bad },
        None,
    )))
    .await?;
    Ok(())
}

/// `/schedule clear` — vide le planificateur.
pub async fn clear(ctx: Context<'_>) -> Result<(), Error> {
    if !require_owner(&ctx).await? {
        return Ok(());
    }
    ctx.data().state.schedule_clear();
    let c = ctx.data().catalog.schedule;
    ctx.send(reply(card(c.list_title, c.cleared, Tone::Good, None)))
        .await?;
    Ok(())
}

/// Autocomplétion de l'action.
pub fn action_autocomplete(_ctx: Context<'_>, partial: &str) -> Vec<serenity::AutocompleteChoice> {
    ["start", "stop", "restart"]
        .iter()
        .filter(|a| a.contains(partial))
        .map(|a| serenity::AutocompleteChoice::new((*a).to_string(), *a))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(when: &str, last_run: Option<&str>) -> Entry {
        Entry {
            id: 1,
            when: when.to_string(),
            action: "restart".to_string(),
            target: "local".to_string(),
            service: "game".to_string(),
            last_run: last_run.map(str::to_string),
            created_at: Some("2026-10-01T00:00:00+00:00".to_string()),
        }
    }

    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        chrono::NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
            .and_utc()
    }

    #[test]
    fn spec_parsing_accepts_and_rejects() {
        assert_eq!(
            parse_when("daily 04:00"),
            Ok(Spec::Daily(NaiveTime::from_hms_opt(4, 0, 0).unwrap()))
        );
        assert!(parse_when("daily 25:00").is_err());
        assert!(parse_when("every 2h").is_err());
        assert!(parse_when("weekly 9 04:00").is_err());
        assert_eq!(
            parse_when("weekly 2 04:00"),
            Ok(Spec::Weekly(
                Weekday::Tue,
                NaiveTime::from_hms_opt(4, 0, 0).unwrap()
            ))
        );
        assert!(parse_when("once 2026-10-20 04:00").is_ok());
        assert!(parse_when("once 20/10/2026 04:00").is_err());
    }

    #[test]
    fn daily_runs_after_time_and_once_per_day() {
        let e = entry("daily 04:00", None);
        assert!(is_due(&e, utc(2026, 10, 13, 4, 0)));
        assert!(is_due(&e, utc(2026, 10, 13, 23, 59))); // rattrapage le jour même
        assert!(!is_due(&e, utc(2026, 10, 13, 3, 59)));
        let ran = entry("daily 04:00", Some("2026-10-13"));
        assert!(!is_due(&ran, utc(2026, 10, 13, 20, 0))); // déjà exécuté aujourd'hui
        assert!(is_due(&ran, utc(2026, 10, 14, 4, 1))); // jour suivant
    }

    #[test]
    fn weekly_runs_only_on_its_day() {
        // 2026-10-13 = mardi (2).
        let e = entry("weekly 2 04:00", None);
        assert!(is_due(&e, utc(2026, 10, 13, 5, 0)));
        assert!(!is_due(&e, utc(2026, 10, 14, 5, 0))); // mercredi
        assert!(!is_due(&e, utc(2026, 10, 13, 2, 0))); // avant l'heure
    }

    #[test]
    fn once_runs_only_on_its_day() {
        let e = entry("once 2026-10-20 04:00", None);
        assert!(!is_due(&e, utc(2026, 10, 19, 23, 59)));
        assert!(is_due(&e, utc(2026, 10, 20, 4, 0)));
        // Jamais APRÈS le jour prévu (une entrée manquée expire)…
        assert!(!is_due(&e, utc(2026, 10, 21, 0, 0)));
        assert!(is_expired(&e, utc(2026, 10, 21, 0, 0)));
        assert!(!is_expired(&e, utc(2026, 10, 20, 23, 0)));
        assert!(!is_due(
            &entry("once 2026-10-20 04:00", Some("2026-10-20")),
            utc(2026, 10, 20, 5, 0)
        ));
    }

    #[test]
    fn created_today_after_time_waits_tomorrow() {
        // Créée le 13 à 10:00 pour « daily 04:00 » : pas de tir le jour
        // même (l'occurrence du jour est passée à la création).
        let mut e = entry("daily 04:00", None);
        e.created_at = Some("2026-10-13T10:00:00+00:00".to_string());
        assert!(!is_due(&e, utc(2026, 10, 13, 23, 0)));
        assert!(is_due(&e, utc(2026, 10, 14, 4, 1)));
        // Créée la veille : rattrapage le jour même autorisé.
        let mut e2 = entry("daily 04:00", None);
        e2.created_at = Some("2026-10-12T10:00:00+00:00".to_string());
        assert!(is_due(&e2, utc(2026, 10, 13, 23, 0)));
    }
}
