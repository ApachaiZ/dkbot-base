//! Backend SSH : exécution de commandes sur une machine distante.
//!
//! Transport : russh (client SSH pur Rust, asynchrone) — aucun binaire
//! externe, aucune dépendance C, cohérent avec le reste du crate.
//!
//! Sécurité :
//! - les clés d'hôte sont VÉRIFIÉES contre un fichier known_hosts maison
//!   (TOFU : la première clé vue est enregistrée, une clé DIFFÉRENTE
//!   ensuite refuse la connexion — protection contre le MITM) ;
//! - l'authentification essaie dans l'ordre : clé privée (si configurée),
//!   puis mot de passe (si configuré) ;
//! - chaque commande est construite à partir d'éléments VALIDÉS (voir la
//!   config et `targets.rs`) — aucun métacaractère shell.
//!
//! Classification des erreurs (garantie « une seule commande power ») :
//! - avant `channel.exec` : rien n'est parti (`sent = false`) ;
//! - après : toute erreur est `sent = true` (la commande a PU s'exécuter).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use russh::keys::{load_secret_key, HashAlg, PrivateKeyWithHashAlg};
use russh::{client, ChannelMsg, Disconnect};

use crate::errors::{BackendError, BackendErrorKind};
use crate::logger;

/// Paramètres de connexion d'une cible SSH (clonables, sans état vivant).
#[derive(Debug, Clone)]
pub struct SshConn {
    pub host: String,
    pub port: u16,
    pub user: String,
    password: Option<String>,
    key_path: Option<String>,
    known_hosts: PathBuf,
    timeout: Duration,
}

impl SshConn {
    pub fn new(
        host: String,
        port: u16,
        user: String,
        password: Option<String>,
        key_path: Option<String>,
        known_hosts: PathBuf,
        timeout: Duration,
    ) -> Self {
        Self {
            host,
            port,
            user,
            password,
            key_path,
            known_hosts,
            timeout,
        }
    }

    /// Timeout de session (utilisé par les gardes de temps extérieures).
    #[allow(dead_code)]
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Ouvre une session SSH authentifiée.
    async fn connect(&self) -> Result<client::Handle<SshHandler>, BackendError> {
        let config = Arc::new(client::Config {
            inactivity_timeout: Some(self.timeout),
            ..Default::default()
        });
        let handler = SshHandler { conn: self.clone() };
        let addr = (self.host.clone(), self.port);
        let mut session = tokio::time::timeout(
            self.timeout + Duration::from_secs(10),
            client::connect(config, addr, handler),
        )
        .await
        .map_err(|_| {
            BackendError::new(
                BackendErrorKind::Timeout,
                false,
                format!("SSH connect to {}:{} timed out", self.host, self.port),
            )
        })?
        .map_err(|e| classify_connect(&e, &self.host))?;

        // Authentification : clé → mot de passe.
        let mut authed = false;
        if let Some(key_path) = &self.key_path {
            if let Ok(key) = load_secret_key(key_path, None) {
                if let Ok(res) = session
                    .authenticate_publickey(
                        &self.user,
                        PrivateKeyWithHashAlg::new(Arc::new(key), None),
                    )
                    .await
                {
                    authed = res.success();
                }
            }
        }
        if !authed {
            if let Some(password) = &self.password {
                let res = session
                    .authenticate_password(&self.user, password)
                    .await
                    .map_err(|e| {
                        BackendError::new(
                            BackendErrorKind::Auth,
                            false,
                            format!("SSH auth error for {}: {e}", self.host),
                        )
                    })?;
                authed = res.success();
            }
        }
        if !authed {
            return Err(BackendError::new(
                BackendErrorKind::Auth,
                false,
                format!(
                    "SSH authentication refused for {}@{}:{} — check SSH_USER, SSH_PASSWORD, SSH_KEY_PATH.",
                    self.user, self.host, self.port
                ),
            ));
        }
        Ok(session)
    }

    /// Exécute une commande et lit la sortie. Retourne
    /// `(code_sortie, stdout, stderr)`.
    async fn exec(
        &self,
        session: &client::Handle<SshHandler>,
        command: &str,
        stdin_data: Option<&str>,
    ) -> Result<(i32, String, String), BackendError> {
        self.exec_with_timeout(session, command, stdin_data, self.timeout)
            .await
    }

