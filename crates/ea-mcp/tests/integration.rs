//! Cableado MCP de extremo a extremo, sobre HTTP de verdad. La lógica de
//! negocio (repositorios de eventos) ya está cubierta por los tests
//! de `ea-store`; esto solo comprueba que el servidor expone el catálogo
//! esperado y que una llamada real de ida y vuelta funciona.

use std::sync::Arc;

use ea_mcp::EaTools;
use ea_store::Store;
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, Implementation, InitializeRequestParams,
    ProtocolVersion,
};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::{ClientLifecycleMode, ClientServiceExt};
use tokio::sync::Mutex;

type Client = rmcp::service::RunningService<rmcp::RoleClient, InitializeRequestParams>;

async fn connect(url: String) -> Client {
    let transport = StreamableHttpClientTransport::from_uri(url);
    let client_info = InitializeRequestParams::new(
        ClientCapabilities::default(),
        Implementation::new("ea-mcp-test-client", "0.0.1"),
    );
    client_info
        .serve_with_lifecycle(
            transport,
            ClientLifecycleMode::Auto {
                preferred_versions: vec![ProtocolVersion::V_2025_11_25],
                legacy_version: None,
            },
        )
        .await
        .expect("conectar al servidor MCP de prueba")
}

fn text_of(result: &rmcp::model::CallToolResult) -> serde_json::Value {
    let text = &result.content[0].as_text().expect("contenido de texto").text;
    serde_json::from_str(text).expect("contenido JSON válido")
}

/// Estado del bus en un directorio temporal compartido por el proceso de
/// pruebas, nunca el real.
fn estado_temporal() -> &'static std::path::Path {
    static ESTADO: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let dir = ESTADO.get_or_init(|| tempfile::tempdir().unwrap());
    unsafe { std::env::set_var("EA_ESTADO_DIR", dir.path()) };
    dir.path()
}

async fn start_server(db_path: std::path::PathBuf) -> String {
    estado_temporal();
    let store = Store::open_at(&db_path).expect("abrir store de prueba");
    let store = Arc::new(Mutex::new(store));
    let rutinas = db_path.parent().unwrap().join("rutinas");
    // Roles de fábrica, no los del usuario.
    let ajustes = ea_ajustes::Fuentes::aislada(&db_path.parent().unwrap().join("ajustes"));

    let router = ea_mcp::router(
        move || EaTools::con_rutinas_dir(store.clone(), rutinas.clone()).con_ajustes(ajustes.clone()),
        Default::default(),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind puerto efímero");
    let addr = listener.local_addr().expect("local_addr");

    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    format!("http://{addr}/mcp")
}

#[tokio::test]
async fn expone_el_catalogo_y_responde_a_una_llamada() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("ecosistema-test.db");
    let url = start_server(db_path).await;
    let client = connect(url).await;

    let tools = client
        .list_tools(Default::default())
        .await
        .expect("tools/list");
    let names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
    for ausente in ["ea_listar_tareas", "ea_ver_tarea", "ea_crear_tarea", "ea_cambiar_estado_tarea"] {
        assert!(!names.contains(&ausente), "{ausente} ya no existe");
    }
    // Los ajustes no existen para los agentes: ni leerlos ni cambiarlos.
    assert!(!names.iter().any(|n| n.contains("ajuste")), "{names:?}");
    for expected in [
        "ea_status",
        "ea_listar_trabajos",
        "ea_ver_trabajo",
        "ea_listar_diario",
        "ea_biblioteca",
        "ea_encolar_rutina",
        "ea_registrar_rutina",
        "ea_reiniciar_servicio",
        "ea_eliminar_rutina",
        "ea_programar_rutina",
        "ea_desprogramar_rutina",
        "ea_retirar_rutina",
        "ea_restaurar_rutina",
    ] {
        assert!(
            names.contains(&expected),
            "falta la herramienta {expected} en el catálogo: {names:?}"
        );
    }

    let status = client
        .call_tool(CallToolRequestParams::new("ea_status"))
        .await
        .expect("llamar a ea_status");
    assert_ne!(status.content.len(), 0, "ea_status no devolvió contenido");

    client.cancel().await.ok();
}

