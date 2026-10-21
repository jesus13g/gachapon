//! `ea rutina ver --version N` de extremo a extremo, con el binario real y
//! todo el estado en directorios temporales.

use std::process::{Command, Output, Stdio};

fn ea(dir: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ea"))
        .args(args)
        .env("EA_CONFIG_DIR", dir.join("config"))
        .env("EA_DB_PATH", dir.join("ea.db"))
        .env("EA_ESTADO_DIR", dir.join("estado"))
        .env("EA_BOVEDA_DIR", dir.join("boveda"))
        .stdin(Stdio::piped())
        .output()
        .expect("lanzar ea")
}

const RUTINA: &str = "[rutina]\nslug = \"informe\"\nversion = 1\nworktree = \"/tmp\"\ncriterio_exito = \"true\"\n\n[[pasos]]\ntexto = \"primer texto\"\n";

#[test]
fn ver_una_version_concreta_de_una_rutina() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("informe.toml");
    std::fs::write(&f, RUTINA).unwrap();
    let o = ea(d.path(), &["rutina", "registrar", f.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    std::fs::write(&f, RUTINA.replace("version = 1", "version = 2").replace("primer", "segundo")).unwrap();
    let o = ea(d.path(), &["rutina", "registrar", f.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));

    let o = ea(d.path(), &["rutina", "ver", "informe", "--version", "1"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("versión 1") && out.contains("primer texto") && !out.contains("segundo texto"), "{out}");
    let (v2, v1) = (out.find("  v2 ").expect("v2 en la lista"), out.find("  v1 ").expect("v1 en la lista"));
    assert!(v2 < v1, "la más reciente primero: {out}");

    let o = ea(d.path(), &["rutina", "ver", "informe", "--version", "7"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("no tiene versión 7"));
}

/// Una rutina retirada (su TOML en la papelera de `rutinas_dir`) no sale en
/// `ea rutina listar` salvo con `--retiradas`, y `ea rutina lanzar` (lo que
/// ejecuta su timer) la rechaza. Retirar sin terminal no hace nada.
#[test]
fn una_rutina_retirada_no_se_lista_ni_se_lanza_desde_la_cli() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("informe.toml");
    std::fs::write(&f, RUTINA).unwrap();
    let o = ea(d.path(), &["rutina", "registrar", f.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let rutinas = d.path().join("config/rutinas");
    assert!(rutinas.join("informe.toml").exists());

    // Sin TTY no se confirma: el fichero sigue en su sitio.
    let o = ea(d.path(), &["rutina", "retirar", "informe"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("terminal"), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(rutinas.join("informe.toml").exists());

    // Retirada como lo haría `ea_retirar_rutina`.
    std::fs::create_dir_all(rutinas.join(".papelera")).unwrap();
    std::fs::rename(rutinas.join("informe.toml"), rutinas.join(".papelera/informe.toml")).unwrap();
    let o = ea(d.path(), &["rutina", "listar"]);
    assert!(!String::from_utf8_lossy(&o.stdout).contains("informe"), "{}", String::from_utf8_lossy(&o.stdout));
    let o = ea(d.path(), &["rutina", "listar", "--retiradas"]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("informe") && out.contains("papelera"), "{out}");
    let o = ea(d.path(), &["rutina", "lanzar", "informe"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("está retirada"), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(!d.path().join("estado/trabajos").exists() || std::fs::read_dir(d.path().join("estado/trabajos")).unwrap().next().is_none());
}
