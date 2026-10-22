//! Worktree propio y presencia (spec 019, F2). Repos git y estado en
//! directorios temporales.

use std::path::{Path, PathBuf};
use std::process::Command;

use ea_bus::{Bus, Modo, Orden, Presencia, RolEquipo, Topes, CADUCIDAD_LATIDO_S};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

fn git(dir: &Path, args: &[&str]) -> String {
    let s = Command::new("git")
        .args(["-c", "user.email=a@b", "-c", "user.name=x", "-C"])
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(s.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&s.stderr));
    String::from_utf8_lossy(&s.stdout).trim().to_string()
}

fn repo(d: &Path, commit: bool) -> PathBuf {
    let r = d.join("repo");
    std::fs::create_dir_all(&r).unwrap();
    git(&r, &["init", "-q", "-b", "main"]);
    std::fs::write(r.join("a.txt"), "hola\n").unwrap();
    if commit {
        git(&r, &["add", "."]);
        git(&r, &["commit", "-q", "-m", "inicial"]);
    }
    r
}

fn orden(wt: &Path, propio: bool) -> Orden {
    Orden {
        id: String::new(),
        modo: Modo::Mision,
        titulo: "prueba".into(),
        worktree: wt.to_string_lossy().into(),
        objetivo: "haz algo".into(),
        criterio_exito: Some("true".into()),
        fases: vec![],
        revisar_cada: None,
        topes: Topes::default(),
        rutina: None,
        notas_revision: vec![],
        skills: vec![],
        equipo: None,
        asignado_a: None,
        revisor: None,
        worktree_propio: propio,
        creada_en: String::new(),
        creada_por: "claude-code".into(),
    }
}

fn bus(d: &Path) -> Bus {
    Bus::abrir_en(&d.join("estado")).unwrap()
}

#[test]
fn sin_worktree_propio_devuelve_la_ruta_de_la_orden() {
    let d = tempfile::tempdir().unwrap();
    let b = bus(d.path());
    // ni siquiera hace falta que sea un repo
    assert_eq!(b.preparar_worktree("t1", &orden(d.path(), false)).unwrap(), d.path());
    assert!(!d.path().join("estado/worktrees").exists());
}

#[test]
fn el_worktree_propio_crea_rama_y_directorio_y_se_reutiliza() {
    let d = tempfile::tempdir().unwrap();
    let r = repo(d.path(), true);
    let b = bus(d.path());
    let o = orden(&r, true);
    let ruta = b.preparar_worktree("t1", &o).unwrap();
    assert_eq!(ruta, b.raiz().join("worktrees/t1"));
    assert!(ruta.join("a.txt").exists());
    assert_eq!(git(&ruta, &["rev-parse", "--abbrev-ref", "HEAD"]), "ea/t1");
    assert_eq!(git(&ruta, &["rev-parse", "HEAD"]), git(&r, &["rev-parse", "HEAD"]));
    // un cambio en el worktree propio no toca el repo
    std::fs::write(ruta.join("nuevo.txt"), "x").unwrap();
    assert!(!r.join("nuevo.txt").exists());
    // segunda llamada: el mismo directorio, con lo que había dentro
    assert_eq!(b.preparar_worktree("t1", &o).unwrap(), ruta);
    assert!(ruta.join("nuevo.txt").exists());
    // otro trabajo del mismo repo: otro worktree y otra rama
    let otra = b.preparar_worktree("t2", &o).unwrap();
    assert_ne!(otra, ruta);
    assert_eq!(git(&otra, &["rev-parse", "--abbrev-ref", "HEAD"]), "ea/t2");
}

#[test]
fn con_la_rama_pero_sin_el_directorio_la_vuelve_a_sacar_sin_pisarla() {
    let d = tempfile::tempdir().unwrap();
    let r = repo(d.path(), true);
    let b = bus(d.path());
    let o = orden(&r, true);
    let ruta = b.preparar_worktree("t1", &o).unwrap();
    // trabajo de la rama: un commit propio
    std::fs::write(ruta.join("hecho.txt"), "x").unwrap();
    git(&ruta, &["add", "."]);
    git(&ruta, &["commit", "-q", "-m", "trabajo"]);
    let punta = git(&ruta, &["rev-parse", "HEAD"]);
    // el directorio se borra a mano (queda registrado en git)
    std::fs::remove_dir_all(&ruta).unwrap();
    // y el repo avanza: la rama no debe recrearse desde HEAD
    std::fs::write(r.join("b.txt"), "b").unwrap();
    git(&r, &["add", "."]);
    git(&r, &["commit", "-q", "-m", "otro"]);
    let de_nuevo = b.preparar_worktree("t1", &o).unwrap();
    assert_eq!(de_nuevo, ruta);
    assert_eq!(git(&de_nuevo, &["rev-parse", "HEAD"]), punta);
    assert!(de_nuevo.join("hecho.txt").exists());
}

