//! Script de collecte EMBARQUÉ + parseur.
//!
//! Le même script POSIX est exécuté sur la machine cible — localement
//! (`sh -s` avec le script sur stdin) ou à distance (`ssh user@host sh`
//! avec le script sur stdin). Un seul format de sortie (`KEY=value`), un
//! seul parseur : les deux backends sont garantis de rapporter le même
//! vocabulaire.
//!
//! Le script ne prend AUCUN argument : l'unité systemd ciblée passe par la
//! variable d'environnement `SERVICE` (validée par l'appelant).

use crate::status::{map_state, Status};

use crate::backends::CopyOutcome;

/// Script POSIX embarqué. Sortie : lignes `KEY=value`, valeurs vides
/// autorisées. L'usage CPU est mesuré sur ~0,5 s (deux lectures de
/// `/proc/stat`).
pub const COLLECT_SCRIPT: &str = r##"#!/bin/sh
# Collecte DakkaBot — exécuté sur la machine cible.
SVC="${SERVICE:-}"

echo "hostname=$(hostname 2>/dev/null)"
echo "os=$(sed -n 's/^PRETTY_NAME="*\([^"]*\)"*$/\1/p' /etc/os-release 2>/dev/null | head -n1)"
echo "kernel=$(uname -r 2>/dev/null)"
UP=$(awk '{print $1}' /proc/uptime 2>/dev/null)
echo "uptime_seconds=$UP"
echo "load1=$(awk '{print $1}' /proc/loadavg 2>/dev/null)"
echo "load5=$(awk '{print $2}' /proc/loadavg 2>/dev/null)"
echo "load15=$(awk '{print $3}' /proc/loadavg 2>/dev/null)"
echo "cpu_model=$(awk -F: '/^model name/{sub(/^ /,"",$2); print $2; exit}' /proc/cpuinfo 2>/dev/null)"
echo "cpu_count=$(grep -c '^processor' /proc/cpuinfo 2>/dev/null)"

# Usage CPU sur ~0.5 s : deux lectures de /proc/stat.
read_cpu() {
  awk '/^cpu /{print $5+$6" "$2+$3+$4+$5+$6+$7+$8+$9}' /proc/stat 2>/dev/null
}
C1=$(read_cpu)
sleep 0.5
C2=$(read_cpu)
I1=${C1%% *}; T1=${C1##* }
I2=${C2%% *}; T2=${C2##* }
CPU_PCT=""
if [ -n "$T1" ] && [ -n "$T2" ] && [ "$T1" != "$T2" ]; then
  CPU_PCT=$(awk -v i1="$I1" -v i2="$I2" -v t1="$T1" -v t2="$T2" \
    'BEGIN{printf "%.1f", ((t2-i2)-(t1-i1))/(t2-t1)*100}')
fi
echo "cpu_pct=$CPU_PCT"

MEMTOT=$(awk '/^MemTotal:/{print $2}' /proc/meminfo 2>/dev/null)
MEMAVAIL=$(awk '/^MemAvailable:/{print $2}' /proc/meminfo 2>/dev/null)
if [ -n "$MEMTOT" ] && [ -n "$MEMAVAIL" ]; then
  echo "ram_used_mb=$(awk -v t="$MEMTOT" -v a="$MEMAVAIL" 'BEGIN{printf "%.0f", (t-a)/1024}')"
  echo "ram_max_mb=$(awk -v t="$MEMTOT" 'BEGIN{printf "%.0f", t/1024}')"
else
  echo "ram_used_mb="
  echo "ram_max_mb="
fi

DISK=$(df -Pk / 2>/dev/null | awk 'NR==2{print $3" "$2}')
if [ -n "$DISK" ]; then
  DU=${DISK%% *}; DT=${DISK##* }
  echo "disk_used_mb=$(awk -v v="$DU" 'BEGIN{printf "%.0f", v/1024}')"
  echo "disk_max_mb=$(awk -v v="$DT" 'BEGIN{printf "%.0f", v/1024}')"
else
  echo "disk_used_mb="
  echo "disk_max_mb="
fi

# État du service applicatif (systemd).
SVC_RAW=""
SVC_ENTER=""
if [ -n "$SVC" ]; then
  if command -v systemctl >/dev/null 2>&1; then
    SVC_RAW=$(systemctl show -p ActiveState --value "$SVC" 2>/dev/null)
    SVC_ENTER=$(systemctl show -p ActiveEnterTimestampMonotonic --value "$SVC" 2>/dev/null)
  else
    SVC_RAW="nosystemd"
  fi
fi
if [ -z "$SVC_RAW" ]; then SVC_RAW="unknown"; fi
echo "svc_state=$SVC_RAW"
echo "svc_enter=$SVC_ENTER"
"##;

/// Script POSIX de COPIE DES SAUVEGARDES, exécuté sur la machine cible.
///
/// Arguments positionnels : `$1` = répertoire des sauvegardes (chemin
/// absolu, validé à la config), `$2` = unité systemd du jeu, `$3` = action
/// (`start`|`stop`|`restart`, suffixe du nom de l'instantané). Le chemin ne
/// passe JAMAIS par une ligne de commande shell : localement il est un
/// ARGV direct, en SSH il est quoté via `sh_quote`.
///
/// Politique :
/// - garde « arrêt avéré » : copie UNIQUEMENT si l'unité est `inactive` ou
///   `failed` (`activating`/`deactivating` refusés), et l'archive est
///   SUPPRIMÉE si l'unité redevient active pendant la création ;
/// - lock de sauvegarde (mkdir atomique, stale après 15 min) : jamais deux
///   copies simultanées sur le même dossier ;
/// - anti-rafale : pas de copie si une copie réussie date de moins d'1 h ;
/// - conservation : les 20 plus récentes seulement (les noms d'archive
///   sont l'HORODATAGE `<AAAAMMJJ-HHMMSS>.tar`, triable par ordre
///   lexicographique), les plus anciennes sont purgées à chaque copie
///   réussie.
///
/// Sortie : `RUNNING` (service actif), `LOCKED` (copie concurrente),
/// `FRESH` (copie récente), ou `COPIED <destination>`.
pub const COPY_SCRIPT: &str = r##"#!/bin/sh
# Copie des sauvegardes DakkaBot — exécuté sur la machine cible.
SAVES=${1%/}
UNIT=${2:-}
if [ -z "$SAVES" ]; then
  echo "missing saves directory argument" >&2
  exit 2
fi
is_stopped() {
  # Arrêt avéré : unité inactive ou en échec. « activating »/
  # « deactivating » (le jeu peut encore écrire) sont refusés.
  if command -v systemctl >/dev/null 2>&1; then
    state=$(systemctl show -p ActiveState --value "$UNIT" 2>/dev/null)
    [ -z "$state" ] || [ "$state" = "inactive" ] || [ "$state" = "failed" ]
  else
    return 0
  fi
}
is_stopped || { echo "RUNNING"; exit 0; }
if [ ! -d "$SAVES" ] || ! ls "$SAVES" >/dev/null 2>&1; then
  # Deux causes fréquentes, mêlées par le même test : dossier absent,
  # ou présent mais inaccessible (droits de traversée d'un parent).
  echo "cannot access saves directory: $SAVES (absent ou permissions insuffisantes)" >&2
  exit 1
fi
BASE="${SAVES}-backups"
mkdir -p "$BASE" || { echo "cannot create $BASE" >&2; exit 1; }

# ── Lock de sauvegarde (mkdir atomique) ──
LOCK="${BASE}/.backup.lock"
LOCKED=0
TMP=""
# Nettoyage sur TOUTE sortie, y compris FRESH/LOCKED/HUP (coupure du canal
# SSH) : le lock acquis ici est TOUJOURS rendu. Seul SIGKILL échappe au
# trap — ses résidus (.tmp orphelins) sont purgés à la copie réussie
# suivante.
trap 'rm -rf "$TMP"; if [ "$LOCKED" = 1 ]; then rmdir "$LOCK" 2>/dev/null; fi' EXIT INT TERM HUP
if ! mkdir "$LOCK" 2>/dev/null; then
  # Lock existant : stale au-delà de 15 min (aucune copie ne dure plus).
  # NB : `stat -c %Y` suppose GNU coreutils (ou busybox) — Linux avec
  # systemd, prérequis déjà implicite du bot.
  LOCK_AGE=$(stat -c %Y "$LOCK" 2>/dev/null || echo 0)
  NOW=$(date +%s 2>/dev/null || echo 0)
  if [ "$NOW" -gt "$((LOCK_AGE + 900))" ]; then
    rm -rf "$LOCK"
    if ! mkdir "$LOCK" 2>/dev/null; then echo "LOCKED"; exit 0; fi
    LOCKED=1
  else
    echo "LOCKED"
    exit 0
  fi
else
  LOCKED=1
fi

# ── Anti-rafale : pas de copie si une copie réussie date de moins d'1 h ──
MARK="${BASE}/.last-backup"
NOW=$(date +%s 2>/dev/null || echo 0)
if [ -f "$MARK" ]; then
  THEN=$(stat -c %Y "$MARK" 2>/dev/null || echo 0)
  if [ "$NOW" -lt "$((THEN + 3600))" ]; then
    echo "FRESH"
    exit 0
  fi
fi

# ── Archive atomique : nom temporaire, renommé une fois terminé ──
# L'archive contient le DOSSIER des saves à sa racine (ex. saves/world/…).
# Restauration : tar -xf <monde>.sav.<date>.<action>.tar dans le dossier parent.
STAMP=$(date -u +%Y%m%d-%H%M%S 2>/dev/null || echo unknown)
DEST="${BASE}/${STAMP}.tar"
TMP="${DEST}.tmp"
if ! tar -cf "$TMP" -C "$(dirname "$SAVES")" "$(basename "$SAVES")" 2>/dev/null; then
  echo "archive failed (tar) : $SAVES" >&2
  exit 1
fi
if is_stopped; then
  # Jamais d'écrasement silencieux (collision de nom improbable : l'anti-
  # rafale d'1 h l'exclut, mais la ceinture reste).
  if [ -e "$DEST" ]; then
    echo "backup name collision: $DEST" >&2
    exit 1
  fi
  mv "$TMP" "$DEST" || { echo "cannot finalize $DEST" >&2; exit 1; }
  touch "$MARK" || true
  # Résidus .tmp de copies tuées par SIGKILL (intrappable) : sous le
  # lock, tout .tmp présent est forcément orphelin (aucune autre copie
  # ne tourne) → suppression inconditionnelle.
  ls -1d "${BASE}"/*.tmp 2>/dev/null | while IFS= read -r old; do
    rm -rf "$old"
  done
  # Purge : conserver les 20 archives les plus récentes (horodatage
  # triable par nom).
  ls -1d "${BASE}"/*.tar 2>/dev/null | sort | tail -n +21 | while IFS= read -r old; do
    rm -rf "$old"
  done
  echo "COPIED ${DEST}"
else
  # L'unité a repris vie pendant l'archivage : snapshot incohérent.
  rm -rf "$TMP"
  echo "RUNNING"
fi
"##;

/// Script POSIX embarqué : LISTE des archives de sauvegarde d'une cible
/// (`sh -s -- <saves> <unité>`). Sortie : liste `ls -la` brute, ou
/// `NOBACKUPS` si aucune archive n'existe.
pub const LIST_SCRIPT: &str = r##"#!/bin/sh
SAVES=${1%/}
BASE="${SAVES}-backups"
if [ ! -d "$BASE" ]; then
  echo "NOBACKUPS"
  exit 0
fi
ls -la "$BASE" | sed '1d'
"##;

/// Script POSIX embarqué : RESTAURATION d'une archive (`sh -s -- <saves>
/// <unité> <archive>`). Gardes mécaniques : service arrêté (sinon refus),
/// nom d'archive STRICTEMENT validé (`.tar`, sans `/` — pas de traversée
/// de chemin), copie de sûreté de l'état actuel AVANT restauration.
/// Sortie : `RESTORED <archive>` ou erreur sur stderr (exit 1).
pub const RESTORE_SCRIPT: &str = r##"#!/bin/sh
SAVES=${1%/}
UNIT=${2:-}
ARCHIVE=${3:-}
BASE="${SAVES}-backups"
is_stopped() {
  if command -v systemctl >/dev/null 2>&1; then
    state=$(systemctl show -p ActiveState --value "$UNIT" 2>/dev/null)
    [ -z "$state" ] || [ "$state" = "inactive" ] || [ "$state" = "failed" ]
  else
    return 0
  fi
}
is_stopped || { echo "RUNNING"; exit 0; }
case "$ARCHIVE" in
  ""|*/*) echo "invalid archive name: $ARCHIVE" >&2; exit 1 ;;
  *.tar) ;;
  *) echo "invalid archive name: $ARCHIVE" >&2; exit 1 ;;
esac
[ -f "$BASE/$ARCHIVE" ] || { echo "archive not found: $ARCHIVE" >&2; exit 1; }
PARENT=$(dirname "$SAVES")
NAME=$(basename "$SAVES")
# Copie de sûreté de l'état actuel AVANT restauration (si non vide).
if [ -n "$(ls -A "$SAVES" 2>/dev/null)" ]; then
  tar -cf "$BASE/pre-restore-$(date -u +%Y%m%d-%H%M%S).tar" -C "$PARENT" "$NAME" 2>/dev/null || true
fi
tar -xf "$BASE/$ARCHIVE" -C "$PARENT" || { echo "restore failed" >&2; exit 1; }
echo "RESTORED $ARCHIVE"
"##;

/// Parse la sortie du script de restauration : `Ok(dest)` pour `RESTORED`,
/// `Some("running")` si le service n'était pas arrêté, `None` sinon
/// (l'erreur exacte est portée par stderr).
pub fn parse_restore_output(out: &str) -> Option<Result<String, String>> {
    let out = out.trim();
    if out == "RUNNING" {
        return Some(Err("service non arrêté au moment de la restauration".into()));
    }
    if let Some(dest) = out.strip_prefix("RESTORED ") {
        let dest = dest.trim();
        if !dest.is_empty() {
            return Some(Ok(dest.to_string()));
        }
    }
    None
}

/// Script POSIX embarqué : diagnostics EN LECTURE SEULE (module `exec`).
/// `sh -s -- <builtin> [<unité>] [<lignes>]`. Les valeurs sont VALIDÉES
/// côté Rust avant d'atteindre ce script (builtin ∈ liste blanche, unité
/// `[A-Za-z0-9_.@:\-]`, lignes 1-500) — aucune entrée libre n'arrive ici.
pub const EXEC_SCRIPT: &str = r##"#!/bin/sh
CMD=${1:-}
UNIT=${2:-}
LINES=${3:-50}
case "$CMD" in
  journal)
    if command -v journalctl >/dev/null 2>&1; then
      journalctl -u "$UNIT" -n "$LINES" --no-pager -o cat 2>&1
    else
      echo "journalctl absent de cette machine" >&2
      exit 1
    fi
    ;;
  ss) ss -lntp 2>&1 ;;
  df) df -h 2>&1 ;;
  ip) ip -brief address 2>&1 ;;
  uptime) uptime 2>&1 ;;
  who) who 2>&1 ;;
  *) echo "unknown builtin: $CMD" >&2; exit 1 ;;
