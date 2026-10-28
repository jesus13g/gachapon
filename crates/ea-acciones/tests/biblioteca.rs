//! Las herramientas `ea_activar_herramienta` y `ea_desactivar_herramienta`
//! (spec 018, F4) con el sistema simulado: raíces temporales y un doble para
//! los comandos oficiales. Nunca se toca el HOME real.

use std::fs;
use std::path::{Path, PathBuf};

use ea_acciones::*;
use ea_ajustes::{Canal as CanalAjustes, Fuentes};
use ea_biblioteca::{Contexto, Raices};
use ea_pruebas::escribe;
use ea_store::{EstadoEjecucion, EventFilter, Store};
use serde_json::{json, Value};

const CODEX: &str = "# config\n[mcp_servers.docs]\ncommand = \"docs-mcp\"  # cmd\n\n[mcp_servers.otro]\ncommand = \"o\"\n";

struct Mundo {
    _d: tempfile::TempDir,
    home: PathBuf,
    sis: SistemaSimulado,
    store: Store,
}

fn mundo() -> Mundo {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    escribe(home.join(".codex/config.toml"), CODEX);
    escribe(home.join(".claude/skills/propia/SKILL.md"), "---\ndescription: Propia\n---\n");
    escribe(
        home.join(".claude/plugins/installed_plugins.json"),
        r#"{"version":2,"plugins":{"foo@mkt":[{"scope":"user","installPath":"/no/existe","version":"1"}]}}"#,
    );
    escribe(home.join(".claude/settings.json"), r#"{"enabledPlugins":{"foo@mkt":true}}"#);
    let raices = Raices::nuevo(home.clone(), None, d.path().join("estado"), None);
    let ctx = Contexto::nuevo(raices, Fuentes::aislada(d.path()), CanalAjustes::Terminal);
    let store = Store::open_at(&d.path().join("t.db")).unwrap();
    Mundo { home, sis: SistemaSimulado::default().con_biblioteca(ctx), store, _d: d }
}

fn lee(r: &Path) -> String {
    fs::read_to_string(r).unwrap()
}

async fn proponer_de(m: &Mundo, h: &str, id: &str) -> ea_store::Propuesta {
    let args = json!({ "elemento": id });
    let hechos = observar(&m.sis, h, &args).await.unwrap();
    let (p, nueva) = proponer(&m.store, &m.sis, &hechos, "claude-code", None, h, &args).unwrap();
    assert!(nueva);
    p
}

fn acciones(s: &Store) -> Vec<String> {
    let mut v: Vec<_> = s
        .list_events(&EventFilter { limit: Some(500), ..Default::default() })
        .unwrap()
        .into_iter()
        .map(|e| e.action)
        .collect();
    v.reverse();
    v
}

#[tokio::test]
async fn proponer_no_cambia_nada_y_aceptar_aplica_con_antes_y_despues_en_el_diario() {
    let mut m = mundo();
    let f = m.home.join(".codex/config.toml");
    let p = proponer_de(&m, "ea_desactivar_herramienta", "codex/mcp/usuario/docs").await;
    assert_eq!(lee(&f), CODEX, "proponer solo simula");
    let a = p.accion.as_ref().unwrap();
    assert_eq!(a.riesgo, "reversible");
    assert!(a.simulacion.contains("codex/mcp/usuario/docs") && a.simulacion.contains("config.toml") && a.simulacion.contains("false"), "{}", a.simulacion);
    assert_eq!(a.argumentos, json!({ "elemento": "codex/mcp/usuario/docs" }));
    // La precondición lleva el cambio entero: fichero, antes y después.
    assert_eq!(a.precondicion["cambio"]["mecanismo"], "campo");
    assert_eq!(a.precondicion["cambio"]["antes"], Value::Null);
    assert_eq!(a.precondicion["cambio"]["despues"], false);

    let aceptada = aceptar_propuesta(&mut m.store, &m.sis, p.id, "usuario", Canal::Web).await.unwrap();
    assert_eq!(aceptada.ejecucion.as_ref().map(|e| e.estado), Some(EstadoEjecucion::Ejecutada), "{aceptada:?}");
    assert!(lee(&f).contains("[mcp_servers.docs]\ncommand = \"docs-mcp\"  # cmd\nenabled = false\n"), "{}", lee(&f));
    assert_eq!(m.sis.mutaciones(), ["biblioteca:desactivar:codex/mcp/usuario/docs"]);

    // El diario: el evento de la acción y el de la biblioteca llevan antes y después.
    let eventos = m.store.list_events(&EventFilter { limit: Some(100), ..Default::default() }).unwrap();
    let ejecutada = eventos.iter().find(|e| e.action == "accion_ejecutada").expect("accion_ejecutada");
    let d: Value = serde_json::from_str(ejecutada.detail.as_deref().unwrap()).unwrap();
    assert_eq!(d["resultado"]["antes"], Value::Null);
    assert_eq!(d["resultado"]["despues"], false);
    assert_eq!(d["resultado"]["mecanismo"], "campo");
    assert!(d["resultado"]["copia"].as_str().unwrap().contains("biblioteca/copias"));
    let cambio = eventos.iter().find(|e| e.action == "biblioteca_cambio").expect("biblioteca_cambio");
    let d: Value = serde_json::from_str(cambio.detail.as_deref().unwrap()).unwrap();
    assert_eq!((d["antes"].clone(), d["despues"].clone(), d["elemento"].clone()), (Value::Null, json!(false), json!("codex/mcp/usuario/docs")));
    assert!(acciones(&m.store).iter().any(|a| a == "propuesta_encolada"));

    // Activar lo deshace y el fichero queda como estaba.
    let p = proponer_de(&m, "ea_activar_herramienta", "codex/mcp/usuario/docs").await;
    aceptar_propuesta(&mut m.store, &m.sis, p.id, "usuario", Canal::Web).await.unwrap();
    assert_eq!(lee(&f), CODEX);
}

#[tokio::test]
async fn la_revalidacion_al_aceptar_rechaza_si_el_fichero_cambio() {
    let mut m = mundo();
    let f = m.home.join(".codex/config.toml");
    let p = proponer_de(&m, "ea_desactivar_herramienta", "codex/mcp/usuario/docs").await;
    let editado = format!("{CODEX}# alguien editó\n");
    fs::write(&f, &editado).unwrap();
    let r = aceptar_propuesta(&mut m.store, &m.sis, p.id, "usuario", Canal::Web).await.unwrap();
    let ej = r.ejecucion.as_ref().unwrap();
    assert_eq!(ej.estado, EstadoEjecucion::Fallida);
    assert!(ej.detalle.as_deref().unwrap_or_default().contains("cambió"), "{r:?}");
    assert_eq!(lee(&f), editado, "no se escribió nada");
    assert!(m.sis.mutaciones().is_empty());
    assert!(acciones(&m.store).contains(&"accion_fallida".to_string()));
}

#[tokio::test]
async fn comando_oficial_mover_y_rechazos_en_la_simulacion() {
    let mut m = mundo();
    // Comando oficial con el doble.
    let p = proponer_de(&m, "ea_desactivar_herramienta", "claude-code/plugin/usuario/foo@mkt").await;
    aceptar_propuesta(&mut m.store, &m.sis, p.id, "usuario", Canal::Terminal).await.unwrap();
    assert_eq!(m.sis.ordenes.argvs(), vec![["claude", "plugin", "disable", "--scope", "user", "foo@mkt"].map(String::from).to_vec()]);
    // Mover.
    let propia = m.home.join(".claude/skills/propia");
    let p = proponer_de(&m, "ea_desactivar_herramienta", "claude-code/skill/usuario/propia").await;
    aceptar_propuesta(&mut m.store, &m.sis, p.id, "usuario", Canal::Web).await.unwrap();
    assert!(!propia.exists());
    let p = proponer_de(&m, "ea_activar_herramienta", "claude-code/skill/usuario/propia").await;
    aceptar_propuesta(&mut m.store, &m.sis, p.id, "usuario", Canal::Web).await.unwrap();
    assert!(propia.join("SKILL.md").exists());
    // Rechazos al simular: ya activado, no existe, id con mala forma, argumento de más.
    for (h, args) in [
        ("ea_activar_herramienta", json!({ "elemento": "codex/mcp/usuario/docs" })),
        ("ea_desactivar_herramienta", json!({ "elemento": "codex/mcp/usuario/no-existe" })),
        ("ea_desactivar_herramienta", json!({ "elemento": "../../etc/passwd" })),
        ("ea_desactivar_herramienta", json!({ "elemento": "codex/mcp/usuario/docs", "extra": 1 })),
    ] {
        let hechos = observar(&m.sis, h, &args).await.unwrap();
        assert!(simular_con(&m.store, &m.sis, &hechos, h, &args).is_err(), "{args}");
    }
    // Sin el binario, el comando oficial se rechaza al simular.
    let sin = SistemaSimulado { ordenes: ea_biblioteca::OrdenesDoble::default().solo_con(&["agy"]), ..SistemaSimulado::default() };
    let ctx = m.sis.biblioteca.lock().unwrap().clone().unwrap();
    let sin = sin.con_biblioteca(ctx);
    let args = json!({ "elemento": "claude-code/plugin/usuario/foo@mkt" });
    let hechos = observar(&sin, "ea_activar_herramienta", &args).await.unwrap();
    // (ya está desactivado por el doble? no: el doble no toca ficheros) — pide desactivar.
    let e = simular_con(&m.store, &sin, &hechos, "ea_desactivar_herramienta", &args).unwrap_err();
    assert!(e.to_string().contains("binario no disponible"), "{e}");
}

#[test]
fn el_catalogo_las_declara_reversibles_supervisadas_y_fuera_del_modelo_local() {
    for n in ["ea_activar_herramienta", "ea_desactivar_herramienta"] {
        let h = buscar(n).unwrap();
        assert_eq!((h.riesgo, h.ejecucion, h.local), (Riesgo::Reversible, Ejecucion::Supervisada, false));
        // El modelo local (el ejecutor) y lo desatendido no las ven.
        assert!(autorizar_mcp("modelo-local", h).is_err(), "{n}");
        assert!(autorizar_mcp("orquestador", h).is_err(), "{n}");
        assert!(autorizar_mcp("sin-rol", h).is_err(), "{n}");
        assert!(autorizar_mcp("claude-code", h).is_ok(), "{n}");
        assert_eq!(campos(n).len(), 1);
    }
}

/// `SoloConsulta` (lo que reciben `ea-mcp` y el orquestador) solo simula y
/// propone: no implementa `Sistema`, así que no hay forma de aplicar. La
/// biblioteca mira un HOME temporal, con el canal más restrictivo (`Web`).
#[tokio::test]
async fn solo_consulta_simula_y_propone_pero_no_ejecuta() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::write(home.join(".codex/config.toml"), CODEX).unwrap();
    let rutinas = d.path().join("rutinas");
    fs::create_dir_all(&rutinas).unwrap();
    let boveda = ea_boveda::Boveda::en(d.path().join("boveda"));
    let mut consulta = SoloConsulta::usuario(rutinas, boveda, d.path().join("estado"));
    let raices = Raices::nuevo(home.clone(), None, d.path().join("estado"), Some(d.path().join("rt")));
    consulta.biblioteca = Some(Contexto::nuevo(raices, Fuentes::aislada(d.path()), CanalAjustes::Web));
    let store = Store::open_at(&d.path().join("t.db")).unwrap();
    let args = json!({ "elemento": "codex/mcp/usuario/docs" });
    let h = "ea_desactivar_herramienta";
    let hechos = observar(&consulta, h, &args).await.unwrap();
    let sim = simular_con(&store, &consulta, &hechos, h, &args).unwrap();
    assert!(sim.texto.contains("config.toml"), "{}", sim.texto);
    let (p, _) = proponer(&store, &consulta, &hechos, "claude-code", None, h, &args).unwrap();
    assert_eq!(p.estado.as_str(), "pendiente");
    assert_eq!(fs::read_to_string(home.join(".codex/config.toml")).unwrap(), CODEX, "nada cambió");
    assert!(!acciones(&store).iter().any(|a| a == "accion_ejecutada" || a == "biblioteca_cambio"));
}

