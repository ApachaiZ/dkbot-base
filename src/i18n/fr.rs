//! Catalogue français. Parité stricte avec `en.rs` : les deux fichiers
//! instancient le même type `Catalog`, toute divergence de clé est une
//! erreur de compilation.

use super::*;

pub static CATALOG: Catalog = Catalog {
    common: Common {
        access_denied_member: Card {
            title: "Accès refusé",
            body: "Demandez au propriétaire du bot de vous accorder l'accès opérateur.",
        },
        access_denied_guild: Card {
            title: "Accès refusé",
            body: "Ce bot est configuré pour un autre serveur Discord.",
        },
        unknown_target: Card {
            title: "Machine inconnue",
            body: "Aucune machine nommée `{alias}`.",
        },
        unknown_service: Card {
            title: "Service inconnu",
            body: "Aucun service nommé `{alias}`.",
        },
        session_reset_target: Card {
            title: "🎯 Session réinitialisée",
            body: "Votre machine de session `{alias}` n'existe plus. Session effacée — les commandes cibleront désormais la machine par défaut.",
        },
        session_reset_service: Card {
            title: "🎯 Session réinitialisée",
            body: "Votre service de session `{alias}` n'existe plus. Session effacée — les commandes cibleront désormais le service par défaut.",
        },
        default_target: Card {
            title: "🎯 Machine par défaut",
            body: "Votre session cible désormais **{alias}**.",
        },
        default_service: Card {
            title: "🎯 Service par défaut",
            body: "Votre session cible désormais le service **{alias}**.",
        },
        generic_error: TitleOnly { title: "⚠ Erreur" },
    },

    confirm: Confirm {
        confirm: "Confirmer",
        cancel: "Annuler",
        cancelled: Card {
            title: "Annulé",
            body: "Aucune modification effectuée.",
        },
        processing: Card {
            title: "Traitement",
            body: "Veuillez patienter…",
        },
        expired: Card {
            title: "Expiré",
            body: "Confirmation expirée. Relancez la commande.",
        },
    },

    help: Card {
        title: "🎛 Contrôle du VPS",
        body: "**Machine**\n/status — tableau de bord en direct (CPU, RAM, disque, charge, uptime, OS, service)\n/start — démarrer le service applicatif\n/stop — arrêter le service applicatif\n/restart — redémarrer le service applicatif\n/ping — mesurer la latence vers la machine\n/target — définir votre machine par défaut pour cette session\n/server — définir votre service par défaut pour cette session\nLes commandes acceptent des options `target` et `service`.\n\n**Propriétaire uniquement**\n/reboot — redémarrer la machine elle-même\n/users add — mentionner et autoriser\n/users remove — mentionner et révoquer\n/users list — voir les opérateurs\n/users clear — réinitialiser les accès\n/logs — voir les dernières lignes de log du service\n\nLes commandes power exigent une confirmation. Toutes les réponses sont privées. Les autorisations ne concernent que ce bot.",
    },

    power: Power {
        actions: Actions {
            start: PowerAction { verb: "Démarrer", cap: "Démarrage", title: "Confirmer le démarrage" },
            stop: PowerAction { verb: "Arrêter", cap: "Arrêt", title: "Confirmer l'arrêt" },
            restart: PowerAction { verb: "Redémarrer", cap: "Redémarrage", title: "Confirmer le redémarrage" },
        },
        access_denied: Card {
            title: "Accès refusé",
            body: "Demandez au propriétaire du bot de vous accorder l'accès opérateur.",
        },
        action_in_progress: Card {
            title: "Action en cours",
            body: "Une action **{action}** est déjà en cours de suivi sur **{name}**.",
        },
        confirm: PowerConfirm {
            warning: "⚠ Cela interrompt le service en cours.\n",
            body: "{verb} le service **{name}** sur **{target}** ?",
        },
        access_revoked: Card {
            title: "Accès révoqué",
            body: "Votre accès opérateur a été révoqué.",
        },
        busy: Card {
            title: "Occupé",
            body: "Une autre action a démarré sur **{name}** pendant votre confirmation.",
        },
        already_in_target_state: Card {
            title: "Déjà dans l'état cible",
            body: "Le service est **{state}**. Aucune commande envoyée.",
        },
        accepted: Card {
            title: "⏳ Action acceptée",
            body: "Commande transmise. Suivi silencieux (espacement progressif), pendant 10 minutes maximum.",
        },
        accepted_after_error: Card {
            title: "⏳ Transmission incertaine — suivi en cours",
            body: "**{cap}** transmis, mais la machine a renvoyé une erreur. Suivi silencieux de l'état réel (espacement progressif), pendant 10 minutes maximum.\n**Aucune seconde commande ne sera envoyée.**",
        },
        backup_ok: "💾 Sauvegarde copiée : `{dest}`",
        backup_missing: "💾 Sauvegarde : impossible — SAVES_PATH n'est pas défini dans la configuration",
        backup_failed: "💾 Sauvegarde : échec — {error}",
        backup_skipped_running: "💾 Sauvegarde : non effectuée — le service n'était pas arrêté au moment de la copie",
        backup_locked: "💾 Sauvegarde : non effectuée — une autre copie est déjà en cours",
        backup_fresh: "💾 Sauvegarde : non répétée — une copie date de moins d'une heure",
        with_backup_note: "\n💾 Sauvegarde pendant l'arrêt (stop → copie → start).",
        start_failed_note: "\n⚠ Le démarrage a échoué : {error} — le service est ARRÊTÉ.",
        completed: Completed {
            title: "✅ Action terminée",
            body: "**{cap}** confirmé sur **{machine}**.\nÉtat actuel : **{state}**\nDurée : {seconds} secondes.",
            tail: "\nLe jeu peut encore nécessiter un peu de temps pour devenir joignable.",
        },
        applied_despite_error: Card {
            title: "✅ Action effectuée",
            body: "**{cap}** effectué sur la machine.\nÉtat vérifié : **{state}**.\n\n*Note : la machine a renvoyé une erreur pendant la transmission, mais l'action a bien été appliquée. Aucune nouvelle commande n'a été envoyée.*",
        },
        eta: "⏱ En moyenne : {seconds} s",
        cancel_tracking: "❌ Annuler le suivi",
        tracking_cancelled: Card {
            title: "Suivi annulé",
            body: "Le suivi a été arrêté. L'action peut continuer en arrière-plan — vérifiez /status.",
        },
        not_confirmed: NotConfirmed {
            title: "⚠ Fin non confirmée",
            body_failed: "La commande a été acceptée, mais les vérifications d'état ont échoué à plusieurs reprises.\nDernier état connu : **{state}**.\nUtilisez /status. **Aucune seconde commande n'a été envoyée.**",
            body_timeout: "La commande a été acceptée, mais la fin n'a pas pu être vérifiée dans les 10 minutes.\nDernier état connu : **{state}**.\nUtilisez /status. **Aucune seconde commande n'a été envoyée.**",
            body_transmit_error: "La transmission a renvoyé une erreur, mais l'action a pu être appliquée. La fin n'a pas pu être vérifiée dans les 10 minutes.\nDernier état connu : **{state}**.\nUtilisez /status. **Aucune seconde commande n'a été envoyée.**",
        },
    },

    reboot: Reboot {
        access_denied: Card {
            title: "Accès refusé",
            body: "Seul le propriétaire configuré peut redémarrer la machine.",
        },
        confirm_title: "Confirmer le redémarrage",
        confirm_body: "Redémarrer la machine **{name}** ?\nToutes les connexions seront coupées — la machine sera injoignable quelques minutes.",
        local_note: "\n⚠ Le bot tourne sur cette machine : il s'arrêtera avec elle et reviendra quand elle reviendra.",
        sent: Card {
            title: "✅ Redémarrage transmis",
            body: "La machine **{name}** redémarre. Elle sera injoignable quelques minutes.",
        },
        sent_local: Card {
            title: "✅ Redémarrage transmis",
            body: "Cette machine redémarre — le bot s'arrête avec elle. Il reviendra quand la machine reviendra.",
        },
    },

    status: Status {
        title: "🖥 {name}",
        machine_online: "🟢 Machine en ligne",
        service: "⚙ Serveur {unit}",
        cpu: "⚡ CPU",
        memory: "🧠 Mémoire",
        storage: "💾 Stockage",
        load: "⚖ Charge",
        uptime: "⏱ Uptime",
        service_uptime: "⚙ Uptime service",
        os: "🐧 Système",
        kernel: "🧬 Noyau",
        address: "🌐 Adresse",
        backend: "🔌 Connexion",
        cores: "cœurs",
        metrics: "ℹ️ Métriques",
        not_reported: "Non rapporté",
        state_running: "🟢 EN LIGNE",
        state_offline: "🔴 HORS LIGNE",
        state_unknown: "🟡 INCONNU",
        refresh: "Rafraîchir",
        local_label: "Locale",
        ssh_label: "SSH",
    },

    ping: Ping {
        failed: PingFailed {
            title: "🏓 Ping échoué",
            body: "Machine **{name}** injoignable.\n{message}",
        },
        pong: Card {
            title: "🏓 Pong",
            body: "La machine **{name}** a répondu en **{ms} ms**.",
        },
    },

    manage: Manage {
        access_denied: Card {
            title: "Accès refusé",
            body: "Seul le propriétaire configuré peut gérer les opérateurs.",
        },
        list: ManageList {
            title_one: "👥 Opérateurs autorisés",
            title_many: "👥 Opérateurs autorisés (page {page}/{totalPages})",
            header: "👑 Propriétaire : {owner}",
            entry: "🛠 {name}",
        },
        clear: ManageClear {
            confirm_title: "⚠ Révoquer tous les opérateurs ?",
            confirm_body: "Le propriétaire conserve l'accès. Les membres et rôles Discord ne sont pas modifiés.",
            done_title: "Accès réinitialisé",
            done_body: "Toutes les autorisations d'opérateur ont été retirées. L'accès du propriétaire est conservé.",
        },
        invalid: Card {
            title: "Sélection invalide",
            body: "Choisissez un membre humain autre que le propriétaire.",
        },
        no_change: NoChange {
            title: "Aucun changement nécessaire",
            body_add: "Ce membre a déjà accès.",
            body_remove: "Ce membre n'a aucune autorisation d'opérateur.",
        },
        confirm: ManageConfirm {
            title: "Confirmer le changement d'accès",
            body: "**{verb}** {username} ?\nCela modifie uniquement l'accès au bot, pas les rôles Discord ni les permissions en jeu.",
            verb_add: "Autoriser",
            verb_remove: "Révoquer",
        },
        done: ManageDone {
            title: "✅ Accès mis à jour",
            body_add: "{username} peut désormais utiliser /status, /start, /stop, /restart et /ping.",
            body_remove: "{username} n'a plus l'accès opérateur.",
        },
    },

    bot_logs: BotLogs {
        access_denied: Card {
            title: "📜 Journaux du bot",
            body: "Réservé au propriétaire.",
        },
        title: "📜 Journaux du bot",
        empty: "Le fichier de log du bot est vide ou absent.",
    },

    logs: Logs {
        card_title: "📜 Logs",
        read_failed: "Impossible de lire les logs : {message}",
        no_systemd: "`journalctl` n'est pas disponible sur la machine cible. Définissez LOG_FILE dans le .env pour lire un fichier de log directement.",
        title_filtered: "📜 {count} ligne(s) correspondant à \"{filter}\"",
        title_last: "📜 {count} dernières lignes de log",
        nothing_filtered: "Aucune ligne ne correspond à \"{filter}\".",
        empty: "Le log est vide.",
    },


    watchdog: Watchdog {
        down: Card {
            title: "🔴 Service **{name}** hors ligne",
            body: "Changement d'état **running → {state}**.\nAdresse : `{address}`.\nVérifiez /status.",
        },
        collect_failed: Card {
            title: "⚠ Lecture impossible pour **{service}**",
            body: "sur **{target}** : {error}",
        },
        high_cpu: Card {
            title: "⚠ CPU élevé sur **{name}**",
            body: "CPU : **{pct}%** (seuil {threshold}%).",
        },
        high_ram: Card {
            title: "⚠ Mémoire élevée sur **{name}**",
            body: "RAM : **{pct}%** ({used}/{max} MB).",
        },
        high_disk: Card {
            title: "⚠ Disque presque plein sur **{name}**",
            body: "Disque : **{pct}%** ({used}/{max} MB).",
        },
        unreachable: Card {
            title: "🔌 Cible **{target}** injoignable",
            body: "Impossible d'interroger la machine ({error}). Les alertes de ses services sont suspendues jusqu'à son retour.",
        },
        target_back: Card {
            title: "🔌 Cible **{target}** de retour",
            body: "La machine répond de nouveau — la surveillance reprend.",
        },
        recovered: Card {
            title: "✅ Service **{name}** de retour",
            body: "Le service est revenu à l'état **{state}** sur **{target}**.",
        },
    },

    modules: Modules {
        access_denied: Card {
            title: "🔐 Modules",
            body: "Réservé au propriétaire.",
        },
        list_title: "🧩 Modules",
        list_hint: "Gérez les modules : `/modules enable <module>` / `/modules disable <module>`.",
        desc_watchdog: "Alertes crash / CPU / RAM / disque / machine injoignable",
        desc_scheduler: "Actions planifiées (stop/start/restart à heure fixe)",
        desc_backups: "Commandes `/backups` (liste et restauration des archives)",
        desc_exec: "Commandes diagnostics en lecture seule (`/exec`)",
        desc_metrics: "Export métriques Prometheus (fichier textfile)",
        on: "✅ activé",
        off: "⛔ désactivé",
        toggled_on: "Module **{module}** ACTIVÉ.",
        toggled_off: "Module **{module}** DÉSACTIVÉ.",
        unknown: "Module inconnu : `{module}`.",
        module_disabled: Card {
            title: "🧩 Module désactivé",
            body: "Le module **{module}** est désactivé. Le propriétaire peut l'activer : `/modules enable {module}`.",
        },
    },

    schedule: Schedule {
        access_denied: Card {
            title: "📅 Planificateur",
            body: "Réservé au propriétaire.",
        },
        list_title: "📅 Actions planifiées",
        empty: "Aucune action planifiée. Ajoutez-en une : `/schedule add daily 04:00 restart`.",
        added: "Planifiée : **#{id}** — `{when}` → **{action}** sur `{target}/{service}`.",
        removed: "Entrée **#{id}** retirée.",
        cleared: "Planificateur vidé.",
        unknown_id: "Entrée inconnue : **#{id}**.",
        invalid_when: "Format attendu : `daily HH:MM`, `weekly D HH:MM` (D : 1=lundi … 7=dimanche) ou `once AAAA-MM-JJ HH:MM`.",
        invalid_past: "Date passée pour `once` : `{when}` — une entrée manquée n'est jamais exécutée en retard.",
        invalid_action: "Action inconnue : `{action}` (attendu : start, stop, restart).",
        unknown_target: "Cible inconnue : `{target}`.",
        unknown_service: "Service inconnu : `{service}`.",
        audit_ok: "Planifiée : {action} ({target}/{service}) — {summary}",
        audit_fail: "Planifiée : {action} ({target}/{service}) — ÉCHEC : {error}",
    },

    backups: Backups {
        access_denied: Card {
            title: "💾 Restauration",
            body: "Réservé au propriétaire (écrase l'état actuel du service).",
        },
        list_title: "💾 Sauvegardes",
        no_saves: "SAVES_PATH n'est pas configuré : aucune sauvegarde possible.",
        empty: "Aucune archive dans `<SAVES_PATH>-backups/`.",
        listing: "Archives de **{service}** (les plus récentes en bas) :",
        restored: "Archive **{archive}** restaurée. Le service est TOUJOURS arrêté — relancez-le quand vous êtes prêt.",
        confirm_title: "💾 Restauration de sauvegarde",
        confirm_body: "Restaurer **{archive}** sur **{target}/{service}** ?\nL'état actuel sera écrasé (une copie de sûreté est faite côté machine). Le service DOIT être arrêté.",
    },

    exec: Exec {
        access_denied: Card {
            title: "🔎 Diagnostics",
            body: "Réservé au propriétaire.",
        },
        title: "🔎 Diagnostic",
        hint: "Builtins en lecture seule : `journal` (journalctl d'une unité), `ss` (ports en écoute), `df` (disques), `ip` (adresses), `uptime`, `who`.",
        unknown: "Builtin inconnu : `{builtin}`.",
        invalid_unit: "Nom d'unité invalide (alphabet `[A-Za-z0-9_.@:-]`, 256 max) — requis pour `journal`.",
    },

    setup: Setup {
        intro: "\n🛠  Première configuration détectée — quelques valeurs sont requises.\n",
        completion_intro: "\n🛠  Des valeurs requises manquent dans {envFile} — complétion.\n",
        file_line: "   Fichier : {envFile}\n\n",
        language_label: "Language / Langue [en/fr]",
        prompt_line: "   {question} : ",
        prompt_line_hidden: "🔒 {question} : ",
        prompt: SetupPrompt {
            discord_token: "Token du bot Discord",
            discord_client_id: "ID d'application Discord",
            discord_owner_id: "ID utilisateur du propriétaire Discord",
            discord_guild_id: "ID du serveur Discord",
            targets: "Cibles (alias=spec, séparées par des virgules — « local » ou « user@hôte[:port] » — ex. « local,prod=root@1.2.3.4 »)",
            ssh_user: "Utilisateur SSH (défaut pour les cibles sans « user@ »)",
            ssh_password: "Mot de passe SSH (optionnel — ou utilisez une clé)",
            ssh_key_path: "Chemin de la clé privée SSH (optionnel, défaut ~/.ssh/id_ed25519 puis id_rsa)",
            services: "Services (alias=unité, séparés par des virgules — ex. « web=nginx, game=myserver »)",
            systemctl_prefix: "Préfixe systemctl si root est requis (ex. « sudo », ou vide)",
            alert_channel: "ID du salon Discord d'alertes du watchdog (optionnel)",
            saves_path: "Chemin du dossier à sauvegarder (SAVES_PATH, vide = pas de copie) : ex. /opt/game/saves",
        },
        invalid: "   ⚠ Valeur invalide, veuillez réessayer.\n",
        complete: "\n✅ Configuration enregistrée dans {envFile}\n\n",
        path_hint: "💡 Pour appeler `dkbot` depuis n'importe où, ajoutez le dossier du binaire à votre PATH : `export PATH=\"{dir}:$PATH\"` (dans ~/.bashrc ou ~/.zshrc).",
    },

    errors: Errors {
        missing_config: "Configuration manquante : {key}",
        invalid_target: "Cible \"{alias}\" invalide : {reason}",
        invalid_service: "Configuration manquante : définissez SERVICE ou SERVICES",
        ssh_auth: "Authentification SSH refusée. Vérifiez SSH_USER, SSH_PASSWORD et SSH_KEY_PATH.{suffix}",
        ssh_network: "Impossible de joindre la machine (erreur réseau/SSH). Vérifiez l'hôte et le port dans TARGETS.{suffix}",
        ssh_timeout: "La machine n'a pas répondu à temps.{suffix}",
        ssh_timeout_sent: "La machine n'a pas répondu à temps après l'envoi de la commande. L'action a pu être appliquée — vérifiez /status. Aucune nouvelle commande n'a été envoyée.{suffix}",
        command_rejected: "La commande a été refusée par la machine : {message}.{suffix}",
        generic: "Commande échouée. {message}{suffix}",
        power_suffix: " Aucun retry automatique des commandes power.",
        setup_incomplete_non_interactive: "Configuration incomplète et --non-interactive a été fourni. Manquantes : {missing}. Remplissez {envFile} manuellement ou fournissez les variables via l'environnement.",
        setup_incomplete_no_tty: "Configuration incomplète et aucun terminal interactif disponible. Manquantes : {missing}. Remplissez {envFile} manuellement ou fournissez les variables via l'environnement.",
        invalid_users_json_shape: "users.json invalide (tableau d'IDs Discord attendu). Corrigez ou supprimez le fichier, puis redémarrez.",
        deploy_missing_access: "Discord a refusé : Missing Access (50001). Le bot n'est pas présent sur la guilde {guild}, ou DISCORD_CLIENT_ID ne correspond pas au token. Invitez le bot : https://discord.com/oauth2/authorize?client_id={client}&scope=bot+applications.commands",
        deploy_unknown_application: "Discord a refusé : application inconnue. DISCORD_CLIENT_ID={client} ne correspond à aucune application pour ce token. Vérifiez l'Application ID dans le portail développeur Discord.",
        internal_panic: "Le bot a rencontré une erreur interne inattendue — il reste en vie, mais consultez le fichier de log pour le détail.",
    },

    audit: Audit {
        power_requested: "Audit : action power '{action}' demandée par {username} ({userId}) sur '{service}' de '{target}'",
        power_send_failed: "Audit : transmission '{action}' de '{service}' sur '{target}' en échec après {ms}ms : {error}",
        power_send_ok: "Audit : transmission '{action}' de '{service}' sur '{target}' : OK en {ms}ms",
        power_tracking_cancelled: "Audit : suivi de l'action '{action}' sur '{service}' annulé par l'utilisateur",
        power_applied_despite_error: "Audit : action '{action}' sur '{service}' appliquée malgré l'erreur de transmission (état {state})",
        power_send_error_polling: "Audit : transmission '{action}' de '{service}' en erreur — bascule sur le suivi d'état (aucun retry power) : {error}",
        power_confirmed: "Audit : action power '{action}' sur '{service}' confirmée en {seconds}s",
        power_not_confirmed: "Audit : action power '{action}' sur '{service}' non confirmée dans les 10 minutes (échecs={failures})",
        backup_copied: "Sauvegarde : copiée vers {dest} ({service}@{target})",
        backup_missing_path: "Sauvegarde : impossible, SAVES_PATH non défini ({service}@{target})",
        backup_failed: "Sauvegarde : ÉCHEC ({service}@{target}) — {error}",
        backup_skipped: "Sauvegarde : ignorée ({service}@{target}) — {reason}",
        reboot_requested: "Audit : redémarrage machine demandé par {username} ({userId}) sur '{target}'",
        reboot_sent: "Audit : commande de redémarrage envoyée à '{target}' en {ms}ms",
        reboot_failed: "Audit : redémarrage de '{target}' en échec : {error}",
        operator_granted: "Audit : opérateur {username} ({target}) autorisé par le propriétaire ({ownerId})",
        operator_revoked: "Audit : opérateur {username} ({target}) révoqué par le propriétaire ({ownerId})",
        operators_cleared: "Audit : tous les opérateurs effacés par le propriétaire ({ownerId})",
        commands_skipped: "Commandes slash inchangées, enregistrement ignoré.",
        commands_registered: "Commandes slash enregistrées.",
        commands_global_purged: "Commandes globales purgées (aucun doublon global/guilde possible).",
        commands_global_purge_failed: "Purge des commandes globales impossible : {error}",
        commands_old_guild_purged: "Commandes de l'ancienne guilde {guild} purgées.",
        commands_old_guild_purge_failed: "Purge des commandes de l'ancienne guilde {guild} impossible : {error}",
        watchdog_disabled: "Watchdog désactivé (DISCORD_ALERT_CHANNEL_ID non défini).",
        watchdog_started: "Watchdog démarré (intervalle {interval}ms, salon {channel}).",
        watchdog_alert_sent: "Watchdog : alerte envoyée — {title}",
        watchdog_alert_failed: "Watchdog : impossible d'envoyer l'alerte : {error}",
        notify_sent: "Audit : notification envoyée — {title}",
        notify_failed: "Audit : notification impossible : {error}",
        watchdog_check_failed: "Watchdog : vérification échouée pour '{service}' : {error}",
        lock_write_failed: "Impossible d'écrire le fichier de verrou : {error}",
        sessions_write_failed: "Impossible d'écrire le fichier de sessions : {error}",
        users_invalid_entry: "Entrée invalide ignorée dans users.json : {entry}",
        users_not_array: "users.json n'est pas un tableau.",
    },
};
