//! El diario: filtros, latidos, anotaciones del usuario y paginación.

use crate::comun::*;

#[tokio::test]
async fn el_diario_oculta_los_latidos_salvo_que_se_pidan() {
    let f = montar();
    {
        let store = f.state.store.lock().await;
        for accion in ["heartbeat", "heartbeat", "trabajo_creado"] {
            store
                .append_event(NewEvent {
                    actor: "orquestador".into(),
                    action: accion.into(),
                    trabajo: if accion == "heartbeat" { None } else { Some("m-1".into()) },
                    outcome: Some(EventOutcome::Ok),
                    duration_ms: None,
                    detail: None,
                })
                .unwrap();
        }
    }

    let (_, sin) = get(&f, "/api/diario").await;
    assert_eq!(sin.as_array().unwrap().len(), 1);
    assert_eq!(sin[0]["accion"], "trabajo_creado");
    assert_eq!(sin[0]["trabajo"], "m-1");

    let (_, del_trabajo) = get(&f, "/api/diario?latidos=1&trabajo=m-1").await;
    assert_eq!(del_trabajo.as_array().unwrap().len(), 1);

    let (_, con) = get(&f, "/api/diario?latidos=1").await;
    assert_eq!(con.as_array().unwrap().len(), 3);

    let (_, e) = get(&f, "/api/estado").await;
    assert_eq!(e["orquestador"]["vivo"], true);
}

fn acciones(d: &Value) -> Vec<String> {
    let mut v: Vec<String> = d.as_array().unwrap().iter().map(|e| e["accion"].as_str().unwrap().to_string()).collect();
    v.sort();
    v
}

#[tokio::test]
async fn el_diario_filtra_por_actor_resultado_y_familia() {
    let f = montar();
    {
        let store = f.state.store.lock().await;
        for (actor, accion, ok) in [
            ("modelo-local", "mcp_leer_nota", true),
            ("modelo-local", "mcp_escribir_nota_denegada", false),
            ("usuario", "propuesta_decidida", false),
            ("usuario", "bus_crear", true),
            ("orquestador", "bus_entregar", false),
        ] {
            store
                .append_event(NewEvent {
                    actor: actor.into(),
                    action: accion.into(),
                    trabajo: None,
                    outcome: Some(if ok { EventOutcome::Ok } else { EventOutcome::Error }),
                    duration_ms: None,
                    detail: None,
                })
                .unwrap();
        }
    }

    let (_, d) = get(&f, "/api/diario?actor=modelo-local").await;
    assert_eq!(acciones(&d), ["mcp_escribir_nota_denegada", "mcp_leer_nota"]);

    let (_, d) = get(&f, "/api/diario?resultado=error").await;
    assert_eq!(acciones(&d), ["bus_entregar", "mcp_escribir_nota_denegada", "propuesta_decidida"]);

    let (_, d) = get(&f, "/api/diario?accion=bus_").await;
    assert_eq!(acciones(&d), ["bus_crear", "bus_entregar"]);

    let (_, d) = get(&f, "/api/diario?actor=modelo-local&resultado=error&accion=mcp_").await;
    assert_eq!(acciones(&d), ["mcp_escribir_nota_denegada"]);

    // Vacíos, como los manda la barra sin filtro: no filtran.
    let (_, d) = get(&f, "/api/diario?actor=&resultado=&accion=").await;
    assert_eq!(d.as_array().unwrap().len(), 5);

    // Las transiciones del bus quedan en la columna `trabajo`: el filtro por
    // trabajo (el que usa la ficha) las encuentra.
    let (s, l) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::CREATED);
    let id = l["trabajo"].as_str().unwrap();
    let (_, d) = get(&f, &format!("/api/diario?trabajo={id}&accion=bus_")).await;
    assert_eq!(acciones(&d), ["bus_crear"]);
}

#[tokio::test]
async fn el_diario_rechaza_un_resultado_o_una_accion_no_validos() {
    let f = montar();
    for q in ["accion=bus%25", "accion=BUS_", "accion=bus-crear", "accion=x%27%20OR%201", "resultado=quizas"] {
        let (s, _) = get(&f, &format!("/api/diario?{q}")).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{q}");
    }
    let (s, _) = get(&f, "/api/diario?accion=propuesta_&resultado=ok").await;
    assert_eq!(s, StatusCode::OK);
}

