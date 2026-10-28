//! `ea trabajo diff`, con el binario real, un repo git de verdad y todo el
//! estado en directorios temporales. El texto es el de `git diff` más los
//! ficheros nuevos sin seguimiento, y los `git` del sistema no ejecutan los
//! programas que el agente pudiera haber dejado configurados en el repo.

use std::path::Path;
use std::process::{Command, Output, Stdio};

use ea_bus::{Bus, Informe, ResultadoInforme};

fn ea(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ea"))
        .args(args)
        .env("EA_CONFIG_DIR", dir.join("config"))
        .env("EA_DB_PATH", dir.join("ea.db"))
        .env("EA_ESTADO_DIR", dir.join("estado"))
        .env("EA_BOVEDA_DIR", dir.join("boveda"))
        .stdin(Stdio::null())
        .output()
        .expect("lanzar ea")
}

/// `git` sin nada más, como lo lanzaría a mano el usuario. Devuelve stdout tal cual.
fn git(wt: &Path, args: &[&str], indice: Option<&Path>) -> String {
    let mut c = Command::new("git");
    c.arg("-C").arg(wt).args(["-c", "user.name=prueba", "-c", "user.email=prueba@localhost", "-c", "commit.gpgsign=false"]);
    if let Some(i) = indice {
        c.env("GIT_INDEX_FILE", i);
    }
    let o = c.args(args).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// Un repo con un commit y, encima, un cambio y ficheros nuevos de todo tipo:
/// texto con y sin salto final, binario, nombre con tilde y con espacio, y
/// `__pycache__` (que no sale). Devuelve el `commit_inicio`.
fn repo(wt: &Path) -> String {
    std::fs::create_dir_all(wt).unwrap();
    git(wt, &["init", "-q"], None);
    git(wt, &["config", "core.autocrlf", "false"], None);
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    git(wt, &["add", "-A"], None);
    git(wt, &["commit", "-q", "-m", "inicio"], None);
    let inicio = git(wt, &["rev-parse", "HEAD"], None).trim().to_string();
    std::fs::write(wt.join("a.txt"), "a\nb\n").unwrap();
    std::fs::write(wt.join("nuevo.txt"), "hola\n").unwrap();
    std::fs::write(wt.join("sin-salto.txt"), "x").unwrap();
    std::fs::write(wt.join("datos.bin"), [0xff, 0xfe, 0x00, 0x01]).unwrap();
    std::fs::write(wt.join("canción.txt"), "ñ\n").unwrap();
    std::fs::create_dir_all(wt.join("sub dir")).unwrap();
    std::fs::write(wt.join("sub dir/espacio.txt"), "e\n").unwrap();
    std::fs::create_dir_all(wt.join("__pycache__")).unwrap();
    std::fs::write(wt.join("__pycache__/m.pyc"), "pyc").unwrap();
    inicio
}

fn orden(wt: &Path) -> ea_bus::Orden {
    serde_json::from_value(serde_json::json!({
        "id": "", "modo": "obra", "titulo": "diff", "worktree": wt.to_string_lossy(), "objetivo": "o",
        "fases": [{ "numero": 1, "titulo": "f1", "instrucciones": "i", "entregable": "e", "criterio": "true" }],
        "creada_en": "", "creada_por": "test",
    }))
    .unwrap()
}

/// Lo que `ea trabajo diff` saca tras el `git diff`: cada fichero nuevo con su
/// contenido (o «binario o ilegible»), por el orden de `git ls-files`.
const NUEVOS: &str = "\n=== fichero nuevo sin seguimiento: canción.txt ===\nñ\n\n\
\n=== fichero nuevo sin seguimiento: datos.bin (binario o ilegible) ===\n\
\n=== fichero nuevo sin seguimiento: nuevo.txt ===\nhola\n\n\
\n=== fichero nuevo sin seguimiento: sin-salto.txt ===\nx\n\
\n=== fichero nuevo sin seguimiento: sub dir/espacio.txt ===\ne\n\n";

#[test]
fn el_diff_es_el_de_git_mas_los_ficheros_nuevos() {
    let d = tempfile::tempdir().unwrap();
    let wt = d.path().join("repo");
    let inicio = repo(&wt);
    let store = ea_store::Store::open_at(&d.path().join("ea.db")).unwrap();
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap().con_store(store);
    let id = bus.crear_trabajo(orden(&wt)).unwrap();
    bus.escribir_informe(&id, &Informe { commit_inicio: Some(inicio.clone()), ..Informe::nuevo(ResultadoInforme::Terminado) })
        .unwrap();

    let o = ea(d.path(), &["trabajo", "diff", &id]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let esperado = git(&wt, &["diff", &inicio, "--", ".", ea_bus::EXCLUIR_DIFF], None) + NUEVOS;
    assert!(esperado.contains("+b\n"), "{esperado}");
    assert_eq!(String::from_utf8_lossy(&o.stdout), esperado);

    // Por fase: entre las instantáneas de árbol, sin los ficheros nuevos aparte.
    let idx = d.path().join("indice");
    git(&wt, &["add", "-A"], Some(&idx));
    let arbol = git(&wt, &["write-tree"], Some(&idx)).trim().to_string();
    let informe = ea_bus::InformeFase { arbol: Some(arbol.clone()), ..Default::default() };
    bus.registrar_fase(&id, 1, ea_bus::EstadoFase::Hecha, &informe).unwrap();
    let o = ea(d.path(), &["trabajo", "diff", &id, "--fase", "1"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let esperado = git(&wt, &["diff", &format!("{inicio}^{{tree}}"), &arbol, "--", ".", ea_bus::EXCLUIR_DIFF], None);
    assert!(esperado.contains("+hola") && !esperado.contains("m.pyc"), "{esperado}");
    assert_eq!(String::from_utf8_lossy(&o.stdout), esperado);
}

/// Lo que el agente pudiera haber dejado en `.git/config` (fsmonitor,
/// `diff.external`, un `textconv` con `.gitattributes`) no se ejecuta al
/// pedir el diff, y el texto sale igual que sin nada plantado.
#[cfg(unix)]
#[test]
fn el_diff_no_ejecuta_lo_que_configure_el_repo() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let wt = d.path().join("repo");
    let inicio = repo(&wt);
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap();
    let id = bus.crear_trabajo(orden(&wt)).unwrap();
    bus.escribir_informe(&id, &Informe { commit_inicio: Some(inicio.clone()), ..Informe::nuevo(ResultadoInforme::Terminado) })
        .unwrap();
    let limpio = ea(d.path(), &["trabajo", "diff", &id]);
    assert!(limpio.status.success(), "{}", String::from_utf8_lossy(&limpio.stderr));

    let marca = d.path().join("marca");
    let g = d.path().join("guion.sh");
    std::fs::write(&g, format!("#!/bin/sh\necho \"$0 $*\" >> '{}'\nexit 1\n", marca.display())).unwrap();
    std::fs::set_permissions(&g, std::fs::Permissions::from_mode(0o755)).unwrap();
    let g = g.to_string_lossy();
    for (clave, valor) in [("core.fsmonitor", &*g), ("diff.external", &*g), ("diff.x.textconv", &*g)] {
        git(&wt, &["config", clave, valor], None);
    }
    std::fs::write(wt.join(".git/info/attributes"), "a.txt diff=x\n").unwrap();
    let o = ea(d.path(), &["trabajo", "diff", &id]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(!marca.exists(), "git ejecutó lo que configuró el repo: {}", std::fs::read_to_string(&marca).unwrap_or_default());
    assert_eq!(o.stdout, limpio.stdout);
}
