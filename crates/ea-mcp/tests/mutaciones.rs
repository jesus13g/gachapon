//! Mutaciones supervisadas vistas desde el servidor MCP, sobre HTTP de
//! verdad: un agente solo puede simular o encolar una propuesta; nunca
//! ejecutar. La base de datos y el directorio de rutinas son temporales.

use std::sync::Arc;

use ea_acciones::{CATALOGO, Riesgo};
use ea_boveda::Boveda;
use ea_mcp::EaTools;
use ea_pruebas::mcp::{conectar, llamar};
use ea_store::{EstadoPropuesta, EventFilter, PropuestaFilter, Store};
use serde_json::json;
use tokio::sync::Mutex;

struct Entorno {
    url: String,
    db: std::path::PathBuf,
    rutinas: std::path::PathBuf,
    /// Ajustes aislados del servidor (los roles se leen en cada llamada).
    ajustes: ea_ajustes::Fuentes,
    _dir: tempfile::TempDir,
}

async fn entorno() -> Entorno {
    // Estado del bus y bóveda en directorios temporales, nunca los reales.
    static ESTADO: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let estado = ESTADO.get_or_init(|| tempfile::tempdir().unwrap()).path();
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let rutinas = dir.path().join("rutinas");
    std::fs::create_dir_all(&rutinas).unwrap();
    std::fs::write(rutinas.join("vieja.toml"), "[rutina]\n").unwrap();
    // «retirada» está en la papelera: lo único que se borra para siempre.
    std::fs::create_dir_all(rutinas.join(".papelera")).unwrap();
    std::fs::write(rutinas.join(".papelera/retirada.toml"), "[rutina]\n").unwrap();
    let store = Arc::new(Mutex::new(Store::open_at(&db).unwrap()));
    let r = rutinas.clone();
    let boveda = dir.path().join("boveda");
    let ajustes = ea_ajustes::Fuentes::aislada(&dir.path().join("ajustes"));
    let f = ajustes.clone();
    let router = ea_mcp::router(
        move || {
            EaTools::con_entorno(store.clone(), r.clone(), Boveda::en(boveda.clone()), estado.to_path_buf()).con_ajustes(f.clone()).con_credencial(credencial())
        },
        Default::default(),
    );
    Entorno { url: ea_pruebas::mcp::servidor(router).await, db, rutinas, ajustes, _dir: dir }
}

fn propuestas(e: &Entorno) -> Vec<ea_store::Propuesta> {
    Store::open_at(&e.db).unwrap().list_propuestas(&PropuestaFilter::default()).unwrap()
}

#[tokio::test]
async fn el_catalogo_mcp_coincide_con_el_catalogo_y_no_hay_forma_de_confirmar() {
    let e = entorno().await;
    let c = conectar(&e.url, "claude-code").await;
    let tools = c.list_tools(Default::default()).await.unwrap().tools;
    let mut expuestas: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
    let mut esperadas: Vec<String> = CATALOGO.iter().map(|h| h.nombre.to_string()).collect();
    expuestas.sort();
    esperadas.sort();
    assert_eq!(expuestas, esperadas, "una sola fuente de verdad: el catálogo");
    // Ninguna herramienta decide propuestas ni confirma nada.
    for n in &expuestas {
        for prohibido in ["decidir", "aceptar", "confirmar", "propuesta", "ejecutar"] {
            assert!(!n.contains(prohibido), "{n}");
        }
    }
    // Las anotaciones MCP reflejan el nivel de riesgo declarado.
    for t in &tools {
        let h = ea_acciones::buscar(&t.name).unwrap();
        let a = t.annotations.as_ref().expect("anotaciones");
        assert_eq!(a.read_only_hint, Some(h.riesgo == Riesgo::Lectura), "{}", t.name);
        if h.riesgo == Riesgo::Irreversible {
            assert_eq!(a.destructive_hint, Some(true), "{}", t.name);
        }
    }
}