#[tokio::test]
async fn atribuye_al_modelo_local_cuando_el_cliente_es_qwen_code() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("ecosistema-test.db");
    let url = start_server(db_path).await;

    let transport = StreamableHttpClientTransport::from_uri(url);
    let client_info = InitializeRequestParams::new(
        ClientCapabilities::default(),
        Implementation::new("qwen-code-mcp-client", "0.0.1"),
    );
    let client = client_info
        .serve_with_lifecycle(
            transport,
            ClientLifecycleMode::Auto {
                preferred_versions: vec![ProtocolVersion::V_2025_11_25],
                legacy_version: None,
            },
        )
        .await
        .expect("conectar como qwen-code");

    client
        .call_tool(CallToolRequestParams::new("ea_status"))
        .await
        .expect("llamar a ea_status como qwen-code");

    let diario = client
        .call_tool(CallToolRequestParams::new("ea_listar_diario"))
        .await
        .expect("listar diario");
    let diario = text_of(&diario);
    let actores: Vec<&str> = diario["eventos"]
        .as_array()
        .expect("eventos")
        .iter()
        .map(|e| e["actor"].as_str().unwrap_or_default())
        .collect();
    assert!(
        actores.contains(&"modelo-local"),
        "las invocaciones de qwen-code deben quedar atribuidas a modelo-local: {actores:?}"
    );
    assert!(
        !actores.contains(&"claude-code"),
        "no debe atribuirse a claude-code lo que hizo qwen-code: {actores:?}"
    );

    client.cancel().await.ok();
}

#[tokio::test]
async fn atribuye_al_orquestador_cuando_el_cliente_es_ea_orchestrator() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("ecosistema-test.db");
    let url = start_server(db_path).await;

    let transport = StreamableHttpClientTransport::from_uri(url);
    let client_info = InitializeRequestParams::new(
        ClientCapabilities::default(),
        Implementation::new("ea-orchestrator", "0.0.1"),
    );
    let client = client_info
        .serve_with_lifecycle(
            transport,
            ClientLifecycleMode::Auto {
                preferred_versions: vec![ProtocolVersion::V_2025_11_25],
                legacy_version: None,
            },
        )
        .await
        .expect("conectar como ea-orchestrator");

    client
        .call_tool(CallToolRequestParams::new("ea_status"))
        .await
        .expect("llamar a ea_status como ea-orchestrator");

    let diario = client
        .call_tool(CallToolRequestParams::new("ea_listar_diario"))
        .await
        .expect("listar diario");
    let diario = text_of(&diario);
    let actores: Vec<&str> = diario["eventos"]
        .as_array()
        .expect("eventos")
        .iter()
        .map(|e| e["actor"].as_str().unwrap_or_default())
        .collect();
    assert!(
        actores.contains(&"orquestador"),
        "las invocaciones de ea-orchestrator deben quedar atribuidas a orquestador: {actores:?}"
    );
    assert!(
        !actores.contains(&"claude-code"),
        "no debe atribuirse a claude-code lo que hizo ea-orchestrator: {actores:?}"
    );

    client.cancel().await.ok();
}

