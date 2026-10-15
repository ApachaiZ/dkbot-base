//! Hôte de plugins — ABI C stable et versionné (spike Phase 0).
//!
//! Les plugins sont des bibliothèques dynamiques (`.so`/`.dylib`/`.dll`)
//! qui exportent `dkbot_plugin_vtable` et parlent à l'hôte à travers une
//! table de fonctions `repr(C)` : seuls des pointeurs, des entiers et des
//! octets traversent la frontière (JSON en entrée/sortie des commandes).

use std::path::Path;
use std::sync::Mutex;

pub use dkbot_host_api::{ABI_VERSION, MAX_OUT, PluginVtable, PluginVtableFn, ENTRY_SYMBOL};

pub struct Plugin {
    pub name: String,
    vtable: *const PluginVtable,
    /// Garde le `.so` chargé pendant toute la vie du plugin.
    _lib: Option<libloading::Library>,
}

pub struct PluginHost {
    plugins: Mutex<Vec<Plugin>>,
}

// SAFETY : la vtable pointe vers une statique immuable du plugin (jamais
// modifiée après chargement) et le .so reste chargé via `Library` (Send +
// Sync). L'accès concurrent est protégé par le Mutex.
unsafe impl Send for PluginHost {}
unsafe impl Sync for PluginHost {}

impl Default for PluginHost {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginHost {
    pub fn new() -> Self {
        PluginHost {
            plugins: Mutex::new(Vec::new()),
        }
    }

    /// Charge tous les plugins du dossier (fichiers `.so`/`.dylib`/`.dll`).
    /// Retourne les noms chargés ; les incompatibilités sont loggées et
    /// ignorées, jamais fatales.
    pub fn load_dir(&self, dir: &Path) -> Vec<String> {
        let mut loaded = Vec::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => return loaded,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_lib = path
                .extension()
                .map(|e| e == "so" || e == "dylib" || e == "dll")
                .unwrap_or(false);
            if !is_lib || !path.is_file() {
                continue;
            }
            match self.load_one(&path) {
                Ok(name) => loaded.push(name),
                Err(err) => crate::logger::error(format!(
                    "plugins: {} refusé — {}",
                    path.display(),
                    err
                )),
            }
        }
        loaded
    }

    fn load_one(&self, path: &Path) -> Result<String, String> {
        // SAFETY : le chemin est un fichier du dossier plugins de l'hôte.
        let lib = unsafe { libloading::Library::new(path) }
            .map_err(|e| format!("chargement échoué : {e}"))?;
        // SAFETY : le symbole est fourni par le plugin ; le type est vérifié
        // par la version d'ABI immédiatement après.
        let vtable_fn: libloading::Symbol<PluginVtableFn> = unsafe {
            lib.get(ENTRY_SYMBOL)
        }
        .map_err(|e| format!("symbole dkbot_plugin_vtable absent : {e}"))?;
        let get_vtable: PluginVtableFn = *vtable_fn;
        // SAFETY : le plugin retourne sa table statique immuable.
        let vtable_ref = unsafe { get_vtable() };

        // SAFETY : la vtable est une statique du plugin ; on ne lit que les
        // champs déclarés, avant tout appel.
        let abi = unsafe { (*vtable_ref).abi_version };
        if abi != ABI_VERSION {
            return Err(format!(
                "ABI {abi} incompatible avec l'hôte (ABI {ABI_VERSION})"
            ));
        }
        // SAFETY : name() retourne une chaîne statique terminée par NUL.
        let name = unsafe { dkbot_host_api::read_cstr(((*vtable_ref).name)()) };
        if name.is_empty() {
            return Err("nom de plugin vide".into());
        }

        {
            let mut plugins = self.plugins.lock().expect("plugin host lock");
            if plugins.iter().any(|p| p.name == name) {
                return Err(format!("déjà chargé : {name}"));
            }
            plugins.push(Plugin {
                name: name.clone(),
                vtable: vtable_ref,
                _lib: Some(lib),
            });
        }
        crate::logger::info(format!("plugins: {name} chargé (ABI {abi})"));
        Ok(name)
    }

    pub fn names(&self) -> Vec<String> {
        self.plugins
            .lock()
            .expect("plugin host lock")
            .iter()
            .map(|p| p.name.clone())
            .collect()
    }

    pub fn list_commands(&self, plugin: &str) -> Result<Vec<String>, String> {
        let plugins = self.plugins.lock().expect("plugin host lock");
        let Some(found) = plugins.iter().find(|p| p.name == plugin) else {
            return Err(format!("plugin inconnu : {plugin}"));
        };
        // SAFETY : list() retourne une chaîne statique JSON du plugin.
        let raw = unsafe { dkbot_host_api::read_cstr(((*found.vtable).list)()) };
        let parsed: Vec<String> =
            serde_json::from_str(&raw).map_err(|e| format!("liste illisible : {e}"))?;
        Ok(parsed)
    }

