//! Setup interactif du premier lancement : complète `.config.d/.env` en
//! posant les questions nécessaires, ou refuse de démarrer (exit 78) avec
//! la liste des variables manquantes en mode `--non-interactive` / sans TTY.

use std::io::Write;

use dialoguer::{Input, Password};

use crate::errors::ExitError;
use crate::i18n::{fill, Catalog};
use crate::paths;
use crate::targets::{is_unset, parse_services, parse_targets};

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().to_string())
}

/// Variables REQUISES pour démarrer.
const REQUIRED: [&str; 5] = [
    "DISCORD_TOKEN",
    "DISCORD_CLIENT_ID",
    "DISCORD_OWNER_ID",
    "DISCORD_GUILD_ID",
    "SERVICES",
];

/// Variables OPTIONNELLES proposées à la création du `.env`.
const OPTIONAL: [&str; 7] = [
    "SSH_USER",
    "SSH_PASSWORD",
    "SSH_KEY_PATH",
    "SYSTEMCTL_PREFIX",
    "DISCORD_ALERT_CHANNEL_ID",
    "LOG_FILE",
    "SAVES_PATH",
];

/// Valeurs « sensibles » : saisie masquée.
const HIDDEN: [&str; 2] = ["DISCORD_TOKEN", "SSH_PASSWORD"];

fn is_numeric_id(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_digit())
}

fn validate(key: &str, value: &str) -> Result<(), String> {
    match key {
        "DISCORD_TOKEN" if is_unset(Some(value)) => Err("token vide ou placeholder".into()),
        "DISCORD_CLIENT_ID"
        | "DISCORD_OWNER_ID"
        | "DISCORD_GUILD_ID"
        | "DISCORD_ALERT_CHANNEL_ID"
            if !is_numeric_id(value) =>
        {
            Err("ID numérique attendu".into())
        }
        "SERVICES" if !value.trim().is_empty() && parse_services(Some(value), None).is_empty() => {
            Err("format attendu : alias=unité (ex. web=nginx)".into())
        }
        "SAVES_PATH" if !crate::config::valid_saves_path(value) => {
            Err("chemin absolu attendu (commence par /)".into())
        }
        _ => Ok(()),
    }
}

/// Ligne de prompt localisée pour une clé.
fn prompt_for(key: &str, catalog: &Catalog) -> String {
    let label = match key {
        "DISCORD_TOKEN" => catalog.setup.prompt.discord_token,
        "DISCORD_CLIENT_ID" => catalog.setup.prompt.discord_client_id,
        "DISCORD_OWNER_ID" => catalog.setup.prompt.discord_owner_id,
        "DISCORD_GUILD_ID" => catalog.setup.prompt.discord_guild_id,
        "TARGETS" => catalog.setup.prompt.targets,
        "SSH_USER" => catalog.setup.prompt.ssh_user,
        "SSH_PASSWORD" => catalog.setup.prompt.ssh_password,
        "SSH_KEY_PATH" => catalog.setup.prompt.ssh_key_path,
        "SERVICES" => catalog.setup.prompt.services,
        "SYSTEMCTL_PREFIX" => catalog.setup.prompt.systemctl_prefix,
        "DISCORD_ALERT_CHANNEL_ID" => catalog.setup.prompt.alert_channel,
        "SAVES_PATH" => catalog.setup.prompt.saves_path,
        _ => key,
    };
    label.to_string()
}

/// Écrit/ajoute des lignes KEY=value dans `.config.d/.env` (les clés
/// existantes sont remplacées, les autres conservées).
fn write_env(lines: &[(String, String)]) -> Result<(), ExitError> {
    let path = paths::env_file();
    let existing: Vec<String> = std::fs::read_to_string(&path)
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default();

    let mut out: Vec<String> = Vec::new();
    for line in existing {
        let key = line.split('=').next().unwrap_or("").trim();
        if !lines.iter().any(|(k, _)| k == key) && !line.trim().is_empty() {
            out.push(line);
        }
    }
    for (k, v) in lines {
        let escaped = if v.contains('#') || v.contains('\n') || v.contains('"') {
            format!("\"{}\"", v.replace('"', "\\\""))
        } else {
            v.clone()
        };
        out.push(format!("{k}={escaped}"));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ExitError::config(e.to_string()))?;
    }
    let mut file = std::fs::File::create(&path).map_err(|e| ExitError::config(e.to_string()))?;
    let content = out.join("\n") + "\n";
    file.write_all(content.as_bytes())
        .map_err(|e| ExitError::config(e.to_string()))?;
    Ok(())
}