/// Cada filtro del diario que no vale responde 400 con su mensaje de siempre;
/// un campo vacío no filtra, salvo las fechas, que no valen vacías.
#[tokio::test]
async fn cada_filtro_del_diario_que_no_vale_dice_por_que() {
    let f = montar();
    for (q, mensaje) in [
        ("desde=ayer", "fecha inválida: 'ayer' (usa RFC3339 o AAAA-MM-DD)"),
        ("hasta=2026-13-01", "fecha inválida: '2026-13-01' (usa RFC3339 o AAAA-MM-DD)"),
        ("desde=", "fecha inválida: '' (usa RFC3339 o AAAA-MM-DD)"),
        ("resultado=quizas", "valor de resultado inválido: quizas"),
        ("accion=BUS_", "acción no válida: «BUS_» (solo [a-z_])"),
        ("antes_de=0", "antes_de no válido: «0» (un id de evento)"),
        ("antes_de=x", "antes_de no válido: «x» (un id de evento)"),
        // El primero que falla, en el orden de siempre: resultado, acción, cursor y fechas.
        ("desde=ayer&antes_de=0&accion=X&resultado=quizas", "valor de resultado inválido: quizas"),
        ("desde=ayer&antes_de=0", "antes_de no válido: «0» (un id de evento)"),
    ] {
        let (s, e) = get(&f, &format!("/api/diario?{q}")).await;
        assert_eq!((s, e["error"].as_str()), (StatusCode::BAD_REQUEST, Some(mensaje)), "{q}");
    }
    for q in ["trabajo=&actor=&resultado=&accion=&antes_de=", "desde=2026-01-01&hasta=2026-01-01T00:00:00%2B02:00", "limite=0", "limite=100000"] {
        let (s, _) = get(&f, &format!("/api/diario?{q}")).await;
        assert_eq!(s, StatusCode::OK, "{q}");
    }
}

// ------------------------------------------------- D4: anotar el diario

/// Todas las filas del diario, latidos incluidos, tal como las guarda la base.
async fn filas_del_diario(f: &Fixture) -> Vec<ea_store::Event> {
    let filtro = ea_store::EventFilter::default();
    f.state.store.lock().await.list_events(&filtro).unwrap()
}

#[tokio::test]
async fn anotar_el_diario_anade_un_evento_y_no_modifica_ninguno() {
    let f = montar();
    let original = f
        .state
        .store
        .lock()
        .await
        .append_event(NewEvent {
            actor: "orquestador".into(),
            action: "bus_entregar".into(),
            trabajo: Some("m-1".into()),
            outcome: Some(EventOutcome::Error),
            duration_ms: Some(42),
            detail: Some("{\"motivo\":\"equivocado\"}".into()),
        })
        .unwrap();
    let antes = filas_del_diario(&f).await;

    // Constancia suelta, con trabajo.
    let (s, n1) = post(&f, "/api/diario/anotar", json!({"texto": "he reiniciado llama-server a mano", "trabajo": "m-1"})).await;
    assert_eq!(s, StatusCode::CREATED, "{n1}");
    assert_eq!(n1["actor"], "usuario");
    assert_eq!(n1["accion"], "usuario_nota");
    assert_eq!(n1["trabajo"], "m-1");
    let d1: Value = serde_json::from_str(n1["detalle"].as_str().unwrap()).unwrap();
    assert_eq!(d1, json!({"texto": "he reiniciado llama-server a mano", "trabajo": "m-1"}));

    // Corrección del evento original.
    let (s, n2) = post(&f, "/api/diario/anotar", json!({"texto": "no falló: lo cancelé yo", "corrige": original.id})).await;
    assert_eq!(s, StatusCode::CREATED, "{n2}");
    let d2: Value = serde_json::from_str(n2["detalle"].as_str().unwrap()).unwrap();
    assert_eq!(d2["corrige"], original.id);

    // Dos filas más, y las de antes, idénticas.
    let despues = filas_del_diario(&f).await;
    assert_eq!(despues.len(), antes.len() + 2);
    for e in &antes {
        let ahora = despues.iter().find(|x| x.id == e.id).expect("sigue estando");
        assert_eq!(serde_json::to_value(ahora).unwrap(), serde_json::to_value(e).unwrap());
    }

    // El corregido lo dice en el listado; la nota está en la familia `usuario_`.
    let (_, d) = get(&f, "/api/diario").await;
    let fila = d.as_array().unwrap().iter().find(|e| e["id"] == original.id).unwrap();
    assert_eq!(fila["corregido_por"], json!([n2["id"]]));
    assert_eq!(fila["detalle"], "{\"motivo\":\"equivocado\"}");
    let (_, d) = get(&f, "/api/diario?accion=usuario_").await;
    assert_eq!(acciones(&d), ["usuario_nota", "usuario_nota"]);
}