#[tokio::test]
async fn simular_no_escribe_nada_pero_queda_en_el_diario() {
    let e = entorno().await;
    let c = conectar(&e.url, "claude-code").await;
    let r = llamar(&c, "ea_promover_nota", json!({ "ruta": "claude-code/inexistente.md", "simular": true })).await;
    assert!(r.is_err(), "una nota inexistente no se simula");
    let r = llamar(&c, "ea_eliminar_rutina", json!({ "slug": "retirada", "simular": true })).await.unwrap();
    assert_eq!(r["riesgo"], "irreversible");

    assert!(propuestas(&e).is_empty());
    assert!(e.rutinas.join(".papelera/retirada.toml").exists());
    let s = Store::open_at(&e.db).unwrap();
    let ev = s.list_events(&EventFilter { limit: Some(50), ..Default::default() }).unwrap();
    let sim = ev.iter().find(|x| x.action == "mcp_eliminar_rutina_simulada").expect("diario");
    assert_eq!(sim.actor, "claude-code");
    assert!(sim.detail.as_ref().unwrap().contains("retirada"));
    assert!(ev.iter().any(|x| x.action == "mcp_eliminar_rutina_simulada"));
}

#[tokio::test]
async fn un_agente_que_pide_ejecutar_solo_consigue_una_propuesta_pendiente() {
    let e = entorno().await;
    let c = conectar(&e.url, "claude-code").await;
    // Aunque el agente intente "confirmarse" con argumentos de su cosecha.
    let r = llamar(&c, "ea_eliminar_rutina", json!({ "slug": "retirada", "confirmar": true, "confirmacion": "si", "token": "x" })).await.unwrap();
    assert_eq!(r["ejecutado"], false);
    assert_eq!(r["propuesta"]["estado"], "pendiente");
    assert_eq!(r["riesgo"], "irreversible");
    assert!(r.get("token").is_none() && r.get("confirmacion").is_none());
    assert!(r["aviso"].as_str().unwrap().contains("NO se ha ejecutado"));
    assert!(r["aviso"].as_str().unwrap().contains("terminal"));

    let ps = propuestas(&e);
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].estado, EstadoPropuesta::Pendiente);
    assert_eq!(ps[0].proponente.as_deref(), Some("claude-code"));
    let a = ps[0].accion.as_ref().unwrap();
    assert_eq!(a.herramienta, "ea_eliminar_rutina");
    assert!(a.argumentos.get("confirmar").is_none());
    assert!(ps[0].ejecucion.is_none());
    assert!(e.rutinas.join(".papelera/retirada.toml").exists(), "el fichero sigue ahí");

    // Repetir no duplica.
    let r2 = llamar(&c, "ea_eliminar_rutina", json!({ "slug": "retirada" })).await.unwrap();
    assert_eq!(r2["propuesta"]["id"], r["propuesta"]["id"]);
    assert_eq!(r2["propuesta"]["nueva"], false);
    assert_eq!(propuestas(&e).len(), 1);
}

#[tokio::test]
async fn el_modelo_local_como_mucho_propone_reversibles_de_su_proyeccion() {
    let e = entorno().await;
    let c = conectar(&e.url, "qwen-code-mcp-client").await;
    // Una nota suya (en una bóveda temporal) que propone promover.
    let r = llamar(&c, "ea_escribir_nota", json!({ "titulo": "Hallazgo", "contenido": "algo" })).await.unwrap();
    let ruta = r["ruta"].as_str().unwrap().to_string();
    let r = llamar(&c, "ea_promover_nota", json!({ "ruta": ruta })).await.unwrap();
    assert_eq!(r["ejecutado"], false);
    assert_eq!(propuestas(&e)[0].proponente.as_deref(), Some("modelo-local"));

    for (n, a) in [
        ("ea_eliminar_rutina", json!({ "slug": "vieja", "simular": true })),
        ("ea_reiniciar_servicio", json!({ "servicio": "ea-mcp", "simular": true })),
        ("ea_encolar_rutina", json!({ "slug": "vieja", "simular": true })),
        ("ea_registrar_rutina", json!({ "toml": "[rutina]", "simular": true })),
        ("ea_programar_rutina", json!({ "slug": "vieja", "calendario": "daily", "simular": true })),
        ("ea_desprogramar_rutina", json!({ "slug": "vieja", "simular": true })),
        ("ea_retirar_rutina", json!({ "slug": "vieja", "simular": true })),
        ("ea_restaurar_rutina", json!({ "slug": "retirada", "simular": true })),
        ("ea_limpiar_worktree", json!({ "trabajo": "t-1", "simular": true })),
        ("ea_activar_herramienta", json!({ "elemento": "codex/mcp/usuario/docs", "simular": true })),
        ("ea_desactivar_herramienta", json!({ "elemento": "codex/mcp/usuario/docs", "simular": true })),
        ("ea_lanzar_sesion", json!({ "agente": "claude-code", "herramientas": ["claude-code/mcp/usuario/x"], "simular": true })),
    ] {
        let err = llamar(&c, n, a).await.unwrap_err();
        assert!(err.contains("no está disponible"), "{n}: {err}");
    }
    assert_eq!(propuestas(&e).len(), 1);
    assert!(e.rutinas.join("vieja.toml").exists());
}

