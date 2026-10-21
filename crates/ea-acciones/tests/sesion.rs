//! `ea_lanzar_sesion` (spec 018, F5): simular, proponer, revalidar y aceptar
//! con el sistema simulado y la terminal de mentira. Un HOME y un runtime de
//! `tempfile`; nunca una ventana de verdad ni el HOME del usuario.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use ea_acciones::*;
use ea_ajustes::{Ajustes, Fuentes};
use ea_biblioteca::{Biblioteca, Raices};
use ea_store::{EstadoEjecucion, EstadoPropuesta, Store};
use serde_json::{json, Value};

const SECRETOS: &[&str] = &["SECRETO_ENV", "SECRETO_HEADER", "SECRETO_RUTA"];

struct Entorno {
    _d: tempfile::TempDir,
    store: Store,
    sis: SistemaSimulado,
    runtime: std::path::PathBuf,
    home: std::path::PathBuf,
}

fn escribe(ruta: &Path, texto: &str) {
    fs::create_dir_all(ruta.parent().unwrap()).unwrap();
    fs::write(ruta, texto).unwrap();
}

fn entorno() -> Entorno {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    let runtime = d.path().join("rt");
    fs::create_dir_all(&runtime).unwrap();
    escribe(&home.join(".claude/skills/propia/SKILL.md"), "---\ndescription: Hace cosas\n---\n");
    escribe(
        &home.join(".claude.json"),
        r#"{"mcpServers":{
            "con-secretos":{"type":"stdio","command":"npx","args":["-y","pkg"],"env":{"API_KEY":"SECRETO_ENV"}},
            "remoto":{"type":"http","url":"https://m.ejemplo.com/SECRETO_RUTA/mcp","headers":{"Authorization":"Bearer SECRETO_HEADER"}}}}"#,
    );
    let raices = Raices::nuevo(home.clone(), None, home.join(".local/state/ea"), Some(runtime.clone()));
    let inv = Biblioteca::cargar(&raices, &Ajustes::por_defecto(&Fuentes::aislada(d.path())));
    let store = Store::open_at(&d.path().join("t.db")).unwrap();
    let sis = SistemaSimulado::default().con_biblioteca(inv, raices);
    Entorno { store, sis, runtime, home, _d: d }
}

const MCP: &str = "claude-code/mcp/usuario/con-secretos";
const REMOTO: &str = "claude-code/mcp/usuario/remoto";
const SKILL: &str = "claude-code/skill/usuario/propia";

async fn simular<C: Consulta>(s: &Store, c: &C, args: &Value) -> Result<Simulacion, ErrorAccion> {
    let h = observar(c, "ea_lanzar_sesion", args).await?;
    simular_con(s, c, &h, "ea_lanzar_sesion", args)
}

fn diario(s: &Store) -> String {
    s.list_events(&Default::default()).unwrap().iter().map(|e| format!("{} {} {:?}", e.actor, e.action, e.detail)).collect::<Vec<_>>().join("\n")
}

fn sin_secretos(t: &str) {
    for s in SECRETOS {
        assert!(!t.contains(s), "se coló {s} en: {t}");
    }
}

#[test]
fn la_herramienta_es_supervisada_reversible_y_no_es_del_ejecutor() {
    let h = buscar("ea_lanzar_sesion").unwrap();
    assert_eq!((h.riesgo, h.ejecucion, h.local), (Riesgo::Reversible, Ejecucion::Supervisada, false));
    assert!(autorizar_mcp("modelo-local", h).is_err(), "el ejecutor no la ve");
    assert!(autorizar_mcp("orquestador", h).is_err());
    assert!(autorizar_mcp("claude-code", h).is_ok(), "el planificador solo propone");
    // La web puede confirmarla (es reversible), pero no es una lectura.
    assert!(Canal::Web.puede_confirmar(h.riesgo).is_ok());
}

#[test]
fn los_argumentos_se_validan_sin_mirar_el_mundo() {
    let ok = |a: Value| validar_argumentos("ea_lanzar_sesion", &a);
    ok(json!({ "agente": "claude-code", "herramientas": [MCP] })).unwrap();
    ok(json!({ "agente": "codex", "herramientas": format!("{MCP}\n{SKILL}, otro"), "directorio": "/tmp/x" })).unwrap();
    assert!(ok(json!({ "agente": "claude-code", "herramientas": [] })).is_err());
    assert!(ok(json!({ "agente": "claude-code", "herramientas": "  " })).is_err());
    assert!(ok(json!({ "agente": "emacs", "herramientas": [MCP] })).is_err());
    assert!(ok(json!({ "agente": "claude-code", "herramientas": [MCP], "extra": 1 })).is_err());
    assert!(ok(json!({ "agente": "claude-code", "herramientas": ["a\nb\u{7}"] })).is_err());
    assert!(ok(json!({ "agente": "claude-code", "herramientas": (0..41).map(|i| format!("x{i}")).collect::<Vec<_>>() })).is_err());
    assert!(ok(json!({ "agente": "claude-code", "herramientas": [MCP], "directorio": "/a\u{0}b" })).is_err());
}

