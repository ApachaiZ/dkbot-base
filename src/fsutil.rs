//! Écriture atomique et sécurisée de petits fichiers d'état.
//!
//! `atomic_write` : écrire dans un fichier temporaire du MÊME répertoire
//! puis `rename` (opération atomique POSIX) — un crash ne laisse jamais un
//! fichier à moitié écrit.
//!
//! `secure_write` : en plus, permissions `0600` (lisible/écrivable par le
//! seul propriétaire) — utilisé pour tout ce qui touche à des données
//! sensibles (opérateurs, sessions, verrous).

use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Écriture atomique : tmp (même répertoire) + rename.
pub fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp_path = Path::new(&tmp);
    {
        let mut file = fs::File::create(tmp_path)?;
        file.write_all(data)?;
        file.sync_all()?;
    }
    match fs::rename(tmp_path, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(tmp_path);
            Err(e)
        }
    }
}

/// Écriture atomique avec permissions `0600` (données sensibles).
/// Les permissions sont posées sur le fichier TEMPORAIRE AVANT le rename :
/// le fichier final n'est à aucun moment plus permissif que 0600.
pub fn secure_write(path: &Path, data: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp_path = Path::new(&tmp);
    {
        // `mode(0o600)` s'applique à la création ; on REPOSE les
        // permissions après ouverture : un `.tmp` orphelin d'un crash
        // précédent (créé en 0644 par une vieille version) ne doit jamais
        // être renommé sur le fichier final avec des permissions larges.
        use std::os::unix::fs::PermissionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(tmp_path)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(data)?;
        file.sync_all()?;
    }
    match fs::rename(tmp_path, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(tmp_path);
            Err(e)
        }
    }
}
