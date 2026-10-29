//! Propuestas y acciones supervisadas: decidir, aceptar (una sola
//! ejecución), la consola de acciones, su historial y el catálogo.

use crate::comun::*;

#[tokio::test]
async fn decidir_una_propuesta() {
    let f = montar();
    let id = {
        let store = f.state.store.lock().await;
        store.crear_propuesta(texto("Revisar incidencias")).unwrap().id
    };

    let (_, e) = get(&f, "/api/estado").await;
    assert_eq!(e["propuestas_pendientes"], 1);

    let (_, lista) = get(&f, "/api/propuestas?estado=pendiente").await;
    assert_eq!(lista[0]["descripcion"], "Revisar incidencias");

    let ruta = format!("/api/propuestas/{id}/decidir");
    let (s, _) = post(&f, &ruta, json!({ "decision": "quizá" })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, p) = post(&f, &ruta, json!({ "decision": "aceptar" })).await;
    assert_eq!((s, p["estado"].as_str()), (StatusCode::OK, Some("aceptada")));

    let (s, _) = post(&f, &ruta, json!({ "decision": "rechazar" })).await;
    assert_eq!(s, StatusCode::CONFLICT);

    let (_, e) = get(&f, "/api/estado").await;
    assert_eq!(e["propuestas_pendientes"], 0);

    let (_, diario) = get(&f, "/api/diario").await;
    assert_eq!(diario[0]["accion"], "propuesta_decidida");
    assert_eq!(diario[0]["actor"], "usuario");
}

