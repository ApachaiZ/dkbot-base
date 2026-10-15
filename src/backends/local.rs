//! Backend LOCAL : exécution de commandes sur la machine du bot.
//!
//! Toute commande est construite à partir d'éléments VALIDÉS (préfixe
//! systemctl filtré à la config, unités systemd strictement validées,
//! actions fixes) — aucun métacaractère shell ne peut y entrer.

use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::errors::{BackendError, BackendErrorKind};

/// Exécute une commande localement avec timeout. Retourne
/// `(code_sortie, stdout, stderr)`.
///
/// Classification des échecs (garantie « une seule commande power ») :
/// - spawn impossible → `Network` (rien n'est parti) ;
/// - timeout → `Timeout` + `sent = true` (la commande a PU être appliquée) ;
/// - exit non nul → `Rejected` (la commande a été reçue et REFUSÉE : rien
///   ne sera appliqué, pas de suivi nécessaire).
pub async fn run_local(
    program: &str,
    args: &[&str],
    stdin_data: Option<&str>,
    timeout: Duration,
) -> Result<(i32, String, String), BackendError> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .env("LANG", "C")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| {
        BackendError::new(
            BackendErrorKind::Network,
            false,
            format!("spawn {program}: {e}"),
        )
    })?;

    if let Some(data) = stdin_data {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(data.as_bytes()).await;
        }
    }
    // Fermer stdin : `sh -s` ne se termine jamais sinon.
    drop(child.stdin.take());

    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Err(BackendError::new(
                BackendErrorKind::Other,
                true,
                format!("wait {program}: {e}"),
            ))
        }
        Err(_) => {
            // Le kill_on_drop a tué l'enfant ; la commande a PU s'exécuter.
            return Err(BackendError::new(
                BackendErrorKind::Timeout,
                true,
                format!("command {program} timed out after {}s", timeout.as_secs()),
            ));
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let code = output.status.code().unwrap_or(-1);
    if code != 0 {
        return Err(BackendError::new(
            BackendErrorKind::Rejected,
            false,
            if stderr.is_empty() {
                format!("exit code {code}")
            } else {
                stderr
            },
        ));
    }
    Ok((code, stdout, stderr))
}

/// Exécute le script de collecte localement et parse le résultat.
/// L'unité systemd passe par la variable d'environnement `SERVICE` (lue par
/// le script) — elle est VALIDÉE par la config, aucune injection possible.
pub async fn collect_local(
    service: &str,
    timeout: Duration,
) -> Result<crate::status::Status, BackendError> {
    use tokio::io::AsyncWriteExt;
    let mut cmd = Command::new("sh");
    cmd.arg("-s")
        .env("LANG", "C")
        .env("SERVICE", service)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| {
        BackendError::new(BackendErrorKind::Network, false, format!("spawn sh: {e}"))
    })?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin
            .write_all(super::script::COLLECT_SCRIPT.as_bytes())
            .await;
    }
    drop(child.stdin.take());
    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Err(BackendError::new(
                BackendErrorKind::Other,
                true,
                format!("wait sh: {e}"),
            ))
        }
        Err(_) => {
            return Err(BackendError::new(
                BackendErrorKind::Timeout,
                true,
                format!("collect timed out after {}s", timeout.as_secs()),
            ))
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let code = output.status.code().unwrap_or(-1);
    if code != 0 {
        return Err(BackendError::new(
            BackendErrorKind::Rejected,
            false,
            if stderr.is_empty() {
                format!("exit code {code}")
            } else {
                stderr
            },
        ));
    }
    Ok(super::script::parse_collect_output(&stdout))
}

/// Commande systemctl locale (préfixe optionnel pour root).
pub async fn systemctl_local(
    prefix: &str,
    action: &str,
    unit: Option<&str>,
    timeout: Duration,
) -> Result<(), BackendError> {
    let mut parts: Vec<&str> = prefix.split_whitespace().collect();
    parts.push("systemctl");
    parts.push(action);
    if let Some(u) = unit {
        parts.push(u);
    }
    let (program, args) = parts
        .split_first()
        .map(|(p, rest)| (*p, rest.to_vec()))
        .unwrap_or(("systemctl", Vec::new()));
    run_local(program, &args, None, timeout).await.map(|_| ())
}

/// `true` local (mesure de latence) et journalctl/tail.
pub async fn ping_local(timeout: Duration) -> Result<(), BackendError> {
    run_local("true", &[], None, timeout).await.map(|_| ())
}