#[tokio::test]
async fn lo_desatendido_no_puede_ni_simular_ni_proponer() {
    let e = entorno().await;
    let c = conectar(&e.url, "ea-orchestrator").await;
    // Las lecturas siguen funcionando.
    llamar(&c, "ea_status", json!({})).await.unwrap();
    for h in CATALOGO.iter().filter(|h| h.riesgo != Riesgo::Lectura) {
        let args = match h.nombre {
            "ea_reiniciar_servicio" => json!({ "servicio": "ea-mcp" }),
            // Denegadas antes de tocar nada, así que no llegan a la bóveda configurada.
            "ea_escribir_nota" => json!({ "titulo": "a", "contenido": "b" }),
            "ea_promover_nota" => json!({ "ruta": "modelo-local/a.md" }),
            "ea_registrar_rutina" => json!({ "toml": "[rutina]" }),
            "ea_programar_rutina" => json!({ "slug": "vieja", "calendario": "daily" }),
            "ea_activar_herramienta" | "ea_desactivar_herramienta" => json!({ "elemento": "codex/mcp/usuario/docs" }),
            "ea_lanzar_sesion" => json!({ "agente": "claude-code", "herramientas": ["claude-code/mcp/usuario/x"] }),
            "ea_limpiar_worktree" => json!({ "trabajo": "t-1" }),
            _ => json!({ "slug": "vieja" }),
        };
        let err = llamar(&c, h.nombre, args).await.unwrap_err();
        assert!(err.contains("no está disponible"), "{}: {err}", h.nombre);
    }
    assert!(propuestas(&e).is_empty());
    assert!(e.rutinas.join("vieja.toml").exists());
}

#[tokio::test]
async fn argumentos_fuera_de_la_lista_cerrada_se_rechazan() {
    let e = entorno().await;
    let c = conectar(&e.url, "claude-code").await;
    for (n, a) in [
        ("ea_reiniciar_servicio", json!({ "servicio": "sshd" })),
        ("ea_eliminar_rutina", json!({ "slug": "../vieja" })),
        ("ea_eliminar_rutina", json!({ "slug": "no-existe" })),
    ] {
        assert!(llamar(&c, n, a).await.is_err(), "{n}");
    }
    assert!(propuestas(&e).is_empty());
}

/// Retirar y restaurar solo proponen, como las demás (spec 004, RF-011), y
/// una rutina retirada no se encola: la simulación da el mismo texto que la
/// web y la CLI (`Ubicacion::motivo_no_lanzable`).
#[tokio::test]
async fn retirar_y_restaurar_proponen_y_una_retirada_no_se_encola() {
    let e = entorno().await;
    let c = conectar(&e.url, "claude-code").await;
    let r = llamar(&c, "ea_retirar_rutina", json!({ "slug": "vieja", "simular": true })).await.unwrap();
    assert_eq!(r["riesgo"], "reversible");
    let t = r["simulacion"].as_str().unwrap_or_default().to_string() + &r.to_string();
    assert!(t.contains("papelera") && t.contains("timer"), "{r}");
    let r = llamar(&c, "ea_restaurar_rutina", json!({ "slug": "retirada" })).await.unwrap();
    assert_eq!(r["ejecutado"], false);
    assert_eq!(r["propuesta"]["estado"], "pendiente");
    assert!(e.rutinas.join(".papelera/retirada.toml").exists() && !e.rutinas.join("retirada.toml").exists());

    let err = llamar(&c, "ea_encolar_rutina", json!({ "slug": "retirada", "simular": true })).await.unwrap_err();
    let motivo = ea_bus::Ubicacion::Retirada.motivo_no_lanzable("retirada").unwrap();
    assert!(err.contains(&motivo), "{err}");
    // Ya retirada: no se retira otra vez; activa: no se restaura.
    assert!(llamar(&c, "ea_retirar_rutina", json!({ "slug": "retirada", "simular": true })).await.is_err());
    assert!(llamar(&c, "ea_restaurar_rutina", json!({ "slug": "vieja", "simular": true })).await.is_err());
}