/// El canal de ajustes es el de la CONFIRMACIÓN, no el del TTY del proceso: un
/// `ea-web` lanzado desde una terminal no puede cambiar las skills del ejecutor.
#[tokio::test]
async fn las_skills_del_ejecutor_solo_cambian_si_confirma_la_terminal_y_no_la_web() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    let skills = d.path().join("qwen-skills");
    fs::create_dir_all(&skills).unwrap();
    for n in ["rust", "tests"] {
        fs::write(skills.join(format!("{n}.md")), "---\ndescripcion: x\n---\n").unwrap();
    }
    fs::create_dir_all(&home).unwrap();
    let fuentes = Fuentes::aislada(d.path());
    fs::create_dir_all(fuentes.fichero().parent().unwrap()).unwrap();
    fs::write(fuentes.fichero(), format!("[agente.skills]\ndir = \"{}\"\n", skills.display().to_string().replace('\\', "/"))).unwrap();
    let original = lee(&fuentes.fichero());
    let raices = Raices::nuevo(home, None, d.path().join("estado"), None);
    // El contexto dice «terminal», como si el proceso tuviera TTY.
    let ctx = Contexto::nuevo(raices, fuentes.clone(), CanalAjustes::Terminal);
    let sis = SistemaSimulado::default().con_biblioteca(ctx);
    let mut store = Store::open_at(&d.path().join("t.db")).unwrap();
    let args = json!({ "elemento": "qwen-code/skill/ecosistema/tests" });
    let h = "ea_desactivar_herramienta";

    let hechos = observar(&sis, h, &args).await.unwrap();
    let (p, _) = proponer(&store, &sis, &hechos, "claude-code", None, h, &args).unwrap();
    let r = aceptar_propuesta(&mut store, &sis, p.id, "usuario", Canal::Web).await.unwrap();
    assert_eq!(r.ejecucion.as_ref().unwrap().estado, EstadoEjecucion::Fallida, "{r:?}");
    assert_eq!(lee(&fuentes.fichero()), original, "la web no escribió ajustes");
    assert!(sis.mutaciones().is_empty());

    // Otra propuesta (la anterior ya se resolvió): la terminal sí.
    let hechos = observar(&sis, h, &args).await.unwrap();
    let (p, _) = proponer(&store, &sis, &hechos, "claude-code", None, h, &args).unwrap();
    let r = aceptar_propuesta(&mut store, &sis, p.id, "usuario", Canal::Terminal).await.unwrap();
    assert_eq!(r.ejecucion.as_ref().unwrap().estado, EstadoEjecucion::Ejecutada, "{r:?}");
    assert!(lee(&fuentes.fichero()).contains("habilitadas"));
}
