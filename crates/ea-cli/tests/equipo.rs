//! `ea equipo` y las opciones de equipo de `ea trabajo crear`, con el binario
//! real y todo el estado en directorios temporales. Despertar y dormir exigen
//! una terminal interactiva: aquí no la hay, así que se prueba que se
//! rechazan sin tocar nada.

use std::path::Path;
use std::process::Output;

/// Un directorio absoluto que existe en cada sistema (`/tmp` no es absoluta en Windows).
#[cfg(windows)]
macro_rules! tmp {
    () => {
        "C:/Windows"
    };
}
#[cfg(not(windows))]
macro_rules! tmp {
    () => {
        "/tmp"
    };
}

use ea_ajustes::{Ajustes, Fuentes};
use ea_bus::{Bus, Miembro, NuevoEquipo, Presencia, RolEquipo};

fn ea(dir: &Path, args: &[&str]) -> Output {
    ea_pruebas::cli::ea(env!("CARGO_BIN_EXE_ea"), dir).args(args).output().expect("lanzar ea")
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn m(agente: &str, rol: RolEquipo) -> Miembro {
    Miembro { agente: agente.into(), rol }
}

/// Un equipo despierto (claude-code planifica, qwen-code ejecuta) en el bus de `dir`.
fn equipo(dir: &Path) -> String {
    let bus = Bus::abrir_en(&dir.join("estado")).unwrap();
    let ajustes = Ajustes::por_defecto(&Fuentes::aislada(&dir.join("config")));
    let e = bus
        .crear_equipo(
            NuevoEquipo {
                titulo: "equipo CLI".into(),
                objetivo: "probar la CLI".into(),
                worktree_base: dir.to_string_lossy().into(),
                miembros: vec![m("claude-code", RolEquipo::Planificador), m("qwen-code", RolEquipo::Ejecutor)],
                creado_por: "test".into(),
            },
            &ajustes,
        )
        .unwrap();
    bus.anotar_presencia(&Presencia::nueva("claude-code", RolEquipo::Planificador, Some(&e.id), std::process::id())).unwrap();
    e.id
}

#[test]
fn despertar_y_dormir_sin_terminal_se_rechazan_sin_tocar_nada() {
    let d = tempfile::tempdir().unwrap();
    let o = ea(d.path(), &["equipo", "despertar", "--todos", "--titulo", "t", "--objetivo", "o", "--worktree", tmp!()]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("terminal interactiva"), "{}", stderr(&o));
    let o = ea(d.path(), &["equipo", "despertar", "--miembro", "codex:ejecutor", "--titulo", "t", "--objetivo", "o", "--worktree", tmp!(), "--sin-terminal"]);
    assert!(!o.status.success() && stderr(&o).contains("terminal interactiva"), "{}", stderr(&o));
    assert!(!d.path().join("estado/equipos").exists(), "no se creó ningún equipo");

    let id = equipo(d.path());
    let o = ea(d.path(), &["equipo", "dormir", &id]);
    assert!(!o.status.success() && stderr(&o).contains("terminal interactiva"), "{}", stderr(&o));
    let o = ea(d.path(), &["equipo", "ver", &id, "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["estado"], "despierto", "sigue despierto");
}

#[test]
fn despertar_pide_todos_o_miembros_pero_no_las_dos_cosas() {
    let d = tempfile::tempdir().unwrap();
    let base = ["equipo", "despertar", "--titulo", "t", "--objetivo", "o", "--worktree", tmp!()];
    let o = ea(d.path(), &base);
    assert!(!o.status.success() && stderr(&o).contains("--todos"), "{}", stderr(&o));
    let mut args = base.to_vec();
    args.extend(["--todos", "--miembro", "codex:ejecutor"]);
    let o = ea(d.path(), &args);
    assert!(!o.status.success() && stderr(&o).contains("cannot be used"), "{}", stderr(&o));
}

#[test]
fn listar_y_ver_en_json_traen_miembros_con_presencia() {
    let d = tempfile::tempdir().unwrap();
    let vacio = ea(d.path(), &["equipo", "listar", "--json"]);
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&vacio.stdout).unwrap(), serde_json::json!([]));

    let id = equipo(d.path());
    let o = ea(d.path(), &["equipo", "listar", "--json"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v[0]["id"], id.as_str());
    let plan = v[0]["miembros"].as_array().unwrap().iter().find(|m| m["agente"] == "claude-code").unwrap();
    assert_eq!(plan["viva"], true);

    let o = ea(d.path(), &["equipo", "ver", &id, "--json"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["objetivo"], "probar la CLI");
    let ejecutor = v["miembros"].as_array().unwrap().iter().find(|m| m["agente"] == "qwen-code").unwrap();
    assert_eq!(ejecutor["viva"], false);
    assert!(ejecutor["presencia"].is_null());

    let texto = String::from_utf8_lossy(&ea(d.path(), &["equipo", "ver", &id]).stdout).into_owned();
    assert!(texto.contains("despierto, libre") && texto.contains("sin presencia"), "{texto}");
    assert!(!ea(d.path(), &["equipo", "ver", "no-existe"]).status.success());
}

#[test]
fn trabajo_crear_con_equipo_asigna_revisa_y_pide_worktree_propio() {
    let d = tempfile::tempdir().unwrap();
    let id = equipo(d.path());
    // `ea trabajo crear` exige, como la web, un repo git con algún commit.
    let repo = ea_pruebas::repo_git(&d.path().join("repo"), true);
    let wt = repo.to_string_lossy().into_owned();
    let base = ["trabajo", "crear", "--titulo", "tarea", "--worktree", &wt, "--objetivo", "haz algo", "--criterio", "true"];
    // asignar sin equipo lo rechaza clap
    let mut args = base.to_vec();
    args.extend(["--asignar", "qwen-code"]);
    assert!(!ea(d.path(), &args).status.success());

    let mut args = base.to_vec();
    args.extend(["--equipo", &id, "--asignar", "qwen-code", "--revisor", "claude-code", "--worktree-propio"]);
    let o = ea(d.path(), &args);
    assert!(o.status.success(), "{}", stderr(&o));
    let trabajo = String::from_utf8_lossy(&o.stdout).trim().to_string();
    let v: serde_json::Value = serde_json::from_slice(&ea(d.path(), &["trabajo", "ver", &trabajo, "--json"]).stdout).unwrap();
    assert_eq!(v["orden"]["equipo"], id.as_str());
    assert_eq!(v["orden"]["asignado_a"], "qwen-code");
    assert_eq!(v["orden"]["revisor"], "claude-code");
    assert_eq!(v["orden"]["worktree_propio"], true);
    // el equipo lo lista, asignado, y la cola del agente lo tiene
    let ver: serde_json::Value = serde_json::from_slice(&ea(d.path(), &["equipo", "ver", &id, "--json"]).stdout).unwrap();
    assert_eq!(ver["trabajos"][0]["id"], trabajo.as_str());
    assert_eq!(ver["trabajos"][0]["asignado_a"], "qwen-code");
    let ejecutor = ver["miembros"].as_array().unwrap().iter().find(|m| m["agente"] == "qwen-code").unwrap();
    assert_eq!(ejecutor["cola"], serde_json::json!([trabajo]));

    // un asignado que no es del equipo se rechaza
    let mut args = base.to_vec();
    args.extend(["--equipo", &id, "--asignar", "codex"]);
    let o = ea(d.path(), &args);
    assert!(!o.status.success() && stderr(&o).contains("no es un ejecutor del equipo"), "{}", stderr(&o));
}

#[test]
fn status_y_agentes_muestran_la_presencia() {
    let d = tempfile::tempdir().unwrap();
    let id = equipo(d.path());
    let o = ea(d.path(), &["status"]);
    let t = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(t.contains(&format!("Equipo {id}")) && t.contains("claude-code (planificador)") && t.contains("despierto, libre"), "{t}");
    let o = ea(d.path(), &["agentes", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let claude = v.as_array().unwrap().iter().find(|a| a["id"] == "claude-code").unwrap();
    assert_eq!(claude["presencia"]["viva"], true);
    assert_eq!(claude["presencia"]["equipo"], id.as_str());
    let qwen = v.as_array().unwrap().iter().find(|a| a["id"] == "qwen-code").unwrap();
    assert!(qwen["presencia"].is_null());
}