    /// Variante d'`exec` avec un délai personnalisé (copie de sauvegardes
    /// potentiellement volumineuses).
    async fn exec_with_timeout(
        &self,
        session: &client::Handle<SshHandler>,
        command: &str,
        stdin_data: Option<&str>,
        timeout: Duration,
    ) -> Result<(i32, String, String), BackendError> {
        let mut channel = session
            .channel_open_session()
            .await
            .map_err(|e| classify_connect(&e, &self.host))?;

        let deadline = timeout + Duration::from_secs(10);
        let outcome = tokio::time::timeout(deadline, async {
            // À partir d'ici : la commande est transmise au serveur.
            channel.exec(true, command).await.map_err(|e| {
                BackendError::new(BackendErrorKind::Other, true, format!("exec: {e}"))
            })?;
            if let Some(data) = stdin_data {
                channel.data(data.as_bytes()).await.map_err(|e| {
                    BackendError::new(BackendErrorKind::Other, true, format!("stdin: {e}"))
                })?;
            }
            channel.eof().await.map_err(|e| {
                BackendError::new(BackendErrorKind::Other, true, format!("eof: {e}"))
            })?;

            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let mut code: Option<u32> = None;
            while let Some(msg) = channel.wait().await {
                match msg {
                    ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
                    ChannelMsg::ExtendedData { data, ext: 1 } => stderr.extend_from_slice(&data),
                    ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
                    ChannelMsg::ExitSignal { .. } if code.is_none() => code = Some(255),
                    _ => {}
                }
            }
            Ok::<_, BackendError>((code, stdout, stderr))
        })
        .await;

        match outcome {
            Ok(Ok((Some(code), stdout, stderr))) => Ok((
                code as i32,
                String::from_utf8_lossy(&stdout).trim().to_string(),
                String::from_utf8_lossy(&stderr).trim().to_string(),
            )),
            Ok(Ok((None, _, _))) => Err(BackendError::new(
                BackendErrorKind::Other,
                true,
                "SSH channel closed without exit status",
            )),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(BackendError::new(
                BackendErrorKind::Timeout,
                true,
                format!("SSH exec on {} timed out", self.host),
            )),
        }
    }

    /// Collecte de statut sur la machine distante.
    pub async fn collect(&self, service: &str) -> Result<crate::status::Status, BackendError> {
        let session = self.connect().await?;
        let command = format!("SERVICE={} sh", sh_quote(service));
        let result = self
            .exec(&session, &command, Some(super::script::COLLECT_SCRIPT))
            .await;
        let _ = session
            .disconnect(Disconnect::ByApplication, "", "done")
            .await;
        let (code, stdout, stderr) = result?;
        if code != 0 {
            return Err(rejected(code, &stdout, &stderr));
        }
        Ok(super::script::parse_collect_output(&stdout))
    }

    /// Action systemctl distante. `prefix` et `unit` sont VALIDÉS en amont
    /// (config + targets.rs) — aucune injection possible.
    pub async fn systemctl(
        &self,
        prefix: &str,
        action: &str,
        unit: &str,
    ) -> Result<(), BackendError> {
        let command = format!("{} systemctl {} {}", prefix.trim(), action, sh_quote(unit));
        self.run_simple(&command).await
    }

    /// Redémarrage de la MACHINE (commande unique sans retry).
    pub async fn reboot(&self, prefix: &str) -> Result<(), BackendError> {
        let command = format!("{} systemctl reboot", prefix.trim());
        self.run_simple(&command).await
    }

    /// Lignes de log du service : journald ou fichier.
    pub async fn logs(
        &self,
        prefix: &str,
        unit: &str,
        lines: u32,
        file: Option<&str>,
    ) -> Result<String, BackendError> {
        let command = match file {
            Some(path) => format!("{} tail -n {} {}", prefix.trim(), lines, sh_quote(path)),
            None => format!(
                "{} journalctl -u {} -n {} --no-pager -o cat",
                prefix.trim(),
                sh_quote(unit),
                lines
            ),
        };
        let session = self.connect().await?;
        let result = self.exec(&session, &command, None).await;
        let _ = session
            .disconnect(Disconnect::ByApplication, "", "done")
            .await;
        let (code, stdout, stderr) = result?;
        if code != 0 {
            return Err(rejected(code, &stdout, &stderr));
        }
        Ok(stdout)
    }

    /// `true` distant (mesure de latence).
    pub async fn ping(&self) -> Result<(), BackendError> {
        self.run_simple("true").await
    }

