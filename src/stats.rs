//! Statistiques persistées des actions power : moyenne de durée par action
//! (« ⏱ En moyenne : 51 s »). Fichier `.config.d/stats.json` (clair) :
//! `{ "<action>": { "count": f64, "sumMs": f64 } }`.

use std::collections::HashMap;

use crate::fsutil;
use crate::logger;
use crate::paths;

fn read_all() -> HashMap<String, (f64, f64)> {
    let mut map = HashMap::new();
    let Ok(raw) = std::fs::read_to_string(paths::stats_file()) else {
        return map;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return map;
    };
    let Some(obj) = value.as_object() else {
        return map;
    };
    for (action, entry) in obj {
        let count = entry.get("count").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let sum = entry.get("sumMs").and_then(|v| v.as_f64()).unwrap_or(0.0);
        map.insert(action.clone(), (count, sum));
    }
    map
}

fn write_all(map: &HashMap<String, (f64, f64)>) {
    let mut obj = serde_json::Map::new();
    for (action, (count, sum)) in map {
        obj.insert(
            action.clone(),
            serde_json::json!({ "count": count, "sumMs": sum }),
        );
    }
    if let Err(e) = fsutil::atomic_write(
        paths::stats_file().as_path(),
        serde_json::to_string(&serde_json::Value::Object(obj))
            .unwrap_or_default()
            .as_bytes(),
    ) {
        logger::warn(format!("Could not write stats: {e}"));
    }
}

/// Moyenne en secondes d'une action (arrondie, au moins 1 s), `None` si
/// aucune donnée exploitable.
pub fn avg_seconds(action: &str) -> Option<u64> {
    let (count, sum_ms) = read_all().get(action).copied()?;
    if count <= 0.0 || sum_ms <= 0.0 {
        return None;
    }
    Some(((sum_ms / count / 1000.0).round() as u64).max(1))
}

/// Enregistre la durée d'une action terminée.
pub fn record(action: &str, elapsed_ms: u64) {
    let mut map = read_all();
    let entry = map.entry(action.to_string()).or_insert((0.0, 0.0));
    entry.0 += 1.0;
    entry.1 += elapsed_ms as f64;
    write_all(&map);
}
