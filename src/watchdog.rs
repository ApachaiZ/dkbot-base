//! Watchdog (module `watchdog`) : surveillance périodique de TOUTES les
//! cibles × services configurés.
//!
//! À chaque tick :
//! - **crash** : transition `running → non-running` d'un service (hors
//!   action volontaire : verrou actif ou arrêt récent respectés) ;
//! - **retour** : un service vu hors ligne revient à `running` ;
//! - **machine injoignable** : aucune collecte possible sur une cible
//!   (alerte, puis **retour de la machine** à la reconnexion) ;
//! - **ressources** : CPU / RAM / disque au-dessus des seuils configurés
//!   (une alerte par machine, pas par service).
//!
//! Cooldown 15 min par type d'alerte et par clé ; délai de grâce de 5 min
//! après un arrêt volontaire. Les alertes partent dans le salon configuré
//! (`DISCORD_ALERT_CHANNEL_ID`). Le module se désactive À LA VOLÉE
//! (`/modules disable watchdog`) : le tick suivant est simplement sauté.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use poise::serenity_prelude as serenity;
use serenity::{ChannelId, Http};

use crate::api::Api;
use crate::config::Config;
use crate::embeds::{card, state, Tone};
use crate::i18n::fill;
use crate::logger;
use crate::modules::MODULE_WATCHDOG;
use crate::power;
use crate::state::State;
use crate::status::display_name;

const DEFAULT_INTERVAL: Duration = Duration::from_secs(60);
const ALERT_COOLDOWN: Duration = Duration::from_secs(15 * 60);
const CRASH_GRACE: Duration = Duration::from_secs(5 * 60);

#[derive(Default)]
struct WatchdogState {
    /// Dernier état vu par clé `cible/service`.
    last_state: HashMap<String, String>,
    /// Cibles actuellement injoignables (pour alerter le RETOUR).
    unreachable: HashMap<String, bool>,
    /// Dernière alerte par clé d'incident (cooldown).
    last_alert: HashMap<String, Instant>,
}

impl WatchdogState {
    fn can_alert(&mut self, key: &str) -> bool {
        let now = Instant::now();
        match self.last_alert.get(key) {
            Some(at) if now.duration_since(*at) < ALERT_COOLDOWN => false,
            _ => {
                self.last_alert.insert(key.to_string(), now);
                true
            }
        }
    }
}

struct CheckEnv {
    http: Arc<Http>,
    channel: ChannelId,
    shared: Arc<Mutex<WatchdogState>>,
}

/// Démarre la boucle (tâche détachée) — premier scan 15 s après le Ready.
pub fn start(http: Arc<Http>, cfg: Config, api: Arc<Api>, st: Arc<State>) {
    let catalog = cfg.language_catalog();
    let Some(channel_id) = cfg.alert_channel_id.clone() else {
        logger::info(catalog.audit.watchdog_disabled);
        return;
    };
    let Ok(channel) = channel_id.parse::<u64>() else {
        logger::info(catalog.audit.watchdog_disabled);
        return;
    };
    let interval_ms = cfg
        .alert_interval_ms
        .filter(|v| *v > 0)
        .unwrap_or_else(|| DEFAULT_INTERVAL.as_millis() as u64);
    logger::info(fill(
        catalog.audit.watchdog_started,
        &[
            ("interval", &interval_ms.to_string()),
            ("channel", &channel_id),
        ],
    ));

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(15)).await;
        let env = CheckEnv {
            http,
            channel: ChannelId::new(channel),
            shared: Arc::new(Mutex::new(WatchdogState::default())),
        };
        let mut ticker = tokio::time::interval(Duration::from_millis(interval_ms));
        loop {
            ticker.tick().await;
            // Module désactivé à la volée : le tick est sauté — la boucle
            // reste vivante pour un ré-engagement tout aussi immédiat.
            if !st.module_enabled(MODULE_WATCHDOG) {
                continue;
            }
            let checks: Vec<_> = cfg
                .targets
                .iter()
                .map(|t| check_target(&cfg, api.clone(), st.clone(), &env, t.alias.clone()))
                .collect();
            futures::future::join_all(checks).await;
        }
    });
}

