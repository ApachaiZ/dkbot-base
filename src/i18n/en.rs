//! English catalog. Strict parity with `fr.rs`: both files instantiate the
//! same `Catalog` type — any missing key is a compile error.

use super::*;

pub static CATALOG: Catalog = Catalog {
    common: Common {
        access_denied_member: Card {
            title: "Access denied",
            body: "Ask the bot owner to grant you operator access.",
        },
        access_denied_guild: Card {
            title: "Access denied",
            body: "This bot is configured for another Discord server.",
        },
        unknown_target: Card {
            title: "Unknown target",
            body: "No machine named `{alias}`.",
        },
        unknown_service: Card {
            title: "Unknown service",
            body: "No service named `{alias}`.",
        },
        session_reset_target: Card {
            title: "🎯 Session reset",
            body: "Your session target `{alias}` no longer exists. Session cleared — commands will now target the default machine.",
        },
        session_reset_service: Card {
            title: "🎯 Session reset",
            body: "Your session service `{alias}` no longer exists. Session cleared — commands will now target the default service.",
        },
        default_target: Card {
            title: "🎯 Default machine",
            body: "Your session now targets **{alias}**.",
        },
        default_service: Card {
            title: "🎯 Default service",
            body: "Your session now targets the **{alias}** service.",
        },
        generic_error: TitleOnly { title: "⚠ Error" },
    },

    confirm: Confirm {
        confirm: "Confirm",
        cancel: "Cancel",
        cancelled: Card {
            title: "Cancelled",
            body: "Nothing was changed.",
        },
        processing: Card {
            title: "Processing",
            body: "Please wait…",
        },
        expired: Card {
            title: "Expired",
            body: "Confirmation expired. Run the command again.",
        },
    },

    help: Card {
        title: "🎛 VPS control",
        body: "**Machine**\n/status — live dashboard (CPU, RAM, disk, load, uptime, OS, service)\n/start — start the application service\n/stop — stop the application service\n/restart — restart the application service\n/ping — measure the round-trip to the machine\n/target — set your default machine for this session\n/server — set your default service for this session\nCommands accept optional `target` and `service` options.\n\n**Owner only**\n/reboot — reboot the machine itself\n/users add — mention and authorize\n/users remove — mention and revoke\n/users list — see operators\n/users clear — reset all access\n/logs — read the service's last log lines\n\nPower commands require confirmation. All replies are private. Authorizations only concern this bot.",
    },

    power: Power {
        actions: Actions {
            start: PowerAction { verb: "Start", cap: "Start", title: "Confirm start" },
            stop: PowerAction { verb: "Stop", cap: "Stop", title: "Confirm stop" },
            restart: PowerAction { verb: "Restart", cap: "Restart", title: "Confirm restart" },
        },
        access_denied: Card {
            title: "Access denied",
            body: "Ask the bot owner to grant you operator access.",
        },
        action_in_progress: Card {
            title: "Action in progress",
            body: "A **{action}** action is already being tracked on **{name}**.",
        },
        confirm: PowerConfirm {
            warning: "⚠ This will interrupt the running service.\n",
            body: "{verb} the **{name}** service on **{target}**?",
        },
        access_revoked: Card {
            title: "Access revoked",
            body: "Your operator access has been revoked.",
        },
        busy: Card {
            title: "Busy",
            body: "Another action started on **{name}** while you were confirming.",
        },
        already_in_target_state: Card {
            title: "Already in target state",
            body: "The service is **{state}**. No command was sent.",
        },
        accepted: Card {
            title: "⏳ Action accepted",
            body: "Command sent. Silent tracking (progressive spacing) for up to 10 minutes.",
        },
        accepted_after_error: Card {
            title: "⏳ Transmission uncertain — tracking",
            body: "**{cap}** transmitted, but the machine returned an error. Tracking the real state (progressive spacing) for up to 10 minutes.\n**No second command will ever be sent.**",
        },
        backup_ok: "💾 Backup copied: `{dest}`",
        backup_missing: "💾 Backup: not possible — SAVES_PATH is not set in the configuration",
        backup_failed: "💾 Backup: failed — {error}",
        backup_skipped_running: "💾 Backup: skipped — the service was not stopped when the copy ran",
        backup_locked: "💾 Backup: skipped — another backup is already running",
        backup_fresh: "💾 Backup: not repeated — a copy was made less than an hour ago",
        with_backup_note: "\n💾 Backup during the stop (stop → copy → start).",
        start_failed_note: "\n⚠ The start failed: {error} — the service is STOPPED.",
        completed: Completed {
            title: "✅ Action complete",
            body: "**{cap}** confirmed on **{machine}**.\nCurrent state: **{state}**\nDuration: {seconds} seconds.",
            tail: "\nThe game may still need a little time to become reachable.",
        },
        applied_despite_error: Card {
            title: "✅ Action applied",
            body: "**{cap}** applied on the machine.\nVerified state: **{state}**.\n\n*Note: the machine returned an error during transmission, but the action was applied. No new command was sent.*",
        },
        eta: "⏱ On average: {seconds} s",
        cancel_tracking: "❌ Stop tracking",
        tracking_cancelled: Card {
            title: "Tracking cancelled",
            body: "Tracking stopped. The action may continue in the background — check /status.",
        },
        not_confirmed: NotConfirmed {
            title: "⚠ End not confirmed",
            body_failed: "The command was accepted, but state checks failed repeatedly.\nLast known state: **{state}**.\nUse /status. **No second command was sent.**",
            body_timeout: "The command was accepted, but completion could not be verified within 10 minutes.\nLast known state: **{state}**.\nUse /status. **No second command was sent.**",
            body_transmit_error: "The transmission returned an error, but the action may have been applied. Completion could not be verified within 10 minutes.\nLast known state: **{state}**.\nUse /status. **No second command was sent.**",
        },
    },

    reboot: Reboot {
        access_denied: Card {
            title: "Access denied",
            body: "Only the configured owner can reboot the machine.",
        },
        confirm_title: "Confirm reboot",
        confirm_body: "Reboot the machine **{name}**?\nAll connections will be cut — the machine will be unreachable for a few minutes.",
        local_note: "\n⚠ The bot runs on this machine: it will stop with it and come back when the machine does.",
        sent: Card {
            title: "✅ Reboot command sent",
            body: "The machine **{name}** is rebooting. It will be unreachable for a few minutes.",
        },
        sent_local: Card {
            title: "✅ Reboot command sent",
            body: "This machine is rebooting — the bot stops with it. It will come back when the machine does.",
        },
    },

    status: Status {
        title: "🖥 {name}",
        machine_online: "🟢 Machine online",
        service: "⚙ Service {unit}",
        cpu: "⚡ CPU",
        memory: "🧠 Memory",
        storage: "💾 Storage",
        load: "⚖ Load",
        uptime: "⏱ Uptime",
        service_uptime: "⚙ Service uptime",
        os: "🐧 System",
        kernel: "🧬 Kernel",
        address: "🌐 Address",
        backend: "🔌 Connection",
        cores: "cores",
        metrics: "ℹ️ Metrics",
        not_reported: "Not reported",
        state_running: "🟢 ONLINE",
        state_offline: "🔴 OFFLINE",
        state_unknown: "🟡 UNKNOWN",
        refresh: "Refresh",
        local_label: "Local",
        ssh_label: "SSH",
    },

    ping: Ping {
        failed: PingFailed {
            title: "🏓 Ping failed",
            body: "Machine **{name}** unreachable.\n{message}",
        },
        pong: Card {
            title: "🏓 Pong",
            body: "The machine **{name}** answered in **{ms} ms**.",
        },
    },

    manage: Manage {
        access_denied: Card {
            title: "Access denied",
            body: "Only the configured owner can manage operators.",
        },
        list: ManageList {
            title_one: "👥 Authorized operators",
            title_many: "👥 Authorized operators (page {page}/{totalPages})",
            header: "👑 Owner: {owner}",
            entry: "🛠 {name}",
        },
        clear: ManageClear {
            confirm_title: "⚠ Revoke all operators?",
            confirm_body: "The owner keeps access. Discord members and roles are not modified.",
            done_title: "Access reset",
            done_body: "All operator authorizations were removed. The owner keeps access.",
        },
        invalid: Card {
            title: "Invalid selection",
            body: "Pick a human member other than the owner.",
        },
        no_change: NoChange {
            title: "No change needed",
            body_add: "This member already has access.",
            body_remove: "This member has no operator authorization.",
        },
        confirm: ManageConfirm {
            title: "Confirm access change",
            body: "**{verb}** {username}?\nThis only changes access to the bot, not Discord roles or in-game permissions.",
            verb_add: "Authorize",
            verb_remove: "Revoke",
        },
        done: ManageDone {
            title: "✅ Access updated",
            body_add: "{username} can now use /status, /start, /stop, /restart and /ping.",
            body_remove: "{username} no longer has operator access.",
        },
    },

    bot_logs: BotLogs {
        access_denied: Card {
            title: "📜 Bot logs",
            body: "Owner only.",
        },
        title: "📜 Bot logs",
        empty: "The bot log file is empty or missing.",
    },

    logs: Logs {
        card_title: "📜 Logs",
        read_failed: "Could not read the logs: {message}",
        no_systemd: "`journalctl` is not available on the target machine. Set LOG_FILE in the .env to read a log file directly.",
        title_filtered: "📜 {count} line(s) matching \"{filter}\"",
        title_last: "📜 {count} last log lines",
        nothing_filtered: "No line matches \"{filter}\".",
        empty: "The log is empty.",
    },


    watchdog: Watchdog {
        down: Card {
            title: "🔴 Service **{name}** offline",
            body: "State changed **running → {state}**.\nAddress: `{address}`.\nCheck /status.",
        },
        collect_failed: Card {
            title: "⚠ Could not read **{service}**",
            body: "on **{target}**: {error}",
        },
        high_cpu: Card {
            title: "⚠ High CPU on **{name}**",
            body: "CPU: **{pct}%** (threshold {threshold}%).",
        },
        high_ram: Card {
            title: "⚠ High memory on **{name}**",
            body: "RAM: **{pct}%** ({used}/{max} MB).",
        },
        high_disk: Card {
            title: "⚠ Disk almost full on **{name}**",
            body: "Disk: **{pct}%** ({used}/{max} MB).",
        },
        unreachable: Card {
            title: "🔌 Target **{target}** unreachable",
            body: "Could not query the machine ({error}). Alerts for its services are suspended until it comes back.",
        },
        target_back: Card {
            title: "🔌 Target **{target}** is back",
            body: "The machine answers again — monitoring resumed.",
        },
        recovered: Card {
            title: "✅ Service **{name}** is back",
            body: "The service is back to **{state}** on **{target}**.",
        },
    },

    modules: Modules {
        access_denied: Card {
            title: "🔐 Modules",
            body: "Owner only.",
        },
        list_title: "🧩 Modules",
        list_hint: "Manage modules: `/modules enable <module>` / `/modules disable <module>`.",
        desc_watchdog: "Crash / CPU / RAM / disk / unreachable-target alerts",
        desc_scheduler: "Scheduled actions (stop/start/restart at fixed times)",
        desc_backups: "`/backups` commands (list and restore archives)",
        desc_exec: "Read-only diagnostic commands (`/exec`)",
        desc_metrics: "Prometheus metrics export (textfile)",
        on: "✅ enabled",
        off: "⛔ disabled",
        toggled_on: "Module **{module}** ENABLED.",
        toggled_off: "Module **{module}** DISABLED.",
        unknown: "Unknown module: `{module}`.",
        module_disabled: Card {
            title: "🧩 Module disabled",
            body: "The **{module}** module is disabled. The owner can enable it: `/modules enable {module}`.",
        },
    },

    schedule: Schedule {
        access_denied: Card {
            title: "📅 Scheduler",
            body: "Owner only.",
        },
        list_title: "📅 Scheduled actions",
        empty: "No scheduled action. Add one: `/schedule add daily 04:00 restart`.",
        added: "Scheduled: **#{id}** — `{when}` → **{action}** on `{target}/{service}`.",
        removed: "Entry **#{id}** removed.",
        cleared: "Scheduler cleared.",
        unknown_id: "Unknown entry: **#{id}**.",
        invalid_when: "Expected format: `daily HH:MM`, `weekly D HH:MM` (D: 1=Monday … 7=Sunday) or `once YYYY-MM-DD HH:MM`.",
        invalid_past: "Past date for `once`: `{when}` — a missed entry is never run late.",
        invalid_action: "Unknown action: `{action}` (expected: start, stop, restart).",
        unknown_target: "Unknown target: `{target}`.",
        unknown_service: "Unknown service: `{service}`.",
        audit_ok: "Scheduled: {action} ({target}/{service}) — {summary}",
        audit_fail: "Scheduled: {action} ({target}/{service}) — FAILED: {error}",
    },

    backups: Backups {
        access_denied: Card {
            title: "💾 Restore",
            body: "Owner only (overwrites the service's current state).",
        },
        list_title: "💾 Backups",
        no_saves: "SAVES_PATH is not configured: no backup possible.",
        empty: "No archive in `<SAVES_PATH>-backups/`.",
        listing: "Archives of **{service}** (most recent at the bottom):",
        restored: "Archive **{archive}** restored. The service is STILL stopped — restart it when ready.",
        confirm_title: "💾 Backup restore",
        confirm_body: "Restore **{archive}** on **{target}/{service}**?\nThe current state will be overwritten (a safety copy is made on the machine). The service MUST be stopped.",
    },

    exec: Exec {
        access_denied: Card {
            title: "🔎 Diagnostics",
            body: "Owner only.",
        },
        title: "🔎 Diagnostic",
        hint: "Read-only builtins: `journal` (a unit's journalctl), `ss` (listening ports), `df` (disks), `ip` (addresses), `uptime`, `who`.",
        unknown: "Unknown builtin: `{builtin}`.",
        invalid_unit: "Invalid unit name (charset `[A-Za-z0-9_.@:-]`, 256 max) — required for `journal`.",
    },

    setup: Setup {
        intro: "\n🛠  First configuration detected — a few values are required.\n",
        completion_intro: "\n🛠  Some required values are missing from {envFile} — completing them.\n",
        file_line: "   File: {envFile}\n\n",
        language_label: "Language / Langue [en/fr]",
        prompt_line: "   {question} : ",
        prompt_line_hidden: "🔒 {question} : ",
        prompt: SetupPrompt {
            discord_token: "Discord bot token",
            discord_client_id: "Discord application ID",
            discord_owner_id: "Discord owner user ID",
            discord_guild_id: "Discord server ID",
            targets: "Targets (alias=spec, comma-separated — \"local\" or \"user@host[:port]\" — e.g. \"local,prod=root@1.2.3.4\")",
            ssh_user: "SSH user (default for targets without \"user@\")",
            ssh_password: "SSH password (optional — or use a key)",
            ssh_key_path: "SSH private key path (optional, default ~/.ssh/id_ed25519 then id_rsa)",
            services: "Services (alias=unit, comma-separated — e.g. \"web=nginx, game=myserver\")",
            systemctl_prefix: "systemctl prefix if root is needed (e.g. \"sudo\", or empty)",
            alert_channel: "Discord alert channel ID for the watchdog (optional)",
            saves_path: "Directory to back up (SAVES_PATH, empty = no copy): e.g. /opt/game/saves",
        },
        invalid: "   ⚠ Invalid value, please retry.\n",
        complete: "\n✅ Configuration saved to {envFile}\n\n",
        path_hint: "💡 To call `dkbot` from anywhere, add the binary's directory to your PATH: `export PATH=\"{dir}:$PATH\"` (in ~/.bashrc or ~/.zshrc).",
    },

    errors: Errors {
        missing_config: "Missing configuration: {key}",
        invalid_target: "Invalid target \"{alias}\": {reason}",
        invalid_service: "Missing configuration: define SERVICE or SERVICES",
        ssh_auth: "SSH authentication refused. Check SSH_USER, SSH_PASSWORD and SSH_KEY_PATH.{suffix}",
        ssh_network: "Cannot reach the machine (SSH/network error). Check the host and port in TARGETS.{suffix}",
        ssh_timeout: "The machine did not answer in time.{suffix}",
        ssh_timeout_sent: "The machine did not answer in time after the command was sent. The action may have been applied — check /status. No new command was sent.{suffix}",
        command_rejected: "The command was rejected by the machine: {message}.{suffix}",
        generic: "Command failed. {message}{suffix}",
        power_suffix: " No automatic retry of power commands.",
        setup_incomplete_non_interactive: "Incomplete configuration and --non-interactive was given. Missing: {missing}. Fill {envFile} manually or provide the variables via the environment.",
        setup_incomplete_no_tty: "Incomplete configuration and no interactive terminal available. Missing: {missing}. Fill {envFile} manually or provide the variables via the environment.",
        invalid_users_json_shape: "Invalid users.json (expected an array of Discord IDs). Fix or delete the file, then restart.",
        deploy_missing_access: "Discord refused: Missing Access (50001). The bot is not present on guild {guild}, or DISCORD_CLIENT_ID does not match the token. Invite the bot: https://discord.com/oauth2/authorize?client_id={client}&scope=bot+applications.commands",
        deploy_unknown_application: "Discord refused: unknown application. DISCORD_CLIENT_ID={client} matches no application for this token. Check the Application ID in the Discord developer portal.",
        internal_panic: "The bot hit an unexpected internal error — it stays alive, but check the log file for details.",
    },

    audit: Audit {
        power_requested: "Audit: power action '{action}' requested by {username} ({userId}) on '{service}' at '{target}'",
        power_send_failed: "Audit: transmission '{action}' of '{service}' at '{target}' failed after {ms}ms: {error}",
        power_send_ok: "Audit: transmission '{action}' of '{service}' at '{target}': OK in {ms}ms",
        power_tracking_cancelled: "Audit: tracking of '{action}' on '{service}' cancelled by user",
        power_applied_despite_error: "Audit: action '{action}' on '{service}' applied despite transmission error (state {state})",
        power_send_error_polling: "Audit: transmission '{action}' of '{service}' in error — switching to state tracking (no power retry): {error}",
        power_confirmed: "Audit: power action '{action}' on '{service}' confirmed in {seconds}s",
        power_not_confirmed: "Audit: power action '{action}' on '{service}' not confirmed within 10 minutes (failures={failures})",
        backup_copied: "Backup: saves copied to {dest} ({service}@{target})",
        backup_missing_path: "Backup: not possible, SAVES_PATH is not set ({service}@{target})",
        backup_failed: "Backup: FAILED ({service}@{target}) — {error}",
        backup_skipped: "Backup: skipped ({service}@{target}) — {reason}",
        reboot_requested: "Audit: reboot requested by {username} ({userId}) on '{target}'",
        reboot_sent: "Audit: reboot command sent to '{target}' in {ms}ms",
        reboot_failed: "Audit: reboot of '{target}' failed: {error}",
        operator_granted: "Audit: operator {username} ({target}) authorized by the owner ({ownerId})",
        operator_revoked: "Audit: operator {username} ({target}) revoked by the owner ({ownerId})",
        operators_cleared: "Audit: all operators cleared by the owner ({ownerId})",
        commands_skipped: "Slash commands unchanged, registration skipped.",
        commands_registered: "Slash commands registered.",
        commands_global_purged: "Global commands purged (no global/guild duplicate possible).",
        commands_global_purge_failed: "Global commands purge failed: {error}",
        commands_old_guild_purged: "Old guild {guild} commands purged.",
        commands_old_guild_purge_failed: "Old guild {guild} commands purge failed: {error}",
        watchdog_disabled: "Watchdog disabled (DISCORD_ALERT_CHANNEL_ID not set).",
        watchdog_started: "Watchdog started (interval {interval}ms, channel {channel}).",
        watchdog_alert_sent: "Watchdog: alert sent — {title}",
        watchdog_alert_failed: "Watchdog: could not send the alert: {error}",
        notify_sent: "Audit: notification sent — {title}",
        notify_failed: "Audit: could not notify: {error}",
        watchdog_check_failed: "Watchdog: check failed for '{service}': {error}",
        lock_write_failed: "Could not write the lock file: {error}",
        sessions_write_failed: "Could not write the sessions file: {error}",
        users_invalid_entry: "Invalid entry ignored in users.json: {entry}",
        users_not_array: "users.json is not an array.",
    },
};