fn diario(e: &Entorno) -> Vec<ea_store::Event> {
    Store::open_at(&e.db).unwrap().list_events(&EventFilter { limit: Some(200), ..Default::default() }).unwrap()
}

fn roles(e: &Entorno, planificador: &str, ejecutor: &str) {
    std::fs::create_dir_all(&e.ajustes.dir_config).unwrap();
    std::fs::write(e.ajustes.dir_config.join("ajustes.toml"), format!("[roles]\nplanificador = \"{planificador}\"\nejecutor = \"{ejecutor}\"\n")).unwrap();
}

#[tokio::test]
async fn un_cliente_desconocido_solo_lee_y_queda_su_nombre_en_el_diario() {
    let e = entorno().await;
    let c = conectar(&e.url, "cliente-raro").await;
    assert!(llamar(&c, "ea_status", json!({})).await.is_ok(), "leer sí");
    for (h, args) in [
        ("ea_encolar_rutina", json!({ "slug": "vieja" })),
        ("ea_eliminar_rutina", json!({ "slug": "retirada", "simular": true })),
        ("ea_escribir_nota", json!({ "ruta": "x.md", "titulo": "x", "contenido": "x" })),
    ] {
        let r = llamar(&c, h, args).await;
        assert!(r.as_ref().is_err_and(|m| m.contains("desconocido")), "{h}: {r:?}");
    }
    assert!(propuestas(&e).is_empty(), "nada se propone");
    let ev = diario(&e);
    let status = ev.iter().find(|x| x.action == "mcp_status").expect("diario");
    assert_eq!(status.actor, "desconocido");
    assert!(status.detail.as_ref().unwrap().contains("\"cliente\":\"cliente-raro\""), "para poder añadirlo al catálogo");
    assert!(ev.iter().any(|x| x.action == "mcp_escribir_nota_denegada" && x.actor == "desconocido"));
}

#[tokio::test]
async fn los_permisos_salen_del_rol_que_ocupa_el_agente_ahora() {
    let e = entorno().await;
    // Por defecto Claude Code es el planificador: puede proponer.
    let claude = conectar(&e.url, "claude-code").await;
    assert!(llamar(&claude, "ea_eliminar_rutina", json!({ "slug": "retirada", "simular": true })).await.is_ok());
    // Codex no ocupa ningún rol: solo lee.
    let codex = conectar(&e.url, "codex-mcp-client").await;
    let r = llamar(&codex, "ea_eliminar_rutina", json!({ "slug": "retirada", "simular": true })).await;
    assert!(r.is_err_and(|m| m.contains("sin-rol")));

    // Codex pasa a planificador, sin reconectar: ahora propone como el rol
    // (`claude-code` en disco) y el diario dice qué agente fue. Claude Code
    // se queda sin rol.
    roles(&e, "codex", "qwen-code");
    assert!(llamar(&codex, "ea_eliminar_rutina", json!({ "slug": "retirada", "simular": true })).await.is_ok());
    let r = llamar(&claude, "ea_eliminar_rutina", json!({ "slug": "retirada", "simular": true })).await;
    assert!(r.is_err_and(|m| m.contains("sin-rol")), "Claude Code sin rol no hereda los permisos del nombre");
    let ev = diario(&e);
    let de_codex = ev
        .iter()
        .find(|x| x.action == "mcp_eliminar_rutina_simulada" && x.detail.as_deref().is_some_and(|d| d.contains("\"agente\":\"codex\"")))
        .expect("simulación de codex en el diario");
    assert_eq!(de_codex.actor, "claude-code", "el actor es el rol planificador");

    // Como ejecutor, OpenCode tiene la proyección reducida: nada irreversible.
    roles(&e, "codex", "opencode");
    let oc = conectar(&e.url, "opencode").await;
    assert!(llamar(&oc, "ea_status", json!({})).await.is_ok());
    let r = llamar(&oc, "ea_eliminar_rutina", json!({ "slug": "retirada", "simular": true })).await;
    assert!(r.is_err_and(|m| m.contains("modelo-local")));
}

/// La credencial de las pruebas, la que manda `conectar`.
fn credencial() -> ea_mcp::Credencial {
    ea_mcp::Credencial::de_texto(ea_pruebas::mcp::CREDENCIAL).unwrap()
}
