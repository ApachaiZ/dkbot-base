//! État partagé du bot : opérateurs (chiffrés), sessions par utilisateur
//! (chiffrées), verrous persistants des actions power (clair).
//!
//! Fichiers (`.config.d/`) :
//! - `users.json` — liste des opérateurs (chiffrée), source de vérité =
//!   le fichier, réécrit à chaque mutation, gardé en mémoire ;
//! - `sessions.json` — `{ uid: { target, service } }` (chiffré), lu une
//!   fois au démarrage ;
//! - `active.lock` — `{ "target:service": { action, startedAt, token } }`
//!   (clair, 0600) : verrous inter-process des actions power, TTL 11 min,
//!   posés de façon ATOMIQUE (insert-si-absent sous mutex + relecture).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rand::RngCore;

use crate::crypto;
use crate::errors::ExitError;
use crate::fsutil;
use crate::i18n::{fill, Catalog};
use crate::logger;
use crate::paths;

/// Verrou orphelin purgé au-delà : doit couvrir le budget complet d'un
/// flux power (`FLOW_BUDGET` 12 min, copie de sauvegardes comprise) — un
/// TTL plus court laisserait le watchdog alerter « crash » pendant la fin
/// d'une action volontaire. Le verrou est de toute façon libéré
/// explicitement en fin de flux : le TTL ne joue qu'en cas de mort du bot.
const LOCK_TTL: Duration = Duration::from_secs(15 * 60);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Nettoyage de la liste des opérateurs : tableau d'IDs Discord (chiffres
/// uniquement), dédupliqué. Une entrée invalide est ignorée avec un log ;
/// une forme non-tableau est une ERREUR (fichier corrompu = blocage délibéré).
fn clean_user_list(data: &serde_json::Value, catalog: &Catalog) -> Result<Vec<String>, String> {
    let Some(array) = data.as_array() else {
        return Err(catalog.audit.users_not_array.to_string());
    };
    let mut out: Vec<String> = Vec::new();
    for entry in array {
        match entry.as_str() {
            Some(id) if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) => {
                if !out.iter().any(|x| x == id) {
                    out.push(id.to_string());
                }
            }
            _ => logger::warn(fill(
                catalog.audit.users_invalid_entry,
                &[("entry", &entry.to_string())],
            )),
        }
    }
    Ok(out)
}

fn load_users(token: &str, catalog: &Catalog) -> Result<Vec<String>, String> {
    let path = paths::users_file();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let Some(value) = crypto::read_json(token, &path) else {
        // Fichier illisible (rotation de token…) : on repart à zéro plutôt
        // que de bloquer le démarrage — le fichier reste sur disque.
        return Ok(Vec::new());
    };
    clean_user_list(&value, catalog)
}

#[derive(serde::Deserialize, serde::Serialize, Default)]
struct UserSession {
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    service: Option<String>,
}

fn load_sessions(token: &str) -> HashMap<String, UserSession> {
    let mut map = HashMap::new();
    if let Some(value) = crypto::read_json(token, paths::sessions_file().as_path()) {
        if let Some(obj) = value.as_object() {
            for (uid, session) in obj {
                if let Ok(s) = serde_json::from_value::<UserSession>(session.clone()) {
                    map.insert(uid.clone(), s);
                }
            }
        }
    }
    map
}

/// État partagé. Les mutex sont des `std::sync::Mutex` délibérés : les
/// sections critiques sont courtes et ne contiennent JAMAIS d'`.await`.
pub struct State {
    token: String,
    owner_id: String,
    catalog: &'static Catalog,
    users: Mutex<Vec<String>>,
    target_sessions: Mutex<HashMap<String, String>>,
    service_sessions: Mutex<HashMap<String, String>>,
    /// États des modules (activés/désactivés à la volée, persistés).
    modules: Mutex<crate::modules::Modules>,
    /// Entrées du planificateur (persistées dans `schedule.json`).
    schedule: Mutex<Vec<crate::schedule::Entry>>,
    /// Moments du dernier relâchement de verrou (mémoire uniquement) —
    /// délai de grâce anti-fausse-alerte du watchdog.
    lock_releases: Mutex<HashMap<String, Instant>>,
    /// Sérialise les lectures-modifications-écritures du fichier de
    /// verrous : la pose d'un verrou est ATOMIQUE (insert-if-absent sous
    /// ce mutex). `std::sync::Mutex` délibéré : aucune section n'await.
    lock_file_mutex: Mutex<()>,
}