#[test]
fn un_repo_sin_commits_o_sin_git_da_error_claro() {
    let d = tempfile::tempdir().unwrap();
    let b = bus(d.path());
    let r = repo(d.path(), false);
    let e = b.preparar_worktree("t1", &orden(&r, true)).unwrap_err().to_string();
    assert!(e.contains("no es un repositorio git con algún commit"), "{e}");
    let plano = d.path().join("plano");
    std::fs::create_dir_all(&plano).unwrap();
    assert!(b.preparar_worktree("t1", &orden(&plano, true)).is_err());
    assert!(b.preparar_worktree("t1", &orden(&d.path().join("no-existe"), true)).is_err());
    assert!(!b.raiz().join("worktrees/t1").exists());
}

#[test]
fn un_id_raro_o_un_directorio_ajeno_se_rechazan() {
    let d = tempfile::tempdir().unwrap();
    let r = repo(d.path(), true);
    let b = bus(d.path());
    let o = orden(&r, true);
    for malo in ["", "../x", "a/b", "-b"] {
        // «-b» es un id válido por caracteres, pero va tras `--` y como rama ea/-b
        if malo == "-b" {
            continue;
        }
        assert!(b.preparar_worktree(malo, &o).is_err(), "{malo:?}");
    }
    // un directorio que no es worktree del repo
    std::fs::create_dir_all(b.raiz().join("worktrees/t9")).unwrap();
    let e = b.preparar_worktree("t9", &o).unwrap_err().to_string();
    assert!(e.contains("no es un worktree"), "{e}");
}

#[test]
fn limpiar_quita_el_worktree_limpio_y_deja_la_rama() {
    let d = tempfile::tempdir().unwrap();
    let r = repo(d.path(), true);
    let b = bus(d.path());
    assert!(!b.limpiar_worktree("t1").unwrap());
    let ruta = b.preparar_worktree("t1", &orden(&r, true)).unwrap();
    // con cambios, git se niega y no se pierde nada
    std::fs::write(ruta.join("sucio.txt"), "x").unwrap();
    assert!(b.limpiar_worktree("t1").is_err());
    assert!(ruta.join("sucio.txt").exists());
    std::fs::remove_file(ruta.join("sucio.txt")).unwrap();
    assert!(b.limpiar_worktree("t1").unwrap());
    assert!(!ruta.exists());
    assert_eq!(git(&r, &["branch", "--list", "ea/t1"]).trim(), "ea/t1");
}

// --- presencia ---------------------------------------------------------------

fn presencia(agente: &str, pid: u32) -> Presencia {
    Presencia::nueva(agente, RolEquipo::Ejecutor, Some("eq1"), pid)
}

#[test]
fn la_presencia_se_anota_late_y_se_quita() {
    let d = tempfile::tempdir().unwrap();
    let b = bus(d.path());
    assert!(b.presencias().is_empty());
    assert_eq!(b.presencia_de("codex").unwrap(), None);
    b.anotar_presencia(&presencia("codex", std::process::id())).unwrap();
    b.anotar_presencia(&presencia("antigravity", std::process::id())).unwrap();
    b.latir("codex", Some("t1")).unwrap();
    let p = b.presencia_de("codex").unwrap().unwrap();
    assert_eq!((p.trabajo.as_deref(), p.equipo.as_deref(), p.rol), (Some("t1"), Some("eq1"), RolEquipo::Ejecutor));
    b.latir("codex", None).unwrap();
    assert_eq!(b.presencia_de("codex").unwrap().unwrap().trabajo, None);
    let todas = b.presencias();
    assert_eq!(todas.iter().map(|(p, v)| (p.agente.as_str(), *v)).collect::<Vec<_>>(), vec![("antigravity", true), ("codex", true)]);
    b.quitar_presencia("codex").unwrap();
    b.quitar_presencia("codex").unwrap(); // idempotente
    assert_eq!(b.presencias().len(), 1);
    assert!(b.latir("codex", None).is_err());
}