esac
"##;

fn num(map: &std::collections::HashMap<&str, &str>, key: &str) -> Option<f64> {
    map.get(key).and_then(|v| v.trim().parse::<f64>().ok())
}

/// Parse la sortie du script de collecte vers l'état canonique.
///
/// L'uptime du service est dérivé de `ActiveEnterTimestampMonotonic`
/// (microsecondes depuis le boot) soustrait à l'uptime système : les deux
/// partagent la même référence de boot (hors suspension, négligeable sur
/// un VPS). Une valeur incohérente est bornée à zéro.
pub fn parse_collect_output(out: &str) -> Status {
    let mut map: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for line in out.lines() {
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim(), v);
        }
    }

    let raw_state = map.get("svc_state").copied().unwrap_or("unknown");
    let state = map_state(raw_state);

    let uptime_seconds = num(&map, "uptime_seconds");
    let service_uptime_seconds = match (num(&map, "svc_enter"), uptime_seconds) {
        (Some(enter_us), Some(uptime)) => Some((uptime - enter_us / 1e6).max(0.0)),
        _ => None,
    };

    Status {
        name: map
            .get("hostname")
            .map(|v| v.trim().to_string())
            .filter(|s| !s.is_empty()),
        state,
        raw_state: raw_state.to_string(),
        service_uptime_seconds,
        cpu_pct: num(&map, "cpu_pct"),
        cpu_model: map
            .get("cpu_model")
            .map(|v| v.trim().to_string())
            .filter(|s| !s.is_empty()),
        cpu_count: num(&map, "cpu_count"),
        load1: num(&map, "load1"),
        load5: num(&map, "load5"),
        load15: num(&map, "load15"),
        ram_mb: num(&map, "ram_used_mb"),
        ram_max_mb: num(&map, "ram_max_mb"),
        disk_mb: num(&map, "disk_used_mb"),
        disk_max_mb: num(&map, "disk_max_mb"),
        uptime_seconds,
        os: map
            .get("os")
            .map(|v| v.trim().to_string())
            .filter(|s| !s.is_empty()),
        kernel: map
            .get("kernel")
            .map(|v| v.trim().to_string())
            .filter(|s| !s.is_empty()),
        address: None,
        backend: None,
    }
}