#[tokio::test]
async fn anotar_el_diario_rechaza_lo_invalido_sin_escribir() {
    let f = montar();
    let antes = filas_del_diario(&f).await.len();
    for cuerpo in [
        json!({"texto": ""}),
        json!({"texto": "   \n "}),
        json!({"texto": "x".repeat(10_001)}),
        json!({"texto": "x", "corrige": 999}),
        json!({"texto": "x", "trabajo": "../etc"}),
        json!({"texto": "x", "otro": 1}),
        json!({"corrige": 1}),
        json!({"texto": 3}),
    ] {
        let (s, r) = post(&f, "/api/diario/anotar", cuerpo.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{cuerpo}: {r}");
        assert!(r["error"].is_string(), "mensaje: {r}");
    }
    assert_eq!(filas_del_diario(&f).await.len(), antes);
    // Justo en el tope, sí.
    let (s, _) = post(&f, "/api/diario/anotar", json!({"texto": "ñ".repeat(10_000)})).await;
    assert_eq!(s, StatusCode::CREATED);
}

#[tokio::test]
async fn anotar_el_diario_sin_clave_da_403() {
    let f = montar();
    for clave in [None, Some("mala")] {
        let (s, _) = enviar_con_clave(
            &f,
            Method::POST,
            "/api/diario/anotar",
            "localhost:8092",
            Some(json!({"texto": "suplantación"})),
            true,
            clave,
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{clave:?}");
    }
    assert!(filas_del_diario(&f).await.is_empty());
}

#[tokio::test]
async fn el_diario_pagina_por_cursor_sin_solapes() {
    let f = montar();
    let anotar = |actor: &str, accion: &str, ok: bool| NewEvent {
        actor: actor.into(),
        action: accion.into(),
        trabajo: None,
        outcome: Some(if ok { EventOutcome::Ok } else { EventOutcome::Error }),
        duration_ms: None,
        detail: None,
    };
    {
        let store = f.state.store.lock().await;
        // 11 errores del bus entre otros eventos que el filtro descarta.
        for i in 0..11 {
            store.append_event(anotar("orquestador", "bus_entregar", false)).unwrap();
            store.append_event(anotar("orquestador", "bus_entregar", true)).unwrap();
            if i % 2 == 0 {
                store.append_event(anotar("usuario", "propuesta_decidida", false)).unwrap();
            }
        }
    }

    let filtro = "accion=bus_&resultado=error&limite=4";
    let mut ids: Vec<i64> = Vec::new();
    let mut antes: Option<i64> = None;
    let mut paginas = 0;
    loop {
        let ruta = match antes {
            Some(id) => format!("/api/diario?{filtro}&antes_de={id}"),
            None => format!("/api/diario?{filtro}"),
        };
        let (s, d) = get(&f, &ruta).await;
        assert_eq!(s, StatusCode::OK, "{ruta}");
        let pagina = d.as_array().unwrap();
        if pagina.is_empty() {
            break;
        }
        paginas += 1;
        assert!(pagina.len() <= 4);
        for e in pagina {
            assert_eq!(e["accion"], "bus_entregar");
            assert_eq!(e["resultado"], "error");
        }
        if paginas == 1 {
            // Un evento nuevo entre página y página no desplaza el cursor.
            let store = f.state.store.lock().await;
            store.append_event(anotar("orquestador", "bus_entregar", false)).unwrap();
        }
        ids.extend(pagina.iter().map(|e| e["id"].as_i64().unwrap()));
        antes = ids.last().copied();
    }
    assert_eq!(paginas, 3, "4 + 4 + 3");
    assert_eq!(ids.len(), 11, "todos, sin huecos: {ids:?}");
    let mut esperado = ids.clone();
    esperado.sort_unstable_by(|a, b| b.cmp(a));
    esperado.dedup();
    assert_eq!(ids, esperado, "de más reciente a más antiguo y sin repetidos");

    // La primera página (lo que refresca el sondeo) ya trae el nuevo.
    let (_, d) = get(&f, &format!("/api/diario?{filtro}")).await;
    assert!(d[0]["id"].as_i64().unwrap() > ids[0]);

    for q in ["antes_de=abc", "antes_de=-3", "antes_de=0", "antes_de=1.5"] {
        let (s, _) = get(&f, &format!("/api/diario?{q}")).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{q}");
    }
    // Vacío, como lo manda la barra sin cursor: no pagina.
    let (s, _) = get(&f, "/api/diario?antes_de=").await;
    assert_eq!(s, StatusCode::OK);
}
