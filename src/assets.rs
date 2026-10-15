//! GIFs des cartes (chargement / succès / erreur / attente).
//!
//! À chaque affichage, UN GIF tiré au hasard. Deux sources dans l'ordre :
//! 1. le DISQUE (`assets/emojis/<état>/`) — personnalisable sans
//!    recompiler ;
//! 2. les octets EMBARQUÉS (pool généré par `build.rs` au build).

use poise::serenity_prelude as serenity;
use rand::Rng;
use serenity::CreateAttachment;
use std::path::PathBuf;

/// Un GIF prêt à être posé sur une carte.
pub struct Gif {
    pub attachment: Option<CreateAttachment>,
    pub image_url: Option<String>,
}

/// Entrée du pool embarqué (générée par build.rs).
struct EmbeddedGif {
    kind: &'static str,
    name: &'static str,
    bytes: &'static [u8],
}

const POOL: &[EmbeddedGif] = include!(concat!(env!("OUT_DIR"), "/emojis_pool.rs"));

/// Le pool `waiting` pioche dans le dossier `loading`.
fn folder_of(kind: &str) -> &str {
    if kind == "waiting" {
        "loading"
    } else {
        kind
    }
}

/// GIF aléatoire du pool embarqué pour un état.
fn random_embedded_gif(kind: &str) -> Option<&'static EmbeddedGif> {
    let pool: Vec<&EmbeddedGif> = POOL.iter().filter(|g| g.kind == folder_of(kind)).collect();
    if pool.is_empty() {
        return None;
    }
    let idx = rand::thread_rng().gen_range(0..pool.len());
    Some(pool[idx])
}

/// GIF aléatoire du dossier disque pour un état.
fn random_disk_gif(kind: &str) -> Option<(PathBuf, String)> {
    let dir = crate::paths::gif_dir(folder_of(kind));
    let entries: Vec<String> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| name.ends_with(".gif"))
        .collect();
    if entries.is_empty() {
        return None;
    }
    let idx = rand::thread_rng().gen_range(0..entries.len());
    let name = entries[idx].clone();
    Some((dir.join(&name), name))
}

/// GIF pour un état (`loading` | `waiting` | `success` | `error`) : disque
/// d'abord, octets embarqués ensuite.
pub fn gif_image(kind: &str) -> Gif {
    if let Some((path, name)) = random_disk_gif(kind) {
        if let Ok(bytes) = std::fs::read(&path) {
            let attachment = CreateAttachment::bytes(bytes, name.clone());
            return Gif {
                image_url: Some(format!("attachment://{name}")),
                attachment: Some(attachment),
            };
        }
    }
    if let Some(gif) = random_embedded_gif(kind) {
        let name = format!("{}.gif", gif.name);
        let attachment = CreateAttachment::bytes(gif.bytes.to_vec(), name.clone());
        return Gif {
            image_url: Some(format!("attachment://{name}")),
            attachment: Some(attachment),
        };
    }
    Gif {
        attachment: None,
        image_url: None,
    }
}