impl State {
    pub fn new(token: &str, owner_id: &str, catalog: &'static Catalog) -> Result<Self, ExitError> {
        let users = match load_users(token, catalog) {
            Ok(users) => users,
            Err(reason) => {
                return Err(ExitError::config(format!(
                    "{} ({reason})",
                    catalog.errors.invalid_users_json_shape
                )))
            }
        };
        let sessions = load_sessions(token);
        let mut target_sessions = HashMap::new();
        let mut service_sessions = HashMap::new();
        for (uid, s) in sessions {
            if let Some(t) = s.target {
                target_sessions.insert(uid.clone(), t);
            }
            if let Some(sv) = s.service {
                service_sessions.insert(uid, sv);
            }
        }
        Ok(Self {
            token: token.to_string(),
            owner_id: owner_id.to_string(),
            catalog,
            users: Mutex::new(users),
            target_sessions: Mutex::new(target_sessions),
            service_sessions: Mutex::new(service_sessions),
            modules: Mutex::new(crate::modules::Modules::load()),
            schedule: Mutex::new(crate::schedule::load()),
            lock_releases: Mutex::new(HashMap::new()),
            lock_file_mutex: Mutex::new(()),
        })
    }

    // ── Opérateurs ─────────────────────────────────────────────────────────

    pub fn is_owner(&self, id: &str) -> bool {
        id == self.owner_id
    }

    pub fn is_allowed(&self, id: &str) -> bool {
        self.is_owner(id) || self.users.lock().expect("users").iter().any(|u| u == id)
    }

    // ── Modules ───────────────────────────────────────────────────────────

    /// Le module est-il actif ? (état explicite, sinon défaut)
    pub fn module_enabled(&self, name: &str) -> bool {
        self.modules.lock().expect("modules").enabled(name)
    }

    /// Bascule un module et persiste. `Err` = nom inconnu.
    pub fn set_module(&self, name: &str, on: bool) -> Result<(), ()> {
        let mut modules = self.modules.lock().expect("modules");
        modules.set(name, on)?;
        modules.save();
        Ok(())
    }

    /// États de tous les modules connus (nom, actif).
    pub fn modules_entries(&self) -> Vec<(String, bool)> {
        self.modules.lock().expect("modules").entries()
    }

    // ── Planificateur ─────────────────────────────────────────────────────

    /// Copie des entrées planifiées.
    pub fn schedule_snapshot(&self) -> Vec<crate::schedule::Entry> {
        self.schedule.lock().expect("schedule").clone()
    }

    /// Ajoute une entrée (id = max+1) et persiste. Retourne l'id.
    pub fn schedule_add(&self, mut entry: crate::schedule::Entry) -> u64 {
        let mut entries = self.schedule.lock().expect("schedule");
        let next = entries.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        entry.id = next;
        entries.push(entry);
        crate::schedule::save(&entries);
        next
    }

    /// Retire une entrée par id et persiste. `false` = id inconnu.
    pub fn schedule_remove(&self, id: u64) -> bool {
        let mut entries = self.schedule.lock().expect("schedule");
        let before = entries.len();
        entries.retain(|e| e.id != id);
        if entries.len() != before {
            crate::schedule::save(&entries);
            true
        } else {
            false
        }
    }

