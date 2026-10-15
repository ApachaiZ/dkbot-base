//! Mises à jour et MODULES (`dkbot update`, `dkbot module install|…`) :
//! le binaire est remplacé depuis les releases GitHub, en binaire unique
//! et zéro dépendance système (HTTP pur Rust).
//!
//! Mécanique sûre (commune aux deux commandes) :
//! 1. l'asset correspondant à l'ARCHITECTURE de la machine est choisi ;
//! 2. téléchargement dans un fichier TEMPORAIRE à côté de l'exécutable
//!    courant (même système de fichiers) ;
//! 3. SHA-256 VÉRIFIÉE contre le fichier `.sha256` de la release — toute
//!    divergence = abandon, rien n'est touché ;
//! 4. `rename` atomique par-dessus l'exécutable courant : le processus en
//!    cours garde son inode, le chemin pointe le nouveau binaire ;
//! 5. daemon supervisé actif → redémarrage à chaud sur le NOUVEAU binaire.
//!
//! Installation système (`/usr/bin`) : root requis. Les données (config,
//! modules, planificateur, sauvegardes) ne sont JAMAIS touchées.
//!
//! MODULES : un « module » est une SAVEUR du binaire publiée sous son
//! propre préfixe de tag et d'asset. `dkbot module install dragonwilds`
//! remplace le binaire générique par la saveur Dragonwilds (code RSDW
//! compilé dedans) ; `dkbot module uninstall` revient à la saveur
//! générique. Le module s'active ensuite dans Discord : `/modules enable
//! dragonwilds`.

use std::io::Write;
use std::time::Duration;

const UA: &str = "dakkabot-update";

/// Une SAVEUR de binaire publiée : dépôt GitHub + préfixe des assets.
struct Flavor {
    id: &'static str,
    repo: &'static str,
    asset_prefix: &'static str,
}

/// Le dépôt du CŒUR du projet (saveur générique, sans module).
const CORE_REPO: &str = "ApachaiZ/DakkaBot";

/// Registre des MODULES installables : (id, dépôt GitHub, préfixe d'assets).
/// Chaque module vit dans SON dépôt et publie SES propres releases —
/// `dkbot module install <id>` les récupère en une commande.
const MODULES: &[(&str, &str, &str)] = &[(
    "dragonwilds",
    "ApachaiZ/DakkaBot-module-dragonwilds",
    "dkbot-dragonwilds",
)];

/// Saveur du binaire COURANT (divergence volontaire entre dépôts :
/// générique sur le cœur, la saveur du module sur le dépôt du module).
const CURRENT: Flavor = Flavor {
    id: "generic",
    repo: "ApachaiZ/DakkaBot",
    asset_prefix: "dkbot",
};

/// Architecture de build au format des assets de release.
fn asset_suffix() -> Result<&'static str, String> {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "linux") => Ok("x86_64-unknown-linux-musl"),
        ("aarch64", "linux") => Ok("aarch64-unknown-linux-musl"),
        (arch, os) => Err(format!("architecture non publiée : {arch}-{os}")),
    }
}

#[derive(serde::Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(serde::Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(600))
        .build()
        .expect("client HTTP")
}

async fn get_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, String> {
    let resp = client()
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("requête {url} : {e}"))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("lecture {url} : {e}"))?;
    if !status.is_success() {
        return Err(format!("{url} : HTTP {status} — {body}"));
    }
    serde_json::from_str(&body).map_err(|e| format!("JSON invalide : {e}"))
}

/// Dernière release de la SAVEUR, depuis SON dépôt (une saveur ne voit
/// jamais les releases d'une autre). `tag` = version précise.
async fn fetch_release(flavor: &Flavor, tag: Option<&str>) -> Result<Release, String> {
    let api = format!("https://api.github.com/repos/{}", flavor.repo);
    match tag {
        Some(t) => get_json(&format!("{api}/releases/tags/{t}")).await,
        None => {
            let releases: Vec<Release> = get_json(&format!("{api}/releases?per_page=20")).await?;
            releases
                .into_iter()
                .find(|r| r.tag_name.starts_with('v'))
                .ok_or_else(|| format!("aucune release pour la saveur « {} »", flavor.id))
        }
    }
}