#[test]
fn la_presencia_caduca_por_pid_muerto_o_latido_viejo() {
    let d = tempfile::tempdir().unwrap();
    let b = bus(d.path());
    b.anotar_presencia(&presencia("codex", std::process::id())).unwrap();
    b.anotar_presencia(&presencia("qwen-code", u32::MAX - 1)).unwrap(); // pid inexistente
    let mut vieja = presencia("opencode", std::process::id());
    vieja.latido = (OffsetDateTime::now_utc() - Duration::seconds(CADUCIDAD_LATIDO_S + 5)).format(&Rfc3339).unwrap();
    b.anotar_presencia(&vieja).unwrap();
    let mut ilegible = presencia("antigravity", std::process::id());
    ilegible.latido = "ayer".into();
    b.anotar_presencia(&ilegible).unwrap();
    let vivas: Vec<(String, bool)> = b.presencias().into_iter().map(|(p, v)| (p.agente, v)).collect();
    assert_eq!(
        vivas,
        vec![("antigravity".into(), false), ("codex".into(), true), ("opencode".into(), false), ("qwen-code".into(), false)]
    );
    // sin dormir: con la hora corrida, la viva caduca
    let luego = OffsetDateTime::now_utc() + Duration::seconds(CADUCIDAD_LATIDO_S + 5);
    assert!(b.presencias_en(luego).iter().all(|(_, v)| !v));
    // un latido la devuelve a la vida
    b.latir("opencode", Some("t2")).unwrap();
    assert!(b.presencias().iter().find(|(p, _)| p.agente == "opencode").unwrap().1);
}

#[test]
fn la_presencia_valida_el_agente_y_salta_lo_ajeno() {
    let d = tempfile::tempdir().unwrap();
    let b = bus(d.path());
    assert!(b.anotar_presencia(&presencia("../x", 1)).is_err());
    assert!(b.presencia_de("raro").is_err());
    assert!(b.quitar_presencia("raro").is_err());
    // una carpeta ajena o un JSON roto no rompen el listado
    std::fs::create_dir_all(b.raiz().join("agentes/raro")).unwrap();
    std::fs::write(b.raiz().join("agentes/raro/presencia.json"), "{").unwrap();
    std::fs::create_dir_all(b.raiz().join("agentes/codex")).unwrap();
    std::fs::write(b.raiz().join("agentes/codex/presencia.json"), "{").unwrap();
    assert!(b.presencias().is_empty());
}

#[test]
fn listar_worktrees_ve_rama_y_distingue_limpio_de_sucio_y_limpiar_no_fuerza() {
    let d = tempfile::tempdir().unwrap();
    let r = repo(d.path(), true);
    let b = bus(d.path());
    assert!(b.listar_worktrees().unwrap().is_empty());
    assert!(b.info_worktree("t1").unwrap().is_none());
    let id = b.crear_trabajo(orden(&r, true)).unwrap().to_string();
    let ruta = b.preparar_worktree(&id, &orden(&r, true)).unwrap();
    let v = b.listar_worktrees().unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].trabajo, id);
    assert_eq!(v[0].rama, format!("ea/{id}"));
    assert_eq!(v[0].limpio, Some(true));
    assert_eq!(v[0].estado_trabajo, Some("en_cola"));
    assert!(!v[0].ocupado && !v[0].trabajo_terminado());
    // un fichero nuevo lo ensucia, y git se niega a quitarlo sin --force
    std::fs::write(ruta.join("nuevo.txt"), "x").unwrap();
    assert_eq!(b.info_worktree(&id).unwrap().unwrap().limpio, Some(false));
    assert!(b.limpiar_worktree(&id).is_err());
    assert!(ruta.exists());
    // limpio otra vez: se quita el directorio y la rama se conserva
    std::fs::remove_file(ruta.join("nuevo.txt")).unwrap();
    assert!(b.limpiar_worktree(&id).unwrap());
    assert!(!ruta.exists());
    assert!(b.listar_worktrees().unwrap().is_empty());
    git(&r, &["rev-parse", "--verify", &format!("refs/heads/ea/{id}")]);
}