/// Exécute `program` dans un GROUPE de process dédié : au timeout, le
/// groupe ENTIER est tué (SIGKILL) — aucun petit-fils ne survit. Utilisé
/// par la copie des sauvegardes : un `tar` ne doit JAMAIS continuer
/// d'écrire pendant que le jeu redémarre (kill_on_drop seul ne tue que
/// le `sh`).
async fn run_local_group(
    program: &str,
    args: &[&str],
    stdin_data: Option<&str>,
    timeout: Duration,
) -> Result<(i32, String, String), BackendError> {
    use tokio::io::AsyncWriteExt;

    let mut cmd = Command::new(program);
    cmd.args(args)
        .env("LANG", "C")
        .process_group(0)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| {
        BackendError::new(
            BackendErrorKind::Network,
            false,
            format!("spawn {program}: {e}"),
        )
    })?;
    if let Some(data) = stdin_data {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(data.as_bytes()).await;
        }
    }
    // Fermer stdin : `sh -s` ne se termine jamais sinon.
    drop(child.stdin.take());

    // PID capturé AVANT le move de `child` dans wait_with_output.
    let pid = child.id().unwrap_or(0);
    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Err(BackendError::new(
                BackendErrorKind::Other,
                true,
                format!("wait {program}: {e}"),
            ))
        }
        Err(_) => {
            // Timeout : tuer le GROUPE entier (SIGKILL sur -pgid).
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
            return Err(BackendError::new(
                BackendErrorKind::Timeout,
                true,
                format!("command {program} timed out after {}s", timeout.as_secs()),
            ));
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let code = output.status.code().unwrap_or(-1);
    if code != 0 {
        return Err(BackendError::new(
            BackendErrorKind::Rejected,
            false,
            if stderr.is_empty() {
                format!("exit code {code}")
            } else {
                stderr
            },
        ));
    }
    Ok((code, stdout, stderr))
}

/// Copie des sauvegardes via le script embarqué. Le chemin des saves et
/// l'unité passent en ARGV positionnel (`sh -s -- <saves> <unité>` avec le
/// script sur stdin) — jamais interpolés dans une ligne de commande :
/// aucun caractère (espaces, quotes) ne peut être interprété par un shell.
/// Le timeout tue le GROUPE de process entier (voir [`run_local_group`]) —
/// les résidus `.tmp` d'un SIGKILL sont purgés par la copie réussie
/// suivante.
pub async fn copy_saves_local(
    unit: &str,
    saves: &str,
    timeout: Duration,
) -> Result<super::CopyOutcome, BackendError> {
    let (_, out, _) = run_local_group(
        "sh",
        &["-s", "--", saves, unit],
        Some(super::script::COPY_SCRIPT),
        timeout,
    )
    .await?;
    super::script::parse_copy_output(&out).ok_or_else(|| {
        BackendError::new(
            BackendErrorKind::Other,
            true,
            "copy script returned unreadable output".to_string(),
        )
    })
}

/// Liste des archives de sauvegarde (module `backups`) : sortie brute de
/// `ls -la` du dossier `-backups`, ou `NOBACKUPS`.
pub async fn backups_list_local(
    unit: &str,
    saves: &str,
    timeout: Duration,
) -> Result<String, BackendError> {
    let (code, out, err) = run_local(
        "sh",
        &["-s", "--", saves, unit],
        Some(super::script::LIST_SCRIPT),
        timeout,
    )
    .await?;
    if code != 0 {
        return Err(BackendError::new(
            BackendErrorKind::Other,
            false,
            format!("list backups: {}", err.trim()),
        ));
    }
    Ok(out)
}

/// Restaure une archive (module `backups`) : le script porte ses gardes
/// (service arrêté, nom d'archive strict, copie de sûreté préalable).
pub async fn backups_restore_local(
    unit: &str,
    saves: &str,
    archive: &str,
    timeout: Duration,
) -> Result<String, BackendError> {
    let (code, out, err) = run_local(
        "sh",
        &["-s", "--", saves, unit, archive],
        Some(super::script::RESTORE_SCRIPT),
        timeout,
    )
    .await?;
    match super::script::parse_restore_output(&out) {
        Some(Ok(dest)) => Ok(dest),
        Some(Err(msg)) => Err(BackendError::new(BackendErrorKind::Other, false, msg)),
        None => Err(BackendError::new(
            BackendErrorKind::Other,
            code != 0,
            format!("restore: {}", err.trim()),
        )),
    }
}

