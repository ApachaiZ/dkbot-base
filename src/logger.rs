//! Logger maison : chaque ligne va dans `logs/dakkabot.log` (rotation à
//! 5 Mo) ET sur la sortie standard — le fichier est la mémoire du bot, la
//! console est pour l'opérateur. Niveaux : ERROR, WARN, INFO.

use std::fs;
use std::io::Write;
use std::sync::Mutex;

use crate::paths;

/// Taille maximale du fichier de log avant rotation.
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error,
    Warn,
    Info,
}

impl Level {
    fn label(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN ",
            Level::Info => "INFO ",
        }
    }
}

struct LoggerState {
    inited: bool,
}

static STATE: Mutex<LoggerState> = Mutex::new(LoggerState { inited: false });

/// Initialise le logger (créé `logs/`). Appelé une fois au démarrage.
pub fn init() {
    let mut st = STATE.lock().expect("poisoned logger state");
    if !st.inited {
        if let Some(parent) = paths::log_file().parent() {
            let _ = fs::create_dir_all(parent);
        }
        st.inited = true;
    }
}

/// Rotation : renomme le fichier courant en `.1` s'il dépasse la limite.
fn rotate() {
    let path = paths::log_file();
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > MAX_LOG_BYTES {
            let _ = fs::rename(&path, path.with_extension("log.1"));
        }
    }
}

/// Écrit une ligne horodatée dans le fichier et sur la sortie standard.
fn write(level: Level, text: &str) {
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let line = format!("{now} [{}] {text}", level.label());
    eprintln!("{line}");
    let path = paths::log_file();
    rotate();
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{line}");
    }
}

pub fn error(text: impl AsRef<str>) {
    write(Level::Error, text.as_ref());
}

pub fn warn(text: impl AsRef<str>) {
    write(Level::Warn, text.as_ref());
}

pub fn info(text: impl AsRef<str>) {
    write(Level::Info, text.as_ref());
}

/// Identifiant tronqué (8 premiers caractères) pour les logs d'audit.
pub fn short_id(id: String) -> String {
    id.chars().take(8).collect()
}

/// Bannière de démarrage (cadre aligné — largeur visuelle des emojis).
pub fn banner(lines: &[String]) {
    use unicode_width::UnicodeWidthStr;
    let width = lines
        .iter()
        .map(|l| UnicodeWidthStr::width(l.as_str()))
        .max()
        .unwrap_or(0)
        .max(10);
    let border = format!("╭{}╮", "─".repeat(width + 2));
    let empty = format!("│{}│", " ".repeat(width + 2));
    let mut out = Vec::new();
    out.push(border);
    out.push(empty.clone());
    for line in lines {
        let pad = width.saturating_sub(UnicodeWidthStr::width(line.as_str()));
        out.push(format!("│ {}{} │", line, " ".repeat(pad)));
    }
    out.push(empty);
    out.push(format!("╰{}╯", "─".repeat(width + 2)));
    for l in &out {
        write(Level::Info, l);
    }
}
