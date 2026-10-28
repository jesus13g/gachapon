//! Los dominios cerrados de la línea de órdenes (U-24): el modo de una orden,
//! la región de la bóveda, el rol y el agente los valida `clap` al leerla,
//! con su mensaje y sin tocar nada, y `--help` enumera los valores.

use std::path::Path;
use std::process::Output;

fn ea(dir: &Path, args: &[&str]) -> Output {
    ea_pruebas::cli::ea(env!("CARGO_BIN_EXE_ea"), dir).args(args).current_dir(dir).output().expect("lanzar ea")
}

fn salida(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// Cada argumento: la orden con un valor que no está en la lista, la ayuda que
/// tiene que enumerarlos y los valores.
const CASOS: [(&[&str], &[&str], &[&str]); 5] = [
    (
        &["trabajo", "crear", "--modo", "tarea", "--titulo", "t", "--worktree", ".", "--objetivo", "o"],
        &["trabajo", "crear", "--help"],
        &["mision", "obra", "rutina"],
    ),
    (&["boveda", "listar", "--region", "privada"], &["boveda", "listar", "--help"], &["modelo-local", "claude-code", "compartida"]),
    (&["boveda", "buscar", "x", "--region", "privada"], &["boveda", "buscar", "--help"], &["modelo-local", "claude-code", "compartida"]),
    (&["cola", "jefe"], &["cola", "--help"], &["planificador", "ejecutor"]),
    (
        &["agentes", "rol", "ejecutor", "nadie"],
        &["agentes", "rol", "--help"],
        &["claude-code", "qwen-code", "codex", "opencode", "antigravity"],
    ),
];

#[test]
fn un_valor_fuera_de_la_lista_lo_rechaza_clap_con_los_valores() {
    let d = tempfile::tempdir().unwrap();
    for (orden, _, valores) in CASOS {
        let o = ea(d.path(), orden);
        let s = salida(&o);
        assert_eq!(o.status.code(), Some(2), "{orden:?}: {s}");
        assert!(s.contains("invalid value") && s.contains("possible values"), "{orden:?}: {s}");
        assert!(valores.iter().all(|v| s.contains(v)), "{orden:?}: {s}");
    }
    // Ni siquiera llegó a abrir el bus ni la base.
    assert!(!d.path().join("estado").exists() && !d.path().join("ea.db").exists());
    // El rol de `agentes rol` también, antes que el agente.
    let s = salida(&ea(d.path(), &["agentes", "rol", "jefe", "codex"]));
    assert!(s.contains("invalid value 'jefe'") && s.contains("planificador, ejecutor"), "{s}");
}

#[test]
fn la_ayuda_enumera_los_valores() {
    let d = tempfile::tempdir().unwrap();
    for (_, ayuda, valores) in CASOS {
        let o = ea(d.path(), ayuda);
        let s = salida(&o);
        assert!(o.status.success(), "{ayuda:?}: {s}");
        for v in valores {
            assert!(s.contains("possible values") && s.contains(v), "{ayuda:?} sin {v}: {s}");
        }
    }
    let s = salida(&ea(d.path(), &["agentes", "rol", "--help"]));
    assert!(s.contains("[possible values: planificador, ejecutor]"), "{s}");
}

#[test]
fn los_valores_de_la_lista_pasan() {
    let d = tempfile::tempdir().unwrap();
    // El rol por su nombre y por su nombre en disco.
    for rol in ["planificador", "ejecutor", "claude-code", "modelo-local"] {
        let o = ea(d.path(), &["cola", rol]);
        assert!(o.status.success(), "{rol}: {}", salida(&o));
    }
    let o = ea(d.path(), &["boveda", "listar", "--region", "compartida"]);
    assert!(o.status.success(), "{}", salida(&o));
    // El modo pasa: lo que falla después es el worktree, que no es un repo.
    let o = ea(d.path(), &["trabajo", "crear", "--modo", "obra", "--titulo", "t", "--worktree", ".", "--objetivo", "o"]);
    let s = salida(&o);
    assert!(!o.status.success() && !s.contains("invalid value"), "{s}");
    // `agentes rol` con valores válidos llega a pedir la terminal (sin TTY).
    let o = ea(d.path(), &["agentes", "rol", "ejecutor", "codex"]);
    let s = salida(&o);
    assert!(!o.status.success() && !s.contains("invalid value") && s.contains("terminal"), "{s}");
}
