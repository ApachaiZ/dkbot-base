//! Actions power applicatives (start / stop / restart) et redémarrage
//! machine (reboot).
//!
//! Cycle de vie (chaque étape est une GARANTIE) :
//! 1. permission → verrou `cible:service` → double confirmation ;
//! 2. re-vérification (permission + verrou) APRÈS la confirmation ;
//! 3. pose du verrou persistant (`active.lock`) ;
//! 4. état initial (fresh) ; si déjà dans l'état cible → carte directe ;
//! 5. UNE seule commande systemctl, jamais de retry ;
//! 6. en cas d'erreur de transmission : soit échec immédiat (rien n'est
//!    parti), soit vérification d'état, soit bascule sur le suivi ;
//! 7. suivi silencieux : polling à backoff 5 s → 30 s, détection de
//!    transition / reset d'uptime du service, 2 mesures stables exigées,
//!    10 min max, bouton « Arrêter le suivi » ;
//! 8. relâchement du verrou + délai de grâce anti-fausse-alerte ;
//! 9. copie des sauvegardes (si `SAVES_PATH`) : AVANT la commande pour un
//!    start (service vérifié arrêté), APRÈS arrêt avéré observé pour stop
//!    (fin de suivi). Pas de sauvegarde au restart : décision explicite —
//!    le redémarrage peut être trop court pour observer l'arrêt (et la
//!    copie d'un service en marche serait incohérente). Le script de
//!    copie garde lui-même la garantie « service inactif » (aucune copie,
//!    ou copie supprimée, si le service se relance). Chemin absent ou
//!    copie impossible : note dans la carte Discord + ligne de log, mais
//!    l'action power continue TOUJOURS.
//!
//! Le « reset d'uptime » d'un restart est détecté sur l'uptime du SERVICE
//! (`ActiveEnterTimestampMonotonic`), pas sur celui de la machine : un
//! redémarrage applicatif ne change pas l'uptime système.

use futures::StreamExt;
use poise::serenity_prelude as serenity;
use rand::RngCore;
use serenity::{
    ButtonStyle, CreateActionRow, CreateButton, CreateEmbed, CreateMessage,
    EditInteractionResponse, MessageId,
};
use std::time::{Duration, Instant};

use crate::api::{Api, BackupOutcome};
use crate::assets::{self, Gif};
use crate::commands::{Context, Data, Error};
use crate::confirm;
use crate::embeds::{card, offline, online, state, Tone};
use crate::i18n::{fill, Catalog, PowerAction};
use crate::logger;
use crate::stats;
use crate::status::display_name;
use std::sync::Arc;

/// Backoff progressif : `[5s ×3, 10s ×3, 15s ×2, 20s ×2, puis 30s]`.
const POLL_DELAYS: [Duration; 11] = [
    Duration::from_secs(5),
    Duration::from_secs(5),
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(10),
    Duration::from_secs(10),
    Duration::from_secs(15),
    Duration::from_secs(15),
    Duration::from_secs(20),
    Duration::from_secs(20),
    Duration::from_secs(30),
];

/// Suivi maximal d'une action avant de déclarer la fin non confirmée.
const TRACKING_TIMEOUT: Duration = Duration::from_secs(600);

/// Budget ABSOLU d'une action power, compté depuis la CONFIRMATION : le
/// token de l'interaction Discord expire 15 min après la commande.
const FLOW_BUDGET: Duration = Duration::from_secs(720);

/// Au-delà de 5 échecs de lecture consécutifs, on abandonne le suivi.
const MAX_FAILURES: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Start,
    Stop,
    Restart,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Start => "start",
            Action::Stop => "stop",
            Action::Restart => "restart",
        }
    }

    /// Textes localisés de l'action (verbe, majuscule, titre de confirmation).
    pub fn text(self, catalog: &Catalog) -> PowerAction {
        match self {
            Action::Start => catalog.power.actions.start,
            Action::Stop => catalog.power.actions.stop,
            Action::Restart => catalog.power.actions.restart,
        }
    }
}

/// Ligne d'estimation « ⏱ En moyenne : 51 s » (durée moyenne persistée).
fn eta_line(action: Action, catalog: &Catalog) -> String {
    stats::avg_seconds(action.as_str())
        .map(|seconds| {
            format!(
                "\n{}",
                fill(catalog.power.eta, &[("seconds", &seconds.to_string())])
            )
        })
        .unwrap_or_default()
}

/// Notification privée (DM) : vraie notification push sur l'appareil de
/// l'utilisateur. Si les DMs sont désactivés, échec silencieux — la carte
/// éphémère dans le salon reste le repli.
async fn notify_user(ctx: &Context<'_>, embed: CreateEmbed) {
    let message = CreateMessage::new()
        .add_embed(embed)
        .allowed_mentions(serenity::CreateAllowedMentions::new());
    let _ = ctx
        .author()
        .direct_message(ctx.serenity_context(), message)
        .await;
}