#[tokio::test]
async fn los_trabajos_del_bus_se_leen_pero_no_se_crean_desde_mcp() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("ecosistema-test.db");
    let url = start_server(db_path).await;
    let client = connect(url).await;

    // Un trabajo creado por el bus (como haría `ea trabajo crear`).
    let wt = tempfile::tempdir().expect("worktree");
    let bus = ea_bus::Bus::abrir().expect("bus temporal");
    let id = bus
        .crear_trabajo(ea_bus::Orden {
            id: String::new(),
            modo: ea_bus::Modo::Mision,
            titulo: "Probar MCP".into(),
            worktree: wt.path().to_string_lossy().into_owned(),
            objetivo: "Verificar la lectura".into(),
            criterio_exito: Some("true".into()),
            fases: vec![],
            revisar_cada: None,
            topes: ea_bus::Topes::default(),
            rutina: None,
            notas_revision: vec![],
            skills: vec![],
            equipo: None,
            asignado_a: None,
            revisor: None,
            worktree_propio: false,
            creada_en: String::new(),
            creada_por: "test".into(),
        })
        .expect("crear trabajo");

    let listado = text_of(
        &client
            .call_tool(CallToolRequestParams::new("ea_listar_trabajos"))
            .await
            .expect("listar trabajos"),
    );
    let ids: Vec<_> = listado["trabajos"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_string()).collect();
    assert!(ids.contains(&id), "{ids:?}");

    let mut args = serde_json::Map::new();
    args.insert("id".into(), serde_json::json!(id));
    let detalle = text_of(
        &client
            .call_tool(CallToolRequestParams::new("ea_ver_trabajo").with_arguments(args))
            .await
            .expect("ver trabajo"),
    );
    assert_eq!(detalle["orden"]["titulo"], "Probar MCP");
    assert!(detalle["informe"].is_null());

    let mut args = serde_json::Map::new();
    args.insert("id".into(), serde_json::json!("no-existe"));
    assert!(client.call_tool(CallToolRequestParams::new("ea_ver_trabajo").with_arguments(args)).await.is_err());

    let status = text_of(&client.call_tool(CallToolRequestParams::new("ea_status")).await.unwrap());
    assert!(status["trabajos_por_estado"].is_object());
    assert!(status["colas"]["modelo-local"].is_array());

    let diario = text_of(&client.call_tool(CallToolRequestParams::new("ea_listar_diario")).await.unwrap());
    let acciones: Vec<&str> = diario["eventos"].as_array().unwrap().iter().map(|e| e["accion"].as_str().unwrap_or_default()).collect();
    for esperada in ["mcp_listar_trabajos", "mcp_ver_trabajo", "mcp_status"] {
        assert!(acciones.contains(&esperada), "falta '{esperada}': {acciones:?}");
    }

    client.cancel().await.ok();
}

#[tokio::test]
async fn ea_status_informa_del_estado_real_del_orquestador() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("ecosistema-test.db");
    let url = start_server(db_path.clone()).await;
    let client = connect(url).await;

    let sin_latido = text_of(
        &client
            .call_tool(CallToolRequestParams::new("ea_status"))
            .await
            .expect("llamar a ea_status"),
    );
    assert_eq!(sin_latido["orquestador"]["vivo"], false);
    assert!(sin_latido["orquestador"]["ultimo_latido"].is_null());

    // Otra conexión al mismo fichero hace de orquestador emitiendo su latido.
    let otro = Store::open_at(&db_path).expect("segunda conexión");
    otro.append_event(ea_store::NewEvent {
        actor: "orquestador".into(),
        action: "heartbeat".into(),
        trabajo: None,
        outcome: None,
        duration_ms: None,
        detail: None,
    })
    .expect("registrar latido");

    let con_latido = text_of(
        &client
            .call_tool(CallToolRequestParams::new("ea_status"))
            .await
            .expect("llamar a ea_status"),
    );
    assert_eq!(con_latido["orquestador"]["vivo"], true);
    assert_eq!(con_latido["orquestador"]["estado"], "vivo");
    assert!(con_latido["orquestador"]["ultimo_latido"].is_string());

    client.cancel().await.ok();
}