#[tokio::test]
async fn aceptar_una_propuesta_con_accion_la_ejecuta_una_sola_vez() {
    let f = montar();
    let id = proponer_accion(&f, "ea_encolar_rutina", json!({ "slug": "revision-diaria" })).await;

    let (_, lista) = get(&f, "/api/propuestas?estado=pendiente").await;
    assert_eq!(lista[0]["accion"]["herramienta"], "ea_encolar_rutina");
    assert_eq!(lista[0]["accion"]["riesgo"], "reversible");
    assert_eq!(lista[0]["proponente"], "claude-code");
    assert!(lista[0]["accion"]["simulacion"].as_str().unwrap().contains("revision-diaria"));
    let (_, ts) = get(&f, "/api/trabajos").await;
    assert!(ts.as_array().unwrap().is_empty(), "proponer no ejecuta");

    let ruta = format!("/api/propuestas/{id}/decidir");
    let (s, p) = post(&f, &ruta, json!({ "decision": "aceptar" })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(p["estado"], "aceptada");
    assert_eq!(p["ejecutada"]["estado"], "ejecutada");
    let (_, ts) = get(&f, "/api/trabajos").await;
    assert_eq!(ts.as_array().unwrap().len(), 1);

    let (s, _) = post(&f, &ruta, json!({ "decision": "aceptar" })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (_, d) = get(&f, "/api/diario").await;
    let ejecuciones = d.as_array().unwrap().iter().filter(|e| e["accion"] == "accion_ejecutada").count();
    assert_eq!(ejecuciones, 1);
}

#[tokio::test]
async fn una_propuesta_cuyo_estado_cambio_falla_con_mensaje_claro() {
    let f = montar();
    let id = proponer_accion(&f, "ea_encolar_rutina", json!({ "slug": "revision-diaria" })).await;
    // Mientras tanto, alguien lanza la rutina.
    let (s, _) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::CREATED);

    let (s, p) = post(&f, &format!("/api/propuestas/{id}/decidir"), json!({ "decision": "aceptar" })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(p["ejecutada"]["estado"], "fallida");
    assert!(p["ejecutada"]["detalle"].as_str().unwrap().contains("cambió desde la simulación"));
    let (_, ts) = get(&f, "/api/trabajos").await;
    assert_eq!(ts.as_array().unwrap().len(), 1, "no se encoló una segunda ejecución");
}

#[tokio::test]
async fn la_web_no_confirma_irreversibles() {
    let f = montar();
    // Solo se borra para siempre una rutina retirada (en la papelera).
    ea_bus::retirar_rutina(&f.state.config.rutinas_dir, "revision-diaria").unwrap();
    let retirada = ea_bus::ruta_retirada(&f.state.config.rutinas_dir, "revision-diaria").unwrap();
    let id = proponer_accion(&f, "ea_eliminar_rutina", json!({ "slug": "revision-diaria" })).await;
    let (_, lista) = get(&f, "/api/propuestas").await;
    assert_eq!(lista[0]["accion"]["riesgo"], "irreversible");

    let (s, e) = post(&f, &format!("/api/propuestas/{id}/decidir"), json!({ "decision": "aceptar" })).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(e["error"].as_str().unwrap().contains("terminal"));
    assert!(retirada.exists());
    let (_, lista) = get(&f, "/api/propuestas?estado=pendiente").await;
    assert_eq!(lista.as_array().unwrap().len(), 1, "sigue pendiente");

    // Rechazarla sí se puede desde la web.
    let (s, p) = post(&f, &format!("/api/propuestas/{id}/decidir"), json!({ "decision": "rechazar" })).await;
    assert_eq!((s, p["estado"].as_str()), (StatusCode::OK, Some("rechazada")));
    assert!(retirada.exists());
}

/// Una acción lenta (un reinicio espera a `systemctl` hasta 30 s) no deja la
/// página sin datos: las lecturas van por otra conexión.
#[tokio::test]
async fn una_accion_en_curso_no_bloquea_las_lecturas() {
    let f = montar();
    let _en_curso = f.state.acciones.lock().await;

    for ruta in ["/api/estado", "/api/trabajos", "/api/diario", "/api/propuestas"] {
        let (s, _) = tokio::time::timeout(std::time::Duration::from_secs(5), get(&f, ruta))
            .await
            .unwrap_or_else(|_| panic!("{ruta} se quedó esperando a la acción en curso"));
        assert_eq!(s, StatusCode::OK, "{ruta}");
    }
}

#[tokio::test]
async fn la_portada_reune_las_propuestas_pendientes() {
    let f = montar();

    let (s, i) = get(&f, "/api/inicio").await;
    assert_eq!(s, StatusCode::OK);
    assert!(i["propuestas"].as_array().unwrap().is_empty());

    f.state.store.lock().await.crear_propuesta(texto("Revisar incidencias")).unwrap();

    let (_, i) = get(&f, "/api/inicio").await;
    assert_eq!(i["propuestas"][0]["descripcion"], "Revisar incidencias");
    assert!(i.get("activas").is_none());
}

#[tokio::test]
async fn la_consola_lista_simula_y_ejecuta_acciones() {
    let f = montar();

    let (s, cat) = get(&f, "/api/acciones").await;
    assert_eq!(s, StatusCode::OK);
    let de = |n: &str| cat.as_array().unwrap().iter().find(|h| h["nombre"] == n).unwrap().clone();
    assert_eq!(de("ea_status")["supervisada"], false);
    assert_eq!(de("ea_eliminar_rutina")["confirmable"], false);
    let encolar = de("ea_encolar_rutina");
    assert_eq!(encolar["confirmable"], true);
    assert_eq!(encolar["campos"][0]["opciones"], json!(["revision-diaria"]));

    let cuerpo = json!({ "herramienta": "ea_encolar_rutina", "argumentos": { "slug": "revision-diaria" } });
    let (s, sim) = post(&f, "/api/acciones/simular", cuerpo.clone()).await;
    assert_eq!(s, StatusCode::OK);
    assert!(sim["texto"].as_str().unwrap().contains("revision-diaria"));
    assert!(!f._dir.path().join("estado/trabajos").exists() || std::fs::read_dir(f._dir.path().join("estado/trabajos")).unwrap().next().is_none());

    let ejecutar = json!({
        "herramienta": "ea_encolar_rutina",
        "argumentos": sim["argumentos"],
        "precondicion": sim["precondicion"],
    });
    let (s, r) = post(&f, "/api/acciones/ejecutar", ejecutar.clone()).await;
    assert_eq!(s, StatusCode::OK);
    let trabajo = r["datos"]["trabajo"].as_str().unwrap();
    assert!(f._dir.path().join("estado/trabajos").join(trabajo).join("orden.json").exists());

    // El mundo ya no es el de la simulación: ni simular ni ejecutar de nuevo.
    let (s, _) = post(&f, "/api/acciones/ejecutar", ejecutar).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = post(&f, "/api/acciones/simular", cuerpo).await;
    assert_eq!(s, StatusCode::CONFLICT);
}

/// A1: el resultado de una acción dice qué produjo, para enlazarlo: el
/// trabajo y el id de su evento `accion_ejecutada` en el diario. Aceptar una
/// propuesta devuelve lo mismo en `resultado`; listarlas, no.
#[tokio::test]
async fn el_resultado_de_una_accion_trae_su_trabajo_y_su_evento() {
    let f = montar();
    let evento_de = |d: &Value, id: i64| d.as_array().unwrap().iter().find(|e| e["id"] == id).cloned();

    let cuerpo = json!({ "herramienta": "ea_encolar_rutina", "argumentos": { "slug": "revision-diaria" } });
    let (_, sim) = post(&f, "/api/acciones/simular", cuerpo).await;
    let (s, r) =
        post(&f, "/api/acciones/ejecutar", json!({ "herramienta": "ea_encolar_rutina", "argumentos": sim["argumentos"], "precondicion": sim["precondicion"] }))
            .await;
    assert_eq!(s, StatusCode::OK);
    let trabajo = r["trabajo"].as_str().unwrap().to_string();
    assert_eq!(r["datos"]["trabajo"], trabajo.as_str());
    let ev = r["evento"].as_i64().expect("trae el evento");
    let (_, d) = get(&f, "/api/diario").await;
    let e = evento_de(&d, ev).expect("el evento está en el diario");
    assert_eq!((e["accion"].as_str(), e["trabajo"].as_str()), (Some("accion_ejecutada"), Some(trabajo.as_str())));

    // La misma rutina, ahora por una propuesta aceptada en la web.
    let (s, _) = post(&f, &format!("/api/trabajos/{trabajo}/cancelar"), json!({})).await;
    assert_eq!(s, StatusCode::OK);
    let id = proponer_accion(&f, "ea_encolar_rutina", json!({ "slug": "revision-diaria" })).await;
    let (_, ps) = get(&f, "/api/propuestas").await;
    assert!(ps.as_array().unwrap().iter().all(|p| p.get("resultado").is_none()), "listar no trae resultado");
    let (s, p) = post(&f, &format!("/api/propuestas/{id}/decidir"), json!({ "decision": "aceptar" })).await;
    assert_eq!((s, p["ejecutada"]["estado"].as_str()), (StatusCode::OK, Some("ejecutada")), "{p}");
    let otro = p["resultado"]["trabajo"].as_str().expect("trae el trabajo").to_string();
    assert_ne!(otro, trabajo);
    let ev = p["resultado"]["evento"].as_i64().expect("trae el evento");
    let (_, d) = get(&f, "/api/diario").await;
    let e = evento_de(&d, ev).unwrap();
    assert_eq!((e["accion"].as_str(), e["trabajo"].as_str()), (Some("accion_ejecutada"), Some(otro.as_str())));

    // Rechazar no ejecuta nada: sin resultado.
    let (s, _) = post(&f, &format!("/api/trabajos/{otro}/cancelar"), json!({})).await;
    assert_eq!(s, StatusCode::OK);
    let id = proponer_accion(&f, "ea_encolar_rutina", json!({ "slug": "revision-diaria" })).await;
    let (_, p) = post(&f, &format!("/api/propuestas/{id}/decidir"), json!({ "decision": "rechazar" })).await;
    assert!(p.get("resultado").is_none(), "{p}");
}

#[tokio::test]
async fn la_consola_valida_y_no_ejecuta_irreversibles() {
    let f = montar();

    let (s, _) = post(&f, "/api/acciones/simular", json!({ "herramienta": "ea_status" })).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "solo las supervisadas");
    let (s, _) = post(&f, "/api/acciones/simular", json!({ "herramienta": "ea_encolar_rutina", "argumentos": { "slug": 1 } })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = post(&f, "/api/acciones/simular", json!({ "herramienta": "ea_reiniciar_servicio", "argumentos": { "servicio": "ea-mcp" } })).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);

    ea_bus::retirar_rutina(&f.state.config.rutinas_dir, "revision-diaria").unwrap();
    let borrar = json!({ "herramienta": "ea_eliminar_rutina", "argumentos": { "slug": "revision-diaria" } });
    let (s, sim) = post(&f, "/api/acciones/simular", borrar).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(sim["confirmable"], false);
    let (s, _) = post(
        &f,
        "/api/acciones/ejecutar",
        json!({
            "herramienta": "ea_eliminar_rutina",
            "argumentos": sim["argumentos"],
            "precondicion": sim["precondicion"],
        }),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(ea_bus::ruta_retirada(&f.state.config.rutinas_dir, "revision-diaria").unwrap().exists());
}

fn evento_mcp(actor: &str, accion: &str, ok: bool) -> NewEvent {
    NewEvent {
        actor: actor.into(),
        action: accion.into(),
        trabajo: None,
        outcome: Some(if ok { EventOutcome::Ok } else { EventOutcome::Error }),
        duration_ms: Some(3),
        detail: Some(json!({ "args": {} }).to_string()),
    }
}

#[tokio::test]
async fn el_historial_de_una_herramienta_incluye_simulaciones_y_ejecuciones() {
    let f = montar();
    {
        let store = f.state.store.lock().await;
        store.append_event(evento_mcp("claude-code", "mcp_encolar_rutina_simulada", true)).unwrap();
        store.append_event(evento_mcp("modelo-local", "mcp_encolar_rutina_denegada", false)).unwrap();
        // Ruido: otra herramienta, otra familia y un `accion_*` sin JSON.
        store.append_event(evento_mcp("claude-code", "mcp_registrar_rutina", true)).unwrap();
        store.append_event(evento_mcp("claude-code", "mcp_status", true)).unwrap();
        store.append_event(NewEvent { detail: Some("no es json".into()), ..evento_mcp("usuario", "accion_fallida", false) }).unwrap();
    }
    // Una propuesta (propuesta_encolada) que el usuario rechaza: el rechazo
    // solo trae el id de la propuesta.
    let id = proponer_accion(&f, "ea_encolar_rutina", json!({ "slug": "revision-diaria" })).await;
    let (s, _) = post(&f, &format!("/api/propuestas/{id}/decidir"), json!({ "decision": "rechazar" })).await;
    assert_eq!(s, StatusCode::OK);
    // Y una ejecución desde la consola (accion_ejecutada, canal web).
    let cuerpo = json!({ "herramienta": "ea_encolar_rutina", "argumentos": { "slug": "revision-diaria" } });
    let (_, sim) = post(&f, "/api/acciones/simular", cuerpo).await;
    let (s, _) =
        post(&f, "/api/acciones/ejecutar", json!({ "herramienta": "ea_encolar_rutina", "argumentos": sim["argumentos"], "precondicion": sim["precondicion"] }))
            .await;
    assert_eq!(s, StatusCode::OK);

    let (s, h) = get(&f, "/api/acciones/ea_encolar_rutina/historial").await;
    assert_eq!(s, StatusCode::OK);
    let h = h.as_array().unwrap();
    let acciones: Vec<&str> = h.iter().map(|e| e["accion"].as_str().unwrap()).collect();
    assert_eq!(
        acciones,
        ["accion_ejecutada", "propuesta_decidida", "propuesta_encolada", "mcp_encolar_rutina_denegada", "mcp_encolar_rutina_simulada",],
        "lo más reciente primero, sin lo de otras herramientas"
    );
    assert_eq!(h[0]["canal"], "web");
    assert_eq!(h[0]["actor"], "usuario");
    assert_eq!(h[0]["resultado"], "ok");
    assert_eq!(h[1]["propuesta"], id);
    assert_eq!(h[2]["actor"], "claude-code");
    assert_eq!(h[3]["canal"], "mcp");
    assert_eq!(h[3]["resultado"], "error");
    // Cada fila lleva el id de su evento en el diario.
    let (_, d) = get(&f, "/api/diario").await;
    let ids: Vec<i64> = d.as_array().unwrap().iter().map(|e| e["id"].as_i64().unwrap()).collect();
    assert!(h.iter().all(|e| ids.contains(&e["id"].as_i64().unwrap())));

    // Las directas también tienen historial; y como mucho diez eventos.
    {
        let store = f.state.store.lock().await;
        for _ in 0..12 {
            store.append_event(evento_mcp("claude-code", "mcp_status", true)).unwrap();
        }
    }
    let (s, h) = get(&f, "/api/acciones/ea_status/historial").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h.as_array().unwrap().len(), 10);
    assert!(h.as_array().unwrap().iter().all(|e| e["accion"] == "mcp_status"));

    let (s, _) = get(&f, "/api/acciones/ea_no_existe/historial").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn el_catalogo_guia_los_formularios_con_sugerencias_y_estados() {
    let f = montar();
    let propia = nota(&f, "claude-code", "Plan de la semana", "Repasar el backlog.");
    let ajena = nota(&f, "modelo-local", "Receta de lentejas", "Lentejas con chorizo.");
    let promovida = nota(&f, "modelo-local", "Hallazgo", "Ya compartido.");
    let (s, _) = post(&f, "/api/boveda/promover", json!({ "ruta": promovida })).await;
    assert_eq!(s, StatusCode::OK);

    let (s, cat) = get(&f, "/api/acciones").await;
    assert_eq!(s, StatusCode::OK);
    let campo = |h: &str, c: &str| {
        let h = cat.as_array().unwrap().iter().find(|x| x["nombre"] == h).unwrap();
        h["campos"].as_array().unwrap().iter().find(|x| x["nombre"] == c).unwrap().clone()
    };

    // La ruta de promover: las notas de los agentes que aún no se han promovido.
    let ruta = campo("ea_promover_nota", "ruta");
    let sug: Vec<&str> = ruta["sugerencias"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert!(sug.contains(&propia.as_str()) && sug.contains(&ajena.as_str()), "{sug:?}");
    assert!(!sug.contains(&promovida.as_str()), "una promovida no se vuelve a promover");
    assert!(sug.iter().all(|r| !r.starts_with("compartida/")));
    assert_eq!(ruta["opciones"], json!([]), "sigue siendo un campo libre");
    assert_eq!(campo("ea_promover_nota", "destino")["sugerencias"], json!([]));

    // El servicio: el estado de cada uno junto a la opción (sin systemctl, desconocido).
    let srv = campo("ea_reiniciar_servicio", "servicio");
    for o in srv["opciones"].as_array().unwrap() {
        let o = o.as_str().unwrap();
        assert_eq!(srv["estados"][o], if o == "ea-web" { "active" } else { "desconocido" }, "{o}");
    }

    // El slug: las rutinas registradas.
    assert_eq!(campo("ea_eliminar_rutina", "slug")["opciones"], json!(["revision-diaria"]));
    assert_eq!(campo("ea_encolar_rutina", "slug")["estados"], json!({}));
}