#[tokio::test]
async fn simular_describe_el_plan_sin_secretos_y_no_escribe_nada() {
    let e = entorno();
    let sim = simular(&e.store, &e.sis, &json!({ "agente": "claude-code", "herramientas": [SKILL, MCP, REMOTO, MCP] })).await.unwrap();
    assert_eq!(sim.riesgo, Riesgo::Reversible);
    // Normalizados: ordenados y sin repetir.
    assert_eq!(sim.argumentos, json!({ "agente": "claude-code", "herramientas": [MCP, REMOTO, SKILL] }));
    for trozo in ["--strict-mcp-config", "--mcp-config", "mcp.json", "ajustes.json", "--plugin-dir", "0600", "complemento/skills/propia"] {
        assert!(sim.texto.contains(trozo), "{trozo}: {}", sim.texto);
    }
    sin_secretos(&sim.texto);
    sin_secretos(&sim.precondicion.to_string());
    assert!(!e.runtime.join("ea-biblioteca").exists(), "simular no escribe");
    assert!(e.sis.terminales().is_empty());
    assert!(e.sis.mutaciones().is_empty());
}

#[tokio::test]
async fn simular_rechaza_antigravity_ids_ajenos_y_ausentes() {
    let e = entorno();
    let falla = |a: Value| {
        let (s, c) = (&e.store, &e.sis);
        async move { simular(s, c, &a).await.unwrap_err().to_string() }
    };
    assert!(falla(json!({ "agente": "antigravity", "herramientas": [MCP] })).await.contains("no admite sesiones en la v1"));
    assert!(falla(json!({ "agente": "codex", "herramientas": [SKILL] })).await.contains("no tiene sentido"));
    assert!(falla(json!({ "agente": "claude-code", "herramientas": ["claude-code/mcp/usuario/fantasma"] })).await.contains("no existe"));
    assert!(falla(json!({ "agente": "claude-code", "herramientas": ["esto-no-es-un-id"] })).await.contains("inválido"));
}

