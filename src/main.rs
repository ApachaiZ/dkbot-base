//! Point d'entrée du bot dakkabot (contrôle fin d'un VPS depuis Discord).
//!
//! Ordre de démarrage :
//! 1. `ensure_env` : complète `.config.d/.env` de façon interactive si
//!    besoin (ou refuse de démarrer avec la liste des variables manquantes
//!    en mode `--non-interactive` / sans TTY) ;
//! 2. langue résolue (`--lang=xx` > `BOT_LANGUAGE` > `en`) ;
//! 3. configuration validée → backends construits → état partagé ;
//! 4. enregistrement des slash commands (hash + purge, voir `deploy.rs`) ;
//! 5. connexion gateway + démarrage du watchdog au premier Ready.
//!
//! Codes de sortie : 78 = erreur de CONFIGURATION (systemd ne redémarre
//! pas en boucle), 1 = tout le reste. Arrêt propre : SIGINT/SIGTERM.

mod api;
mod assets;
mod backends;
mod backups;
mod commands;
mod config;
mod confirm;
mod crypto;
mod daemon;
mod deploy;
mod doctor;
mod embeds;
mod errors;
mod exec;
mod fsutil;
mod i18n;
mod logger;
mod logs_cmd;
mod manage;
mod members;
mod metrics;
mod modules;
mod notify;
mod paths;
mod plugin;
mod plugin_commands;
mod power;
mod schedule;
mod setup;
mod state;
mod stats;
mod status;
mod targets;
mod update;
mod watchdog;

use poise::serenity_prelude as serenity;
use serenity::GatewayIntents;
use std::sync::Arc;

use crate::commands::Data;
use crate::errors::ExitError;