/// Édite la carte de suivi : embed + GIF (optionnel) + boutons, sur la
/// RÉPONSE ORIGINALE de l'interaction (token d'interaction uniquement).
async fn edit_card(
    ctx: &Context<'_>,
    mut embed: CreateEmbed,
    gif: Option<&Gif>,
    components: Vec<CreateActionRow>,
) -> Result<(), Error> {
    let edit = match gif {
        Some(g) if g.attachment.is_some() => {
            embed = embed.image(g.image_url.as_deref().unwrap_or_default());
            EditInteractionResponse::new()
                .embed(embed)
                .new_attachment(g.attachment.as_ref().unwrap().clone())
                .components(components)
        }
        _ => EditInteractionResponse::new()
            .embed(embed)
            .clear_attachments()
            .components(components),
    };
    confirm::edit_original(ctx, edit).await?;
    Ok(())
}

/// Tente la copie des sauvegardes (le service est censé être arrêté) et
/// retourne la ligne localisée à ajouter à la carte finale. Journalise
/// toutes les issues — y compris l'absence de `SAVES_PATH` (demande
/// explicite : l'information doit aussi apparaître dans les logs).
pub(crate) async fn backup_after_stop(
    api: &Arc<Api>,
    catalog: &Catalog,
    target_alias: &str,
    alias: &str,
    service_name: &str,
    deadline: Instant,
) -> Option<String> {
    match api.backup_saves(target_alias, alias, deadline).await {
        BackupOutcome::NotConfigured => {
            logger::warn(fill(
                catalog.audit.backup_missing_path,
                &[("service", service_name), ("target", target_alias)],
            ));
            Some(catalog.power.backup_missing.to_string())
        }
        BackupOutcome::Copied { dest } => {
            logger::info(fill(
                catalog.audit.backup_copied,
                &[
                    ("service", service_name),
                    ("target", target_alias),
                    ("dest", &dest),
                ],
            ));
            Some(fill(catalog.power.backup_ok, &[("dest", &dest)]))
        }
        BackupOutcome::SkippedRunning => {
            logger::warn(fill(
                catalog.audit.backup_skipped,
                &[
                    ("service", service_name),
                    ("target", target_alias),
                    ("reason", "service non arrêté au moment de la copie"),
                ],
            ));
            Some(catalog.power.backup_skipped_running.to_string())
        }
        BackupOutcome::SkippedLocked => {
            logger::warn(fill(
                catalog.audit.backup_skipped,
                &[
                    ("service", service_name),
                    ("target", target_alias),
                    ("reason", "lock de sauvegarde détenu (copie concurrente)"),
                ],
            ));
            Some(catalog.power.backup_locked.to_string())
        }
        BackupOutcome::SkippedFresh => {
            logger::info(fill(
                catalog.audit.backup_skipped,
                &[
                    ("service", service_name),
                    ("target", target_alias),
                    ("reason", "copie datant de moins d'une heure"),
                ],
            ));
            Some(catalog.power.backup_fresh.to_string())
        }
        BackupOutcome::Failed(error) => {
            logger::error(fill(
                catalog.audit.backup_failed,
                &[
                    ("service", service_name),
                    ("target", target_alias),
                    ("error", &error),
                ],
            ));
            Some(fill(catalog.power.backup_failed, &[("error", &error)]))
        }
    }
}

/// Ajoute une note de sauvegarde en fin de corps de carte, si présente.
fn append_note(body: &mut String, note: Option<&str>) {
    if let Some(note) = note {
        body.push('\n');
        body.push_str(note);
    }
}

/// L'état cible est-il atteint ?
/// - `transition` : le service est passé par un état non-running pendant le
///   suivi (cycle de redémarrage observé) ;
/// - `use_initial` : variante de VÉRIFICATION post-erreur (un restart est
///   accompli si l'état initial était arrêté et que le service tourne) ;
/// - reset d'uptime du SERVICE : l'uptime applicatif a reculé → le service
///   vient de redémarrer.
pub(crate) fn reached(
    action: Action,
    current: &crate::status::Status,
    before: &crate::status::Status,
    transition: bool,
    use_initial: bool,
) -> bool {
    let uptime_reset = before.service_uptime_seconds.is_some()
        && current.service_uptime_seconds.is_some()
        && current.service_uptime_seconds < before.service_uptime_seconds;
    match action {
        Action::Stop => offline(current),
        Action::Start => online(current),
        Action::Restart => {
            online(current) && ((use_initial && offline(before)) || transition || uptime_reset)
        }
    }
}