/// Un equipo despierto en el bus temporal, como lo dejaría `ea equipo despertar`.
fn equipo_de_prueba() -> String {
    let d = tempfile::tempdir().unwrap();
    let bus = ea_bus::Bus::abrir().expect("bus temporal");
    let mut ajustes = ea_ajustes::Ajustes::por_defecto(&ea_ajustes::Fuentes::aislada(d.path()));
    ajustes.orquestador.paralelo = 4;
    let m = |a: &str, r| ea_bus::Miembro { agente: a.into(), rol: r };
    let e = bus
        .crear_equipo(
            ea_bus::NuevoEquipo {
                titulo: "equipo MCP".into(),
                objetivo: "probar la lectura".into(),
                worktree_base: "/tmp/repo".into(),
                miembros: vec![m("claude-code", ea_bus::RolEquipo::Planificador), m("qwen-code", ea_bus::RolEquipo::Ejecutor)],
                creado_por: "test".into(),
            },
            &ajustes,
        )
        .expect("despertar");
    bus.anotar_presencia(&ea_bus::Presencia::nueva("claude-code", ea_bus::RolEquipo::Planificador, Some(&e.id), std::process::id()))
        .unwrap();
    e.id
}

#[tokio::test]
async fn los_equipos_se_leen_con_su_presencia_pero_no_se_crean_ni_se_duermen_desde_mcp() {
    let dir = tempfile::tempdir().expect("tempdir");
    let url = start_server(dir.path().join("ecosistema-test.db")).await;
    let client = connect(url).await;
    let id = equipo_de_prueba();

    let listado = text_of(&client.call_tool(CallToolRequestParams::new("ea_listar_equipos")).await.expect("listar equipos"));
    let fila = listado["equipos"].as_array().unwrap().iter().find(|e| e["id"] == id.as_str()).expect("el equipo");
    assert_eq!(fila["estado"], "despierto");
    let plan = fila["miembros"].as_array().unwrap().iter().find(|m| m["agente"] == "claude-code").unwrap();
    assert_eq!(plan["viva"], true, "su presencia tiene el pid de este proceso");

    let mut args = serde_json::Map::new();
    args.insert("id".into(), serde_json::json!(id));
    let ver = text_of(&client.call_tool(CallToolRequestParams::new("ea_ver_equipo").with_arguments(args)).await.expect("ver equipo"));
    assert_eq!(ver["objetivo"], "probar la lectura");
    assert!(ver["trabajos"].is_array());
    let ejecutor = ver["miembros"].as_array().unwrap().iter().find(|m| m["agente"] == "qwen-code").unwrap();
    assert_eq!(ejecutor["viva"], false);

    let mut args = serde_json::Map::new();
    args.insert("id".into(), serde_json::json!("no-existe"));
    assert!(client.call_tool(CallToolRequestParams::new("ea_ver_equipo").with_arguments(args)).await.is_err());

    let status = text_of(&client.call_tool(CallToolRequestParams::new("ea_status")).await.unwrap());
    assert!(status["equipos"].as_array().unwrap().iter().any(|e| e["id"] == id.as_str()));
    let p = status["presencias"].as_array().unwrap().iter().find(|p| p["agente"] == "claude-code").unwrap();
    assert_eq!((p["viva"].clone(), p["equipo"].clone()), (serde_json::json!(true), serde_json::json!(id)));

    // CE-005: ninguna herramienta MCP crea, despierta o duerme equipos.
    let tools = client.list_tools(Default::default()).await.expect("tools/list");
    let nombres: Vec<String> = tools.tools.iter().map(|t| t.name.to_string()).collect();
    let de_equipos: Vec<&String> = nombres.iter().filter(|n| n.contains("equipo")).collect();
    assert_eq!(de_equipos, ["ea_listar_equipos", "ea_ver_equipo"], "{nombres:?}");
    for t in &tools.tools {
        if t.name.contains("equipo") {
            assert_eq!(t.annotations.as_ref().and_then(|a| a.read_only_hint), Some(true), "{}", t.name);
        }
    }
    for prohibida in ["despertar", "dormir", "crear_equipo", "ea_equipo"] {
        assert!(!nombres.iter().any(|n| n.contains(prohibida)), "{prohibida}: {nombres:?}");
    }
    // Y en el catálogo de acciones, las dos son lecturas directas.
    for n in ["ea_listar_equipos", "ea_ver_equipo"] {
        let h = ea_acciones::buscar(n).expect(n);
        assert_eq!(h.riesgo, ea_acciones::Riesgo::Lectura);
    }
    assert!(ea_acciones::CATALOGO.iter().filter(|h| h.nombre.contains("equipo")).all(|h| h.riesgo == ea_acciones::Riesgo::Lectura));

    client.cancel().await.ok();
}