/// Pose une question et valide la réponse (re-prompt si invalide).
/// Une réponse vide est refusée pour les champs requis, acceptée (champ
/// laissé non défini) pour les optionnels.
fn ask(catalog: &Catalog, key: &str, hidden: bool) -> Option<String> {
    let question = prompt_for(key, catalog);
    let label = if hidden {
        fill(catalog.setup.prompt_line_hidden, &[("question", &question)])
    } else {
        fill(catalog.setup.prompt_line, &[("question", &question)])
    };
    loop {
        let value: String = if hidden {
            Password::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt(label.trim_end())
                .allow_empty_password(true)
                .interact()
                .ok()?
        } else {
            Input::<String>::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt(label.trim_end())
                .allow_empty(true)
                .interact_text()
                .ok()?
        };
        let value = value.trim().to_string();
        if value.is_empty() {
            // Aucun champ REQUIS n'accepte de réponse vide : l'utilisateur
            // doit définir SES services (n'importe quelle unité systemd).
            if REQUIRED.contains(&key) {
                println!("{}", catalog.setup.invalid.trim_end());
                continue;
            }
            return Some(String::new());
        }
        match validate(key, &value) {
            Ok(()) => return Some(value),
            Err(reason) => {
                println!("{}   ({reason})", catalog.setup.invalid.trim_end());
                continue;
            }
        }
    }
}

/// Les champs requis sont-ils tous définis ? `get` lit une variable
/// (environnement réel ou table de test). `SERVICES` est aussi satisfait
/// par la forme mono-service `SERVICE=unité` (repli documenté du parseur).
fn missing_required(get: impl Fn(&str) -> Option<String>) -> Vec<&'static str> {
    REQUIRED
        .iter()
        .copied()
        .filter(|k| {
            if *k == "SERVICES" {
                is_unset(get("SERVICES").as_deref())
                    && parse_services(None, get("SERVICE").as_deref()).is_empty()
            } else {
                is_unset(get(k).as_deref())
            }
        })
        .collect()
}

/// Pose la question d'un champ REQUIS ; réponse vide refusée (re-prompt).
/// `None` = abandon (EOF).
fn ask_required(catalog: &Catalog, key: &str) -> Option<String> {
    ask(catalog, key, HIDDEN.contains(&key))
}

/// Astuce PATH affichée après le setup : le dossier du binaire doit être
/// dans le PATH pour appeler `dkbot` depuis n'importe où. Muette pour les
/// dossiers système déjà dans le PATH (`/usr/bin`, `/bin`, `/usr/local/bin`).
fn print_path_hint(catalog: &Catalog) {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let standard = ["/usr/bin", "/bin", "/usr/local/bin"].iter().any(|d| {
                std::fs::canonicalize(d)
                    .ok()
                    .zip(std::fs::canonicalize(dir).ok())
                    .map(|(a, b)| a == b)
                    .unwrap_or(false)
            });
            if !standard {
                println!(
                    "{}",
                    fill(
                        catalog.setup.path_hint,
                        &[("dir", &dir.display().to_string())]
                    )
                );
            }
        }
    }
}