/// Vérification ponctuelle après un échec de transmission : jusqu'à 3
/// lectures espacées, SANS aucune nouvelle commande. Retourne l'état si
/// l'objectif est atteint.
async fn verify_target_state(
    data: &Data,
    target: &str,
    service: &str,
    action: Action,
    before: &crate::status::Status,
    deadline: Instant,
) -> Option<crate::status::Status> {
    for attempt in 0..3 {
        if attempt > 0 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            tokio::time::sleep(remaining.min(POLL_DELAYS[0])).await;
        }
        if Instant::now() >= deadline {
            return None;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        let read = tokio::time::timeout(remaining, data.api.get(target, service, true)).await;
        if let Ok(Ok(current)) = read {
            if reached(action, &current, before, false, true) {
                return Some(current);
            }
        }
    }
    None
}

/// Point d'entrée des commandes `/start`, `/stop`, `/restart`.
pub async fn power(
    ctx: Context<'_>,
    action: Action,
    target: Option<String>,
    service: Option<String>,
    with_backup: bool,
) -> Result<(), Error> {
    let data = ctx.data();
    let catalog = data.catalog;

    // Permission EN AMONT : ne pas exposer l'état du verrou à un inconnu.
    if !data.state.is_allowed(&ctx.author().id.to_string()) {
        let c = catalog.power.access_denied;
        ctx.send(crate::commands::reply(card(
            c.title,
            c.body,
            Tone::Bad,
            None,
        )))
        .await?;
        return Ok(());
    }

    // Résolution cible + service (explicite → session → défaut).
    let Some((target_alias, target_spec)) =
        crate::commands::resolve_target_opt(&ctx, target.as_deref()).await?
    else {
        return Ok(());
    };
    let Some((alias, _)) = crate::commands::resolve_service_opt(&ctx, service.as_deref()).await?
    else {
        return Ok(());
    };
    let act = action.text(catalog);

    // Nom affiché : le nom d'hôte si connu (cache 5 s), sinon l'alias.
    let mut service_name = alias.clone();
    if let Ok(status) = data.api.get(&target_alias, &alias, false).await {
        service_name = display_name(&status, &alias);
    }
    let machine_name = target_spec.display();

    // Relecture du verrou à chaque appel : l'état disque est la source de
    // vérité.
    let lock_key = format!("{target_alias}:{alias}");
    let active = data.state.load_persisted_lock();
    if let Some(active_action) = active.get(&lock_key) {
        let c = catalog.power.action_in_progress;
        let body = fill(
            c.body,
            &[("action", active_action), ("name", &service_name)],
        );
        ctx.send(crate::commands::reply(card(
            c.title,
            &body,
            Tone::Wait,
            None,
        )))
        .await?;
        return Ok(());
    }

    // ── Confirmation ──
    let warning = if matches!(action, Action::Stop | Action::Restart) {
        catalog.power.confirm.warning
    } else {
        ""
    };
    let backup_extra = if with_backup {
        catalog.power.with_backup_note
    } else {
        ""
    };
    let confirm_body = format!(
        "{warning}{backup_extra}{}",
        fill(
            catalog.power.confirm.body,
            &[
                ("verb", act.verb),
                ("name", &service_name),
                ("target", &machine_name)
            ]
        )
    );
    let (track_msg, confirmed) =
        confirm::confirm(&ctx, card(act.title, &confirm_body, Tone::Wait, None)).await?;
    if !confirmed {
        return Ok(());
    }

    // Re-vérification POST-confirmation + pose ATOMIQUE du verrou
    // (insert-si-absent sous mutex : deux confirmations simultanées ne
    // peuvent plus passer toutes les deux).
    if !data.state.is_allowed(&ctx.author().id.to_string()) {
        let c = catalog.power.access_revoked;
        edit_card(
            &ctx,
            card(c.title, c.body, Tone::Bad, None),
            None,
            Vec::new(),
        )
        .await?;
        return Ok(());
    }
    if !data.state.write_lock(&lock_key, action.as_str()) {
        let c = catalog.power.busy;
        let body = fill(c.body, &[("name", &service_name)]);
        edit_card(
            &ctx,
            card(c.title, &body, Tone::Wait, None),
            None,
            Vec::new(),
        )
        .await?;
        return Ok(());
    }

    // ── Verrou posé : l'action est lancée ──
    let action_start = Instant::now();
    let flow_deadline = action_start + FLOW_BUDGET;

    let result = run_action(
        &ctx,
        track_msg,
        action,
        &target_alias,
        &alias,
        &machine_name,
        action_start,
        flow_deadline,
        with_backup,
    )
    .await;
    // Délai de grâce anti-fausse-alerte du watchdog : uniquement pour un
    // ARRÊT volontaire (Action::Stop) — quel qu'en soit le dénouement. Un
    // start/restart n'enregistre jamais de grâce : un vrai crash dans les
    // 5 min suivant doit alerter. Pour un Stop, la commande a été transmise
    // (ou le service était déjà arrêté) : l'alerte « crash » serait du
    // bruit — l'utilisateur sait que le service est/sera arrêté.
    if action == Action::Stop {
        data.state.record_lock_release(&lock_key);
    }
    data.state.clear_lock(&lock_key);
    result.map(|_| ())
}