/// Diagnostic en lecture seule (module `exec`) — arguments déjà validés
/// côté Rust (`builtin`, unité, lignes bornées).
pub async fn exec_diagnostic_local(
    args: &[String],
    timeout: Duration,
) -> Result<String, BackendError> {
    let mut argv: Vec<&str> = vec!["-s", "--"];
    argv.extend(args.iter().map(|a| a.as_str()));
    let (code, out, err) =
        run_local("sh", &argv, Some(super::script::EXEC_SCRIPT), timeout).await?;
    if code != 0 {
        return Err(BackendError::new(
            BackendErrorKind::Other,
            false,
            format!("exec: {}", err.trim()),
        ));
    }
    Ok(out)
}

/// Lignes de log du service : journald (`journalctl`) ou fichier (`tail`).
pub async fn logs_local(
    prefix: &str,
    unit: &str,
    lines: u32,
    file: Option<&str>,
    timeout: Duration,
) -> Result<String, BackendError> {
    let lines = lines.to_string();
    let mut parts: Vec<&str> = prefix.split_whitespace().collect();
    match file {
        Some(path) => parts.extend(["tail", "-n", lines.as_str(), path]),
        None => parts.extend([
            "journalctl",
            "-u",
            unit,
            "-n",
            lines.as_str(),
            "--no-pager",
            "-o",
            "cat",
        ]),
    };
    let (program, args) = parts
        .split_first()
        .map(|(p, rest)| (*p, rest.to_vec()))
        .unwrap_or(("journalctl", Vec::new()));
    let (_, out, _) = run_local(program, &args, None, timeout).await?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::CopyOutcome;

    /// Exécute le VRAI script embarqué via le backend local (aucun réseau,
    /// tout dans un dossier temporaire). L'unité est un nom impossible :
    /// le script la traite comme arrêtée et archive — le test vérifie la
    /// mécanique complète (argv, garde, `tar`, parse de la sortie).
    #[tokio::test]
    async fn copy_saves_local_copies_into_timestamped_backup() {
        let base = std::env::temp_dir().join(format!("dkbot-saves-test-{}", std::process::id()));
        let saves = base.join("saves");
        std::fs::create_dir_all(saves.join("world")).unwrap();
        std::fs::write(saves.join("world/save.sav"), "data").unwrap();

        let unit = format!("dkbot-no-such-unit-{}", std::process::id());
        let outcome =
            copy_saves_local(&unit, saves.to_str().unwrap(), Duration::from_secs(60)).await;
        let dest = match outcome {
            Ok(CopyOutcome::Copied { dest }) => dest,
            other => panic!("expected Copied, got {other:?}"),
        };
        // `<saves>-backups/<horodatage>.tar` — une archive tar contenant
        // le dossier des saves à sa racine.
        assert!(dest.ends_with(".tar"), "dest = {dest}");
        let (_, listing, _) = run_local(
            "tar",
            &["-tf", dest.as_str()],
            None,
            Duration::from_secs(30),
        )
        .await
        .unwrap();
        // L'archive contient le dossier des saves (entrées + fichiers).
        assert!(
            listing.lines().any(|l| l == "saves/world/save.sav"),
            "listing = {listing}"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// Le timeout tue le GROUPE de process entier : un petit-fils
    /// (`sleep`) ne survit JAMAIS au kill — c'est la garantie « pas de
    /// copie orpheline » de la sauvegarde.
    #[tokio::test]
    async fn timeout_kills_the_whole_process_group() {
        let outcome =
            run_local_group("sh", &["-c", "sleep 61"], None, Duration::from_secs(1)).await;
        assert!(
            matches!(&outcome, Err(e) if e.kind == BackendErrorKind::Timeout),
            "outcome = {outcome:?}"
        );
        // Aucun orphelin : `pgrep` ne doit rien trouver. (Si pgrep est
        // absent de l'environnement, l'assertion est sans objet.) Une
        // petite retraite tolère un zombie qui traînerait quelques ms.
        let mut orphan = true;
        for _ in 0..5 {
            if let Ok(out) = std::process::Command::new("pgrep")
                .args(["-f", "sleep 61"])
                .output()
            {
                orphan = out.status.success();
                if !orphan {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
        assert!(!orphan, "un processus `sleep 61` survit au kill de groupe");
    }
}