    /// Copie des sauvegardes : script embarqué sur stdin, chemin des
    /// saves et unité en ARGUMENTS quotés (`sh_quote`) — jamais interpolés
    /// à nu.
    pub async fn copy_saves(
        &self,
        unit: &str,
        saves: &str,
        timeout: Duration,
    ) -> Result<super::CopyOutcome, BackendError> {
        let session = self.connect().await?;
        let command = format!("sh -s -- {} {}", sh_quote(saves), sh_quote(unit));
        let result = self
            .exec_with_timeout(
                &session,
                &command,
                Some(super::script::COPY_SCRIPT),
                timeout,
            )
            .await;
        let _ = session
            .disconnect(Disconnect::ByApplication, "", "done")
            .await;
        let (code, stdout, stderr) = result?;
        if code != 0 {
            return Err(rejected(code, &stdout, &stderr));
        }
        super::script::parse_copy_output(&stdout).ok_or_else(|| {
            BackendError::new(
                BackendErrorKind::Other,
                true,
                "copy script returned unreadable output".to_string(),
            )
        })
    }

    /// Liste des archives de sauvegarde (module `backups`).
    pub async fn backups_list(
        &self,
        unit: &str,
        saves: &str,
        timeout: Duration,
    ) -> Result<String, BackendError> {
        let session = self.connect().await?;
        let command = format!("sh -s -- {} {}", sh_quote(saves), sh_quote(unit));
        let result = self
            .exec_with_timeout(
                &session,
                &command,
                Some(super::script::LIST_SCRIPT),
                timeout,
            )
            .await;
        let _ = session
            .disconnect(Disconnect::ByApplication, "", "done")
            .await;
        let (code, stdout, stderr) = result?;
        if code != 0 {
            return Err(rejected(code, &stdout, &stderr));
        }
        Ok(stdout)
    }

    /// Restaure une archive (module `backups`) : le script porte ses gardes.
    pub async fn backups_restore(
        &self,
        unit: &str,
        saves: &str,
        archive: &str,
        timeout: Duration,
    ) -> Result<String, BackendError> {
        let session = self.connect().await?;
        let command = format!(
            "sh -s -- {} {} {}",
            sh_quote(saves),
            sh_quote(unit),
            sh_quote(archive)
        );
        let result = self
            .exec_with_timeout(
                &session,
                &command,
                Some(super::script::RESTORE_SCRIPT),
                timeout,
            )
            .await;
        let _ = session
            .disconnect(Disconnect::ByApplication, "", "done")
            .await;
        let (code, stdout, stderr) = result?;
        match super::script::parse_restore_output(&stdout) {
            Some(Ok(dest)) => Ok(dest),
            Some(Err(msg)) => Err(BackendError::new(BackendErrorKind::Other, false, msg)),
            None => Err(rejected(code, &stdout, &stderr)),
        }
    }

    /// Diagnostic en lecture seule (module `exec`) — arguments déjà
    /// validés côté Rust.
    pub async fn exec_diagnostic(
        &self,
        args: &[String],
        timeout: Duration,
    ) -> Result<String, BackendError> {
        let session = self.connect().await?;
        let quoted: Vec<String> = args.iter().map(|a| sh_quote(a)).collect();
        let command = format!("sh -s -- {}", quoted.join(" "));
        let result = self
            .exec_with_timeout(
                &session,
                &command,
                Some(super::script::EXEC_SCRIPT),
                timeout,
            )
            .await;
        let _ = session
            .disconnect(Disconnect::ByApplication, "", "done")
            .await;
        let (code, stdout, stderr) = result?;
        if code != 0 {
            return Err(rejected(code, &stdout, &stderr));
        }
        Ok(stdout)
    }

    /// Connexion + commande simple (sortie jetée).
    async fn run_simple(&self, command: &str) -> Result<(), BackendError> {
        let session = self.connect().await?;
        let result = self.exec(&session, command, None).await;
        let _ = session
            .disconnect(Disconnect::ByApplication, "", "done")
            .await;
        let (code, stdout, stderr) = result?;
        if code != 0 {
            return Err(rejected(code, &stdout, &stderr));
        }
        Ok(())
    }
}

/// Handler russh : la vérification de clé d'hôte (TOFU) s'y joue.
#[derive(Clone)]
struct SshHandler {
    conn: SshConn,
}

impl client::Handler for SshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let fingerprint: String = match server_public_key {
            russh::keys::PublicKeyOrCertificate::PublicKey { key, .. } => {
                key.fingerprint(HashAlg::Sha256).to_string()
            }
            russh::keys::PublicKeyOrCertificate::Certificate(cert) => {
                cert.public_key().fingerprint(HashAlg::Sha256).to_string()
            }
        };
        Ok(verify_and_remember_host_key(&self.conn, &fingerprint))
    }
}

