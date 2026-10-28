//! El estado general (`/api/estado`), los servicios, las sesiones de
//! Claude Code con sus subagentes y la escena de Inicio (`/api/ecosistema`).

use crate::comun::*;

#[tokio::test]
async fn el_estado_resume_trabajos_y_agentes_y_el_sistema_antiguo_no_existe() {
    let f = montar();

    let (s, e) = get(&f, "/api/estado").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(e["trabajos"]["en_cola"], 0);
    assert_eq!(e["trabajos"]["cerrado"], 0);
    assert!(e.get("tareas").is_none());
    assert_eq!(e["agentes"][0]["nombre"], "claude-code");
    assert!(e["agentes"][0]["ultimo"].is_null());

    let (s, _) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::CREATED);
    let (_, e) = get(&f, "/api/estado").await;
    assert_eq!(e["trabajos"]["en_cola"], 1);

    for ruta in ["/api/tareas", "/api/tareas/1"] {
        let (s, _) = get(&f, ruta).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{ruta}");
    }
}

#[tokio::test]
async fn el_estado_dice_el_sandbox_del_ultimo_latido() {
    let f = montar();
    let (_, e) = get(&f, "/api/estado").await;
    assert!(e["orquestador"]["sandbox"].is_null());
    {
        let store = f.state.store.lock().await;
        store
            .append_event(NewEvent {
                actor: "orquestador".into(),
                action: "heartbeat".into(),
                trabajo: None,
                outcome: Some(EventOutcome::Ok),
                duration_ms: None,
                detail: Some(
                    r#"{"sistema":"Windows","sandbox":"no disponible","disponible":false,"motivo":"sin sandbox nativo"}"#.into(),
                ),
            })
            .unwrap();
    }
    let (_, e) = get(&f, "/api/estado").await;
    assert_eq!(e["orquestador"]["vivo"], true);
    assert_eq!(e["orquestador"]["sandbox"]["sistema"], "Windows");
    assert_eq!(e["orquestador"]["sandbox"]["disponible"], false);
    assert_eq!(e["orquestador"]["sandbox"]["motivo"], "sin sandbox nativo");
}