/// Cœur de l'action (après verrouillage). Toutes les étapes ÉDITENT la
/// carte originale via le token d'interaction — aucun nouveau message,
/// 100 % éphémère.
///
/// `Err` = l'action n'a pas pu démarrer (aucune commande envoyée) ou une
/// édition Discord finale a échoué — la commande, elle, n'est JAMAIS
/// renvoyée.
#[allow(clippy::too_many_arguments)]
async fn run_action(
    ctx: &Context<'_>,
    track_msg: MessageId,
    action: Action,
    target_alias: &str,
    alias: &str,
    machine_name: &str,
    action_start: Instant,
    flow_deadline: Instant,
    with_backup: bool,
) -> Result<(), Error> {
    let data = ctx.data();
    let catalog = data.catalog;
    let act = action.text(catalog);
    let author_id = ctx.author().id;

    // Vérification d'état AVANT le loader : pas de GIF quand il n'y a rien
    // à faire.
    let before = data.api.get(target_alias, alias, true).await?;
    let name = display_name(&before, alias);
    // Un restart demandé sur un service DÉJÀ arrêté ne peut pas montrer de
    // transition ni de reset d'uptime : l'état initial hors ligne suffit.
    let use_initial = action == Action::Restart && offline(&before);
    if (action == Action::Start && online(&before)) || (action == Action::Stop && offline(&before))
    {
        let c = catalog.power.already_in_target_state;
        let mut body = fill(c.body, &[("state", &state(&before))]);
        // Service déjà arrêté : l'arrêt est avéré → copie des sauvegardes.
        if action == Action::Stop {
            if let Some(note) = backup_after_stop(
                &data.api,
                catalog,
                target_alias,
                alias,
                &name,
                flow_deadline,
            )
            .await
            {
                append_note(&mut body, Some(&note));
            }
        }
        let gif = assets::gif_image("error");
        edit_card(
            ctx,
            card(c.title, &body, Tone::Good, None),
            Some(&gif),
            Vec::new(),
        )
        .await?;
        return Ok(());
    }

    // ── Sauvegarde AVANT le démarrage ──
    // Le service vient d'être vérifié arrêté ci-dessus : la copie est sûre
    // (le script garde lui-même la garantie « service inactif »).
    let mut backup_note: Option<String> = None;
    if action == Action::Start {
        backup_note = backup_after_stop(
            &data.api,
            catalog,
            target_alias,
            alias,
            &name,
            flow_deadline,
        )
        .await;
    }

    // ── UNE seule commande systemctl, jamais de retry ──
    // Restart AVEC sauvegarde : la première commande est un STOP (la copie
    // aura lieu à l'arrêt confirmé, puis un start sera envoyé).
    let backup_phase = action == Action::Restart && with_backup;
    let send_action = if backup_phase {
        "stop"
    } else {
        action.as_str()
    };
    let mut send_error: Option<crate::errors::BackendError> = None;
    let mut had_send_error = false;
    let power_start = Instant::now();
    match data.api.send_power(target_alias, alias, send_action).await {
        Ok(()) => {
            logger::info(fill(
                catalog.audit.power_send_ok,
                &[
                    ("action", send_action),
                    ("service", name.as_str()),
                    ("target", machine_name),
                    ("ms", &power_start.elapsed().as_millis().to_string()),
                ],
            ));
        }
        Err(err) => {
            send_error = Some(err.clone());
            had_send_error = true;
            logger::error(fill(
                catalog.audit.power_send_failed,
                &[
                    ("action", send_action),
                    ("service", name.as_str()),
                    ("target", machine_name),
                    ("error", &err.message.clone()),
                    ("ms", &power_start.elapsed().as_millis().to_string()),
                ],
            ));
        }
    }
    // Le cache contient l'état pré-commande : invalidation obligatoire.
    data.api.clear_cache(Some(target_alias), Some(alias)).await;

    if let Some(err) = send_error.as_ref() {
        if !err.may_have_been_transmitted() {
            // Rien n'a pu partir (refus, réseau, auth…) : erreur immédiate.
            return Err(err.clone().into());
        }
        // L'action a PU être transmise (timeout, coupure après envoi…) :
        // vérification rapide de l'état réel — uniquement des LECTURES.
        if let Some(confirmed) =
            verify_target_state(data, target_alias, alias, action, &before, flow_deadline).await
        {
            logger::info(fill(
                catalog.audit.power_applied_despite_error,
                &[
                    ("action", action.as_str()),
                    ("service", name.as_str()),
                    ("state", &state(&confirmed)),
                ],
            ));
            stats::record(action.as_str(), action_start.elapsed().as_millis() as u64);
            let c = catalog.power.applied_despite_error;
            // Stop : service vérifié arrêté → copie. Start : copie déjà
            // faite avant la commande. Restart : pas de sauvegarde (le
            // redémarrage peut être trop court pour garantir un arrêt
            // observable — décision explicite du projet).
            let note: Option<String> = match action {
                Action::Stop => {
                    backup_after_stop(
                        &data.api,
                        catalog,
                        target_alias,
                        alias,
                        &name,
                        flow_deadline,
                    )
                    .await
                }
                Action::Start => backup_note.clone(),
                Action::Restart => backup_note.clone(),
            };
            let mut body = fill(
                c.body,
                &[
                    ("cap", act.cap),
                    ("state", &state(&confirmed)),
                    ("machine", machine_name),
                ],
            );
            append_note(&mut body, note.as_deref());
            let gif = assets::gif_image("success");
            edit_card(
                ctx,
                card(c.title, &body, Tone::Good, None),
                Some(&gif),
                Vec::new(),
            )
            .await?;
            notify_user(ctx, card(c.title, &body, Tone::Good, None)).await;
            crate::notify::audit(
                ctx.http(),
                &data.cfg,
                data.catalog,
                c.title,
                &body,
                Tone::Good,
            )
            .await;
            return Ok(());
        }
        logger::warn(fill(
            catalog.audit.power_send_error_polling,
            &[
                ("action", action.as_str()),
                ("service", name.as_str()),
                ("error", err.message.as_str()),
            ],
        ));
    }

    logger::info(fill(
        catalog.audit.power_requested,
        &[
            ("action", action.as_str()),
            ("username", &ctx.author().name.clone()),
            ("userId", &logger::short_id(author_id.to_string())),
            ("service", name.as_str()),
            ("target", machine_name),
        ],
    ));

    // ── Suivi silencieux ──
    let ack_key = if send_error.is_some() {
        catalog.power.accepted_after_error
    } else {
        catalog.power.accepted
    };
    let ack_body = format!(
        "{}{}",
        fill(ack_key.body, &[("cap", act.cap)]),
        eta_line(action, catalog)
    );
    let waiting = assets::gif_image("waiting");

    // Bouton « Arrêter le suivi » : l'action reste irréversible (la commande
    // est déjà partie), on arrête seulement la surveillance.
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let uuid: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let cancel_id = format!("pw:{uuid}:cancel");
    let cancel_row = CreateActionRow::Buttons(vec![CreateButton::new(&cancel_id)
        .label(catalog.power.cancel_tracking)
        .style(ButtonStyle::Secondary)]);
    edit_card(
        ctx,
        card(ack_key.title, &ack_body, Tone::Wait, None),
        Some(&waiting),
        vec![cancel_row.clone()],
    )
    .await?;

    let mut collector = Box::pin(
        serenity::collector::ComponentInteractionCollector::new(&ctx.serenity_context().shard)
            .message_id(track_msg)
            .author_id(author_id)
            .timeout(TRACKING_TIMEOUT)
            .stream(),
    );

    let start = Instant::now();
    let mut iteration = 0usize;
    let mut transition = false;
    let mut stable: u32 = 0;
    let mut last_known = before.clone();
    let mut failures: u32 = 0;
    let mut cancelled = false;
    let mut collector_done = false;
    // Phase « stop → copie → start » d'un restart avec sauvegarde.
    let mut start_sent = false;
    let mut start_send_error: Option<String> = None;
    let mut off_stable: u32 = 0;

    'tracking: while start.elapsed() < TRACKING_TIMEOUT {
        if Instant::now() >= flow_deadline {
            break 'tracking;
        }
        let delay = POLL_DELAYS[iteration.min(POLL_DELAYS.len() - 1)];
        iteration += 1;

        let remaining = flow_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break 'tracking;
        }
        let wake = remaining.min(delay);
        if collector_done {
            tokio::time::sleep(wake).await;
        } else {
            tokio::select! {
                click = collector.next() => {
                    match click {
                        Some(click) => {
                            let _ = click.defer(ctx.http()).await;
                            cancelled = true;
                            break 'tracking;
                        }
                        None => {
                            collector_done = true;
                            continue;
                        }
                    }
                }
                _ = tokio::time::sleep(wake) => {}
            }
        }
        if Instant::now() >= flow_deadline {
            break 'tracking;
        }

        // ── Un tour de polling ──
        let current = match data.api.get(target_alias, alias, true).await {
            Ok(status) => {
                last_known = status.clone();
                failures = 0;
                status
            }
            Err(_) => {
                failures += 1;
                if failures >= MAX_FAILURES {
                    break 'tracking;
                }
                continue;
            }
        };

        if !online(&current) {
            transition = true;
        }

        // ── Restart AVEC sauvegarde : stop confirmé → copie → start ──
        if backup_phase && !start_sent {
            off_stable = if !online(&current) { off_stable + 1 } else { 0 };
            if off_stable >= 2 {
                // Arrêt avéré : le service EST arrêté — copie sûre.
                if let Some(note) = backup_after_stop(
                    &data.api,
                    catalog,
                    target_alias,
                    alias,
                    &name,
                    flow_deadline,
                )
                .await
                {
                    backup_note = Some(note);
                }
                // Puis démarrage : UNE commande, jamais de retry.
                match data.api.send_power(target_alias, alias, "start").await {
                    Ok(()) => {}
                    Err(err) => {
                        start_send_error = Some(err.message);
                        logger::error(fill(
                            catalog.audit.power_send_failed,
                            &[
                                ("action", "start"),
                                ("service", name.as_str()),
                                ("target", machine_name),
                                ("error", &start_send_error.clone().unwrap_or_default()),
                                ("ms", "0"),
                            ],
                        ));
                    }
                }
                start_sent = true;
                stable = 0;
                continue;
            }
        }

        let is_reached = reached(action, &current, &before, transition, use_initial);
        stable = if is_reached { stable + 1 } else { 0 };
        if stable >= 2 {
            let seconds = start.elapsed().as_secs();
            logger::info(fill(
                catalog.audit.power_confirmed,
                &[
                    ("action", action.as_str()),
                    ("service", name.as_str()),
                    ("seconds", &seconds.to_string()),
                ],
            ));
            let tail = if action != Action::Stop {
                catalog.power.completed.tail
            } else {
                ""
            };
            // ── Sauvegarde : stop = après arrêt confirmé ; start = déjà
            //    faite avant la commande ; restart = aucune (décision
            //    explicite : l'arrêt n'est pas garanti observable).
            let note: Option<String> = match action {
                Action::Stop => {
                    backup_after_stop(
                        &data.api,
                        catalog,
                        target_alias,
                        alias,
                        &name,
                        flow_deadline,
                    )
                    .await
                }
                Action::Start => backup_note.clone(),
                Action::Restart => backup_note.clone(),
            };
            let mut body = format!(
                "{}{}",
                fill(
                    catalog.power.completed.body,
                    &[
                        ("cap", act.cap),
                        ("machine", machine_name),
                        ("state", &state(&current).to_uppercase()),
                        ("seconds", &seconds.to_string()),
                    ],
                ),
                tail
            );
            append_note(&mut body, note.as_deref());
            // Échec de start : ne l'annoncer que si l'état final le CONFIRME
            // (un timeout de transmission peut avoir abouti — le suivi a
            // alors vu le service repartir).
            if let Some(err) = start_send_error.as_deref() {
                if !online(&current) {
                    append_note(
                        &mut body,
                        Some(&fill(catalog.power.start_failed_note, &[("error", err)])),
                    );
                } else {
                    logger::warn(format!("restart avec sauvegarde : erreur de transmission du start ({err}) mais l'état final est RUNNING"));
                }
            }
            stats::record(action.as_str(), action_start.elapsed().as_millis() as u64);
            let gif = assets::gif_image("success");
            edit_card(
                ctx,
                card(catalog.power.completed.title, &body, Tone::Good, None),
                Some(&gif),
                Vec::new(),
            )
            .await?;
            notify_user(
                ctx,
                card(catalog.power.completed.title, &body, Tone::Good, None),
            )
            .await;
            crate::notify::audit(
                ctx.http(),
                &data.cfg,
                data.catalog,
                catalog.power.completed.title,
                &body,
                Tone::Good,
            )
            .await;
            return Ok(());
        }
    }

    // ── Sortie sans confirmation ──
    if cancelled {
        logger::info(fill(
            catalog.audit.power_tracking_cancelled,
            &[("action", action.as_str()), ("service", name.as_str())],
        ));
        let c = catalog.power.tracking_cancelled;
        edit_card(
            ctx,
            card(c.title, c.body, Tone::Info, None),
            None,
            Vec::new(),
        )
        .await?;
        return Ok(());
    }

    logger::warn(fill(
        catalog.audit.power_not_confirmed,
        &[
            ("action", action.as_str()),
            ("service", name.as_str()),
            ("failures", &failures.to_string()),
        ],
    ));
    let c = catalog.power.not_confirmed;
    let body_key = if had_send_error {
        c.body_transmit_error
    } else if failures >= MAX_FAILURES {
        c.body_failed
    } else {
        c.body_timeout
    };
    let mut body = fill(body_key, &[("state", &state(&last_known))]);
    append_note(&mut body, backup_note.as_deref());
    if let Some(err) = start_send_error.as_deref() {
        if !online(&last_known) {
            append_note(
                &mut body,
                Some(&fill(catalog.power.start_failed_note, &[("error", err)])),
            );
        }
    }
    let gif = assets::gif_image("error");
    edit_card(
        ctx,
        card(c.title, &body, Tone::Wait, None),
        Some(&gif),
        Vec::new(),
    )
    .await?;
    notify_user(ctx, card(c.title, &body, Tone::Wait, None)).await;
    crate::notify::audit(
        ctx.http(),
        &data.cfg,
        data.catalog,
        c.title,
        &body,
        Tone::Wait,
    )
    .await;
    Ok(())
}

