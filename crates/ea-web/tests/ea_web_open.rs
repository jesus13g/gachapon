//! `bin/ea-web-open` con `hyprctl`, `curl` y `omarchy-launch-webapp` de
//! mentira en el PATH: qué URL abre según el fragmento, la clave y si ya hay
//! ventana. Todo en un directorio temporal; no toca el escritorio real.

#![cfg(unix)] // el lanzador es un script de shell para Linux/Hyprland

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/ea-web-open")
}

fn stub(dir: &Path, nombre: &str, cuerpo: &str) {
    let ruta = dir.join(nombre);
    fs::write(&ruta, format!("#!/bin/sh\n{cuerpo}\n")).unwrap();
    fs::set_permissions(&ruta, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Lanza el script y devuelve la URL que habría abierto.
fn abre(arg: Option<&str>, clave: Option<&str>, hay_ventana: bool) -> String {
    abre_con(arg, clave, hay_ventana, |_| (script(), vec![("EA_WEB_PORT", "8092".into())]))
}

/// Como [`abre`], con el script y las variables que dé `preparar` (recibe
/// el directorio temporal de la prueba). Sin `EA_WEB_PORT` ni `EA_BIN`
/// salvo que los dé.
fn abre_con(arg: Option<&str>, clave: Option<&str>, hay_ventana: bool, preparar: impl FnOnce(&Path) -> (PathBuf, Vec<(&str, PathBuf)>)) -> String {
    let d = tempfile::tempdir().unwrap();
    let bin = d.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let abierta = d.path().join("abierta");
    let ventana = if hay_ventana { r#"[{"class": "chrome-localhost__-Default"}]"# } else { "[]" };
    stub(&bin, "hyprctl", &format!("[ \"$1\" = -j ] && echo '{ventana}'; exit 0"));
    stub(&bin, "curl", "exit 0");
    stub(&bin, "omarchy-launch-webapp", &format!("printf %s \"$1\" > '{}'", abierta.display()));
    let run = d.path().join("run");
    fs::create_dir_all(run.join("ea-web")).unwrap();
    if let Some(c) = clave {
        fs::write(run.join("ea-web/clave"), c).unwrap();
    }
    let (script, vars) = preparar(d.path());
    let mut cmd = Command::new("bash");
    cmd.arg(&script)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("XDG_RUNTIME_DIR", &run)
        .env("HOME", d.path().join("home-vacio"))
        .env_remove("EA_WEB_PORT")
        .env_remove("EA_BIN")
        .envs(vars);
    if let Some(a) = arg {
        cmd.arg(a);
    }
    let salida = cmd.output().unwrap();
    assert!(salida.status.success(), "{}", String::from_utf8_lossy(&salida.stderr));
    fs::read_to_string(&abierta).unwrap_or_default()
}

#[test]
fn sin_fragmento_abre_con_la_clave() {
    assert_eq!(abre(None, Some("K"), false), "http://localhost:8092/#clave=K");
}

#[test]
fn el_fragmento_va_delante_de_la_clave() {
    assert_eq!(abre(Some("#trabajos/20261020-101500-ab12"), Some("K"), false), "http://localhost:8092/#trabajos/20261020-101500-ab12&clave=K");
    // Sin '#' delante, igual.
    assert_eq!(abre(Some("diario?latidos=1"), Some("K"), false), "http://localhost:8092/#diario?latidos=1&clave=K");
}

#[test]
fn sin_clave_abre_el_fragmento_en_solo_lectura() {
    // (El script espera la clave unos segundos antes de rendirse.)
    assert_eq!(abre(Some("#trabajos/x"), None, false), "http://localhost:8092/#trabajos/x");
}

#[test]
fn con_ventana_abierta_solo_enfoca_salvo_con_fragmento() {
    assert_eq!(abre(None, Some("K"), true), "");
    assert_eq!(abre(Some("#trabajos/x"), Some("K"), true), "http://localhost:8092/#trabajos/x&clave=K");
}

#[test]
fn sin_puerto_lo_pregunta_al_ea_de_ea_bin() {
    let url = abre_con(None, Some("K"), false, |d| {
        stub(d, "mi-ea", "[ \"$1 $2 $3\" = 'ajustes obtener red.puerto_web' ] && echo 9123");
        (script(), vec![("EA_BIN", d.join("mi-ea"))])
    });
    assert_eq!(url, "http://localhost:9123/#clave=K");
}

#[test]
fn sin_ea_bin_usa_el_ea_junto_al_script() {
    // Una copia del script junto a un `ea` de mentira, como si estuviera al
    // lado de los binarios; nada en una ruta fija del HOME.
    let url = abre_con(None, Some("K"), false, |d| {
        let junto = d.join("junto");
        fs::create_dir(&junto).unwrap();
        fs::copy(script(), junto.join("ea-web-open")).unwrap();
        stub(&junto, "ea", "echo 9124");
        (junto.join("ea-web-open"), vec![])
    });
    assert_eq!(url, "http://localhost:9124/#clave=K");
}

#[test]
fn sin_ea_ni_puerto_usa_el_de_siempre() {
    let url = abre_con(None, Some("K"), false, |d| {
        let solo = d.join("solo");
        fs::create_dir(&solo).unwrap();
        fs::copy(script(), solo.join("ea-web-open")).unwrap();
        (solo.join("ea-web-open"), vec![])
    });
    assert_eq!(url, "http://localhost:8092/#clave=K");
}