/// Une cible : collectes des services, alertes machine et alertes par
/// service. La première collecte réussie porte les métriques de la machine.
async fn check_target(cfg: &Config, api: Arc<Api>, st: Arc<State>, env: &CheckEnv, target: String) {
    let catalog = cfg.language_catalog();

    let mut statuses: Vec<(String, crate::status::Status)> = Vec::new();
    let mut first_error: Option<String> = None;
    let mut failed: Vec<(String, String)> = Vec::new();
    for (alias, _) in &cfg.services {
        match api.get(&target, alias, false).await {
            Ok(status) => statuses.push((alias.clone(), status)),
            Err(e) => {
                if first_error.is_none() {
                    first_error = Some(e.message.clone());
                }
                failed.push((alias.clone(), e.message.clone()));
                logger::warn(fill(
                    catalog.audit.watchdog_check_failed,
                    &[("service", alias.as_str()), ("error", &e.message)],
                ));
            }
        }
    }
    // Collectes partielles : alerter par service défaillant (cooldown) —
    // un échec persistant ne doit pas rester silencieux.
    for (alias, error) in &failed {
        let key = format!("{target}/{alias}:collect");
        let should_alert = env.shared.lock().expect("watchdog state").can_alert(&key);
        if should_alert {
            let c = catalog.watchdog.collect_failed;
            send_alert(
                &env.http,
                env.channel,
                &fill(c.title, &[("service", alias)]),
                &fill(c.body, &[("target", &target), ("error", error)]),
                Tone::Wait,
                catalog,
            )
            .await;
        }
    }

    // ── Machine injoignable / de retour ──
    if statuses.is_empty() {
        let key = format!("target:{target}:unreachable");
        let should_alert = {
            let mut shared = env.shared.lock().expect("watchdog state");
            shared.unreachable.insert(target.clone(), true);
            shared.can_alert(&key)
        };
        if should_alert {
            let c = catalog.watchdog.unreachable;
            send_alert(
                &env.http,
                env.channel,
                &fill(c.title, &[("target", &target)]),
                &fill(
                    c.body,
                    &[("error", first_error.as_deref().unwrap_or("no response"))],
                ),
                Tone::Bad,
                catalog,
            )
            .await;
        }
        return;
    }
    let was_unreachable = {
        let mut shared = env.shared.lock().expect("watchdog state");
        shared.unreachable.remove(&target) == Some(true)
    };
    if was_unreachable {
        let c = catalog.watchdog.target_back;
        send_alert(
            &env.http,
            env.channel,
            &fill(c.title, &[("target", &target)]),
            c.body,
            Tone::Good,
            catalog,
        )
        .await;
    }

    // ── Ressources de la MACHINE (une alerte par cible, pas par service) ──
    let machine = &statuses[0].1;
    check_resources(cfg, env, &target, machine, catalog).await;

    // ── État de chaque service ──
    for (alias, status) in statuses {
        check_service(cfg, st.clone(), env, &target, &alias, &status).await;
    }
}

/// Alertes CPU / RAM / disque d'une machine (clés par cible).
async fn check_resources(
    cfg: &Config,
    env: &CheckEnv,
    target: &str,
    status: &crate::status::Status,
    catalog: &crate::i18n::Catalog,
) {
    let mut alerts: Vec<(String, String, Tone)> = Vec::new();
    {
        let mut shared = env.shared.lock().expect("watchdog state");
        if let Some(cpu) = status.cpu_pct {
            if cpu > cfg.watchdog_cpu_pct && shared.can_alert(&format!("{target}:cpu")) {
                let c = catalog.watchdog.high_cpu;
                alerts.push((
                    fill(c.title, &[("name", target)]),
                    fill(
                        c.body,
                        &[
                            ("pct", &format!("{cpu:.1}")),
                            ("threshold", &cfg.watchdog_cpu_pct.to_string()),
                        ],
                    ),
                    Tone::Wait,
                ));
            }
        }
        if let (Some(used), Some(max)) = (status.ram_mb, status.ram_max_mb) {
            let pct = if max > 0.0 { used / max * 100.0 } else { 0.0 };
            if pct > cfg.watchdog_ram_pct && shared.can_alert(&format!("{target}:ram")) {
                let c = catalog.watchdog.high_ram;
                alerts.push((
                    fill(c.title, &[("name", target)]),
                    fill(
                        c.body,
                        &[
                            ("pct", &format!("{pct:.1}")),
                            ("used", &format!("{used:.0}")),
                            ("max", &format!("{max:.0}")),
                        ],
                    ),
                    Tone::Wait,
                ));
            }
        }
        if let (Some(used), Some(max)) = (status.disk_mb, status.disk_max_mb) {
            let pct = if max > 0.0 { used / max * 100.0 } else { 0.0 };
            if pct > cfg.watchdog_disk_pct && shared.can_alert(&format!("{target}:disk")) {
                let c = catalog.watchdog.high_disk;
                alerts.push((
                    fill(c.title, &[("name", target)]),
                    fill(
                        c.body,
                        &[
                            ("pct", &format!("{pct:.1}")),
                            ("used", &format!("{used:.0}")),
                            ("max", &format!("{max:.0}")),
                        ],
                    ),
                    Tone::Wait,
                ));
            }
        }
    }
    for (title, body, tone) in alerts {
        send_alert(&env.http, env.channel, &title, &body, tone, catalog).await;
    }
}