/// Complète `.config.d/.env` de façon interactive si besoin.
/// `Err` = configuration incomplète et démarrage impossible (exit 78).
pub fn ensure_env(
    non_interactive: bool,
    initial_catalog: &'static Catalog,
) -> Result<(), ExitError> {
    // Le `.env` existait-il AVANT le chargement ? S'il existe, ses valeurs
    // sont utilisées telles quelles : on ne redemande que les valeurs
    // requises manquantes (pas la langue, les cibles ni les optionnels).
    let env_present = paths::env_file().is_file();
    crate::config::load_dotenv();
    let missing = missing_required(env);
    if missing.is_empty() {
        return Ok(());
    }

    let tty = std::io::IsTerminal::is_terminal(&std::io::stdin());
    if non_interactive || !tty {
        let list = missing.join(", ");
        let env_file = paths::env_file().display().to_string();
        let template = if non_interactive {
            initial_catalog.errors.setup_incomplete_non_interactive
        } else {
            initial_catalog.errors.setup_incomplete_no_tty
        };
        return Err(ExitError::config(fill(
            template,
            &[("missing", &list), ("envFile", &env_file)],
        )));
    }

    // ── Complétion : le `.env` existe déjà → ne demander QUE les valeurs
    //    requises manquantes (langue, cibles et optionnels : inchangés).
    if env_present {
        let catalog = match env("BOT_LANGUAGE").as_deref() {
            Some(l) if l.eq_ignore_ascii_case("en") => crate::i18n::catalog("en"),
            Some(l) if l.eq_ignore_ascii_case("fr") => crate::i18n::catalog("fr"),
            _ => initial_catalog,
        };
        println!(
            "{}",
            fill(
                catalog.setup.completion_intro,
                &[("envFile", &paths::env_file().display().to_string())]
            )
        );
        let mut lines: Vec<(String, String)> = Vec::new();
        for key in &missing {
            if let Some(value) = ask_required(catalog, key) {
                lines.push((key.to_string(), value));
            }
        }
        write_env(&lines)?;
        println!(
            "{}",
            fill(
                catalog.setup.complete,
                &[("envFile", &paths::env_file().display().to_string())]
            )
        );
        print_path_hint(catalog);
        return Ok(());
    }

    println!("{}", initial_catalog.setup.intro);
    println!(
        "{}",
        fill(
            initial_catalog.setup.file_line,
            &[("envFile", &paths::env_file().display().to_string())]
        )
    );

    // Langue d'abord (bascule à chaud des prompts).
    let lang_value = env("BOT_LANGUAGE").unwrap_or_else(|| "fr".to_string());
    let lang: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt(initial_catalog.setup.language_label)
        .default(lang_value)
        .interact_text()
        .unwrap_or_else(|_| "fr".to_string());
    let lang = if lang.trim().eq_ignore_ascii_case("fr") {
        "fr"
    } else {
        "en"
    };
    let catalog = crate::i18n::catalog(lang);

    let mut lines: Vec<(String, String)> = vec![("BOT_LANGUAGE".to_string(), lang.to_string())];

    for key in REQUIRED {
        if is_unset(env(key).as_deref()) {
            if let Some(value) = ask_required(catalog, key) {
                lines.push((key.to_string(), value));
            }
        }
    }

    // Cibles : défaut « local » (cas d'usage principal : la machine du bot).
    let targets_value = env("TARGETS").unwrap_or_else(|| "local".to_string());
    let targets: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt(fill(
            catalog.setup.prompt_line,
            &[("question", catalog.setup.prompt.targets)],
        ))
        .default(targets_value)
        .interact_text()
        .unwrap_or_else(|_| "local".to_string());
    if parse_targets(Some(&targets), None).is_empty() {
        println!("{}", catalog.setup.invalid);
        return Err(ExitError::config(fill(
            catalog.errors.invalid_target,
            &[("alias", "TARGETS"), ("reason", "aucune cible valide")],
        )));
    }
    lines.push(("TARGETS".to_string(), targets));

    for key in OPTIONAL {
        if is_unset(env(key).as_deref()) {
            if let Some(value) = ask(catalog, key, HIDDEN.contains(&key)) {
                if !value.is_empty() {
                    lines.push((key.to_string(), value));
                }
            }
        }
    }

    write_env(&lines)?;
    println!(
        "{}",
        fill(
            catalog.setup.complete,
            &[("envFile", &paths::env_file().display().to_string())]
        )
    );
    print_path_hint(catalog);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn services_validation_accepts_empty_and_pairs() {
        // Vide = service standard unique (accepté).
        assert!(validate("SERVICES", "").is_ok());
        assert!(validate("SERVICES", "web=nginx").is_ok());
        // Sans « alias=unité » ou unité invalide : refusé.
        assert!(validate("SERVICES", "sans-egal").is_err());
        assert!(validate("SERVICES", "a=unité invalide!").is_err());
    }

    #[test]
    fn saves_path_validation_requires_absolute() {
        assert!(validate("SAVES_PATH", "/opt/game/saves").is_ok());
        assert!(validate("SAVES_PATH", "relative").is_err());
    }

    #[test]
    fn missing_required_accepts_mono_service_fallback() {
        fn full(k: &str) -> Option<String> {
            match k {
                "DISCORD_TOKEN" => Some("token".into()),
                "DISCORD_CLIENT_ID" | "DISCORD_OWNER_ID" | "DISCORD_GUILD_ID" => Some("1".into()),
                _ => None,
            }
        }
        // Sans SERVICES ni SERVICE : SERVICES est manquant.
        assert_eq!(missing_required(full), vec!["SERVICES"]);
        // Avec SERVICE seul (forme mono-service) : rien ne manque.
        fn mono(k: &str) -> Option<String> {
            if k == "SERVICE" {
                Some("myservice".into())
            } else {
                full(k)
            }
        }
        assert!(missing_required(mono).is_empty());
    }
}
