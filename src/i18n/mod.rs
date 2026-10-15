//! Internationalisation (en / fr).
//!
//! Les catalogues sont des structs Rust typées compilées dans le binaire :
//! une clé manquante ou renommée est une ERREUR DE COMPILATION, pas un bug
//! de production. La parité en/fr est structurelle (même type [`Catalog`]).
//! Les textes avec variables (`{name}`, `{seconds}`…) sont des gabarits
//! interpolés par [`fill`] au moment de l'affichage.

pub mod en;
pub mod fr;

/// Une « carte » d'embed : un titre + un corps.
#[derive(Debug, Clone, Copy)]
pub struct Card {
    pub title: &'static str,
    pub body: &'static str,
}

/// Objet de catalogue réduit à un titre.
#[derive(Debug, Clone, Copy)]
pub struct TitleOnly {
    pub title: &'static str,
}

// ── Formes spécifiques du catalogue ────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct PowerAction {
    pub verb: &'static str,
    pub cap: &'static str,
    pub title: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Actions {
    pub start: PowerAction,
    pub stop: PowerAction,
    pub restart: PowerAction,
}

#[derive(Debug, Clone, Copy)]
pub struct PowerConfirm {
    pub warning: &'static str,
    pub body: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Completed {
    pub title: &'static str,
    pub body: &'static str,
    pub tail: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct NotConfirmed {
    pub title: &'static str,
    pub body_failed: &'static str,
    pub body_timeout: &'static str,
    pub body_transmit_error: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct PingFailed {
    pub title: &'static str,
    pub body: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct ManageList {
    pub title_one: &'static str,
    pub title_many: &'static str,
    pub header: &'static str,
    pub entry: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct ManageClear {
    pub confirm_title: &'static str,
    pub confirm_body: &'static str,
    pub done_title: &'static str,
    pub done_body: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct NoChange {
    pub title: &'static str,
    pub body_add: &'static str,
    pub body_remove: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct ManageConfirm {
    pub title: &'static str,
    pub body: &'static str,
    pub verb_add: &'static str,
    pub verb_remove: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct ManageDone {
    pub title: &'static str,
    pub body_add: &'static str,
    pub body_remove: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct SetupPrompt {
    pub discord_token: &'static str,
    pub discord_client_id: &'static str,
    pub discord_owner_id: &'static str,
    pub discord_guild_id: &'static str,
    pub targets: &'static str,
    pub ssh_user: &'static str,
    pub ssh_password: &'static str,
    pub ssh_key_path: &'static str,
    pub services: &'static str,
    pub systemctl_prefix: &'static str,
    pub alert_channel: &'static str,
    pub saves_path: &'static str,
}

// ── Sections du catalogue ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct Common {
    pub access_denied_member: Card,
    pub access_denied_guild: Card,
    pub unknown_target: Card,
    pub unknown_service: Card,
    pub session_reset_target: Card,
    pub session_reset_service: Card,
    pub default_target: Card,
    pub default_service: Card,
    pub generic_error: TitleOnly,
}

#[derive(Debug, Clone, Copy)]
pub struct Confirm {
    pub confirm: &'static str,
    pub cancel: &'static str,
    pub cancelled: Card,
    pub processing: Card,
    pub expired: Card,
}

#[derive(Debug, Clone, Copy)]
pub struct Power {
    pub actions: Actions,
    pub access_denied: Card,
    pub action_in_progress: Card,
    pub confirm: PowerConfirm,
    pub access_revoked: Card,
    pub busy: Card,
    pub already_in_target_state: Card,
    pub accepted: Card,
    pub accepted_after_error: Card,
    pub backup_ok: &'static str,
    pub backup_missing: &'static str,
    pub backup_failed: &'static str,
    pub backup_skipped_running: &'static str,
    pub backup_locked: &'static str,
    pub backup_fresh: &'static str,
    pub with_backup_note: &'static str,
    pub start_failed_note: &'static str,
    pub completed: Completed,
    pub applied_despite_error: Card,
    pub eta: &'static str,
    pub cancel_tracking: &'static str,
    pub tracking_cancelled: Card,
    pub not_confirmed: NotConfirmed,
}

#[derive(Debug, Clone, Copy)]
pub struct Reboot {
    pub access_denied: Card,
    pub confirm_title: &'static str,
    pub confirm_body: &'static str,
    pub local_note: &'static str,
    pub sent: Card,
    pub sent_local: Card,
}

#[derive(Debug, Clone, Copy)]
pub struct Status {
    pub title: &'static str,
    pub machine_online: &'static str,
    pub service: &'static str,
    pub cpu: &'static str,
    pub memory: &'static str,
    pub storage: &'static str,
    pub load: &'static str,
    pub uptime: &'static str,
    pub service_uptime: &'static str,
    pub os: &'static str,
    pub kernel: &'static str,
    pub address: &'static str,
    pub backend: &'static str,
    pub cores: &'static str,
    pub metrics: &'static str,
    pub not_reported: &'static str,
    pub state_running: &'static str,
    pub state_offline: &'static str,
    pub state_unknown: &'static str,
    pub refresh: &'static str,
    pub local_label: &'static str,
    pub ssh_label: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Ping {
    pub failed: PingFailed,
    pub pong: Card,
}

#[derive(Debug, Clone, Copy)]
pub struct Manage {
    pub access_denied: Card,
    pub list: ManageList,
    pub clear: ManageClear,
    pub invalid: Card,
    pub no_change: NoChange,
    pub confirm: ManageConfirm,
    pub done: ManageDone,
}

#[derive(Debug, Clone, Copy)]
pub struct BotLogs {
    pub access_denied: Card,
    pub title: &'static str,
    pub empty: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Logs {
    pub card_title: &'static str,
    pub read_failed: &'static str,
    pub no_systemd: &'static str,
    pub title_filtered: &'static str,
    pub title_last: &'static str,
    pub nothing_filtered: &'static str,
    pub empty: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Watchdog {
    pub down: Card,
    pub collect_failed: Card,
    pub high_cpu: Card,
    pub high_ram: Card,
    pub high_disk: Card,
    pub unreachable: Card,
    pub target_back: Card,
    pub recovered: Card,
}

#[derive(Debug, Clone, Copy)]
pub struct Modules {
    pub access_denied: Card,
    pub list_title: &'static str,
    pub list_hint: &'static str,
    pub desc_watchdog: &'static str,
    pub desc_scheduler: &'static str,
    pub desc_backups: &'static str,
    pub desc_exec: &'static str,
    pub desc_metrics: &'static str,
    pub on: &'static str,
    pub off: &'static str,
    pub toggled_on: &'static str,
    pub toggled_off: &'static str,
    pub unknown: &'static str,
    pub module_disabled: Card,
}

#[derive(Debug, Clone, Copy)]
pub struct Schedule {
    pub access_denied: Card,
    pub list_title: &'static str,
    pub empty: &'static str,
    pub added: &'static str,
    pub removed: &'static str,
    pub cleared: &'static str,
    pub unknown_id: &'static str,
    pub invalid_when: &'static str,
    pub invalid_past: &'static str,
    pub invalid_action: &'static str,
    pub unknown_target: &'static str,
    pub unknown_service: &'static str,
    pub audit_ok: &'static str,
    pub audit_fail: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Backups {
    pub access_denied: Card,
    pub list_title: &'static str,
    pub no_saves: &'static str,
    pub empty: &'static str,
    pub listing: &'static str,
    pub restored: &'static str,
    pub confirm_title: &'static str,
    pub confirm_body: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Exec {
    pub access_denied: Card,
    pub title: &'static str,
    pub hint: &'static str,
    pub unknown: &'static str,
    pub invalid_unit: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Setup {
    pub intro: &'static str,
    pub completion_intro: &'static str,
    pub file_line: &'static str,
    pub language_label: &'static str,
    pub prompt_line: &'static str,
    pub prompt_line_hidden: &'static str,
    pub prompt: SetupPrompt,
    pub invalid: &'static str,
    pub complete: &'static str,
    pub path_hint: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Errors {
    pub missing_config: &'static str,
    pub invalid_target: &'static str,
    pub invalid_service: &'static str,
    pub ssh_auth: &'static str,
    pub ssh_network: &'static str,
    pub ssh_timeout: &'static str,
    pub ssh_timeout_sent: &'static str,
    pub command_rejected: &'static str,
    pub generic: &'static str,
    pub power_suffix: &'static str,
    pub setup_incomplete_non_interactive: &'static str,
    pub setup_incomplete_no_tty: &'static str,
    pub invalid_users_json_shape: &'static str,
    pub deploy_missing_access: &'static str,
    pub deploy_unknown_application: &'static str,
    pub internal_panic: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Audit {
    pub power_requested: &'static str,
    pub power_send_failed: &'static str,
    pub power_send_ok: &'static str,
    pub power_tracking_cancelled: &'static str,
    pub power_applied_despite_error: &'static str,
    pub power_send_error_polling: &'static str,
    pub power_confirmed: &'static str,
    pub power_not_confirmed: &'static str,
    pub backup_copied: &'static str,
    pub backup_missing_path: &'static str,
    pub backup_failed: &'static str,
    pub backup_skipped: &'static str,
    pub reboot_requested: &'static str,
    pub reboot_sent: &'static str,
    pub reboot_failed: &'static str,
    pub operator_granted: &'static str,
    pub operator_revoked: &'static str,
    pub operators_cleared: &'static str,
    pub commands_skipped: &'static str,
    pub commands_registered: &'static str,
    pub commands_global_purged: &'static str,
    pub commands_global_purge_failed: &'static str,
    pub commands_old_guild_purged: &'static str,
    pub commands_old_guild_purge_failed: &'static str,
    pub watchdog_disabled: &'static str,
    pub watchdog_started: &'static str,
    pub watchdog_alert_sent: &'static str,
    pub watchdog_alert_failed: &'static str,
    pub notify_sent: &'static str,
    pub notify_failed: &'static str,
    pub watchdog_check_failed: &'static str,
    pub lock_write_failed: &'static str,
    pub sessions_write_failed: &'static str,
    pub users_invalid_entry: &'static str,
    pub users_not_array: &'static str,
}

/// Le catalogue complet — c'est CE type que `en.rs` et `fr.rs` instancient.
#[derive(Debug, Clone, Copy)]
pub struct Catalog {
    pub common: Common,
    pub confirm: Confirm,
    pub help: Card,
    pub power: Power,
    pub reboot: Reboot,
    pub status: Status,
    pub ping: Ping,
    pub manage: Manage,
    pub logs: Logs,
    pub bot_logs: BotLogs,
    pub watchdog: Watchdog,
    pub modules: Modules,
    pub schedule: Schedule,
    pub backups: Backups,
    pub exec: Exec,
    pub setup: Setup,
    pub errors: Errors,
    pub audit: Audit,
}

pub const SUPPORTED: [&str; 2] = ["en", "fr"];
pub const DEFAULT_LANG: &str = "en";

/// Résolution de la langue au démarrage : `--lang=xx` > `BOT_LANGUAGE` > `en`.
pub fn resolve_initial_lang(args: &[String]) -> &'static str {
    for a in args {
        if let Some(code) = a.strip_prefix("--lang=") {
            let code = code.trim().to_lowercase();
            if SUPPORTED.contains(&code.as_str()) {
                return code_of(code.as_str());
            }
            eprintln!(
                "[i18n] Unknown --lang value \"{code}\", falling back to \"{DEFAULT_LANG}\"."
            );
        }
    }
    let env = std::env::var("BOT_LANGUAGE")
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    if SUPPORTED.contains(&env.as_str()) {
        return code_of(env.as_str());
    }
    if !env.is_empty() {
        eprintln!("[i18n] Unknown BOT_LANGUAGE \"{env}\", falling back to \"{DEFAULT_LANG}\".");
    }
    DEFAULT_LANG
}

fn code_of(code: &str) -> &'static str {
    match code {
        "fr" => "fr",
        _ => "en",
    }
}

/// Catalogue actif pour une langue (validée) — référence statique.
pub fn catalog(lang: &str) -> &'static Catalog {
    match lang {
        "fr" => &fr::CATALOG,
        _ => &en::CATALOG,
    }
}

/// Interpolation d'un gabarit : remplace chaque `{nom}` par la valeur.
/// Les clés inconnues restent telles quelles.
pub fn fill(template: &str, params: &[(&str, &str)]) -> String {
    if params.is_empty() {
        return template.to_string();
    }
    let mut out = template.to_string();
    for (key, value) in params {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_replaces_known_params_only() {
        assert_eq!(
            fill("Hello {name}, {missing} stays", &[("name", "Apach")]),
            "Hello Apach, {missing} stays"
        );
    }

    #[test]
    fn fill_handles_multiple_occurrences() {
        assert_eq!(fill("{a} and {a}", &[("a", "x")]), "x and x");
    }

    #[test]
    fn both_catalogs_are_compiled_in() {
        assert!(en::CATALOG.status.state_running.contains("ONLINE"));
        assert!(fr::CATALOG.status.state_running.contains("LIGNE"));
    }

    #[test]
    fn resolve_lang_prefers_cli_flag() {
        assert_eq!(
            resolve_initial_lang(&["bot".into(), "--lang=fr".into()]),
            "fr"
        );
        assert_eq!(
            resolve_initial_lang(&["bot".into(), "--lang=zz".into()]),
            DEFAULT_LANG
        );
    }
}
