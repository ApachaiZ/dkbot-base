//! Plugin factice — preuve de concept (ABI v1, aucune dépendance).

use std::ffi::c_char;

use dkbot_host_api::{PluginVtable, export_plugin};

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
    abi_version: dkbot_host_api::ABI_VERSION,
    name: plugin_name,
    list: plugin_list,
    run: plugin_run,
    shutdown: plugin_shutdown,
};

// Point d'entrée — symbole recherché par l'hôte (ABI v1).
export_plugin!(&VTABLE);
