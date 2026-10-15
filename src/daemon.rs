//! Daemon autonome (superviseur embarqué) — pas de systemd par défaut.
//!
//! Sous-commandes du binaire :
//! ```text
//! dkbot                      → bot au premier plan
//! dkbot run                  → forme explicite (utilisée par le superviseur)
//! dkbot start|stop|restart   → cycle de vie du daemon supervisé
//! dkbot status               → état du daemon (pid, uptime)
//! dkbot logs [--lines N]     → dernière ligne de log du bot
//! dkbot doctor               → diagnostic local (lectures seules)
//! dkbot install [<dir>] [--daemon] → binaire dans les cibles standard
//! dkbot uninstall            → retrait (données conservées)
//! ```
//!
//! Racine des données, par priorité : `--root <dir>` (flag explicite) >
//! registre `install.json` à côté du binaire (installations dans un
//! dossier dédié) > `~/.config/dakkabot/` (installation standard) >
//! répertoire courant (repli sans HOME). Le processus s'y `chdir` au
//! démarrage : les chemins relatifs (`.config.d/`, `logs/`, `assets/`)
//! sont ceux du dossier de données.
//!
//! Deux voies d'installation du BINAIRE coexistent :
//! - `cargo install` standard (`--path .` ou `--git <dépôt>`) : binaire
//!   dans `~/.cargo/bin`, données dans `~/.config/dakkabot/` ;
//! - `dkbot install` : copie DIRECTE du binaire courant (donc
//!   l'architecture de la machine). SANS argument, le binaire est posé
//!   dans les cibles système standard `/usr/bin` et `/bin` (si réel) et
//!   les DONNÉES vivent dans `~/.config/dakkabot/` — SEUL le binaire est
//!   installé, root requis ; AVEC un dossier, binaire + registre
//!   `install.json` dans ce dossier (données au même endroit).
//!   `--daemon` ajoute l'unité systemd (explicite, root requis).
//!
//! Mécanique du daemon : `start` détache (`setsid`) un SUPERVISEUR léger
//! (le même binaire en mode `supervise`) qui exécute le bot en enfant et
//! le relance en cas de crash — backoff borné, **exit 78 (configuration)
//! = abandon immédiat** (pas de boucle de relance), compteur remis à zéro
//! après 30 s de stabilité. `stop` envoie SIGTERM au superviseur, qui
//! arrête l'enfant proprement. Tout l'état vit dans `<racine>/daemon.json`
//! (mode 0600) ; les logs du bot dans `<racine>/logs/`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use crate::errors::ExitError;

const REGISTRY_FILE: &str = "install.json";
const STATE_FILE: &str = "daemon.json";
/// Cibles système STANDARD du binaire quand `dkbot install` est appelé
/// sans argument. `/bin` est souvent un symlink vers `/usr/bin` : le
/// binaire n'est copié qu'une fois (cibles canoniques dédoublonnées).
pub const STANDARD_BIN_DIRS: [&str; 2] = ["/usr/bin", "/bin"];
/// Nom de l'unité systemd installée par `install --daemon`.
pub const UNIT_NAME: &str = "dakkabot.service";
const RESTART_DELAY: Duration = Duration::from_secs(5);
const MAX_RESTARTS: u32 = 10;
const STABLE_UPTIME: Duration = Duration::from_secs(30);
const KILL_TIMEOUT: Duration = Duration::from_secs(10);
const BACKOFF_CAP: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Registry {
    root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DaemonState {
    /// PID du SUPERVISEUR (pas du bot).
    pid: u32,
    started_at: String,
}

/// Racine des données imposée par `--root <dir>` (prioritaire).
static ROOT_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

/// Analyse `--root <dir>` / `--root=<dir>` dans les arguments (appelé une
/// fois au démarrage, avant toute résolution de la racine des données).
pub fn apply_root_override(args: &[String]) {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let value = if let Some(v) = arg.strip_prefix("--root=") {
            Some(v.to_string())
        } else if arg == "--root" {
            iter.next().cloned()
        } else {
            None
        };
        if let Some(dir) = value.filter(|v| !v.trim().is_empty()) {
            let _ = ROOT_OVERRIDE.set(PathBuf::from(dir));
            return;
        }
    }
}