#[tokio::test]
async fn proponer_no_abre_nada_y_aceptar_materializa_y_abre_la_terminal() {
    let mut e = entorno();
    let args = json!({ "agente": "claude-code", "herramientas": [MCP, REMOTO, SKILL] });
    let h = observar(&e.sis, "ea_lanzar_sesion", &args).await.unwrap();
    let (p, nueva) = proponer(&e.store, &e.sis, &h, "claude-code", None, "ea_lanzar_sesion", &args).unwrap();
    assert!(nueva);
    sin_secretos(&format!("{p:?}"));
    assert!(e.sis.terminales().is_empty(), "proponer no abre nada");
    assert!(!e.runtime.join("ea-biblioteca").exists());

    let p = aceptar_propuesta(&mut e.store, &e.sis, p.id, "usuario", Canal::Web).await.unwrap();
    assert_eq!(p.estado, EstadoPropuesta::Aceptada);
    assert_eq!(p.ejecucion.as_ref().unwrap().estado, EstadoEjecucion::Ejecutada, "{:?}", p.ejecucion);

    // La terminal recibe el argv sin shell: env, binario y flags.
    let lanzadas = e.sis.terminales();
    assert_eq!(lanzadas.len(), 1);
    let argv = &lanzadas[0];
    assert_eq!(argv[0], "claude", "sin proyecto no hay `env`");
    assert!(argv.contains(&"--strict-mcp-config".to_string()));
    let mcp_json = argv[argv.iter().position(|a| a == "--mcp-config").unwrap() + 1].clone();
    sin_secretos(&argv.join(" "));

    // El secreto solo está en el fichero 0600 del runtime.
    let contenido = fs::read_to_string(&mcp_json).unwrap();
    assert!(contenido.contains("SECRETO_ENV") && contenido.contains("SECRETO_HEADER") && contenido.contains("SECRETO_RUTA"));
    assert_eq!(fs::metadata(&mcp_json).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(Path::new(&mcp_json).starts_with(e.runtime.join("ea-biblioteca/sesiones")));
    assert_eq!(e.sis.mutaciones().len(), 1);

    // El diario: agente, ids y argv; ningún secreto.
    let d = diario(&e.store);
    sin_secretos(&d);
    assert!(d.contains("accion_ejecutada") && d.contains("ea_lanzar_sesion"), "{d}");
    assert!(d.contains(MCP) && d.contains("claude-code") && d.contains("--strict-mcp-config"), "{d}");
}

#[tokio::test]
async fn la_funcion_de_las_interfaces_ejecuta_con_el_gesto_del_usuario() {
    let mut e = entorno();
    let r = lanzar_sesion(&mut e.store, &e.sis, "usuario", Canal::Terminal, "opencode", &[REMOTO.to_string()], None).await.unwrap();
    assert_eq!(r.datos["agente"], "opencode");
    assert_eq!(r.datos["herramientas"], json!([REMOTO]));
    let t = e.sis.terminales();
    assert_eq!(t[0][0], "env");
    assert!(t[0][1].starts_with("OPENCODE_CONFIG="), "{t:?}");
    assert_eq!(t[0][2..], ["opencode", "--standalone"].map(String::from), "{t:?}");
    // OpenCode lleva su configuración por OPENCODE_CONFIG (un fichero), no por el entorno.
    assert!(e.sis.terminales()[0].iter().all(|a| !a.contains("SECRETO")));
    let fichero = e.runtime.join("ea-biblioteca/sesiones");
    let sesion = fs::read_dir(&fichero).unwrap().next().unwrap().unwrap().path();
    assert!(fs::read_to_string(sesion.join("opencode.json")).unwrap().contains("SECRETO_HEADER"));
}

#[tokio::test]
async fn con_directorio_la_terminal_se_abre_con_env_chdir() {
    let mut e = entorno();
    let proyecto = e.home.join("proy");
    fs::create_dir_all(&proyecto).unwrap();
    let r = lanzar_sesion(&mut e.store, &e.sis, "usuario", Canal::Web, "claude-code", &[MCP.to_string()], Some(proyecto.to_str().unwrap())).await.unwrap();
    let argv = r.datos["argv"].as_array().unwrap();
    assert_eq!(argv[0], "env");
    assert_eq!(argv[1], "-C");
    assert_eq!(argv[2], proyecto.to_str().unwrap());
    assert_eq!(e.sis.terminales()[0][..3], ["env", "-C", proyecto.to_str().unwrap()].map(String::from));
}

#[tokio::test]
async fn revalida_al_aceptar_si_cambio_el_inventario() {
    let mut e = entorno();
    let args = json!({ "agente": "claude-code", "herramientas": [MCP, SKILL] });
    let h = observar(&e.sis, "ea_lanzar_sesion", &args).await.unwrap();

    // 1. Cambia la definición del MCP (la URL del remoto no, la del comando sí).
    let (p, _) = proponer(&e.store, &e.sis, &h, "claude-code", None, "ea_lanzar_sesion", &args).unwrap();
    {
        let mut b = e.sis.biblioteca.lock().unwrap();
        let inv = &mut b.as_mut().unwrap().0;
        let m = inv.elementos.iter_mut().find(|x| x.id == MCP).unwrap();
        m.def_mcp.as_mut().unwrap().comando = Some("otro-comando".into());
    }
    let p = aceptar_propuesta(&mut e.store, &e.sis, p.id, "usuario", Canal::Web).await.unwrap();
    let ej = p.ejecucion.unwrap();
    assert_eq!(ej.estado, EstadoEjecucion::Fallida);
    assert!(ej.detalle.unwrap().contains("el estado cambió"), "plan distinto");
    assert!(e.sis.terminales().is_empty());

    // 2. Desaparece un id.
    let h = observar(&e.sis, "ea_lanzar_sesion", &args).await.unwrap();
    let (p, _) = proponer(&e.store, &e.sis, &h, "claude-code", None, "ea_lanzar_sesion", &args).unwrap();
    e.sis.biblioteca.lock().unwrap().as_mut().unwrap().0.elementos.retain(|x| x.id != SKILL);
    let p = aceptar_propuesta(&mut e.store, &e.sis, p.id, "usuario", Canal::Web).await.unwrap();
    let ej = p.ejecucion.unwrap();
    assert_eq!(ej.estado, EstadoEjecucion::Fallida);
    assert!(ej.detalle.unwrap().contains("no existe"));
    assert!(e.sis.terminales().is_empty());
    assert!(diario(&e.store).contains("accion_fallida"));
}

#[tokio::test]
async fn si_la_terminal_falla_la_accion_falla_y_queda_en_el_diario() {
    let mut e = entorno();
    *e.sis.terminal.fallo.lock().unwrap() = Some("no hay terminal que abrir".into());
    let err = lanzar_sesion(&mut e.store, &e.sis, "usuario", Canal::Web, "claude-code", &[MCP.to_string()], None).await.unwrap_err();
    assert!(err.to_string().contains("no hay terminal"), "{err}");
    let d = diario(&e.store);
    assert!(d.contains("accion_fallida"), "{d}");
    sin_secretos(&d);
}

#[tokio::test]
async fn solo_con_la_capa_de_consulta_se_simula_o_se_propone_pero_no_se_lanza() {
    // `Consulta` (lo único que reciben ea-mcp y el orquestador) no tiene
    // `sesion_lanzar`: es de `Sistema`. Con el doble solo como `Consulta`:
    fn como_consulta<C: Consulta>(c: &C) -> &C {
        c
    }
    let e = entorno();
    let c = como_consulta(&e.sis);
    let args = json!({ "agente": "codex", "herramientas": [] });
    assert!(observar(c, "ea_lanzar_sesion", &args).await.is_ok());
    let args = json!({ "agente": "claude-code", "herramientas": [MCP] });
    let h = observar(c, "ea_lanzar_sesion", &args).await.unwrap();
    let (p, _) = proponer(&e.store, c, &h, "claude-code", None, "ea_lanzar_sesion", &args).unwrap();
    assert_eq!(p.estado, EstadoPropuesta::Pendiente);
    assert!(e.sis.terminales().is_empty() && e.sis.mutaciones().is_empty());
    assert!(!e.runtime.join("ea-biblioteca").exists());
}