/// `/reboot` — redémarrage de la MACHINE (owner uniquement).
///
/// Une commande unique sans retry et SANS suivi : sur la machine locale le
/// bot lui-même s'arrête ; à distance la machine devient injoignable. La
/// carte finale annonce la transmission ; un timeout = « transmission
/// incertaine » (le reboot a pu partir).
pub async fn reboot(ctx: Context<'_>, target: Option<String>) -> Result<(), Error> {
    let data = ctx.data();
    let catalog = data.catalog;

    if !data.state.is_owner(&ctx.author().id.to_string()) {
        let c = catalog.reboot.access_denied;
        ctx.send(crate::commands::reply(card(
            c.title,
            c.body,
            Tone::Bad,
            None,
        )))
        .await?;
        return Ok(());
    }

    let Some((target_alias, target_spec)) =
        crate::commands::resolve_target_opt(&ctx, target.as_deref()).await?
    else {
        return Ok(());
    };
    let machine_name = target_spec.display();

    // ── Confirmation (avertissement renforcé pour la machine locale) ──
    let mut body = fill(catalog.reboot.confirm_body, &[("name", &machine_name)]);
    if target_spec.is_local() {
        body.push_str(catalog.reboot.local_note);
    }
    let (_, confirmed) = confirm::confirm(
        &ctx,
        card(catalog.reboot.confirm_title, &body, Tone::Wait, None),
    )
    .await?;
    if !confirmed {
        return Ok(());
    }

    logger::info(fill(
        catalog.audit.reboot_requested,
        &[
            ("username", &ctx.author().name.clone()),
            ("userId", &logger::short_id(ctx.author().id.to_string())),
            ("target", &machine_name),
        ],
    ));

    // ── UNE commande, jamais de retry ──
    let power_start = Instant::now();
    match data.api.reboot(&target_alias).await {
        Ok(()) => {
            logger::info(fill(
                catalog.audit.reboot_sent,
                &[
                    ("target", &machine_name),
                    ("ms", &power_start.elapsed().as_millis().to_string()),
                ],
            ));
            let c = if target_spec.is_local() {
                catalog.reboot.sent_local
            } else {
                catalog.reboot.sent
            };
            let body = fill(c.body, &[("name", &machine_name)]);
            let gif = assets::gif_image("success");
            edit_card(
                &ctx,
                card(c.title, &body, Tone::Good, None),
                Some(&gif),
                Vec::new(),
            )
            .await?;
            notify_user(&ctx, card(c.title, &body, Tone::Good, None)).await;
            crate::notify::audit(
                ctx.http(),
                &data.cfg,
                data.catalog,
                c.title,
                &body,
                Tone::Good,
            )
            .await;
            Ok(())
        }
        Err(err) => {
            logger::error(fill(
                catalog.audit.reboot_failed,
                &[("target", &machine_name), ("error", &err.message.clone())],
            ));
            if err.may_have_been_transmitted() {
                // Transmission incertaine : la machine a PU redémarrer.
                let c = catalog.power.not_confirmed;
                let body = fill(c.body_transmit_error, &[("state", "unknown")]);
                let gif = assets::gif_image("error");
                edit_card(
                    &ctx,
                    card(c.title, &body, Tone::Wait, None),
                    Some(&gif),
                    Vec::new(),
                )
                .await?;
                Ok(())
            } else {
                Err(err.into())
            }
        }
    }
}