#[tokio::test]
async fn servicios_degradan_con_limpieza() {
    let f = montar();

    let (_, e) = get(&f, "/api/estado").await;
    assert_eq!(e["servicios"].as_array().unwrap().len(), 3);
    assert_eq!(e["servicios"][0]["activo"], "desconocido");
    assert_eq!(e["orquestador"]["vivo"], false);

    let (s, _) = post(&f, "/api/servicios/sshd/reiniciar", json!({})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = post(&f, "/api/servicios/ea-mcp/reiniciar", json!({})).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn las_sesiones_de_claude_code_solo_cuentan_si_el_proceso_vive() {
    let f = montar();
    let dir = f._dir.path().join("sesiones");
    let (s, v) = get(&f, "/api/sesiones").await;
    assert_eq!((s, v["sesiones"].as_array().unwrap().len()), (StatusCode::OK, 0), "sin directorio");

    std::fs::create_dir_all(&dir).unwrap();
    let propio = std::process::id();
    let ses = |pid: u32, nombre: &str| {
        json!({"pid": pid, "sessionId": "x", "cwd": tmp!(), "name": nombre, "status": "busy",
               "startedAt": 1000, "updatedAt": 2000})
        .to_string()
    };
    std::fs::write(dir.join(format!("{propio}.json")), ses(propio, "viva")).unwrap();
    std::fs::write(dir.join("999999999.json"), ses(999_999_999, "muerta")).unwrap();
    std::fs::write(dir.join("roto.json"), "no es json").unwrap();

    let (_, v) = get(&f, "/api/sesiones").await;
    let lista = v["sesiones"].as_array().unwrap();
    assert_eq!(lista.len(), 1);
    assert_eq!(lista[0]["nombre"], "viva");
    assert_eq!(lista[0]["ocupada"], true);
    assert_eq!(lista[0]["pid"], propio);
}

#[tokio::test]
async fn cada_sesion_lleva_sus_subagentes_en_marcha_primero() {
    let f = montar();
    let raiz = f._dir.path();
    let dir = raiz.join("sesiones");
    std::fs::create_dir_all(&dir).unwrap();
    let propio = std::process::id();
    let ses = |pid: u32, id: &str| {
        json!({"pid": pid, "sessionId": id, "cwd": tmp!(), "name": id, "status": "idle",
               "startedAt": 1000, "updatedAt": 2000})
        .to_string()
    };
    std::fs::write(dir.join(format!("{propio}.json")), ses(propio, "viva")).unwrap();
    std::fs::write(dir.join("999999999.json"), ses(999_999_999, "muerta")).unwrap();

    // La transcripción del padre y el meta.json de un subagente.
    let transcript = raiz.join("proyecto/viva.jsonl");
    std::fs::create_dir_all(raiz.join("proyecto/viva/subagents")).unwrap();
    std::fs::write(
        raiz.join("proyecto/viva/subagents/agent-a1.meta.json"),
        json!({"agentType": "Explore", "description": "Buscar el tablero", "model": "haiku"}).to_string(),
    )
    .unwrap();
    let marca = |sesion: &str, id: &str, desde: u64, hasta: Option<u64>| {
        let d = raiz.join("subagentes").join(sesion);
        std::fs::create_dir_all(&d).unwrap();
        let mut m = json!({"agente": id, "tipo": "Explore", "transcript": transcript, "desde": desde});
        if let Some(h) = hasta {
            m["hasta"] = json!(h);
        }
        std::fs::write(d.join(format!("{id}.json")), m.to_string()).unwrap();
    };
    marca("viva", "a0", 100, Some(200));
    marca("viva", "a2", 150, Some(300));
    marca("viva", "a1", 400, None);
    marca("muerta", "a9", 100, None);
    // Una marca de stop sin start ni meta (sin tipo): no es un subagente que mostrar.
    let d = raiz.join("subagentes/viva");
    std::fs::write(d.join("fantasma.json"), json!({"agente": "fantasma", "tipo": "", "transcript": "", "desde": 500, "hasta": 500}).to_string()).unwrap();

    let (_, v) = get(&f, "/api/sesiones").await;
    let lista = v["sesiones"].as_array().unwrap();
    assert_eq!(lista.len(), 1);
    let subs = lista[0]["subagentes"].as_array().unwrap();
    let ids: Vec<_> = subs.iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["a1", "a2", "a0"], "en marcha primero, luego los terminados más recientes");
    assert_eq!(subs[0]["descripcion"], "Buscar el tablero");
    assert_eq!(subs[0]["modelo"], "haiku");
    assert!(subs[0]["hasta"].is_null());
    assert_eq!(subs[1]["descripcion"], "Explore", "sin meta.json se usa el tipo");
    assert_eq!(subs[1]["hasta"], 300);
}

#[tokio::test]
async fn el_ecosistema_junta_sesiones_subagentes_presencias_y_trabajos() {
    let f = montar();
    let raiz = f._dir.path();
    let (s, v) = get(&f, "/api/ecosistema").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!((v["agentes"].as_array().unwrap().len(), v["tareas"].as_array().unwrap().len()), (0, 0), "sin nada vivo, nada: {v}");

    // Una sesión viva de Claude Code con su transcripción (el modelo sale de ahí).
    let propio = std::process::id();
    let cwd = tmp!();
    escribe(
        raiz.join("sesiones").join(format!("{propio}.json")),
        &json!({"pid": propio, "sessionId": "s-uno", "cwd": cwd, "name": "f5", "status": "busy", "startedAt": 1000, "updatedAt": 2000}).to_string(),
    );
    let dir_proy: String = cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let transcript = raiz.join("proyectos").join(&dir_proy).join("s-uno.jsonl");
    escribe(&transcript, "{\"message\":{\"model\":\"claude-opus-5-5\"}}\n{\"message\":{\"model\":\"<synthetic>\"}}\n");
    // Dos subagentes: uno en marcha y otro que Claude Code dio por acabado sin SubagentStop.
    let subs = raiz.join("proyectos").join(&dir_proy).join("s-uno/subagents");
    escribe(subs.join("agent-a1.meta.json"), &json!({"agentType": "Explore", "description": "Buscar", "model": "haiku"}).to_string());
    escribe(subs.join("agent-a2.meta.json"), &json!({"agentType": "Plan", "description": "Planear", "finished": true}).to_string());
    let ahora = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    for (id, tipo) in [("a1", "Explore"), ("a2", "Plan")] {
        escribe(
            raiz.join("subagentes/s-uno").join(format!("{id}.json")),
            &json!({"agente": id, "tipo": tipo, "transcript": transcript, "desde": ahora}).to_string(),
        );
    }

    // El ejecutor despierto (presencia viva) y una misión en su cola.
    let bus = ea_bus::Bus::abrir_en(&f.state.config.estado_dir).unwrap();
    bus.anotar_presencia(&ea_bus::Presencia::nueva("qwen-code", ea_bus::RolEquipo::Ejecutor, None, propio)).unwrap();
    let id = bus.crear_trabajo(orden(ea_bus::Modo::Mision, "Primera", tmp!())).unwrap();

    let (_, v) = get(&f, "/api/ecosistema").await;
    let ag = |id: &str| v["agentes"].as_array().unwrap().iter().find(|a| a["id"] == id).cloned().unwrap_or_else(|| panic!("falta {id}: {v}"));
    let claude = ag("claude:s-uno");
    assert_eq!((claude["familia"].as_str(), claude["modelo"].as_str(), claude["estado"].as_str()), (Some("claude"), Some("claude-opus-5-5"), Some("trabajando")));
    assert_eq!(claude["rol"], "planificador");
    assert!(claude.get("pistas").is_none(), "las pistas no salen de la API");
    let a1 = ag("claude:s-uno/a1");
    assert_eq!((a1["padre"].as_str(), a1["modelo"].as_str(), a1["estado"].as_str()), (Some("claude:s-uno"), Some("haiku"), Some("trabajando")));
    assert_eq!(ag("claude:s-uno/a2")["estado"], "terminado", "finished en el meta.json");
    let q = ag("agente:qwen-code");
    assert_eq!((q["familia"].as_str(), q["local"].as_bool(), q["estado"].as_str()), (Some("qwen"), Some(true), Some("esperando")));

    let t = &v["tareas"][0];
    assert_eq!(t["trabajo"], id.as_str());
    assert_eq!((t["lugar"].as_str(), t["planificador"].as_str()), (Some("agente:qwen-code"), Some("claude:s-uno")));
}
