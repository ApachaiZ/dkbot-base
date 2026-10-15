//! Plugin factice — preuve de concept (ABI v1, aucune dépendance).

use std::ffi::c_char;

// Copie locale de la vtable ABI v1 (Phase 1 la déplacera dans un crate
// partagé `dkbot-host-api`).
#[repr(C)]
pub struct PluginVtable {
    pub abi_version: u32,
    pub name: unsafe extern "C" fn() -> *const c_char,
    pub list: unsafe extern "C" fn() -> *const c_char,
    pub run: unsafe extern "C" fn(cmd: *const c_char, args_json: *const c_char, out: *mut u8, out_cap: usize) -> i32,
    pub shutdown: unsafe extern "C" fn(),
}

static NAME: &[u8] = b"dummy-plugin\0";
static CMDS: &[u8] = b"[\"ping\"]\0";

unsafe extern "C" fn plugin_name() -> *const c_char {
    NAME.as_ptr() as *const c_char
}

unsafe extern "C" fn plugin_list() -> *const c_char {
    CMDS.as_ptr() as *const c_char
}

unsafe extern "C" fn plugin_run(
    cmd: *const c_char,
    args_json: *const c_char,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    let cmd = std::ffi::CStr::from_ptr(cmd).to_string_lossy();
    if cmd != "ping" {
        return -1;
    }
    let _args = std::ffi::CStr::from_ptr(args_json).to_string_lossy();
    let resp = "{\"plugin\":\"dummy-plugin\",\"reply\":\"pong\"}";
    let bytes = resp.as_bytes();
    let n = bytes.len().min(out_cap);
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, n);
    n as i32
}

unsafe extern "C" fn plugin_shutdown() {}

static VTABLE: PluginVtable = PluginVtable {
    abi_version: 1,
    name: plugin_name,
    list: plugin_list,
    run: plugin_run,
    shutdown: plugin_shutdown,
};

/// Point d'entrée du plugin — symbole recherché par l'hôte.
#[no_mangle]
pub unsafe extern "C" fn dkbot_plugin_vtable() -> *const PluginVtable {
    &VTABLE
}