/// Parse la sortie du script de copie : `RUNNING`, `LOCKED`, `FRESH` ou
/// `COPIED <chemin>`.
pub fn parse_copy_output(out: &str) -> Option<CopyOutcome> {
    let out = out.trim();
    match out {
        "RUNNING" => return Some(CopyOutcome::Running),
        "LOCKED" => return Some(CopyOutcome::Locked),
        "FRESH" => return Some(CopyOutcome::Fresh),
        _ => {}
    }
    if let Some(dest) = out.strip_prefix("COPIED ") {
        let dest = dest.trim();
        if !dest.is_empty() {
            return Some(CopyOutcome::Copied {
                dest: dest.to_string(),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_output_parsing() {
        assert_eq!(
            parse_restore_output("RESTORED 20261013-101501.tar"),
            Some(Ok("20261013-101501.tar".to_string()))
        );
        assert_eq!(
            parse_restore_output("RUNNING"),
            Some(Err(
                "service non arrêté au moment de la restauration".to_string()
            ))
        );
        assert_eq!(parse_restore_output("garbage"), None);
    }

    const SAMPLE: &str = "\
hostname=vps-1
os=Ubuntu 24.04.1 LTS
kernel=6.8.0-49-generic
uptime_seconds=3600
load1=0.42
load5=0.31
load15=0.28
cpu_model=Intel(R) Xeon(R) CPU E5-2680 v4 @ 2.40GHz
cpu_count=4
cpu_pct=12.5
ram_used_mb=2048
ram_max_mb=8192
disk_used_mb=51200
disk_max_mb=102400
svc_state=active
svc_enter=1800000000
";

    #[test]
    fn parses_full_sample() {
        let s = parse_collect_output(SAMPLE);
        assert_eq!(s.name.as_deref(), Some("vps-1"));
        assert_eq!(s.state, "running");
        assert_eq!(s.raw_state, "active");
        assert_eq!(s.cpu_pct, Some(12.5));
        assert_eq!(s.cpu_count, Some(4.0));
        assert_eq!(s.ram_mb, Some(2048.0));
        assert_eq!(s.ram_max_mb, Some(8192.0));
        assert_eq!(s.disk_max_mb, Some(102400.0));
        assert_eq!(s.uptime_seconds, Some(3600.0));
        assert_eq!(s.os.as_deref(), Some("Ubuntu 24.04.1 LTS"));
        // 1800 s après le boot : le service tourne depuis 1800 s.
        assert_eq!(s.service_uptime_seconds, Some(1800.0));
    }

    #[test]
    fn maps_systemd_states() {
        for (raw, expected) in [
            ("active", "running"),
            ("inactive", "stopped"),
            ("failed", "failed"),
            ("activating", "starting"),
            ("deactivating", "stopping"),
            ("nosystemd", "unknown"),
            ("", "unknown"),
            ("weird", "weird"),
        ] {
            assert_eq!(map_state(raw), expected, "raw={raw}");
        }
    }

    #[test]
    fn tolerates_empty_output() {
        let s = parse_collect_output("");
        assert_eq!(s.state, "unknown");
        assert_eq!(s.name, None);
        assert_eq!(s.cpu_pct, None);
    }

    #[test]
    fn clamps_negative_service_uptime() {
        let out = "uptime_seconds=100\nsvc_state=active\nsvc_enter=500000000\n";
        let s = parse_collect_output(out);
        assert_eq!(s.service_uptime_seconds, Some(0.0));
    }

    #[test]
    fn parses_copy_outcomes() {
        assert!(matches!(
            parse_copy_output("RUNNING"),
            Some(CopyOutcome::Running)
        ));
        assert!(matches!(
            parse_copy_output("LOCKED"),
            Some(CopyOutcome::Locked)
        ));
        assert!(matches!(
            parse_copy_output("FRESH"),
            Some(CopyOutcome::Fresh)
        ));
        assert_eq!(
            parse_copy_output("COPIED /opt/game/saves-backups/20261013-1015.tar"),
            Some(CopyOutcome::Copied {
                dest: "/opt/game/saves-backups/20261013-1015.tar".into()
            })
        );
        assert_eq!(parse_copy_output("COPIED "), None);
        assert_eq!(parse_copy_output("garbage"), None);
        assert_eq!(parse_copy_output(""), None);
    }
}
