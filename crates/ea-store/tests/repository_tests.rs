use std::path::Path;

use ea_store::{
    AccionPropuesta, EstadoEjecucion, EstadoPropuesta, EventFilter, EventOutcome, NewEvent, NuevaPropuesta,
    PropuestaFilter, Store, StoreError,
};

fn open_test_store() -> Store {
    Store::open_at(Path::new(":memory:")).expect("abrir store en memoria")
}

fn evento(actor: &str, action: &str, trabajo: Option<&str>) -> NewEvent {
    NewEvent {
        actor: actor.into(),
        action: action.into(),
        trabajo: trabajo.map(Into::into),
        outcome: Some(ea_store::EventOutcome::Ok),
        duration_ms: None,
        detail: None,
    }
}

#[test]
fn un_evento_suelto_se_lista_por_actor() {
    let store = open_test_store();
    store.append_event(evento("orquestador", "heartbeat", None)).unwrap();
    store.append_event(evento("otro", "x", None)).unwrap();

    let events = store
        .list_events(&EventFilter {
            actor: Some("orquestador".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].action, "heartbeat");
    assert_eq!(events[0].trabajo, None);
}

#[test]
fn los_eventos_se_asocian_y_filtran_por_trabajo() {
    let store = open_test_store();
    store.append_event(evento("orquestador", "trabajo_iniciado", Some("m-001"))).unwrap();
    store.append_event(evento("orquestador", "trabajo_iniciado", Some("m-002"))).unwrap();
    store.append_event(evento("orquestador", "heartbeat", None)).unwrap();

    let del_trabajo = store.list_events_by_trabajo("m-001").unwrap();
    assert_eq!(del_trabajo.len(), 1);
    assert_eq!(del_trabajo[0].trabajo.as_deref(), Some("m-001"));
}

#[test]
fn list_events_can_exclude_an_action() {
    let store = open_test_store();
    for action in ["heartbeat", "heartbeat", "voz_escucha_iniciada"] {
        store.append_event(evento("orquestador", action, None)).unwrap();
    }
    let events = store
        .list_events(&EventFilter {
            exclude_action: Some("heartbeat".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].action, "voz_escucha_iniciada");
}

#[test]
fn filtra_por_resultado_y_prefijo_de_accion() {
    let store = open_test_store();
    let con = |actor: &str, action: &str, outcome| NewEvent {
        outcome: Some(outcome),
        ..evento(actor, action, None)
    };
    store.append_event(con("usuario", "bus_crear", EventOutcome::Ok)).unwrap();
    store.append_event(con("orquestador", "bus_entregar", EventOutcome::Error)).unwrap();
    store.append_event(con("usuario", "propuesta_decidida", EventOutcome::Error)).unwrap();
    // `busXcrear` casaría con `bus_` si el `_` no se escapase.
    store.append_event(con("usuario", "busXcrear", EventOutcome::Error)).unwrap();

    let lista = |f: EventFilter| -> Vec<String> {
        store.list_events(&f).unwrap().into_iter().map(|e| e.action).collect()
    };

    let mut bus = lista(EventFilter { action_prefix: Some("bus_".into()), ..Default::default() });
    bus.sort();
    assert_eq!(bus, ["bus_crear", "bus_entregar"]);

    let mut errores = lista(EventFilter { outcome: Some(EventOutcome::Error), ..Default::default() });
    errores.sort();
    assert_eq!(errores, ["busXcrear", "bus_entregar", "propuesta_decidida"]);

    let ambos = lista(EventFilter {
        outcome: Some(EventOutcome::Error),
        action_prefix: Some("bus_".into()),
        ..Default::default()
    });
    assert_eq!(ambos, ["bus_entregar"]);

    // Un comodín en el prefijo se compara literalmente.
    assert!(lista(EventFilter { action_prefix: Some("%".into()), ..Default::default() }).is_empty());
}

#[test]
fn el_cursor_before_id_pagina_sin_huecos_ni_repetidos() {
    let store = open_test_store();
    // 7 eventos `bus_` intercalados con otros que el filtro descarta.
    for i in 0..7 {
        store.append_event(evento("usuario", "bus_crear", Some(&format!("m-{i}")))).unwrap();
        store.append_event(evento("usuario", "propuesta_decidida", None)).unwrap();
    }
    let filtro = |antes: Option<i64>| EventFilter {
        action_prefix: Some("bus_".into()),
        before_id: antes,
        limit: Some(3),
        ..Default::default()
    };

    let mut vistos: Vec<i64> = Vec::new();
    let mut antes = None;
    let mut paginas = 0;
    loop {
        let pagina = store.list_events(&filtro(antes)).unwrap();
        if pagina.is_empty() {
            break;
        }
        paginas += 1;
        if paginas == 2 {
            // Un evento nuevo entre página y página no se cuela ni desplaza.
            store.append_event(evento("usuario", "bus_crear", Some("m-nuevo"))).unwrap();
        }
        assert!(pagina.iter().all(|e| e.action == "bus_crear"));
        vistos.extend(pagina.iter().map(|e| e.id));
        antes = pagina.last().map(|e| e.id);
    }
    assert_eq!(paginas, 3, "3 + 3 + 1");
    let mut ordenados = vistos.clone();
    ordenados.sort_unstable_by(|a, b| b.cmp(a));
    ordenados.dedup();
    assert_eq!(vistos, ordenados, "de más reciente a más antiguo, sin repetidos");
    assert_eq!(vistos.len(), 7);
    assert!(store.list_events(&filtro(Some(9_999))).unwrap().is_empty(), "id inexistente: nada");
}

#[test]
fn el_diario_es_append_only_a_nivel_de_motor() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("diario.db");
    let store = Store::open_at(&path).unwrap();
    let e = store.append_event(evento("cli-test", "evento_suelto", None)).unwrap();
    assert_eq!(store.get_event(e.id).unwrap().action, "evento_suelto");

    let conn = rusqlite::Connection::open(&path).unwrap();
    assert!(conn.execute("DELETE FROM events", []).is_err());
    assert!(conn.execute("UPDATE events SET actor = 'b'", []).is_err());
}

fn accion() -> AccionPropuesta {
    AccionPropuesta {
        herramienta: "ea_reiniciar_servicio".into(),
        argumentos: serde_json::json!({ "servicio": "ea-mcp" }),
        riesgo: "reversible".into(),
        simulacion: "Reiniciaría ea-mcp".into(),
        precondicion: serde_json::json!({ "cargado": true }),
    }
}

fn texto(descripcion: &str) -> NuevaPropuesta {
    NuevaPropuesta {
        proponente: "claude-code".into(),
        descripcion: descripcion.into(),
        accion: None,
    }
}

#[test]
fn propuesta_lifecycle_pendiente_to_decidida() {
    let store = open_test_store();
    let propuesta = store.crear_propuesta(texto("Se detectaron errores")).unwrap();
    assert_eq!(propuesta.estado, EstadoPropuesta::Pendiente);

    let pendientes = store
        .list_propuestas(&PropuestaFilter {
            estado: Some(EstadoPropuesta::Pendiente),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(pendientes.len(), 1);

    let decidida = store.decidir_propuesta(propuesta.id, EstadoPropuesta::Aceptada).unwrap();
    assert_eq!(decidida.estado, EstadoPropuesta::Aceptada);
    assert!(decidida.decidido_en.is_some());

    let pendientes = store
        .list_propuestas(&PropuestaFilter {
            estado: Some(EstadoPropuesta::Pendiente),
            ..Default::default()
        })
        .unwrap();
    assert!(pendientes.is_empty());
}

#[test]
fn propuesta_con_accion_se_reclama_una_sola_vez() {
    let store = open_test_store();
    let p = store
        .crear_propuesta(NuevaPropuesta {
            proponente: "claude-code".into(),
            descripcion: "reiniciar".into(),
            accion: Some(accion()),
        })
        .unwrap();
    assert_eq!(p.accion.as_ref().unwrap(), &accion());
    assert_eq!(p.proponente.as_deref(), Some("claude-code"));

    // Aceptarla "a secas" está prohibido: aceptar es ejecutar.
    assert!(matches!(
        store.decidir_propuesta(p.id, EstadoPropuesta::Aceptada),
        Err(StoreError::PropuestaConAccion(_))
    ));

    let r = store.reclamar_propuesta(p.id).unwrap();
    assert_eq!(r.estado, EstadoPropuesta::Aceptada);
    assert_eq!(r.ejecucion.unwrap().estado, EstadoEjecucion::Ejecutando);
    assert!(matches!(store.reclamar_propuesta(p.id), Err(StoreError::PropuestaYaDecidida(_))));

    let fin = store
        .registrar_ejecucion_propuesta(p.id, EstadoEjecucion::Ejecutada, "ok")
        .unwrap();
    assert_eq!(fin.ejecucion.as_ref().unwrap().estado, EstadoEjecucion::Ejecutada);
    assert!(fin.ejecucion.unwrap().ejecutada_en.is_some());
    // Un resultado ya registrado no se reescribe.
    assert!(store.registrar_ejecucion_propuesta(p.id, EstadoEjecucion::Fallida, "x").is_err());
}

#[test]
fn una_propuesta_de_texto_no_se_puede_reclamar_ni_decidir_dos_veces() {
    let store = open_test_store();
    let p = store.crear_propuesta(texto("texto")).unwrap();
    assert!(matches!(store.reclamar_propuesta(p.id), Err(StoreError::PropuestaSinAccion(_))));
    store.decidir_propuesta(p.id, EstadoPropuesta::Rechazada).unwrap();
    assert!(matches!(
        store.decidir_propuesta(p.id, EstadoPropuesta::Aceptada),
        Err(StoreError::PropuestaYaDecidida(_))
    ));
}

/// Varios procesos arrancando a la vez sobre una base recién creada (los
/// servicios al iniciar sesión): todos deben abrirla, no solo el primero.
#[test]
fn aperturas_simultaneas_de_una_base_nueva_no_fallan() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ruta = dir.path().join("nueva.db");
    let barrera = std::sync::Arc::new(std::sync::Barrier::new(8));

    let hilos: Vec<_> = (0..8)
        .map(|_| {
            let ruta = ruta.clone();
            let barrera = barrera.clone();
            std::thread::spawn(move || {
                barrera.wait();
                Store::open_at(&ruta).map(|_| ())
            })
        })
        .collect();

    for hilo in hilos {
        hilo.join().expect("hilo").expect("abrir la base nueva");
    }
}

#[test]
fn una_base_del_esquema_antiguo_se_rechaza_sin_tocarla() {
    let dir = tempfile::tempdir().unwrap();
    // Con la tabla `tasks` y versión 1, y con versión mayor que la conocida.
    for (nombre, sql) in [
        ("con_tasks.db", "CREATE TABLE tasks (id INTEGER PRIMARY KEY); PRAGMA user_version = 1;"),
        ("version_5.db", "CREATE TABLE otra (id INTEGER); PRAGMA user_version = 5;"),
    ] {
        let path = dir.path().join(nombre);
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(sql).unwrap();
            conn.execute_batch("INSERT INTO tasks DEFAULT VALUES;").ok();
        }
        let antes = std::fs::read(&path).unwrap();
        let err = Store::open_at(&path).err().expect("debe fallar");
        assert!(matches!(err, StoreError::EsquemaAntiguo), "{nombre}: {err}");
        assert!(err.to_string().contains("esquema antiguo"));
        // Intacta: ni migrada ni borrada.
        let conn = rusqlite::Connection::open(&path).unwrap();
        let tablas: i64 = conn
            .query_row("SELECT count(*) FROM sqlite_master WHERE name = 'events'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tablas, 0);
        drop(conn);
        assert_eq!(std::fs::read(&path).unwrap().len(), antes.len());
    }
}

#[test]
fn el_indice_de_la_boveda_busca_sin_acentos_ni_mayusculas() {
    let mut store = open_test_store();
    let e = |ruta: &str, titulo: &str| ea_store::EntradaIndice {
        ruta: ruta.into(),
        region: "compartida".into(),
        autor: None,
        titulo: titulo.into(),
        origen: None,
        promocion: None,
        creada: None,
        actualizada: None,
        tags: "".into(),
        mtime_ns: 1,
        bytes: 10,
    };
    store.boveda_indexar(&e("compartida/a.md", "Café"), "Una reunión muy larga").unwrap();
    store.boveda_indexar(&e("compartida/b.md", "Otra"), "nada").unwrap();
    let r = store.boveda_buscar("\"CAFE\" \"reunion\"", None, 10, 8).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].ruta, "compartida/a.md");
    assert!(r[0].fragmento.contains('«'));
    // Reindexar reemplaza, no duplica; olvidar quita también el texto.
    store.boveda_indexar(&e("compartida/a.md", "Té"), "otra cosa").unwrap();
    assert!(store.boveda_buscar("\"cafe\"", None, 10, 8).unwrap().is_empty());
    assert_eq!(store.boveda_buscar("\"te\"", None, 10, 8).unwrap().len(), 1);
    store.boveda_olvidar("compartida/a.md").unwrap();
    assert!(store.boveda_buscar("\"te\"", None, 10, 8).unwrap().is_empty());
    assert_eq!(store.boveda_contar_por_region().unwrap(), vec![("compartida".to_string(), 1)]);
    store.boveda_indexar(&e("compartida/a/c.md", "C"), "x").unwrap();
    let todas: Vec<_> = store.boveda_todas().unwrap().into_iter().map(|r| r.ruta).collect();
    assert_eq!(todas, vec!["compartida/a/c.md".to_string(), "compartida/b.md".to_string()]);
}

#[test]
fn las_versiones_de_una_rutina_se_listan_de_la_mas_reciente_a_la_primera() {
    let store = open_test_store();
    let fila = |slug: &str, version: i64, por: &str| ea_store::FilaRutina {
        slug: slug.into(),
        version,
        definicion: format!("slug = \"{slug}\"\nversion = {version}\n"),
        creada_en: format!("2026-10-{:02}T10:00:00Z", 10 + version),
        creada_por: por.into(),
    };
    store.guardar_rutina(&fila("informe", 1, "usuario")).unwrap();
    store.guardar_rutina(&fila("informe", 2, "claude-code")).unwrap();
    store.guardar_rutina(&fila("otra", 1, "usuario")).unwrap();
    let v = store.listar_versiones_rutina("informe").unwrap();
    assert_eq!(v.iter().map(|f| f.version).collect::<Vec<_>>(), vec![2, 1]);
    assert_eq!(v[0].creada_por, "claude-code");
    assert!(v[1].definicion.contains("version = 1"));
    assert!(store.listar_versiones_rutina("no-existe").unwrap().is_empty());
}
