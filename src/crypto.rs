//! Chiffrement au repos des fichiers d'état sensibles (opérateurs,
//! sessions, roster) — AES-256-GCM.
//!
//! Schéma :
//! - clé = SHA-256(`"dakkadakkabot-v1:" + DISCORD_TOKEN`) — le token est le
//!   seul secret du bot, il sert aussi de clé de dérivation ;
//! - IV 12 octets aléatoires (OsRng) par chiffrement ;
//! - format disque : `base64(iv(12) ‖ tag(16) ‖ ciphertext)`.
//!
//! ⚠ Le sel ci-dessous est GELÉ depuis le renommage du projet (DakkaBot) :
//! le changer rendrait illisibles les fichiers déjà chiffrés avec lui.
//! Une rotation du token rend `users.json`, `sessions.json` et
//! `members.roster.json` illisibles (il faut les supprimer).

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rand::RngCore;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

/// Sel de dérivation de clé (préfixe du token).
const KEY_SALT: &str = "dakkadakkabot-v1:";

fn key(token: &str) -> [u8; 32] {
    Sha256::digest(format!("{KEY_SALT}{token}").as_bytes()).into()
}

/// Chiffre un objet JSON vers le blob base64(`iv‖ct‖tag`).
pub fn encrypt_json(token: &str, obj: &Value) -> String {
    let cipher = Aes256Gcm::new_from_slice(&key(token)).expect("clé AES-256 valide");
    let mut iv = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut iv);
    let plaintext = serde_json::to_string(obj).unwrap_or_else(|_| "{}".to_string());
    let mut blob = cipher
        .encrypt(Nonce::from_slice(&iv), plaintext.as_bytes())
        .expect("chiffrement AES-GCM");
    // GCM renvoie `ct ‖ tag(16)` : on déplace le tag EN QUEUE pour
    // obtenir le format disque `iv ‖ ct ‖ tag`.
    let tag = blob.split_off(blob.len() - 16);
    let mut out = Vec::with_capacity(12 + blob.len() + 16);
    out.extend_from_slice(&iv);
    out.extend_from_slice(&blob); // le corps chiffré
    out.extend_from_slice(&tag);
    BASE64.encode(out)
}

/// Déchiffre un blob base64(`iv‖ct‖tag`) vers un objet JSON.
pub fn decrypt_json(token: &str, blob: &str) -> Result<Value, String> {
    let buf = BASE64
        .decode(blob.trim())
        .map_err(|e| format!("base64 invalide : {e}"))?;
    if buf.len() < 28 {
        return Err("blob trop court".to_string());
    }
    let (iv, rest) = buf.split_at(12);
    let (ct, tag) = rest.split_at(rest.len() - 16);
    // GCM attend `ct ‖ tag`.
    let mut data = Vec::with_capacity(ct.len() + 16);
    data.extend_from_slice(ct);
    data.extend_from_slice(tag);
    let cipher = Aes256Gcm::new_from_slice(&key(token)).expect("clé AES-256 valide");
    let plaintext = cipher
        .decrypt(Nonce::from_slice(iv), data.as_slice())
        .map_err(|_| "clé incorrecte ou contenu altéré".to_string())?;
    serde_json::from_slice(&plaintext).map_err(|e| format!("JSON invalide : {e}"))
}

/// Lecture tolérante : chiffré uniquement. Un fichier illisible (rotation
/// de token, altération) renvoie `None` — l'état repart à zéro, le fichier
/// reste sur disque. PAS de repli clair : un projet neuf n'a pas d'état
/// hérité, et accepter du clair affaiblirait la garantie « chiffré au
/// repos ».
pub fn read_json(token: &str, file: &Path) -> Option<Value> {
    let raw = std::fs::read_to_string(file).ok()?;
    decrypt_json(token, &raw).ok()
}

/// Écriture chiffrée (atomique + permissions 0600).
pub fn write_json(token: &str, file: &Path, obj: &Value) -> std::io::Result<()> {
    crate::fsutil::secure_write(file, encrypt_json(token, obj).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn roundtrip() {
        let token = "test-token";
        let value = json!({"a": [1, 2, 3], "b": "x"});
        let blob = encrypt_json(token, &value);
        assert_eq!(decrypt_json(token, &blob).unwrap(), value);
    }

    #[test]
    fn wrong_key_fails() {
        let blob = encrypt_json("token-a", &json!([1]));
        assert!(decrypt_json("token-b", &blob).is_err());
    }

    #[test]
    fn tampered_blob_fails() {
        let blob = encrypt_json("token", &json!({"k": "v"}));
        let mut bytes = BASE64.decode(&blob).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        let tampered = BASE64.encode(&bytes);
        assert!(decrypt_json("token", &tampered).is_err());
    }

    #[test]
    fn format_is_iv_ct_tag() {
        let blob = encrypt_json("token", &json!({"k": "v"}));
        let buf = BASE64.decode(&blob).unwrap();
        assert!(buf.len() > 28);
        assert_eq!(buf.len(), 12 + (buf.len() - 28) + 16); // iv + ct + tag
    }
}