/// Exécution SANS interaction Discord (planificateur) : verrou, UNE
/// commande jamais répétée, suivi jusqu'à confirmation (échéance 10 min),
/// sauvegarde à l'arrêt avéré (stop confirmé / avant start). Retourne un
/// résumé pour l'audit ou une erreur descriptive.
pub async fn run_headless(
    api: &Arc<Api>,
    st: &Arc<crate::state::State>,
    catalog: &Catalog,
    action: Action,
    target: &str,
    service: &str,
) -> Result<String, String> {
    let key = lock_key(target, service);
    if !st.write_lock(&key, action.as_str()) {
        return Err(format!("verrou déjà détenu ({key})"));
    }
    let deadline = Instant::now() + Duration::from_secs(600);
    let outcome = run_headless_locked(api, catalog, action, target, service, deadline).await;
    if action == Action::Stop {
        st.record_lock_release(&key); // grâce anti-fausse-alerte du watchdog
    }
    st.clear_lock(&key);
    outcome
}

async fn run_headless_locked(
    api: &Arc<Api>,
    catalog: &Catalog,
    action: Action,
    target: &str,
    service: &str,
    deadline: Instant,
) -> Result<String, String> {
    let before = api
        .get(target, service, true)
        .await
        .map_err(|e| e.message)?;
    let name = display_name(&before, service);
    let use_initial = action == Action::Restart && offline(&before);

    // Déjà dans l'état cible : pas de commande (mais sauvegarde pour stop).
    if (action == Action::Start && online(&before)) || (action == Action::Stop && offline(&before))
    {
        let mut summary = format!(
            "{} ({name}) : déjà en état {}.",
            action.as_str(),
            state(&before)
        );
        if action == Action::Stop {
            if let Some(n) = backup_after_stop(api, catalog, target, service, &name, deadline).await
            {
                summary.push_str(&format!(" {n}"));
            }
        }
        return Ok(summary);
    }

    // Sauvegarde AVANT le démarrage (service vérifié arrêté ci-dessus).
    let mut note: Option<String> = None;
    if action == Action::Start {
        note = backup_after_stop(api, catalog, target, service, &name, deadline).await;
    }

    api.send_power(target, service, action.as_str())
        .await
        .map_err(|e| e.message)?;
    api.clear_cache(Some(target), Some(service)).await;

    // Suivi : backoff 5 s → 30 s, 2 états stables exigés, échéance absolue.
    const DELAYS: [u64; 11] = [5, 5, 5, 10, 10, 10, 15, 15, 20, 20, 30];
    let start = Instant::now();
    let mut transition = false;
    let mut stable: u32 = 0;
    let mut last = before.clone();
    let mut i = 0usize;
    loop {
        if Instant::now() >= deadline {
            return Err(format!(
                "non confirmé après 10 min (dernier état : {})",
                state(&last)
            ));
        }
        tokio::time::sleep(Duration::from_secs(DELAYS[i.min(DELAYS.len() - 1)])).await;
        i += 1;
        let current = match api.get(target, service, true).await {
            Ok(s) => s,
            Err(_) => continue, // échec de LECTURE : on continue le suivi
        };
        if !online(&current) {
            transition = true;
        }
        last = current.clone();
        let is_reached = reached(action, &current, &before, transition, use_initial);
        stable = if is_reached { stable + 1 } else { 0 };
        if stable >= 2 {
            let seconds = start.elapsed().as_secs();
            if action == Action::Stop {
                if let Some(n) =
                    backup_after_stop(api, catalog, target, service, &name, deadline).await
                {
                    note = Some(n);
                }
            }
            let mut summary = format!("{} ({name}) confirmé en {seconds}s.", action.as_str());
            if let Some(n) = note {
                summary.push_str(&format!(" {n}"));
            }
            return Ok(summary);
        }
    }
}

