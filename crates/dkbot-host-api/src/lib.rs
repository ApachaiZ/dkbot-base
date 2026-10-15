//! `dkbot-host-api` — le contrat ABI entre l'hôte `dkbot` et ses plugins.
//!
//! # Pourquoi un crate dédié
//!
//! Les plugins sont des bibliothèques dynamiques compilées SÉPARÉMENT du
//! cœur. Pour qu'ils puissent se parler, les deux côtés partagent ce crate :
//! les types `repr(C)` de la vtable et les constantes de version. La version
//! d'ABI est vérifiée au chargement : un plugin trop ancien ou trop récent
//! est refusé proprement, jamais appelé.
//!
//! # Règles du contrat
//!
//! - **Rien d'autre que des octets ne traverse la frontière** : les commandes
//!   reçoivent et renvoient du JSON. Pas de types Rust partagés (pas de
//!   `serde` au niveau de la vtable), pas d'enums à données.
//! - **Tout pointeur passé au plugin est valide pendant l'appel seulement** ;
//!   le plugin ne doit rien retenir.
//! - **Les tampons sont bornés** : l'hôte fournit `out`/`out_cap` ; le plugin
//!   écrit au plus `out_cap` octets et retourne le nombre écrit (négatif =
//!   erreur, le plugin est seul responsable de ne pas déborder).
//! - **Bump de version** : toute modification incompatible (signature, layout)
//!   incrémente `ABI_VERSION`. Les ajouts compatibles (nouvelle commande,
//!   nouveau champ en fin de struct avec défaut) ne la changent pas.

use std::ffi::{c_char, CStr};

/// Version courante de l'ABI. Toute différence avec la version de l'hôte
/// empêche le chargement.
pub const ABI_VERSION: u32 = 1;

/// Taille maximale du tampon de réponse fourni par l'hôte.
pub const MAX_OUT: usize = 4096;

/// Exécute une commande : `cmd` + `args_json` (JSON) → réponse JSON écrite
/// dans `out` (borné par `out_cap`). Retour : octets écrits, 0 = vide,
/// négatif = erreur (code libre).
pub type PluginRunFn = unsafe extern "C" fn(
    cmd: *const c_char,
    args_json: *const c_char,
    out: *mut u8,
    out_cap: usize,
) -> i32;

/// La table des fonctions d'un plugin — unique point d'entrée, immuable,
/// stockée en statique côté plugin.
#[repr(C)]
pub struct PluginVtable {
    pub abi_version: u32,
    /// Nom du plugin (chaîne statique UTF-8 terminée par NUL).
    pub name: unsafe extern "C" fn() -> *const c_char,
    /// Liste des commandes exposées, en JSON : `["ping","…"]`.
    pub list: unsafe extern "C" fn() -> *const c_char,
    pub run: PluginRunFn,
    pub shutdown: unsafe extern "C" fn(),
}

/// Signature du symbole exporté par chaque plugin : `dkbot_plugin_vtable`.
pub type PluginVtableFn = unsafe extern "C" fn() -> *const PluginVtable;

/// Nom du symbole d'entrée recherché par l'hôte (avec le NUL final).
pub const ENTRY_SYMBOL: &[u8] = b"dkbot_plugin_vtable\0";

/// # Safety
/// `ptr` doit pointer vers une chaîne statique UTF-8 terminée par NUL
/// (typiquement le retour de `name()` ou `list()`).
pub unsafe fn read_cstr(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    CStr::from_ptr(ptr).to_string_lossy().into_owned()
}

/// Écrit `bytes` dans le tampon de sortie fourni par l'hôte, en respectant
/// `cap`. Retourne le nombre d'octets écrits (compatible avec le contrat
/// de `PluginRunFn`).
///
/// # Safety
/// `out` doit être valide pour `cap` octets.
pub unsafe fn write_out(out: *mut u8, cap: usize, bytes: &[u8]) -> i32 {
    let n = bytes.len().min(cap);
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, n);
    n as i32
}

/// Déclare l'entrée de plugin : à appeler dans le plugin avec sa vtable
/// statique.
///
/// ```ignore
/// dkbot_host_api::export_plugin!(&VTABLE);
/// ```
#[macro_export]
macro_rules! export_plugin {
    ($vtable:expr) => {
        #[no_mangle]
        pub unsafe extern "C" fn dkbot_plugin_vtable(
        ) -> *const $crate::PluginVtable {
            $vtable
        }
    };
}
