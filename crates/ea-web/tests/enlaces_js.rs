//! Las pruebas en JavaScript de la página (`tests/js/`), lanzadas con `node`.
//! Sin `node` en el PATH se saltan con un aviso: el panel no lo necesita para
//! funcionar, solo estas pruebas.

use std::path::Path;
use std::process::Command;

fn node() -> Option<Command> {
    match Command::new("node").arg("--version").output() {
        Ok(s) if s.status.success() => Some(Command::new("node")),
        _ => {
            eprintln!("aviso: no hay `node` en el PATH; se saltan las pruebas de JavaScript");
            None
        }
    }
}

#[test]
fn el_parseo_del_hash_pasa_sus_pruebas() {
    let Some(mut node) = node() else { return };
    let prueba = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/js/enlaces.test.js");
    let salida = node.arg("--test").arg(&prueba).output().expect("lanzar node");
    assert!(
        salida.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&salida.stdout),
        String::from_utf8_lossy(&salida.stderr)
    );
}

#[test]
fn los_scripts_de_la_pagina_son_javascript_valido() {
    let static_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("static");
    for js in ["app.js", "cliente.js", "enlaces.js", "tour.js"] {
        let Some(mut node) = node() else { return };
        let salida = node.arg("--check").arg(static_dir.join(js)).output().expect("lanzar node");
        assert!(salida.status.success(), "{js}: {}", String::from_utf8_lossy(&salida.stderr));
    }
}