/// Résolution « verrou » réutilisée par le watchdog : la clé de verrou d'un
/// couple cible/service.
pub fn lock_key(target: &str, service: &str) -> String {
    format!("{target}:{service}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(state: &str, service_uptime: Option<f64>) -> crate::status::Status {
        crate::status::Status {
            state: state.to_string(),
            service_uptime_seconds: service_uptime,
            ..Default::default()
        }
    }

    #[test]
    fn start_reaches_running() {
        let before = status("stopped", None);
        let current = status("running", None);
        assert!(reached(Action::Start, &current, &before, false, false));
        assert!(!reached(
            Action::Start,
            &status("stopped", None),
            &before,
            false,
            false
        ));
    }

    #[test]
    fn stop_reaches_stopped() {
        let before = status("running", Some(5000.0));
        assert!(reached(
            Action::Stop,
            &status("stopped", None),
            &before,
            false,
            false
        ));
        assert!(!reached(
            Action::Stop,
            &status("running", None),
            &before,
            false,
            false
        ));
    }

    #[test]
    fn restart_needs_transition_or_uptime_reset() {
        let before = status("running", Some(5000.0));
        // Toujours running sans reset d'uptime ni transition : PAS confirmé.
        assert!(!reached(
            Action::Restart,
            &status("running", Some(9000.0)),
            &before,
            false,
            false
        ));
        // Reset d'uptime du service (5 000 → 40 s) : confirmé.
        assert!(reached(
            Action::Restart,
            &status("running", Some(40.0)),
            &before,
            false,
            false
        ));
        // Passage observé par un état non-running : confirmé.
        assert!(reached(
            Action::Restart,
            &status("running", Some(9000.0)),
            &before,
            true,
            false
        ));
        // Restart après un état initial arrêté (vérification post-erreur).
        let before_stopped = status("stopped", None);
        assert!(reached(
            Action::Restart,
            &status("running", None),
            &before_stopped,
            false,
            true
        ));
        assert!(!reached(
            Action::Restart,
            &status("running", None),
            &before_stopped,
            false,
            false
        ));
    }

    #[test]
    fn uptime_reset_requires_both_values() {
        let before = status("running", Some(5000.0));
        // Uptime inconnu côté courant : jamais un reset.
        assert!(!reached(
            Action::Restart,
            &status("running", None),
            &before,
            false,
            false
        ));
        let before_no_uptime = status("running", None);
        assert!(!reached(
            Action::Restart,
            &status("running", Some(40.0)),
            &before_no_uptime,
            false,
            false
        ));
    }

    #[test]
    fn lock_key_joins_target_and_service() {
        assert_eq!(lock_key("prod", "game"), "prod:game");
    }
}