    /// Marque une entrée exécutée aujourd'hui (anti-répétition).
    pub fn schedule_mark_run(&self, id: u64) {
        let mut entries = self.schedule.lock().expect("schedule");
        if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
            entry.last_run = Some(chrono::Utc::now().format("%Y-%m-%d").to_string());
            crate::schedule::save(&entries);
        }
    }

    /// Vide le planificateur et persiste.
    pub fn schedule_clear(&self) {
        let mut entries = self.schedule.lock().expect("schedule");
        entries.clear();
        crate::schedule::save(&entries);
    }

    pub fn get_users(&self) -> Vec<String> {
        self.users.lock().expect("users").clone()
    }

    pub fn save_users(&self, next: Vec<String>) {
        // La mémoire bascule même si l'écriture échoue (log).
        if let Err(e) = crypto::write_json(
            &self.token,
            paths::users_file().as_path(),
            &serde_json::Value::Array(
                next.iter()
                    .map(|u| serde_json::Value::String(u.clone()))
                    .collect(),
            ),
        ) {
            logger::error(fill(
                self.catalog.audit.sessions_write_failed,
                &[("error", &e.to_string())],
            ));
        }
        *self.users.lock().expect("users") = next;
    }

    // ── Sessions ───────────────────────────────────────────────────────────

    pub fn get_target_session(&self, user_id: &str) -> Option<String> {
        self.target_sessions
            .lock()
            .expect("sessions")
            .get(user_id)
            .cloned()
    }

    pub fn set_target_session(&self, user_id: &str, alias: &str) {
        self.target_sessions
            .lock()
            .expect("sessions")
            .insert(user_id.to_string(), alias.to_string());
        self.write_sessions();
    }

    pub fn clear_target_session(&self, user_id: &str) {
        let removed = self
            .target_sessions
            .lock()
            .expect("sessions")
            .remove(user_id)
            .is_some();
        if removed {
            self.write_sessions();
        }
    }

    pub fn get_service_session(&self, user_id: &str) -> Option<String> {
        self.service_sessions
            .lock()
            .expect("sessions")
            .get(user_id)
            .cloned()
    }

    pub fn set_service_session(&self, user_id: &str, alias: &str) {
        self.service_sessions
            .lock()
            .expect("sessions")
            .insert(user_id.to_string(), alias.to_string());
        self.write_sessions();
    }

    pub fn clear_service_session(&self, user_id: &str) {
        let removed = self
            .service_sessions
            .lock()
            .expect("sessions")
            .remove(user_id)
            .is_some();
        if removed {
            self.write_sessions();
        }
    }

    fn write_sessions(&self) {
        let target = self.target_sessions.lock().expect("sessions").clone();
        let service = self.service_sessions.lock().expect("sessions").clone();
        let mut obj = serde_json::Map::new();
        let mut uids: Vec<&String> = target.keys().chain(service.keys()).collect();
        uids.sort();
        uids.dedup();
        for uid in uids {
            obj.insert(
                uid.clone(),
                serde_json::json!({
                    "target": target.get(uid).cloned().unwrap_or_default(),
                    "service": service.get(uid).cloned().unwrap_or_default(),
                }),
            );
        }
        if let Err(e) = crypto::write_json(
            &self.token,
            paths::sessions_file().as_path(),
            &serde_json::Value::Object(obj),
        ) {
            logger::error(fill(
                self.catalog.audit.sessions_write_failed,
                &[("error", &e.to_string())],
            ));
        }
    }

    // ── Verrous (actions power) ────────────────────────────────────────────

    /// Pose le verrou persistant d'une action (`target:service`), de
    /// façon ATOMIQUE : insert-si-absent sous mutex de processus +
    /// relecture de vérification (jeton unique). Retourne `false` si la
    /// clé est déjà verrouillée (un autre flux a gagné) — l'appelant doit
    /// alors s'arrêter SANS envoyer de commande.
    ///
    /// Note inter-processus : deux INSTANCES du bot écriraient sans se
    /// voir — le bot est conçu pour une instance unique (un seul process
    /// connecté au gateway Discord).
    pub fn write_lock(&self, lock_key: &str, action: &str) -> bool {
        let _guard = self.lock_file_mutex.lock().expect("lock mutex");
        let path = paths::active_lock_file();
        let mut locks = read_locks(&path);
        if locks.contains_key(lock_key) {
            return false;
        }
        // Jeton unique : la relecture ne peut PAS se tromper de flux.
        let mut bytes = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        locks.insert(
            lock_key.to_string(),
            serde_json::json!({ "action": action, "startedAt": now_ms(), "token": token }),
        );
        if let Err(e) = fsutil::secure_write(
            &path,
            serde_json::to_string(&serde_json::Value::Object(locks))
                .unwrap_or_default()
                .as_bytes(),
        ) {
            logger::error(fill(
                self.catalog.audit.lock_write_failed,
                &[("error", &e.to_string())],
            ));
            return false;
        }
        // Relecture : notre jeton doit être là (l'écriture a réussi).
        read_locks(&path)
            .get(lock_key)
            .and_then(|v| v.get("token"))
            .and_then(|t| t.as_str())
            .map(|t| t == token)
            .unwrap_or(false)
    }

    /// Retire le verrou d'une clé (supprime le fichier s'il devient vide).
    /// Sérialisé par le même mutex que [`Self::write_lock`].
    pub fn clear_lock(&self, lock_key: &str) {
        let _guard = self.lock_file_mutex.lock().expect("lock mutex");
        let path = paths::active_lock_file();
        let mut locks = read_locks(&path);
        locks.remove(lock_key);
        if locks.is_empty() {
            let _ = std::fs::remove_file(&path);
        } else if let Err(e) = fsutil::secure_write(
            &path,
            serde_json::to_string(&serde_json::Value::Object(locks))
                .unwrap_or_default()
                .as_bytes(),
        ) {
            logger::error(fill(
                self.catalog.audit.lock_write_failed,
                &[("error", &e.to_string())],
            ));
        }
    }

    /// Actions en cours (`clé → action`), verrous orphelins purgés.
    /// Lecture pure : ne réécrit jamais le fichier.
    pub fn load_persisted_lock(&self) -> HashMap<String, String> {
        let path = paths::active_lock_file();
        let mut out = HashMap::new();
        for (k, v) in read_locks(&path) {
            let started = v.get("startedAt").and_then(|t| t.as_u64()).unwrap_or(0);
            if now_ms().saturating_sub(started) > LOCK_TTL.as_millis() as u64 {
                continue;
            }
            if let Some(action) = v.get("action").and_then(|a| a.as_str()) {
                out.insert(k, action.to_string());
            }
        }
        out
    }

    /// Clés actuellement verrouillées (watchdog : pas d'alerte crash pendant
    /// une action volontaire).
    pub fn read_lock_snapshot(&self) -> Vec<String> {
        self.load_persisted_lock().into_keys().collect()
    }

    /// Enregistre le relâchement d'un verrou (délai de grâce du watchdog).
    pub fn record_lock_release(&self, lock_key: &str) {
        self.lock_releases
            .lock()
            .expect("lock_releases")
            .insert(lock_key.to_string(), Instant::now());
    }

    /// Temps écoulé depuis le dernier relâchement du verrou (`None` si
    /// jamais relâché).
    pub fn lock_release_elapsed(&self, lock_key: &str) -> Option<Duration> {
        self.lock_releases
            .lock()
            .expect("lock_releases")
            .get(lock_key)
            .map(|at| at.elapsed())
    }
}

fn read_locks(path: &std::path::Path) -> serde_json::Map<String, serde_json::Value> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return serde_json::Map::new();
    };
    match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(serde_json::Value::Object(obj)) => obj,
        _ => serde_json::Map::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn clean_user_list_keeps_numeric_ids_only() {
        let catalog = crate::i18n::catalog("en");
        let value = json!(["123", "abc", "", "456", "123", 789]);
        let users = clean_user_list(&value, catalog).unwrap();
        assert_eq!(users, vec!["123".to_string(), "456".to_string()]);
    }

    #[test]
    fn clean_user_list_rejects_non_array() {
        let catalog = crate::i18n::catalog("en");
        assert!(clean_user_list(&json!({"a": 1}), catalog).is_err());
    }
}
