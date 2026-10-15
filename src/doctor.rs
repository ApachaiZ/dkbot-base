//! `dkbot doctor` — diagnostic local de l'installation : configuration,
//! systemctl, cibles (ping), services (état réel), accès aux sauvegardes,
//! canaux d'alerte. Aucune commande n'est envoyée (lectures seules).
//! Sortie : 0 = tout va bien, 1 = problème(s), 78 = configuration invalide.

use crate::api::Api;
use crate::i18n;
use crate::paths;

/// Point d'entrée CLI (appelé après `enter_data_root`).
pub async fn run() -> i32 {
    println!("🩺 dkbot doctor — diagnostic de l'installation\n");
    let mut problems: u32 = 0;

    // ── 1. Configuration (le .env existant, SANS prompt) ──
    let _ = dotenvy::from_path(paths::env_file());
    let catalog = i18n::catalog("fr");
    let cfg = match crate::config::load(catalog, "fr") {
        Ok(c) => {
            println!(
                "✅ Configuration : {} cible(s), {} service(s), langue {}",
                c.targets.len(),
                c.services.len(),
                c.language
            );
            c
        }
        Err(e) => {
            eprintln!("❌ Configuration : {}", e.msg);
            return 78;
        }
    };

    // ── 2. systemctl sur la machine locale ──
    match std::process::Command::new("systemctl")
        .arg("--version")
        .output()
    {
        Ok(o) if o.status.success() => {
            println!("✅ systemctl présent (machine locale)")
        }
        _ => {
            println!("⚠ systemctl absent de la machine locale — la cible `local` échouera");
            problems += 1;
        }
    }

    // ── 3. API + cibles ──
    let api = match Api::new(&cfg) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("❌ Construction des backends : {e}");
            return 1;
        }
    };
    for t in &cfg.targets {
        match api.ping(&t.alias).await {
            Ok(()) => println!("✅ Cible `{}` joignable ({})", t.alias, t.display()),
            Err(e) => {
                println!("❌ Cible `{}` INJOIGNABLE : {}", t.alias, e.message);
                problems += 1;
            }
        }
    }

    // ── 4. État réel de chaque service ──
    for t in &cfg.targets {
        for (alias, unit) in &cfg.services {
            match api.get(&t.alias, alias, true).await {
                Ok(status) => {
                    println!("   · {alias} ({unit}) sur `{}` : {}", t.alias, status.state)
                }
                Err(e) => {
                    println!(
                        "   · {alias} ({unit}) sur `{}` : lecture impossible ({})",
                        t.alias, e.message
                    );
                    problems += 1;
                }
            }
        }
    }

    // ── 5. Sauvegardes ──
    match &cfg.saves_path {
        Some(saves) => match api
            .backups_list(&cfg.targets[0].alias, &cfg.services[0].0)
            .await
        {
            Ok(out) if out.trim() == "NOBACKUPS" => println!(
                "⚠ Sauvegardes : {}-backups/ existe mais aucune archive pour l'instant",
                saves
            ),
            Ok(out) => println!(
                "✅ Sauvegardes accessibles : {} archive(s) dans {}-backups/",
                out.lines().count(),
                saves
            ),
            Err(e) => {
                println!("❌ Sauvegardes INACCESSIBLES : {}", e.message);
                problems += 1;
            }
        },
        None => println!("ℹ SAVES_PATH non configuré — pas de sauvegarde."),
    }

    // ── 6. Canaux Discord ──
    if cfg.alert_channel_id.is_none() {
        println!("⚠ DISCORD_ALERT_CHANNEL_ID non défini — watchdog et audit muets.");
        problems += 1;
    }

    // ── 7. Modules / planificateur ──
    let modules = crate::modules::Modules::load();
    let (enabled, disabled): (Vec<_>, Vec<_>) =
        modules.entries().into_iter().partition(|(_, on)| *on);
    println!(
        "ℹ Modules actifs : {}",
        enabled
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    if !disabled.is_empty() {
        println!(
            "ℹ Modules désactivés : {}",
            disabled
                .iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let schedules = crate::schedule::load();
    println!("ℹ Planificateur : {} entrée(s)", schedules.len());

    if problems == 0 {
        println!("\n✅ Tout va bien.");
        0
    } else {
        println!("\n⚠ {problems} problème(s) détecté(s).");
        1
    }
}