async fn download(url: &str, dest: &std::path::Path) -> Result<Vec<u8>, String> {
    let resp = client()
        .get(url)
        .header("Accept", "application/octet-stream")
        .send()
        .await
        .map_err(|e| format!("téléchargement {url} : {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("téléchargement {url} : HTTP {status}"));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("lecture du binaire : {e}"))?
        .to_vec();
    let mut file = std::fs::File::create(dest).map_err(|e| format!("créer {dest:?} : {e}"))?;
    file.write_all(&bytes)
        .map_err(|e| format!("écrire {dest:?} : {e}"))?;
    Ok(bytes)
}

/// SHA-256 hexadécimal (le projet embarque déjà sha2 — pas de dépendance).
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Vérifie `bytes` contre le fichier `.sha256` de la release.
async fn verify_sha256(bytes: &[u8], sha_url: &str) -> Result<(), String> {
    let expected = client()
        .get(sha_url)
        .header("Accept", "text/plain")
        .send()
        .await
        .map_err(|e| format!("téléchargement du .sha256 : {e}"))?
        .text()
        .await
        .map_err(|e| format!("lecture du .sha256 : {e}"))?
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
    if expected.len() != 64 {
        return Err(format!("fichier .sha256 illisible : « {expected} »"));
    }
    let actual = sha256_hex(bytes);
    if !actual.eq_ignore_ascii_case(&expected) {
        return Err(format!(
            "SOMME DE CONTRÔLE INVALIDE\nattendu : {expected}\nobtenu  : {actual}\nLe binaire téléchargé a été abandonné."
        ));
    }
    Ok(())
}

fn is_writable_dir(dir: &std::path::Path) -> bool {
    let probe = dir.join(format!(".dkbot-write-test-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[derive(serde::Deserialize)]
struct DaemonState {
    pid: u32,
}

fn read_daemon_pid(root: &std::path::Path) -> Option<DaemonState> {
    let raw = std::fs::read_to_string(root.join("daemon.json")).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Télécharge, vérifie et remplace le binaire par la saveur visée, puis
/// redémarre le daemon à chaud s'il tourne. Retourne un résumé.
async fn switch_to(flavor: &Flavor, tag: Option<&str>) -> Result<String, String> {
    let current = env!("CARGO_PKG_VERSION");
    let suffix = asset_suffix()?;
    let release = fetch_release(flavor, tag).await?;

    let asset_name = format!("{}-{suffix}", flavor.asset_prefix);
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .ok_or_else(|| {
            format!(
                "aucun asset « {asset_name} » dans la release {}",
                release.tag_name
            )
        })?;
    let sha_asset = release
        .assets
        .iter()
        .find(|a| a.name == format!("{asset_name}.sha256"))
        .ok_or_else(|| format!("fichier de somme manquant pour {asset_name}"))?;

    println!("Téléchargement de {} ({})…", release.tag_name, asset_name);

    let exe = std::env::current_exe().map_err(|e| format!("current_exe : {e}"))?;
    let exe_dir = exe.parent().ok_or("exécutable sans répertoire parent")?;
    if !is_writable_dir(exe_dir) {
        return Err(format!(
            "répertoire non inscriptible : {} (root requis pour une installation système)",
            exe_dir.display()
        ));
    }

    let tmp = exe_dir.join(format!(".dkbot-update-{}.tmp", std::process::id()));
    let bytes = download(&asset.browser_download_url, &tmp).await?;
    verify_sha256(&bytes, &sha_asset.browser_download_url).await?;

    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("chmod : {e}"))?;
    std::fs::rename(&tmp, &exe).map_err(|e| format!("rename atomique : {e}"))?;

    let mut out = format!(
        "✅ Binaire remplacé : {} ({} → {} — saveur « {} »)",
        exe.display(),
        current,
        release.tag_name,
        flavor.id
    );

    let root = crate::daemon::data_root();
    if let Some(state) = read_daemon_pid(&root) {
        if crate::daemon::pid_alive(state.pid) {
            if let Err(e) = crate::daemon::stop_at(&root) {
                return Err(format!(
                    "binaire remplacé mais arrêt du daemon impossible : {e}"
                ));
            }
            crate::daemon::spawn_detached_from(&exe, &root)
                .map_err(|e| format!("binaire remplacé mais relance du daemon impossible : {e}"))?;
            out.push_str("\n✅ Daemon redémarré sur le nouveau binaire.");
        }
    }
    Ok(out)
}

/// Point d'entrée CLI. `0` = succès, `1` = échec.
pub async fn run(args: &[String]) -> i32 {
    let outcome = if args.get(1).map(|s| s.as_str()) == Some("module") {
        module(args).await
    } else {
        update(args).await
    };
    match outcome {
        Ok(msg) => {
            println!("{msg}");
            0
        }
        Err(e) => {
            eprintln!("failed: {e}");
            1
        }
    }
}

/// `dkbot update [vX.Y.Z]` — met à jour la saveur COURANTE.
async fn update(args: &[String]) -> Result<String, String> {
    let current = env!("CARGO_PKG_VERSION");
    let requested = args
        .iter()
        .skip_while(|a| *a != "update")
        .nth(1)
        .filter(|a| !a.starts_with("--"))
        .map(|s| s.as_str());
    let release = fetch_release(&CURRENT, requested).await?;
    if release.tag_name == format!("v{current}") {
        return Ok(format!(
            "Déjà à jour ({}, saveur « {} »).",
            release.tag_name, CURRENT.id
        ));
    }
    switch_to(&CURRENT, requested).await
}

/// `dkbot module status|install|uninstall [<nom>]` — gestion des SAVEURS.
async fn module(args: &[String]) -> Result<String, String> {
    let verb = args.get(2).map(|s| s.as_str()).unwrap_or("status");
    let name = args
        .get(3)
        .map(|s| s.as_str())
        .filter(|s| !s.starts_with("--"));
    let available = MODULES
        .iter()
        .map(|(id, _, _)| *id)
        .collect::<Vec<_>>()
        .join(", ");
    let generic = Flavor {
        id: "generic",
        repo: CORE_REPO,
        asset_prefix: "dkbot",
    };

    match verb {
        "status" => {
            let mut out = format!(
                "Saveur actuelle : « {} » ({}, assets {}-*).",
                CURRENT.id, CURRENT.repo, CURRENT.asset_prefix
            );
            for (id, repo, _) in MODULES {
                let state = if id == &CURRENT.id {
                    "installée"
                } else {
                    "non installée"
                };
                out.push_str(&format!("\n· {:<12} {} — {repo}", id, state));
            }
            Ok(out)
        }
        "list" => Ok(format!("Modules installables : {available}")),
        "install" | "uninstall" => {
            if verb == "uninstall" {
                // `uninstall` revient TOUJOURS à la saveur générique (cœur).
                if CURRENT.id == "generic" {
                    return Ok("Saveur générique déjà installée — rien à retirer.".into());
                }
                return switch_to(&generic, None).await;
            }
            let Some(name) = name else {
                return Err(format!(
                    "nom de module attendu — modules disponibles : {available}"
                ));
            };
            if verb == "install" {
                let (id, repo, asset_prefix) = MODULES
                    .iter()
                    .find(|(id, _, _)| *id == name)
                    .copied()
                    .ok_or_else(|| {
                        format!("module inconnu : {name} (disponibles : {available})")
                    })?;
                if id == CURRENT.id {
                    return Ok(format!(
                        "Le module « {name} » est DÉJÀ la saveur installée."
                    ));
                }
                let flavor = Flavor {
                    id,
                    repo,
                    asset_prefix,
                };
                let mut out = switch_to(&flavor, None).await?;
                out.push_str(&format!(
                    "\n💡 Activez-le ensuite dans Discord : `/modules enable {name}`."
                ));
                Ok(out)
            } else {
                unreachable!("uninstall a déjà retourné")
            }
        }
        other => Err(format!(
            "sous-commande inconnue : {other} (attendu : status, list, install, uninstall)"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_maps_known_targets() {
        assert!(asset_suffix().is_ok());
    }

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_ne!(sha256_hex(b"binary"), sha256_hex(b"other"));
    }

    #[test]
    fn module_registry_is_consistent() {
        // Un id par module, un dépôt non vide, un préfixe d'assets non vide,
        // et des préfixes deux à deux distincts.
        let mut seen = std::collections::HashSet::new();
        for (id, repo, assets) in MODULES {
            assert!(!id.is_empty());
            assert!(repo.starts_with("ApachaiZ/"));
            assert!(!assets.is_empty());
            assert!(seen.insert(assets), "préfixe d'assets dupliqué : {assets}");
        }
        assert_eq!(MODULES[0].0, "dragonwilds");
        // La saveur générique vit dans le dépôt du cœur.
        assert_eq!(CORE_REPO, "ApachaiZ/DakkaBot");
    }
}