/// Racine des données, par priorité :
/// 1. `--root <dir>` (explicite) ;
/// 2. le registre `install.json` à côté du binaire (installations
///    antérieures — compatibilité) ;
/// 3. `~/.config/dakkabot/` (installation cargo standard) ;
/// 4. le répertoire courant (repli sans HOME).
pub fn data_root() -> PathBuf {
    if let Some(root) = ROOT_OVERRIDE.get() {
        return root.clone();
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let reg = dir.join(REGISTRY_FILE);
            if let Ok(raw) = std::fs::read_to_string(&reg) {
                if let Ok(r) = serde_json::from_str::<Registry>(&raw) {
                    return PathBuf::from(r.root);
                }
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config").join("dakkabot");
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn state_file(root: &Path) -> PathBuf {
    root.join(STATE_FILE)
}

pub(crate) fn pid_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    // kill(pid, 0) : permission refusée = le processus existe.
    let ret = unsafe { libc::kill(pid as i32, 0) };
    ret == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn read_state(root: &Path) -> Option<DaemonState> {
    let raw = std::fs::read_to_string(state_file(root)).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_state(root: &Path, pid: u32) -> Result<(), ExitError> {
    let state = DaemonState {
        pid,
        started_at: chrono::Utc::now().to_rfc3339(),
    };
    crate::fsutil::secure_write(
        &state_file(root),
        serde_json::to_string(&state)
            .map_err(|e| ExitError::config(e.to_string()))?
            .as_bytes(),
    )
    .map_err(|e| ExitError::config(e.to_string()))
}

fn remove_state(root: &Path) {
    let _ = std::fs::remove_file(state_file(root));
}

/// Entre dans la racine des données (créée au besoin) — appelé au
/// démarrage par le bot : les chemins relatifs (`.config.d/`, `logs/`,
/// `assets/`) sont alors ceux du dossier de données.
pub fn enter_data_root() {
    let root = data_root();
    if std::fs::create_dir_all(&root).is_ok() {
        let _ = std::env::set_current_dir(&root);
    }
}

// ── Install / uninstall ────────────────────────────────────────────────────

fn is_root() -> bool {
    (unsafe { libc::geteuid() }) == 0
}

fn service_user() -> String {
    std::env::var("SUDO_USER")
        .ok()
        .filter(|u| !u.trim().is_empty() && u != "root")
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "dakkabot".to_string())
}

/// Home de l'utilisateur du service (via passwd) — `sudo dkbot install`
/// s'exécute en root mais les DONNÉES vont dans le home de l'UTILISATEUR.
fn service_user_home() -> Option<PathBuf> {
    let user = service_user();
    let c = std::ffi::CString::new(user.clone()).ok()?;
    let pw = unsafe { libc::getpwnam(c.as_ptr()) };
    if pw.is_null() {
        return std::env::var_os("HOME").map(PathBuf::from);
    }
    let home = unsafe { std::ffi::CStr::from_ptr((*pw).pw_dir) };
    Some(PathBuf::from(home.to_string_lossy().into_owned()))
}

/// Dossier de données de l'installation STANDARD : `~/.config/dakkabot/`
/// (home de l'utilisateur du service, pas celui du root qui exécute sudo).
pub fn standard_data_root() -> PathBuf {
    let home = service_user_home().or_else(|| std::env::var_os("HOME").map(PathBuf::from));
    home.map(|h| h.join(".config").join("dakkabot"))
        .unwrap_or_else(data_root)
}

/// Cibles binaires de l'installation STANDARD : `/usr/bin` et `/bin` si
/// c'est un VRAI répertoire (pas un symlink) — cibles canoniques
/// dédoublonnées (`/bin` → `/usr/bin` sur la plupart des distributions).
pub fn standard_bin_targets() -> Vec<PathBuf> {
    let mut targets = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    for d in STANDARD_BIN_DIRS {
        let p = PathBuf::from(d);
        if !p.is_dir() {
            continue;
        }
        let canon = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
        if seen.contains(&canon) {
            continue;
        }
        seen.push(canon);
        targets.push(p);
    }
    targets
}

/// Queue durcie commune aux deux unités systemd générées.
fn unit_hardening_tail(readwrite: &str) -> String {
    format!(
        r#"Restart=on-failure
RestartSec=5s
# exit 78 (configuration) ne redémarre PAS en boucle.
RestartPreventExitStatus=78

# Logs vers journald (le bot écrit en parallèle dans logs/dakkabot.log).
StandardOutput=journal
StandardError=journal
SyslogIdentifier=dakkabot

# ── Durcissement ──
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictSUIDSGID=true
LockPersonality=true
# Préfixe « - » : systemd tolère l'absence du chemin au chargement.
# ⚠ Cibles SSH : ProtectHome=true rend ~/.ssh en lecture seule. Pointez
# SSH_KNOWN_HOSTS_FILE vers un chemin INSCRIPTIBLE (le backend refuse de
# se connecter s'il ne peut pas mémoriser la clé d'hôte — TOFU
# fail-closed).
# ⚠ SAVES_PATH (et ses backups) HORS de ces chemins nécessite d'élargir
# ReadWritePaths.
ReadWritePaths={readwrite}

[Install]
WantedBy=multi-user.target
"#
    )
}

/// Unité systemd de l'installation DANS UN DOSSIER DÉDIÉ (`install <dir>`
/// + `--daemon`) : binaire et données dans le dossier, `--root` explicite.
pub fn generate_unit(user: &str, dir: &Path) -> String {
    format!(
        r#"[Unit]
Description=dakkabot — Discord bot for fine-grained VPS control
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User={user}
Group={user}
WorkingDirectory={dir}

# systemd injecte directement les variables du fichier .env.
# Format KEY=VALUE, compatible avec le chargeur .env du bot.
EnvironmentFile={dir}/.config.d/.env

# Binaire installé par `dkbot install` (registre install.json à côté) ;
# --root explicite, prioritaire sur le registre et ~/.config/dakkabot.
# --non-interactive : jamais de prompt, exit 78 = configuration.
ExecStart={dir}/dkbot run --root {dir} --non-interactive

{tail}"#,
        user = user,
        dir = dir.display(),
        tail = unit_hardening_tail(&format!(
            "-{dir}/.config.d -{dir}/logs",
            dir = dir.display()
        )),
    )
}

/// Unité systemd de l'installation STANDARD (`install` sans argument +
/// `--daemon`) : binaire dans `/usr/bin`, données dans
/// `~/.config/dakkabot/` de l'utilisateur du service.
pub fn generate_unit_standard(user: &str) -> String {
    format!(
        r#"[Unit]
Description=dakkabot — Discord bot for fine-grained VPS control
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User={user}
Group={user}
WorkingDirectory=%h

# systemd injecte directement les variables du fichier .env
# (données standard dans ~/.config/dakkabot).
EnvironmentFile=%h/.config/dakkabot/.config.d/.env

# Binaire système + --root explicite vers les données standard ;
# --non-interactive : jamais de prompt, exit 78 = configuration.
ExecStart=/usr/bin/dkbot run --root %h/.config/dakkabot --non-interactive

{tail}"#,
        user = user,
        tail = unit_hardening_tail("- %h/.config/dakkabot/.config.d - %h/.config/dakkabot/logs"),
    )
}

/// L'unité dakkabot.service est-elle active ? (lecture via systemctl,
/// silencieuse si absent).
fn unit_active() -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", UNIT_NAME])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Le processus `pid` exécute-t-il CE binaire ? (lecture de
/// `/proc/<pid>/exe` — un exécutable en cours d'exécution est verrouillé
/// et ne peut pas être écrasé).
fn pid_runs_exe(pid: u32, exe: &Path) -> bool {
    let link = PathBuf::from(format!("/proc/{pid}/exe"));
    std::fs::read_link(&link)
        .ok()
        .and_then(|p| std::fs::canonicalize(&p).ok())
        .zip(std::fs::canonicalize(exe).ok())
        .map(|(a, b)| a == b)
        .unwrap_or(false)
}

/// Quand l'installation tourne en root (sudo), les fichiers copiés sont
/// root-owned : le service (User=utilisateur) ne pourrait ni lire le
/// `.env` 0600 ni écrire ses logs. Passe la propriété à l'utilisateur du
/// service (uid:gid via passwd). Sans effet hors root.
fn chown_to_service_user(path: &Path) {
    if !is_root() {
        return;
    }
    let Ok(c) = std::ffi::CString::new(service_user()) else {
        return;
    };
    let pw = unsafe { libc::getpwnam(c.as_ptr()) };
    if pw.is_null() {
        return;
    }
    let (uid, gid) = unsafe { ((*pw).pw_uid, (*pw).pw_gid) };
    let Ok(p) = std::ffi::CString::new(path.to_string_lossy().as_bytes()) else {
        return;
    };
    unsafe {
        libc::chown(p.as_ptr(), uid, gid);
    }
}

fn copy_if_missing(src: &Path, dst: &Path) -> Result<bool, String> {
    if !src.exists() {
        return Ok(false);
    }
    if dst.exists() {
        return Ok(false); // jamais d'écrasement des DONNÉES
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    std::fs::copy(src, dst)
        .map_err(|e| format!("copy {} → {}: {e}", src.display(), dst.display()))?;
    Ok(true)
}

fn copy_dir_if_missing(src: &Path, dst: &Path) -> Result<bool, String> {
    if !src.is_dir() {
        return Ok(false);
    }
    if dst.exists() {
        return Ok(false);
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let status = Command::new("cp")
        .arg("-r")
        .arg(src)
        .arg(dst)
        .status()
        .map_err(|e| format!("cp -r {} → {}: {e}", src.display(), dst.display()))?;
    if !status.success() {
        return Err(format!(
            "cp -r {} → {} failed",
            src.display(),
            dst.display()
        ));
    }
    Ok(true)
}

fn run_cmd(cmd: &mut Command, what: &str) -> Result<(), String> {
    let out = cmd.output().map_err(|e| format!("{what}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{what} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

/// Installe le BINAIRE SEUL : copie directe du binaire courant (donc
/// l'architecture de la machine), sans daemon.
///
/// - SANS dossier (`None`) : cibles système STANDARD (`/usr/bin` et
///   `/bin` si réel, dédoublonnées) — root requis — PAS de registre, et
///   les DONNÉES vivent dans `~/.config/dakkabot/` (home de l'utilisateur
///   du service) ; les fichiers du répertoire courant (`.config.d/.env`,
///   `assets/`) y sont copiés si absents — jamais écrasés ;
/// - AVEC dossier : binaire + registre `install.json` dans ce dossier,
///   données au même endroit. Si le daemon autonome de CE dossier tourne,
///   il est arrêté avant l'écrasement puis relancé sur le binaire
///   installé (upgrade à chaud).
///
/// `daemon` (`--daemon`) = unité systemd EN PLUS (explicite, root requis).
pub fn install(dir: Option<&str>, daemon: bool) -> Result<String, String> {
    // Échouer AVANT toute modification : pas de demi-installation.
    if daemon && !is_root() {
        return Err(
            "root requis pour --daemon. Relancez : sudo dkbot install [<dir>] --daemon".into(),
        );
    }
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let mut done: Vec<String> = Vec::new();
    match dir {
        None => install_standard(&exe, daemon, &mut done)?,
        Some(d) => install_into(
            &exe,
            PathBuf::from(d.trim_end_matches('/')),
            daemon,
            &mut done,
        )?,
    }
    Ok(done.join("\n"))
}

/// Installation STANDARD (`install` sans argument) : binaire dans les
/// cibles système, données dans `~/.config/dakkabot/`.
fn install_standard(exe: &Path, daemon: bool, done: &mut Vec<String>) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if !is_root() {
        return Err(
            "root requis pour installer dans /usr/bin. Relancez : sudo dkbot install [--daemon]"
                .into(),
        );
    }
    let targets = standard_bin_targets();
    if targets.is_empty() {
        return Err(format!(
            "aucune cible système standard disponible ({}) — passez un dossier explicite",
            STANDARD_BIN_DIRS.join(", ")
        ));
    }
    let data = standard_data_root();

    // Un binaire EN COURS D'EXÉCUTION est verrouillé (ETXTBSY) : arrêter
    // l'unité systemd active et/ou le daemon autonome qui exécute CE
    // binaire AVANT l'écrasement.
    let unit_was_active = unit_active();
    if unit_was_active {
        run_cmd(Command::new("systemctl").args(["stop", UNIT_NAME]), "stop")?;
    }
    let mut daemon_was_active = false;
    if let Some(state) = read_state(&data) {
        if pid_alive(state.pid)
            && targets
                .iter()
                .any(|d| pid_runs_exe(state.pid, &d.join(env!("CARGO_BIN_NAME"))))
        {
            daemon_was_active = true;
            let _ = stop_at(&data);
        }
    }

    // 1. Binaire (toujours rafraîchi) dans chaque cible standard.
    let mut installed: Vec<String> = Vec::new();
    for dir in &targets {
        let target_exe = dir.join(env!("CARGO_BIN_NAME"));
        let same_file = std::fs::canonicalize(exe)
            .ok()
            .zip(std::fs::canonicalize(&target_exe).ok())
            .map(|(a, b)| a == b)
            .unwrap_or(false);
        if same_file {
            continue; // réinstallation : le binaire EST déjà la cible
        }
        std::fs::copy(exe, &target_exe).map_err(|e| format!("copy binary: {e}"))?;
        std::fs::set_permissions(&target_exe, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("chmod binary: {e}"))?;
        installed.push(target_exe.display().to_string());
    }
    if installed.is_empty() {
        done.push("Binaire déjà en place dans les cibles standard (aucune copie)".into());
    } else {
        done.push(format!("Binaire installé : {}", installed.join(", ")));
    }

    // 2. Données dans ~/.config/dakkabot/ (jamais d'écrasement).
    std::fs::create_dir_all(&data).map_err(|e| format!("mkdir {}: {e}", data.display()))?;
    // sudo : les données doivent appartenir à l'utilisateur du service.
    chown_to_service_user(&data);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let copied_env = copy_if_missing(&cwd.join(".config.d/.env"), &data.join(".config.d/.env"))?;
    let copied_assets = copy_dir_if_missing(&cwd.join("assets"), &data.join("assets"))?;
    if copied_env {
        std::fs::set_permissions(
            data.join(".config.d/.env"),
            std::fs::Permissions::from_mode(0o600),
        )
        .map_err(|e| format!("chmod .env: {e}"))?;
        chown_to_service_user(&data.join(".config.d"));
        chown_to_service_user(&data.join(".config.d/.env"));
        done.push(format!(
            "Configuration copiée : {}",
            data.join(".config.d/.env").display()
        ));
    }
    if copied_assets {
        done.push("Assets copiés : assets/".into());
    }
    done.push(format!(
        "Données : {} (.config.d/, logs/, assets/)",
        data.display()
    ));

    // 3. systemd UNIQUEMENT si demandé explicitement (--daemon) ; sinon,
    //    restaurer ce qui tournait sur le binaire écrasé.
    if daemon {
        install_unit(&generate_unit_standard(&service_user()))?;
        done.push("Unité systemd installée et démarrée (--daemon)".into());
    } else if unit_was_active {
        run_cmd(
            Command::new("systemctl").args(["start", UNIT_NAME]),
            "start",
        )?;
        done.push("Unité systemd redémarrée sur le binaire installé".into());
    } else if daemon_was_active {
        spawn_detached_from(&targets[0].join(env!("CARGO_BIN_NAME")), &data)?;
        for _ in 0..50 {
            if let Some(state) = read_state(&data) {
                if pid_alive(state.pid) {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        done.push(format!(
            "Daemon redémarré sur le binaire installé (pid {})",
            read_state(&data).map(|s| s.pid).unwrap_or(0)
        ));
    }
    Ok(())
}

/// Installation DANS UN DOSSIER DÉDIÉ (`install <dir>`) : binaire +
/// registre + données dans le dossier.
fn install_into(
    exe: &Path,
    dir: PathBuf,
    daemon: bool,
    done: &mut Vec<String>,
) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;

    // Le binaire de CE répertoire tourne-t-il ? L'arrêter pour pouvoir
    // l'écraser (un exécutable en cours d'exécution est verrouillé —
    // ETXTBSY). Daemon autonome ET unité systemd éventuelle.
    let was_active = read_state(&dir).map(|s| pid_alive(s.pid)).unwrap_or(false);
    if was_active {
        let _ = stop_at(&dir);
    }
    let unit_was_active = unit_active();
    if unit_was_active {
        run_cmd(Command::new("systemctl").args(["stop", UNIT_NAME]), "stop")?;
    }

    // 1. Binaire (toujours rafraîchi) + registre.
    let target_exe = dir.join(env!("CARGO_BIN_NAME"));
    let same_file = std::fs::canonicalize(exe)
        .ok()
        .zip(std::fs::canonicalize(&target_exe).ok())
        .map(|(a, b)| a == b)
        .unwrap_or(false);
    if !same_file {
        std::fs::copy(exe, &target_exe).map_err(|e| format!("copy binary: {e}"))?;
        std::fs::set_permissions(&target_exe, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("chmod binary: {e}"))?;
    }

    let registry = Registry {
        root: dir.display().to_string(),
    };
    crate::fsutil::secure_write(
        &dir.join(REGISTRY_FILE),
        serde_json::to_string(&registry)
            .map_err(|e| format!("registry json: {e}"))?
            .as_bytes(),
    )
    .map_err(|e| format!("write registry: {e}"))?;
    // sudo : le registre (0600) doit rester LISIBLE par le service, sinon
    // data_root() retomberait silencieusement sur ~/.config/dakkabot.
    chown_to_service_user(&dir.join(REGISTRY_FILE));

    // 2. Données du répertoire courant (jamais d'écrasement).
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let copied_env = copy_if_missing(&cwd.join(".config.d/.env"), &dir.join(".config.d/.env"))?;
    let copied_assets = copy_dir_if_missing(&cwd.join("assets"), &dir.join("assets"))?;
    if copied_env {
        std::fs::set_permissions(
            dir.join(".config.d/.env"),
            std::fs::Permissions::from_mode(0o600),
        )
        .map_err(|e| format!("chmod .env: {e}"))?;
    }
    // sudo : les données doivent appartenir à l'utilisateur du service.
    chown_to_service_user(&dir);
    chown_to_service_user(&dir.join(".config.d"));
    chown_to_service_user(&dir.join(".config.d/.env"));

    done.push(format!("Binaire installé : {}", target_exe.display()));
    if copied_env {
        done.push("Configuration copiée : .config.d/.env".into());
    }
    if copied_assets {
        done.push("Assets copiés : assets/".into());
    }

    // 3. systemd UNIQUEMENT si demandé explicitement (--daemon) ; sinon,
    //    restaurer ce qui tournait sur le binaire écrasé.
    if daemon {
        install_unit(&generate_unit(&service_user(), &dir))?;
        done.push("Unité systemd installée et démarrée (--daemon)".into());
    } else if unit_was_active {
        run_cmd(
            Command::new("systemctl").args(["start", UNIT_NAME]),
            "start",
        )?;
        done.push("Unité systemd redémarrée sur le binaire installé".into());
    } else if was_active {
        if is_root() {
            // Ne PAS relancer en root : le daemon appartiendrait à root et
            // l'utilisateur ne pourrait plus l'arrêter.
            done.push(format!(
                "Daemon autonome arrêté avant l'écrasement — relancez-le : {} start",
                target_exe.display()
            ));
        } else {
            // Relance du daemon autonome sur le NOUVEAU binaire installé
            // (pas le binaire du checkout qui exécute install).
            spawn_detached_from(&target_exe, &dir)?;
            for _ in 0..50 {
                if let Some(state) = read_state(&dir) {
                    if pid_alive(state.pid) {
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            done.push(format!(
                "Daemon redémarré sur le binaire installé (pid {})",
                read_state(&dir).map(|s| s.pid).unwrap_or(0)
            ));
        }
    } else {
        done.push(format!(
            "Binaire SEUL (pas de daemon). Démarrez : {} start",
            target_exe.display()
        ));
    }
    Ok(())
}

/// Écrit l'unité systemd, recharge et démarre (root — appelé uniquement
/// par les branches qui ont déjà vérifié).
fn install_unit(unit: &str) -> Result<(), String> {
    let unit_path = PathBuf::from("/etc/systemd/system").join(UNIT_NAME);
    std::fs::write(&unit_path, unit).map_err(|e| format!("write {}: {e}", unit_path.display()))?;
    run_cmd(
        Command::new("systemctl").arg("daemon-reload"),
        "daemon-reload",
    )?;
    run_cmd(
        Command::new("systemctl").args(["enable", "--now", UNIT_NAME]),
        "enable --now",
    )?;
    Ok(())
}

/// Retire l'installation : arrêt du daemon, binaire(s) + registre
/// supprimés, DONNÉES conservées (config, stats, logs). L'unité systemd
/// éventuelle est retirée aussi (root requis).
pub fn uninstall() -> Result<String, String> {
    let root = data_root();
    let _ = stop(); // daemon autonome éventuel
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let mut done = Vec::new();

    // Binaire dans une cible système standard ? Retirer TOUTES les cibles
    // (/usr/bin, /bin) — l'installation standard n'écrit PAS de registre.
    let exe_dir = exe
        .parent()
        .map(|d| std::fs::canonicalize(d).unwrap_or_else(|_| d.to_path_buf()));
    let in_standard = standard_bin_targets().iter().any(|t| {
        exe_dir
            .as_ref()
            .map(|ed| ed == &std::fs::canonicalize(t).unwrap_or_else(|_| t.clone()))
            .unwrap_or(false)
    });
    if in_standard {
        if !is_root() {
            return Err(
                "root requis pour désinstaller l'installation système. Relancez : sudo dkbot uninstall"
                    .into(),
            );
        }
        for dir in standard_bin_targets() {
            let target = dir.join(env!("CARGO_BIN_NAME"));
            if target.exists() {
                std::fs::remove_file(&target).map_err(|e| format!("remove binary: {e}"))?;
                done.push(format!("Binaire retiré : {}", target.display()));
            }
        }
    } else {
        if let Some(dir) = exe.parent() {
            let target = dir.join(env!("CARGO_BIN_NAME"));
            if target.exists() {
                std::fs::remove_file(&target).map_err(|e| format!("remove binary: {e}"))?;
                done.push(format!("Binaire retiré : {}", target.display()));
            }
            let reg = dir.join(REGISTRY_FILE);
            if reg.exists() {
                std::fs::remove_file(&reg).map_err(|e| format!("remove registry: {e}"))?;
            }
        }
        // Binaire de checkout / cargo (pas de registre à côté) exécuté en
        // root : retirer AUSSI l'installation système standard.
        let has_registry = exe
            .parent()
            .map(|d| d.join(REGISTRY_FILE).exists())
            .unwrap_or(false);
        if is_root() && !has_registry {
            for dir in standard_bin_targets() {
                let target = dir.join(env!("CARGO_BIN_NAME"));
                if target.exists() {
                    std::fs::remove_file(&target).map_err(|e| format!("remove binary: {e}"))?;
                    done.push(format!("Binaire retiré : {}", target.display()));
                }
            }
        }
    }
    // Unité systemd éventuelle (install --daemon antérieur).
    let unit_path = PathBuf::from("/etc/systemd/system").join(UNIT_NAME);
    if unit_path.exists() {
        if !is_root() {
            done.push(
                "Unité systemd encore présente : retirez-la avec sudo dkbot uninstall".into(),
            );
        } else {
            let _ = Command::new("systemctl")
                .args(["disable", "--now", UNIT_NAME])
                .status();
            std::fs::remove_file(&unit_path).map_err(|e| format!("remove unit: {e}"))?;
            let _ = Command::new("systemctl").arg("daemon-reload").status();
            done.push("Unité systemd retirée".into());
        }
    }
    done.push(format!(
        "Données CONSERVÉES dans {} (.config.d/, logs/, assets/)",
        root.display()
    ));
    Ok(done.join("\n"))
}

// ── Cycle de vie du daemon ────────────────────────────────────────────────

pub(crate) fn spawn_detached_from(exe: &Path, root: &Path) -> Result<(), String> {
    let _ = std::fs::create_dir_all(root.join("logs"));
    let sup_log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("logs").join("supervisor.log"))
        .map_err(|e| format!("open supervisor.log: {e}"))?;
    let mut cmd = Command::new(exe);
    cmd.arg("supervise")
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(sup_log));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Détaché du terminal de contrôle : SIGHUP du shell appelant ne
        // tuera plus le superviseur.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    cmd.spawn().map_err(|e| format!("spawn supervisor: {e}"))?;
    Ok(())
}

pub fn start() -> Result<String, String> {
    start_at(&data_root())
}

pub fn start_at(root: &Path) -> Result<String, String> {
    if let Some(state) = read_state(root) {
        if pid_alive(state.pid) {
            return Ok(format!(
                "Déjà actif (superviseur pid {}, démarré à {})",
                state.pid, state.started_at
            ));
        }
        remove_state(root); // état obsolète
    }
    if !std::env::current_exe().map(|e| e.exists()).unwrap_or(false) {
        return Err("binaire courant introuvable".into());
    }
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    spawn_detached_from(&exe, root)?;
    // Attendre l'état écrit par le superviseur (50 × 100 ms).
    for _ in 0..50 {
        if let Some(state) = read_state(root) {
            if pid_alive(state.pid) {
                return Ok(format!(
                    "Daemon démarré (superviseur pid {}, logs : {})",
                    state.pid,
                    root.join("logs").join("dakkabot.log").display()
                ));
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("le superviseur n'a pas démarré — consultez logs/supervisor.log".into())
}

pub fn stop() -> Result<String, String> {
    stop_at(&data_root())
}

pub fn stop_at(root: &Path) -> Result<String, String> {
    let Some(state) = read_state(root) else {
        return Ok("Daemon arrêté (aucun superviseur connu).".into());
    };
    if !pid_alive(state.pid) {
        remove_state(root);
        return Ok("Daemon arrêté (état obsolète purgé).".into());
    }
    // SIGTERM au superviseur : il arrête l'enfant proprement.
    let ret = unsafe { libc::kill(state.pid as i32, libc::SIGTERM) };
    if ret != 0 {
        return Err(format!(
            "signal au superviseur {} impossible : {}",
            state.pid,
            std::io::Error::last_os_error()
        ));
    }
    for _ in 0..(KILL_TIMEOUT.as_millis() / 100) {
        if !pid_alive(state.pid) {
            remove_state(root);
            return Ok(format!("Daemon arrêté (superviseur pid {}).", state.pid));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!(
        "le superviseur {} ne s'est pas arrêté à temps",
        state.pid
    ))
}

pub fn restart() -> Result<String, String> {
    let stopped = stop()?;
    let started = start()?;
    Ok(format!("{stopped}\n{started}"))
}

pub fn status() -> String {
    let root = data_root();
    match read_state(&root) {
        Some(state) if pid_alive(state.pid) => format!(
            "Daemon ACTIF (superviseur pid {}, démarré à {})",
            state.pid, state.started_at
        ),
        Some(_) => "Daemon ARRÊTÉ (état obsolète — lancez `start`).".to_string(),
        None => "Daemon ARRÊTÉ.".to_string(),
    }
}

pub fn logs(lines: usize) -> Result<String, String> {
    let root = data_root();
    let path = root.join("logs").join("dakkabot.log");
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("lecture de {} : {e}", path.display()))?;
    let mut tail: Vec<&str> = raw.lines().rev().take(lines.max(1)).collect();
    tail.reverse();
    Ok(tail.join("\n"))
}

// ── Superviseur (processus détaché) ───────────────────────────────────────

/// Politique de relance (fonction PURE, testée) :
/// - exit 78 (configuration) → abandon immédiat ;
/// - uptime stable ≥ 30 s → compteur remis à zéro ;
/// - plus de 10 redémarrages rapides → abandon ;
/// - sinon relance après backoff 5 s → 30 s.
pub enum RestartAction {
    Restart { after: Duration },
    GiveUp { reason: &'static str },
}

pub fn restart_after(exit_code: Option<i32>, uptime: Duration, restarts: u32) -> RestartAction {
    if exit_code == Some(78) {
        return RestartAction::GiveUp {
            reason: "exit 78 (configuration invalide) — jamais de boucle de relance",
        };
    }
    if uptime >= STABLE_UPTIME {
        return RestartAction::Restart {
            after: Duration::ZERO,
        };
    }
    if restarts >= MAX_RESTARTS {
        return RestartAction::GiveUp {
            reason: "plus de 10 redémarrages rapides",
        };
    }
    let after = (RESTART_DELAY * (restarts + 1)).min(BACKOFF_CAP);
    RestartAction::Restart { after }
}

/// Boucle du superviseur : exécute le bot en enfant, le relance selon la
/// politique ci-dessus, s'arrête proprement sur SIGTERM/SIGINT.
pub async fn supervise() -> i32 {
    let root = data_root();
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(_) => return 1,
    };
    let _ = std::fs::create_dir_all(root.join("logs"));
    // stdout/stderr de l'enfant dans un fichier SÉPARÉ : le logger du bot
    // écrit déjà dakkabot.log — rediriger le stdio dessus doublerait
    // chaque ligne.
    let stdout_log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("logs").join("stdout.log"))
        .expect("ouvrir logs/stdout.log");

    if let Err(e) = write_state(&root, std::process::id()) {
        eprintln!("supervise: cannot write state: {e}");
        return 1;
    }
    eprintln!("[supervise] started, pid {}", std::process::id());

    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("handler SIGTERM");
    let mut int = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .expect("handler SIGINT");

    let mut restarts: u32 = 0;
    loop {
        let mut cmd = tokio::process::Command::new(&exe);
        cmd.arg("run")
            .current_dir(&root)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::from(
                stdout_log.try_clone().expect("clone stdout log"),
            ))
            .stderr(std::process::Stdio::from(
                stdout_log.try_clone().expect("clone stdout log"),
            ));
        unsafe {
            cmd.pre_exec(|| {
                libc::setpgid(0, 0); // l'enfant a son propre groupe : SIGTERM ciblé
                Ok(())
            });
        }
        let started = std::time::Instant::now();
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[supervise] spawn failed: {e}");
                let action = restart_after(None, Duration::ZERO, restarts);
                match action {
                    RestartAction::GiveUp { reason } => {
                        eprintln!("[supervise] giving up: {reason}");
                        remove_state(&root);
                        return 1;
                    }
                    RestartAction::Restart { after } => {
                        restarts += 1;
                        if !after.is_zero() {
                            tokio::time::sleep(after).await;
                        }
                        continue;
                    }
                }
            }
        };

        tokio::select! {
            status = child.wait() => {
                let uptime = started.elapsed();
                let code = status.ok().and_then(|s| s.code());
                let action = restart_after(code, uptime, restarts);
                match action {
                    RestartAction::Restart { after } => {
                        restarts = if uptime >= STABLE_UPTIME { 0 } else { restarts + 1 };
                        if !after.is_zero() {
                            eprintln!("[supervise] bot exited ({code:?}), restarting in {after:?}");
                            tokio::time::sleep(after).await;
                        }
                    }
                    RestartAction::GiveUp { reason } => {
                        eprintln!("[supervise] giving up: {reason}");
                        remove_state(&root);
                        return 1;
                    }
                }
            }
            _ = term.recv() => {
                shutdown_child(&mut child).await;
                remove_state(&root);
                eprintln!("[supervise] stopped (SIGTERM)");
                return 0;
            }
            _ = int.recv() => {
                shutdown_child(&mut child).await;
                remove_state(&root);
                eprintln!("[supervise] stopped (SIGINT)");
                return 0;
            }
        }
    }
}

async fn shutdown_child(child: &mut tokio::process::Child) {
    let _ = child.start_kill(); // SIGKILL : le bot est déjà protégé par son propre handler
    let _ = tokio::time::timeout(KILL_TIMEOUT, child.wait()).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_policy_never_loops_on_config_error() {
        // exit 78 → abandon immédiat, quelle que soit la situation.
        assert!(matches!(
            restart_after(Some(78), Duration::ZERO, 0),
            RestartAction::GiveUp { .. }
        ));
        // Stable ≥ 30 s → relance immédiate, compteur remis à zéro.
        assert!(matches!(
            restart_after(Some(1), STABLE_UPTIME, 9),
            RestartAction::Restart { after } if after.is_zero()
        ));
        // Trop de redémarrages rapides → abandon.
        assert!(matches!(
            restart_after(Some(1), Duration::from_secs(2), MAX_RESTARTS),
            RestartAction::GiveUp { .. }
        ));
        // Backoff borné à 30 s.
        assert!(matches!(
            restart_after(Some(1), Duration::from_secs(2), 3),
            RestartAction::Restart { after } if after == RESTART_DELAY * 4
        ));
    }

    #[test]
    fn registry_resolves_data_root() {
        let dir = std::env::temp_dir().join(format!("dakkabot_daemon_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let registry = Registry {
            root: dir.display().to_string(),
        };
        std::fs::write(
            dir.join(REGISTRY_FILE),
            serde_json::to_string(&registry).unwrap(),
        )
        .unwrap();
        // data_root lit le registre À CÔTÉ de l'exe courant : on teste le
        // parse du registre, pas le chemin de l'exe (binaire de test).
        let raw = std::fs::read_to_string(dir.join(REGISTRY_FILE)).unwrap();
        let parsed: Registry = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.root, dir.display().to_string());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn generated_unit_targets_dir_and_no_prompt() {
        let unit = generate_unit("dakkabot", Path::new("/opt/dakkabot"));
        assert!(unit.contains("User=dakkabot"));
        assert!(unit.contains("Group=dakkabot"));
        assert!(unit.contains("WorkingDirectory=/opt/dakkabot"));
        assert!(unit
            .contains("ExecStart=/opt/dakkabot/dkbot run --root /opt/dakkabot --non-interactive"));
        assert!(unit.contains("EnvironmentFile=/opt/dakkabot/.config.d/.env"));
        assert!(unit.contains("NoNewPrivileges=true"));
        assert!(unit.contains("ReadWritePaths=-/opt/dakkabot/.config.d -/opt/dakkabot/logs"));
        assert!(unit.contains("WantedBy=multi-user.target"));
        assert!(unit.contains("RestartPreventExitStatus=78"));
    }

    #[test]
    fn generated_unit_standard_targets_system_bin_and_user_config() {
        let unit = generate_unit_standard("testuser");
        assert!(unit.contains("User=testuser"));
        assert!(unit.contains("Group=testuser"));
        // Binaire système, données dans le home de l'utilisateur du service.
        assert!(unit
            .contains("ExecStart=/usr/bin/dkbot run --root %h/.config/dakkabot --non-interactive"));
        assert!(unit.contains("EnvironmentFile=%h/.config/dakkabot/.config.d/.env"));
        assert!(unit
            .contains("ReadWritePaths=- %h/.config/dakkabot/.config.d - %h/.config/dakkabot/logs"));
        assert!(unit.contains("NoNewPrivileges=true"));
        assert!(unit.contains("RestartPreventExitStatus=78"));
        assert!(unit.contains("WantedBy=multi-user.target"));
    }
}