    pub fn run(
        &self,
        plugin: &str,
        cmd: &str,
        args_json: &str,
    ) -> Result<String, String> {
        let plugins = self.plugins.lock().expect("plugin host lock");
        let Some(found) = plugins.iter().find(|p| p.name == plugin) else {
            return Err(format!("plugin inconnu : {plugin}"));
        };

        let cmd_c = std::ffi::CString::new(cmd).map_err(|_| "commande invalide".to_string())?;
        let args_c =
            std::ffi::CString::new(args_json).map_err(|_| "arguments invalides".to_string())?;
        let mut out = vec![0u8; MAX_OUT];

        // SAFETY : pointeurs valides, tampon borné, la vtable appartient au
        // plugin chargé (le .so est gardé vivant par `_lib`).
        let written = unsafe {
            ((*found.vtable).run)(cmd_c.as_ptr(), args_c.as_ptr(), out.as_mut_ptr(), MAX_OUT)
        };
        if written < 0 {
            return Err(format!("commande {cmd} refusée par le plugin (code {written})"));
        }
        let written = (written as usize).min(MAX_OUT);
        let raw = String::from_utf8_lossy(&out[..written]).into_owned();
        if raw.is_empty() {
            return Ok("{}".to_string());
        }
        Ok(raw)
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        let plugins = self.plugins.get_mut().expect("plugin host lock");
        for plugin in plugins.iter() {
            // SAFETY : shutdown() est la fonction d'arrêt du plugin, appelée
            // tant que le .so est encore chargé.
            unsafe { ((*plugin.vtable).shutdown)() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{c_char, CStr};

    static NAME: &[u8] = b"test-plugin\0";
    static LIST: &[u8] = b"[\"echo\"]\0";

    unsafe extern "C" fn t_name() -> *const c_char {
        NAME.as_ptr() as *const c_char
    }
    unsafe extern "C" fn t_list() -> *const c_char {
        LIST.as_ptr() as *const c_char
    }
    unsafe extern "C" fn t_run(
        cmd: *const c_char,
        args: *const c_char,
        out: *mut u8,
        cap: usize,
    ) -> i32 {
        let cmd = CStr::from_ptr(cmd).to_string_lossy();
        if cmd != "echo" {
            return -1;
        }
        let args = CStr::from_ptr(args).to_string_lossy();
        let resp = format!("echo:{args}");
        let bytes = resp.as_bytes();
        let n = bytes.len().min(cap);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, n);
        n as i32
    }
    unsafe extern "C" fn t_shutdown() {}

    static VTABLE: PluginVtable = PluginVtable {
        abi_version: ABI_VERSION,
        name: t_name,
        list: t_list,
        run: t_run,
        shutdown: t_shutdown,
    };

    fn host_with_test_plugin() -> PluginHost {
        let host = PluginHost::new();
        {
            let mut plugins = host.plugins.lock().expect("lock");
            plugins.push(Plugin {
                name: "test-plugin".to_string(),
                vtable: &VTABLE,
                _lib: None,
            });
        }
        host
    }

    #[test]
    fn fake_plugin_runs_through_host() {
        let host = host_with_test_plugin();
        assert_eq!(host.names(), vec!["test-plugin"]);
        assert_eq!(host.list_commands("test-plugin").unwrap(), vec!["echo"]);
        assert_eq!(host.run("test-plugin", "echo", "hello").unwrap(), "echo:hello");
    }

    /// Chargement RÉEL d'un `.so` : à lancer avec
    /// `DKBOT_TEST_PLUGINS=/tmp/plugins-test cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn loads_a_real_shared_object() {
        let Some(dir) = std::env::var_os("DKBOT_TEST_PLUGINS") else {
            panic!("DKBOT_TEST_PLUGINS non défini");
        };
        let host = PluginHost::new();
        let loaded = host.load_dir(std::path::Path::new(&dir));
        assert!(loaded.contains(&"dummy-plugin".to_string()), "chargés : {loaded:?}");
        assert_eq!(host.list_commands("dummy-plugin").unwrap(), vec!["ping"]);
        assert_eq!(
            host.run("dummy-plugin", "ping", "{}").unwrap(),
            "{\"plugin\":\"dummy-plugin\",\"reply\":\"pong\"}"
        );
    }

    #[test]
    fn unknown_plugin_is_an_error() {
        let host = host_with_test_plugin();
        assert!(host.run("autre", "echo", "{}").is_err());
        assert!(host.list_commands("autre").is_err());
    }

    #[test]
    fn plugin_error_code_is_an_error() {
        let host = host_with_test_plugin();
        assert!(host.run("test-plugin", "inconnue", "{}").is_err());
    }
}