/// TOFU : vérifie l'empreinte de la clé d'hôte contre `known_hosts`
/// (format maison : `hôte:port SHA256:…`). Une clé inconnue est
/// enregistrée PUIS la persistance est VÉRIFIÉE (relue) ; une clé
/// DIFFÉRENTE refuse la connexion. TOUT échec d'entrée/sortie du fichier
/// refuse aussi la connexion (fail-closed) : un TOFU qui ne peut pas se
/// souvenir est un TOFU désactivé.
fn verify_and_remember_host_key(conn: &SshConn, fingerprint: &str) -> bool {
    let prefix = format!("{}:{} ", conn.host, conn.port);
    let entry = format!("{prefix}{fingerprint}");

    // Lecture : un fichier ILLISIBLE (permissions, corruption) = refus.
    let content = match std::fs::read_to_string(&conn.known_hosts) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            logger::error(format!(
                "SSH: cannot READ {} — refusing to connect to {}:{} ({e}).",
                conn.known_hosts.display(),
                conn.host,
                conn.port
            ));
            return false;
        }
    };

    // Comparaison EXACTE et EXCLUSIVE : TOUTES les entrées de ce
    // hôte:port doivent porter CE jeton (et il doit y en avoir au moins
    // une). Un fichier contenant DEUX empreintes différentes pour le même
    // hôte = ambiguïté → refus : on ne laisse jamais un attaquant faire
    // persister une seconde clé lors de premières connexions concurrentes.
    let known: Vec<&str> = content.lines().filter(|l| l.starts_with(&prefix)).collect();
    let all_match = |lines: &[&str]| entries_all_match(lines, fingerprint);
    if !known.is_empty() {
        if all_match(&known) {
            return true;
        }
        logger::error(format!(
            "SSH: HOST KEY MISMATCH for {}:{} — possible man-in-the-middle. Refusing to connect. Remove the entry from {} if the host was reinstalled.",
            conn.host,
            conn.port,
            conn.known_hosts.display()
        ));
        return false;
    }

    // Première connexion : TOFU — enregistrer PUIS relire pour vérifier.
    if !append_host_key(conn, &entry) {
        logger::error(format!(
            "SSH: cannot PERSIST the host key of {} to {} — refusing to connect (TOFU would be silently disabled). Check the file permissions or point SSH_KNOWN_HOSTS_FILE somewhere writable.",
            conn.host,
            conn.known_hosts.display()
        ));
        return false;
    }
    let persisted = std::fs::read_to_string(&conn.known_hosts)
        .map(|c| {
            let lines: Vec<&str> = c.lines().filter(|l| l.starts_with(&prefix)).collect();
            all_match(&lines)
        })
        .unwrap_or(false);
    if persisted {
        logger::warn(format!(
            "SSH: new host key for {} saved to {} (first connection — TOFU).",
            conn.host,
            conn.known_hosts.display()
        ));
        return true;
    }
    logger::error(format!(
        "SSH: host key for {} could not be verified after persistence — refusing to connect.",
        conn.host
    ));
    false
}

/// Jeton d'empreinte d'une ligne `hôte:port SHA256:…` : le dernier mot,
/// s'il reste un champ APRÈS l'hôte (une ligne sans empreinte → `None`).
fn entry_fingerprint(line: &str) -> Option<&str> {
    let mut fields = line.split_whitespace();
    fields.next()?; // le couple hôte:port
    fields.last()
}

/// Toutes les entrées d'un hôte portent-elles EXACTEMENT ce jeton (et y
/// en a-t-il au moins une) ? C'est la sémantique de vérification : DEUX
/// empreintes différentes pour le même hôte = refus (ambiguïté).
fn entries_all_match(lines: &[&str], fingerprint: &str) -> bool {
    !lines.is_empty()
        && lines
            .iter()
            .all(|line| entry_fingerprint(line) == Some(fingerprint))
}

/// Ajoute une entrée au fichier known_hosts. Retourne `false` si
/// l'écriture échoue (répertoire non créable, disque plein, permissions…).
fn append_host_key(conn: &SshConn, entry: &str) -> bool {
    if let Some(parent) = conn.known_hosts.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&conn.known_hosts)
        .map(|mut file| {
            use std::io::Write;
            writeln!(file, "{entry}")
        })
        .is_ok()
}

fn is_timeout(msg: &str) -> bool {
    let m = msg.to_lowercase();
    m.contains("timeout") || m.contains("timed out")
}