/// Un `initialize` por TCP a pelo con este `Host` (un cliente HTTP pondría el
/// de la URL); devuelve el código de estado de la respuesta.
fn codigo_con_host(direccion: &str, host: &str) -> u16 {
    use std::io::{BufRead, BufReader, Write};
    let cuerpo = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"prueba-host","version":"0"}}}"#;
    let mut s = std::net::TcpStream::connect(direccion).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    write!(
        s,
        "POST /mcp HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\n\r\n{cuerpo}",
        cuerpo.len()
    )
    .unwrap();
    let mut linea = String::new();
    BufReader::new(s).read_line(&mut linea).unwrap();
    linea.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or_else(|| panic!("respuesta rara: {linea:?}"))
}

/// S-12: el filtro de `Host` lo fija `ea_mcp::router`, no un valor por
/// defecto de rmcp. Un nombre ajeno (una página que reenlaza su DNS a
/// 127.0.0.1) recibe un 403; los nombres locales, la respuesta normal.
#[tokio::test]
async fn solo_atiende_los_hosts_locales() {
    let dir = tempfile::tempdir().unwrap();
    let url = start_server(dir.path().join("t.db")).await;
    let direccion = url.trim_start_matches("http://").trim_end_matches("/mcp").to_string();
    let puerto = direccion.rsplit_once(':').unwrap().1.to_string();
    for (host, esperado) in [
        ("evil.example".to_string(), 403),
        (format!("evil.example:{puerto}"), 403),
        (format!("127.0.0.1.evil.example:{puerto}"), 403),
        (format!("127.0.0.1:{puerto}"), 200),
        (format!("localhost:{puerto}"), 200),
        (format!("[::1]:{puerto}"), 200),
    ] {
        let (d, h) = (direccion.clone(), host.clone());
        let codigo = tokio::task::spawn_blocking(move || codigo_con_host(&d, &h)).await.unwrap();
        assert_eq!(codigo, esperado, "Host: {host}");
    }
}

/// A-28: con SIGTERM (lo que manda systemd al parar o reiniciar el servicio)
/// `ea-mcp` deja de escuchar y sale con 0, en vez de morir por la señal.
#[cfg(unix)]
#[test]
fn para_limpio_con_sigterm() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let puerto = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut hijo = Command::new(env!("CARGO_BIN_EXE_ea-mcp"))
        .env("EA_MCP_PORT", puerto.to_string())
        .env("EA_CONFIG_DIR", d.join("config"))
        .env("EA_DB_PATH", d.join("ea.db"))
        .env("EA_ESTADO_DIR", d.join("estado"))
        .env("EA_BOVEDA_DIR", d.join("boveda"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let esperar = |hijo: &mut std::process::Child, hasta: Duration| {
        let inicio = Instant::now();
        loop {
            if let Some(st) = hijo.try_wait().unwrap() {
                return Some(st);
            }
            if inicio.elapsed() > hasta {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    let inicio = Instant::now();
    while std::net::TcpStream::connect(("127.0.0.1", puerto)).is_err() {
        assert!(esperar(&mut hijo, Duration::ZERO).is_none(), "ea-mcp salió antes de escuchar");
        assert!(inicio.elapsed() < Duration::from_secs(30), "ea-mcp no llegó a escuchar");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(Command::new("kill").args(["-TERM", &hijo.id().to_string()]).status().unwrap().success());
    let Some(st) = esperar(&mut hijo, Duration::from_secs(10)) else {
        let _ = hijo.kill();
        panic!("ea-mcp no paró con SIGTERM");
    };
    assert!(st.success(), "ea-mcp debía salir con 0 tras SIGTERM: {st}");
    assert!(std::net::TcpStream::connect(("127.0.0.1", puerto)).is_err(), "ya no escucha");
}
