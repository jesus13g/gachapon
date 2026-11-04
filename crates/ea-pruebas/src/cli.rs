//! El binario `ea` con todo su estado en un directorio temporal.

use std::path::Path;
use std::process::{Command, Stdio};

/// `ea` (`bin`: el `env!("CARGO_BIN_EXE_ea")` de la prueba) con todo bajo
/// `dir` y sin nada del entorno real: ajustes en `config`, base en `ea.db`,
/// bus en `estado` y bóveda en `boveda` (los `EA_*`), `HOME` en `home` y los
/// `XDG_*` en `xdg/…` (el runtime, `xdg/rt`). Quita los `EA_*` y `XDG_*`
/// heredados y `DBUS_SESSION_BUS_ADDRESS`, así que un `systemctl --user` no
/// llega al del usuario. Sin entrada; los argumentos los pone quien llama.
pub fn ea(bin: &str, dir: &Path) -> Command {
    let xdg = |d: &str| dir.join("xdg").join(d);
    for d in [dir.join("home"), xdg("config"), xdg("datos"), xdg("estado"), xdg("cache"), xdg("rt")] {
        std::fs::create_dir_all(d).unwrap();
    }
    let mut c = Command::new(bin);
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("EA_") || k.to_string_lossy().starts_with("XDG_") {
            c.env_remove(&k);
        }
    }
    c.env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env("EA_CONFIG_DIR", dir.join("config"))
        .env("EA_DB_PATH", dir.join("ea.db"))
        .env("EA_ESTADO_DIR", dir.join("estado"))
        .env("EA_BOVEDA_DIR", dir.join("boveda"))
        .env("HOME", dir.join("home"))
        .env("USERPROFILE", dir.join("home"))
        .env("APPDATA", dir.join("home/AppData/Roaming"))
        .env("LOCALAPPDATA", xdg("rt"))
        .env("XDG_CONFIG_HOME", xdg("config"))
        .env("XDG_DATA_HOME", xdg("datos"))
        .env("XDG_STATE_HOME", xdg("estado"))
        .env("XDG_CACHE_HOME", xdg("cache"))
        .env("XDG_RUNTIME_DIR", xdg("rt"))
        .stdin(Stdio::null());
    c
}