/// Erreur pendant la connexion/handshake : rien n'est parti.
fn classify_connect(e: &russh::Error, host: &str) -> BackendError {
    let kind = if is_timeout(&e.to_string()) {
        BackendErrorKind::Timeout
    } else {
        BackendErrorKind::Network
    };
    BackendError::new(kind, false, format!("SSH {host}: {e}"))
}

/// Le code de sortie non nul d'une commande distante = « commande refusée ».
pub fn rejected(code: i32, stdout: &str, stderr: &str) -> BackendError {
    let message = if !stderr.is_empty() {
        stderr.to_string()
    } else if !stdout.is_empty() {
        stdout.to_string()
    } else {
        format!("exit code {code}")
    };
    BackendError::new(BackendErrorKind::Rejected, false, message)
}

/// Quote simple pour une ligne de commande shell distante (chemins de
/// fichier configurés par le propriétaire).
fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_timeout_detects_messages() {
        assert!(is_timeout("connection timed out"));
        assert!(is_timeout("Timeout waiting for response"));
        assert!(!is_timeout("authentication failed"));
    }

    #[test]
    fn sh_quote_escapes_single_quotes() {
        assert_eq!(sh_quote("nginx"), "'nginx'");
        assert_eq!(sh_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn entry_fingerprint_extracts_token() {
        assert_eq!(
            entry_fingerprint("host:22 SHA256:abc123"),
            Some("SHA256:abc123")
        );
        assert_eq!(entry_fingerprint("host:22"), None);
    }

    #[test]
    fn all_match_requires_exclusive_equality() {
        let a = "host:22 SHA256:aaa";
        let b = "host:22 SHA256:bbb";
        // Aucune entrée → pas de match (rien à vérifier).
        assert!(!entries_all_match(&[], "SHA256:aaa"));
        // Une entrée identique → match.
        assert!(entries_all_match(&[a], "SHA256:aaa"));
        // Une entrée identique + une autre empreinte → REFUS (ambiguïté).
        assert!(!entries_all_match(&[a, b], "SHA256:aaa"));
        assert!(!entries_all_match(&[a, b], "SHA256:bbb"));
        // Ligne malformée → pas de match.
        assert!(!entries_all_match(&["host:22"], "SHA256:aaa"));
    }

    /// Connexion de test pointée sur un fichier known_hosts temporaire.
    fn test_conn(dir: &std::path::Path) -> SshConn {
        SshConn::new(
            "vps.test".into(),
            22,
            "root".into(),
            None,
            None,
            dir.join("known_hosts"),
            Duration::from_secs(5),
        )
    }

    #[test]
    fn tofu_persists_accepts_same_key_and_refuses_mismatch() {
        let dir = std::env::temp_dir().join(format!("dakkabot-tofu-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = test_conn(&dir);

        // Première connexion : TOFU → accepté, entrée persistée.
        assert!(verify_and_remember_host_key(&conn, "SHA256:key1"));
        let content = std::fs::read_to_string(&conn.known_hosts).unwrap();
        assert!(content.contains("vps.test:22 SHA256:key1"));

        // Même clé ensuite → accepté.
        assert!(verify_and_remember_host_key(&conn, "SHA256:key1"));

        // Clé différente → refus (MITM).
        assert!(!verify_and_remember_host_key(&conn, "SHA256:key2"));

        // Fichier illisible → refus (fail-closed, pas de TOFU muet).
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&conn.known_hosts, std::fs::Permissions::from_mode(0o000))
            .unwrap();
        assert!(!verify_and_remember_host_key(&conn, "SHA256:key1"));
        std::fs::set_permissions(&conn.known_hosts, std::fs::Permissions::from_mode(0o600))
            .unwrap();

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn two_persisted_keys_trust_neither() {
        // Deux « premières connexions » concurrentes (une MITM) : le
        // fichier finit avec DEUX empreintes → AUCUNE n'est plus acceptée
        // (fail-closed, intervention humaine requise).
        let dir = std::env::temp_dir().join(format!("dakkabot-race-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = test_conn(&dir);
        assert!(append_host_key(&conn, "vps.test:22 SHA256:key1"));
        assert!(append_host_key(&conn, "vps.test:22 SHA256:key2"));
        assert!(!verify_and_remember_host_key(&conn, "SHA256:key1"));
        assert!(!verify_and_remember_host_key(&conn, "SHA256:key2"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rejected_prefers_stderr() {
        let e = rejected(1, "out", "err detail");
        assert_eq!(e.message, "err detail");
        assert_eq!(e.kind, BackendErrorKind::Rejected);
        assert!(!e.may_have_been_transmitted());
    }
}