/// Crash et retour d'un service (clés par `cible/service`).
async fn check_service(
    cfg: &Config,
    st: Arc<State>,
    env: &CheckEnv,
    target: &str,
    alias: &str,
    status: &crate::status::Status,
) {
    let catalog = cfg.language_catalog();
    let name = display_name(status, alias);
    let current_state = state(status);
    let state_key = format!("{target}/{alias}");
    let lock_key = power::lock_key(target, alias);
    let address = status
        .address
        .as_ref()
        .map(|a| match a.port {
            Some(port) => format!("{}:{port}", a.ip),
            None => a.ip.clone(),
        })
        .unwrap_or_else(|| "unknown".to_string());

    let mut alerts: Vec<(String, String, Tone)> = Vec::new();
    {
        let mut shared = env.shared.lock().expect("watchdog state");
        let prev = shared.last_state.get(&state_key).cloned();

        // Crash : transition running → non-running, hors action volontaire.
        if prev.as_deref() == Some("running") && current_state != "running" {
            let locked = st.read_lock_snapshot().contains(&lock_key);
            let in_grace = st
                .lock_release_elapsed(&lock_key)
                .map(|elapsed| elapsed < CRASH_GRACE)
                .unwrap_or(false);
            if !locked && !in_grace && shared.can_alert(&format!("{state_key}:crash")) {
                let c = catalog.watchdog.down;
                alerts.push((
                    fill(c.title, &[("name", &name)]),
                    fill(c.body, &[("state", &current_state), ("address", &address)]),
                    Tone::Bad,
                ));
            }
        }

        // Retour : vu ARRÊTÉ (inactive/failed) → running.
        if prev.as_deref().is_some()
            && matches!(prev.as_deref(), Some("inactive") | Some("failed"))
            && current_state == "running"
            && shared.can_alert(&format!("{state_key}:recovered"))
        {
            let c = catalog.watchdog.recovered;
            alerts.push((
                fill(c.title, &[("name", &name)]),
                fill(c.body, &[("state", &current_state), ("target", target)]),
                Tone::Good,
            ));
        }

        shared.last_state.insert(state_key, current_state);
    }
    for (title, body, tone) in alerts {
        send_alert(&env.http, env.channel, &title, &body, tone, catalog).await;
    }
}

async fn send_alert(
    http: &Http,
    channel: ChannelId,
    title: &str,
    body: &str,
    tone: Tone,
    catalog: &crate::i18n::Catalog,
) {
    let message = serenity::CreateMessage::new()
        .add_embed(card(title, body, tone, Some("DAKKABOT • WATCHDOG")))
        .allowed_mentions(serenity::CreateAllowedMentions::new());
    match channel.send_message(http, message).await {
        Ok(_) => logger::warn(fill(catalog.audit.watchdog_alert_sent, &[("title", title)])),
        Err(e) => logger::error(fill(
            catalog.audit.watchdog_alert_failed,
            &[("error", &e.to_string())],
        )),
    }
}
