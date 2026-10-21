//! `ea ajustes` de extremo a extremo, con el binario real y todo el estado en
//! directorios temporales (configuración, base de datos, bus y bóveda).

use std::process::{Command, Output, Stdio};

fn ea(dir: &std::path::Path, args: &[&str], extra: &[(&str, &str)]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_ea"));
    c.args(args)
        .env("EA_CONFIG_DIR", dir.join("config"))
        .env("EA_DB_PATH", dir.join("ea.db"))
        .env("EA_ESTADO_DIR", dir.join("estado"))
        .env("EA_BOVEDA_DIR", dir.join("boveda"))
        .env_remove("EA_WEB_PORT")
        .env_remove("EA_MCP_PORT")
        .stdin(Stdio::piped());
    for (k, v) in extra {
        c.env(k, v);
    }
    c.output().expect("lanzar ea")
}

#[test]
fn ver_en_json_es_json_valido() {
    let d = tempfile::tempdir().unwrap();
    let o = ea(d.path(), &["ajustes", "ver", "--json"], &[]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["fichero"], d.path().join("config/ajustes.toml").display().to_string());
    assert_eq!(v["secciones"].as_array().unwrap().len(), 8);
}

#[test]
fn obtener_da_el_valor_efectivo() {
    let d = tempfile::tempdir().unwrap();
    let o = ea(d.path(), &["ajustes", "obtener", "red.puerto_web"], &[]);
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "8092");
    let o = ea(d.path(), &["ajustes", "obtener", "red.puerto_web"], &[("EA_WEB_PORT", "9000")]);
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "9000");
    let o = ea(d.path(), &["ajustes", "obtener", "no.existe"], &[]);
    assert!(!o.status.success());
    // claves con patrón y nombres antiguos
    let o = ea(d.path(), &["ajustes", "obtener", "roles.ejecutor"], &[]);
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "qwen-code");
    let o = ea(d.path(), &["ajustes", "obtener", "modelo.nombre"], &[]);
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "qwen3-coder-30b");
    let o = ea(d.path(), &["ajustes", "obtener", "modelos.nada.nombre"], &[]);
    assert!(!o.status.success());
    let o = ea(d.path(), &["ajustes", "ver"], &[]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("agentes.codex.bin"));
}

#[test]
fn establecer_sin_terminal_no_escribe_nada() {
    let d = tempfile::tempdir().unwrap();
    let o = ea(d.path(), &["ajustes", "establecer", "web.sondeo_ms=5000"], &[]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("terminal interactiva"), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(!d.path().join("config/ajustes.toml").exists());
}