/// Parcours la chaîne de causes pour retrouver un [`ExitError`] (code 78).
fn find_exit_code(err: &(dyn std::error::Error + 'static)) -> Option<i32> {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(err);
    while let Some(e) = current {
        if let Some(exit) = e.downcast_ref::<ExitError>() {
            return Some(exit.code);
        }
        current = e.source();
    }
    None
}

/// Arrêt propre sur Ctrl-C (SIGINT) ou SIGTERM.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("installation du handler SIGTERM");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

// ── Optimisation du runtime : 2 worker threads suffisent ──
// Le bot est I/O-bound (gateway Discord, commandes locales, SSH en threads
// bloquants). Chaque worker tokio = une pile de thread + l'état de
// l'exécuteur.
#[tokio::main(worker_threads = 2)]
async fn main() {
    // Panique : on log AVANT le hook par défaut (les panics sont contenus
    // par poise, mais autant les rendre visibles dans le fichier de log).
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        logger::error(format!("panic: {info}"));
        default_hook(info);
    }));

    let args: Vec<String> = std::env::args().collect();
    let non_interactive = args.iter().any(|a| a == "--non-interactive");
    // `--root <dir>` : toutes les données dans ce dossier (prioritaire sur
    // le registre install.json et sur ~/.config/dakkabot).
    daemon::apply_root_override(&args);
    if args.iter().any(|a| a == "-h" || a == "--help") {
        eprintln!(
            "dkbot — Discord bot for fine-grained VPS control (local or SSH).\n\n\
             Usage: dkbot [--root <dir>] [--non-interactive] [--lang=fr|en]\n\
                   dkbot run                     # bot au premier plan\n\
                   dkbot start|stop|restart      # daemon autonome (superviseur)\n\
                   dkbot status                  # état du daemon\n\
                   dkbot logs [--lines N]        # fin du log du bot\n\
                   dkbot install [<dir>] [--daemon]\n\
                   dkbot uninstall\n\
                   dkbot doctor                  # diagnostic local\n\
                   dkbot update [vX.Y.Z]         # mises à jour via releases\n\
                   dkbot plugin install|list|remove <id>  # plugins dynamiques (.so, SHA-256)\n\n\
             Installation : cargo install --path . (binaire dans ~/.cargo/bin,\n\
             données dans ~/.config/dakkabot) OU dkbot install — copie directe\n\
             du binaire courant (architecture de la machine) : sans <dir>, dans\n\
             /usr/bin et /bin (root requis), données dans ~/.config/dakkabot ;\n\
             avec <dir>, binaire + registre install.json dans <dir>.\n\
             --daemon = unité systemd en plus (root requis).\n\
             Données : --root <dir> > registre install.json > ~/.config/dakkabot/.\n\
             Le premier lancement crée .config.d/.env de façon interactive.\n\
             Exit codes: 78 = configuration error, 1 = other failures."
        );
        std::process::exit(0);
    }

    // ── Sous-commandes daemon (avant toute configuration) ──
    if let Some(sub) = args.get(1).map(|s| s.as_str()) {
        if sub == "supervise" {
            std::process::exit(daemon::supervise().await);
        }
        if sub == "update" || sub == "module" || sub == "plugin" {
            std::process::exit(update::run(&args).await);
        }
        // Commandes héritées NON portées : refuser EXPLICITEMENT au lieu de
        // démarrer un second bot (un « dkbot mission-control » par mégarde
        // ouvrirait une 2e session Discord → réponses en double,
        // « Unknown interaction »).
        const UNIMPLEMENTED: [&str; 6] = [
            "mission-control",
            "monitor",
            "tui",
            "completions",
            "config",
            "help",
        ];
        if UNIMPLEMENTED.contains(&sub) {
            eprintln!(
                "dkbot : « {sub} » n'est pas implémenté. Commandes : run | start | stop | restart | status | logs | install | uninstall"
            );
            std::process::exit(1);
        }
        let handled = matches!(
            sub,
            "start"
                | "stop"
                | "restart"
                | "status"
                | "logs"
                | "install"
                | "reinstall"
                | "uninstall"
        );
        if handled {
            let outcome: Result<String, String> = match sub {
                "install" | "reinstall" => {
                    // Sans argument : cibles système STANDARD (/usr/bin,
                    // /bin) et données dans ~/.config/dakkabot. Le
                    // premier token non-flag est le dossier —
                    // `install --daemon <dir>` fonctionne aussi.
                    let dir = {
                        let mut it = args.iter().skip(2);
                        let mut found = None;
                        while let Some(a) = it.next() {
                            if a == "--root" {
                                it.next(); // --root <valeur> : ignoré ici
                                continue;
                            }
                            if a.starts_with("--") {
                                continue;
                            }
                            found = Some(a.as_str());
                            break;
                        }
                        found
                    };
                    let daemon = args.iter().any(|a| a == "--daemon");
                    daemon::install(dir, daemon)
                }
                "uninstall" => daemon::uninstall(),
                "start" => daemon::start(),
                "stop" => daemon::stop(),
                "restart" => daemon::restart(),
                "status" => Ok(daemon::status()),
                "logs" => {
                    let n = args
                        .iter()
                        .position(|a| a == "--lines")
                        .and_then(|i| args.get(i + 1))
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(25);
                    daemon::logs(n)
                }
                _ => unreachable!(),
            };
            match outcome {
                Ok(msg) => {
                    println!("{msg}");
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("{sub} failed: {e}");
                    std::process::exit(1);
                }
            }
        }
    }

    // Binaire installé : travailler dans sa racine de données (registre
    // install.json), sinon portable (répertoire courant).
    daemon::enter_data_root();

    // `dkbot doctor` : diagnostic local (lectures seules), sans Discord.
    if args.iter().any(|a| a == "doctor") {
        std::process::exit(doctor::run().await);
    }

    // ── 1. Environnement complet (setup interactif si TTY) ──
    let initial_catalog = i18n::catalog(i18n::resolve_initial_lang(&args));
    if let Err(e) = setup::ensure_env(non_interactive, initial_catalog) {
        eprintln!("{}", e.msg);
        std::process::exit(e.code);
    }

    // ── 2. Rechargement du .env (éventuellement écrit par le setup) ──
    let _ = dotenvy::from_path(paths::env_file());
    let language = i18n::resolve_initial_lang(&args);
    let catalog = i18n::catalog(language);

    logger::init();

    // ── 3. Configuration ──
    let cfg = match config::load(catalog, language) {
        Ok(cfg) => cfg,
        Err(e) => {
            logger::error(e.msg.clone());
            eprintln!("{}", e.msg);
            std::process::exit(e.code);
        }
    };

    // ── 4. État partagé + couche API (backends local/SSH) ──
    let state = match state::State::new(&cfg.token, &cfg.owner_id, catalog) {
        Ok(s) => s,
        Err(e) => {
            // 78 = erreur de configuration (users.json corrompu…) : le
            // superviseur ne redémarre pas en boucle.
            logger::error(format!("Startup failed: {e}"));
            std::process::exit(e.code);
        }
    };
    let api = match api::Api::new(&cfg) {
        Ok(api) => api,
        Err(e) => {
            logger::error(format!("Startup failed: {e}"));
            std::process::exit(e.code);
        }
    };
    let api = Arc::new(api);

    let token = cfg.token.clone();
    let client_id = cfg.client_id.clone();
    let plugins = Arc::new(plugin::PluginHost::new());
    let loaded = plugins.load_dir(&paths::plugins_dir());
    if !loaded.is_empty() {
        logger::info(format!("plugins chargés : {}", loaded.join(", ")));
    }
    let state = Arc::new(state);
    let data = Data {
        cfg,
        api,
        state,
        catalog,
        plugins,
    };

    // ── 5. Enregistrement des slash commands (AVANT la connexion) ──
    let http = serenity::Http::new(&token);
    http.set_application_id(serenity::ApplicationId::new(
        client_id.parse().unwrap_or_default(),
    ));
    if let Err(e) = deploy::deploy_commands_if_needed(
        &http,
        &data.cfg,
        catalog,
        &commands::commands(),
        &data.plugins,
    )
    .await
    {
        let code = find_exit_code(&*e).unwrap_or(1);
        logger::error(format!("Startup failed: {e}"));
        eprintln!("{e}");
        std::process::exit(code);
    }

    // ── 6. Framework poise + client gateway ──
    let options = poise::FrameworkOptions {
        commands: commands::commands(),
        command_check: Some(commands::command_check),
        on_error: |err| Box::pin(async move { commands::handle_error(err).await }),
        event_handler: |ctx, event, framework, data| {
            Box::pin(async move {
                commands::on_ready(ctx, event, framework, data).await;
                // Interactions des groupes de plugins (/dw …) : consommées
                // ici, après l'échec silencieux de poise (commande inconnue).
                if let serenity::FullEvent::InteractionCreate { interaction } = event {
                    let consumed = crate::plugin_commands::handle_plugin_interaction(
                        ctx,
                        interaction,
                        &data.plugins,
                        &data.state,
                    )
                    .await;
                    crate::plugin_commands::handle_plugin_autocomplete(
                        ctx,
                        interaction,
                        &data.plugins,
                        &data.state,
                    )
                    .await;
                    // Les manifestes peuvent changer en cours d'exécution
                    // (ex. switch cheat : commandes masquées ou réaffichées) :
                    // re-synchronise les slash commands si nécessaire.
                    if consumed {
                        if let Err(e) =
                            deploy::sync_if_changed(ctx, &framework.options().commands, data).await
                        {
                            logger::error(format!("resynchronisation des commandes : {e}"));
                        }
                    }
                }
                Ok(())
            })
        },
        ..Default::default()
    };
    let framework = poise::Framework::builder()
        .options(options)
        .setup(move |_ctx, _ready, _framework| Box::pin(async move { Ok(data) }))
        .build();

    // GuildMembers (intent privilégié) : requis pour que le sélecteur de
    // membres de /users liste TOUS les membres de la guilde. Doit aussi
    // être activé dans le portail développeur (SERVER MEMBERS INTENT).
    let intents = GatewayIntents::GUILDS | GatewayIntents::GUILD_MEMBERS;
    let mut client = match serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await
    {
        Ok(client) => client,
        Err(e) => {
            let code = find_exit_code(&e).unwrap_or(1);
            logger::error(format!("Startup failed: {e}"));
            eprintln!("{e}");
            std::process::exit(code);
        }
    };

    // ── 7. Boucle principale (gateway vs signal d'arrêt) ──
    tokio::select! {
        result = client.start() => {
            if let Err(e) = result {
                let code = find_exit_code(&e).unwrap_or(1);
                logger::error(format!("Startup failed: {e}"));
                std::process::exit(code);
            }
        }
        _ = shutdown_signal() => {
            logger::info("Received signal, shutting down…");
            let _ = client.shard_manager.shutdown_all().await;
        }
    }
}
