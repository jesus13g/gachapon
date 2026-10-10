//! Pruebas del flujo de mutaciones supervisadas con el sistema simulado:
//! nunca se toca `systemd` ni el disco.

use ea_acciones::*;
use ea_store::{
    EstadoEjecucion, EstadoPropuesta, EventFilter, NewTask, Store, StoreError, TaskFilter,
    TaskOrigin, TaskStatus,
};
use serde_json::{json, Value};

fn store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_at(&dir.path().join("t.db")).unwrap();
    (s, dir)
}

fn sistema() -> SistemaSimulado {
    SistemaSimulado::default()
        .con_servicio("ea-mcp", "active")
        .con_rutina("vieja", "[rutina]\ndescripcion = \"x\"\n")
        .con_rutina("revision-diaria", "[rutina]\ndescripcion = \"y\"\n")
}

fn tarea(s: &mut Store) -> i64 {
    s.create_task(NewTask {
        created_by: "usuario".into(),
        assigned_to: "claude-code".into(),
        origin: TaskOrigin::Manual,
        parent_task_id: None,
        title: "Prueba".into(),
        request: "x".into(),
    })
    .unwrap()
    .id
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

async fn simular(s: &Store, sis: &SistemaSimulado, nombre: &str, args: &Value) -> Result<Simulacion, ErrorAccion> {
    let h = observar(sis, nombre, args).await?;
    simular_con(s, sis, &h, nombre, args)
}

async fn proponer_(s: &Store, sis: &SistemaSimulado, prop: &str, nombre: &str, args: &Value) -> Result<(ea_store::Propuesta, bool), ErrorAccion> {
    let h = observar(sis, nombre, args).await?;
    proponer(s, sis, &h, prop, None, None, nombre, args)
}

fn todas_las_llamadas(t: i64) -> Vec<(&'static str, Value)> {
    vec![
        ("ea_cambiar_estado_tarea", json!({ "id": t, "estado": "completada", "resultado": "ok" })),
        ("ea_encolar_rutina", json!({ "slug": "revision-diaria" })),
        ("ea_reiniciar_servicio", json!({ "servicio": "ea-mcp" })),
        ("ea_eliminar_rutina", json!({ "slug": "vieja" })),
    ]
}

#[tokio::test]
async fn simular_describe_sin_ningun_efecto() {
    let (mut s, _d) = store();
    let t = tarea(&mut s);
    let sis = sistema();
    let eventos_antes = acciones(&s).len();
    for (nombre, args) in todas_las_llamadas(t) {
        let sim = simular(&s, &sis, nombre, &args).await.unwrap();
        assert!(!sim.texto.is_empty(), "{nombre}");
        assert_eq!(sim.riesgo, buscar(nombre).unwrap().riesgo);
    }
    assert_eq!(acciones(&s).len(), eventos_antes, "simular no escribe en el diario ni en ningún sitio");
    assert!(sis.mutaciones().is_empty());
    assert_eq!(s.get_task(t).unwrap().status, TaskStatus::Pendiente);
    assert_eq!(s.list_tasks(&TaskFilter::default()).unwrap().len(), 1);
    assert!(sis.rutina_contenido("vieja").unwrap().is_some());
}

#[tokio::test]
async fn argumentos_invalidos_y_fuera_de_lista_se_rechazan() {
    let (mut s, _d) = store();
    let t = tarea(&mut s);
    let sis = sistema();
    for (nombre, args) in [
        ("ea_reiniciar_servicio", json!({ "servicio": "sshd" })),
        ("ea_reiniciar_servicio", json!({ "servicio": "ea-mcp; rm -rf /" })),
        ("ea_eliminar_rutina", json!({ "slug": "../../etc/passwd" })),
        ("ea_cambiar_estado_tarea", json!({ "id": t, "estado": "inventado" })),
        ("ea_cambiar_estado_tarea", json!({ "id": t, "estado": "completada", "extra": 1 })),
        ("ea_cambiar_estado_tarea", json!({ "id": 999, "estado": "completada" })),
        ("ea_cambiar_estado_tarea", json!({ "id": t, "estado": "pendiente" })), // ya lo está
        ("ea_eliminar_rutina", json!({ "slug": "no-existe" })),
        ("ea_crear_tarea", json!({})), // no es supervisada
        ("ea_inexistente", json!({})),
    ] {
        assert!(simular(&s, &sis, nombre, &args).await.is_err(), "{nombre} {args}");
    }
}

#[tokio::test]
async fn proponer_encola_pero_no_ejecuta_y_no_duplica() {
    let (mut s, _d) = store();
    let t = tarea(&mut s);
    let sis = sistema();
    let args = json!({ "id": t, "estado": "completada" });

    let (p, nueva) = proponer_(&s, &sis, "claude-code", "ea_cambiar_estado_tarea", &args).await
        .unwrap();
    assert!(nueva);
    assert_eq!(p.estado, EstadoPropuesta::Pendiente);
    assert_eq!(p.proponente.as_deref(), Some("claude-code"));
    let a = p.accion.as_ref().unwrap();
    assert_eq!(a.riesgo, "reversible");
    assert!(a.simulacion.contains("completada"));
    assert!(p.ejecucion.is_none());
    assert_eq!(s.get_task(t).unwrap().status, TaskStatus::Pendiente, "proponer no ejecuta");

    let (p2, nueva2) = proponer_(&s, &sis, "modelo-local", "ea_cambiar_estado_tarea", &args).await
        .unwrap();
    assert!(!nueva2);
    assert_eq!(p2.id, p.id);
    assert!(acciones(&s).contains(&"propuesta_encolada".to_string()));
}

#[tokio::test]
async fn la_cola_de_propuestas_con_accion_tiene_tope() {
    let (s, _d) = store();
    let mut sis = sistema();
    for i in 0..=MAX_PEND {
        sis = sis.con_rutina(&format!("r{i}"), "x");
    }
    for i in 0..MAX_PEND {
        proponer_(&s, &sis, "claude-code", "ea_eliminar_rutina", &json!({ "slug": format!("r{i}") })).await
            .unwrap();
    }
    let e = proponer_(&s, &sis, "claude-code", "ea_eliminar_rutina", &json!({ "slug": format!("r{MAX_PEND}") })).await
        .unwrap_err();
    assert!(e.to_string().contains("pendientes"));
}
const MAX_PEND: usize = ea_acciones::flujo::MAX_PENDIENTES_CON_ACCION;

#[tokio::test]
async fn aceptar_ejecuta_y_registra_en_propuesta_y_diario() {
    let (mut s, _d) = store();
    let t = tarea(&mut s);
    let sis = sistema();
    let (p, _) = proponer_(&s, &sis, "claude-code", "ea_cambiar_estado_tarea", &json!({ "id": t, "estado": "completada", "resultado": "hecho" })).await
    .unwrap();

    let p = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Web).await.unwrap();
    assert_eq!(p.estado, EstadoPropuesta::Aceptada);
    let ej = p.ejecucion.unwrap();
    assert_eq!(ej.estado, EstadoEjecucion::Ejecutada);
    assert!(ej.detalle.unwrap().contains("completada"));
    assert_eq!(s.get_task(t).unwrap().status, TaskStatus::Completada);
    assert_eq!(s.get_task(t).unwrap().result.as_deref(), Some("hecho"));

    let ev = s
        .list_events(&EventFilter { limit: Some(50), ..Default::default() })
        .unwrap()
        .into_iter()
        .find(|e| e.action == "accion_ejecutada")
        .expect("evento accion_ejecutada");
    assert_eq!(ev.actor, "usuario");
    assert_eq!(ev.task_id, Some(t));
    let d: Value = serde_json::from_str(&ev.detail.unwrap()).unwrap();
    assert_eq!(d["herramienta"], "ea_cambiar_estado_tarea");
    assert_eq!(d["argumentos"]["resultado"], "hecho");
    assert_eq!(d["proponente"], "claude-code");
    assert_eq!(d["canal"], "web");
    assert_eq!(d["propuesta"], p.id);
}

#[tokio::test]
async fn aceptar_dos_veces_no_ejecuta_dos_veces() {
    let (mut s, _d) = store();
    let sis = sistema();
    let (p, _) = proponer_(&s, &sis, "claude-code", "ea_reiniciar_servicio", &json!({ "servicio": "ea-mcp" })).await
        .unwrap();
    aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Terminal).await.unwrap();
    let e = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Terminal).await.unwrap_err();
    assert!(matches!(e, ErrorAccion::Store(StoreError::PropuestaYaDecidida(_))));
    assert_eq!(sis.mutaciones(), vec!["reiniciar:ea-mcp"]);
    // y rechazarla después tampoco procede
    assert!(rechazar_propuesta(&s, p.id, "usuario", Canal::Web).is_err());
}

#[tokio::test]
async fn el_almacen_impide_aceptar_sin_ejecutar_una_propuesta_con_accion() {
    let (s, _d) = store();
    let sis = sistema();
    let (p, _) = proponer_(&s, &sis, "claude-code", "ea_reiniciar_servicio", &json!({ "servicio": "ea-mcp" })).await
        .unwrap();
    let e = s.decidir_propuesta(p.id, EstadoPropuesta::Aceptada).unwrap_err();
    assert!(matches!(e, StoreError::PropuestaConAccion(_)));
    assert_eq!(s.get_propuesta(p.id).unwrap().estado, EstadoPropuesta::Pendiente);
}

#[tokio::test]
async fn revalida_antes_de_ejecutar_si_el_estado_cambio() {
    let (mut s, _d) = store();
    let t = tarea(&mut s);
    let sis = sistema();
    let (p, _) = proponer_(&s, &sis, "claude-code", "ea_cambiar_estado_tarea", &json!({ "id": t, "estado": "completada" })).await
        .unwrap();
    // Mientras tanto, alguien inicia la tarea.
    s.update_task_status(t, TaskStatus::EnProgreso, "usuario", None).unwrap();

    let p = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Terminal).await.unwrap();
    let ej = p.ejecucion.unwrap();
    assert_eq!(ej.estado, EstadoEjecucion::Fallida);
    assert!(ej.detalle.unwrap().contains("cambió desde la simulación"));
    assert_eq!(s.get_task(t).unwrap().status, TaskStatus::EnProgreso, "no hizo otra cosa");
    assert!(acciones(&s).contains(&"accion_fallida".to_string()));

    // Rutina que cambia de contenido entre la simulación y la ejecución.
    let (p, _) = proponer_(&s, &sis, "claude-code", "ea_eliminar_rutina", &json!({ "slug": "vieja" })).await
        .unwrap();
    sis.rutinas.lock().unwrap().insert("vieja".into(), "otro contenido".into());
    let p = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Terminal).await.unwrap();
    assert_eq!(p.ejecucion.unwrap().estado, EstadoEjecucion::Fallida);
    assert!(sis.rutina_contenido("vieja").unwrap().is_some());
    assert!(sis.mutaciones().is_empty());

    // Servicio desinstalado.
    let (p, _) = proponer_(&s, &sis, "claude-code", "ea_reiniciar_servicio", &json!({ "servicio": "ea-mcp" })).await
        .unwrap();
    sis.servicios.lock().unwrap().clear();
    let p = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Terminal).await.unwrap();
    assert_eq!(p.ejecucion.unwrap().estado, EstadoEjecucion::Fallida);
}

#[tokio::test]
async fn un_fallo_del_sistema_queda_como_ejecucion_fallida() {
    let (mut s, _d) = store();
    let sis = sistema();
    *sis.fallo_reinicio.lock().unwrap() = Some("Unit not found".into());
    let (p, _) = proponer_(&s, &sis, "claude-code", "ea_reiniciar_servicio", &json!({ "servicio": "ea-mcp" })).await
        .unwrap();
    let p = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Web).await.unwrap();
    let ej = p.ejecucion.unwrap();
    assert_eq!(ej.estado, EstadoEjecucion::Fallida);
    assert!(ej.detalle.unwrap().contains("Unit not found"));
}

#[tokio::test]
async fn un_irreversible_no_se_confirma_desde_la_web_y_la_propuesta_sigue_pendiente() {
    let (mut s, _d) = store();
    let sis = sistema();
    let (p, _) = proponer_(&s, &sis, "claude-code", "ea_eliminar_rutina", &json!({ "slug": "vieja" })).await
        .unwrap();
    assert_eq!(p.accion.as_ref().unwrap().riesgo, "irreversible");

    let e = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Web).await.unwrap_err();
    assert!(matches!(e, ErrorAccion::Prohibida(_)), "{e}");
    assert_eq!(s.get_propuesta(p.id).unwrap().estado, EstadoPropuesta::Pendiente);
    assert!(sis.rutina_contenido("vieja").unwrap().is_some());
    assert!(acciones(&s).contains(&"accion_fallida".to_string()), "el intento por la web queda en el diario");

    let e = ejecutar_directo(&mut s, &sis, "usuario", Canal::Web, "ea_eliminar_rutina", &json!({ "slug": "vieja" }))
        .await
        .unwrap_err();
    assert!(matches!(e, ErrorAccion::Prohibida(_)));
    assert!(acciones(&s).contains(&"accion_fallida".to_string()), "el intento queda en el diario");
    assert!(sis.mutaciones().is_empty());

    // Desde la terminal sí.
    let p = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Terminal).await.unwrap();
    assert_eq!(p.ejecucion.unwrap().estado, EstadoEjecucion::Ejecutada);
    assert!(sis.rutina_contenido("vieja").unwrap().is_none());
    assert_eq!(sis.mutaciones(), vec!["eliminar_rutina:vieja"]);
}

#[tokio::test]
async fn las_propuestas_de_texto_libre_siguen_siendo_constancia() {
    let (mut s, _d) = store();
    let sis = sistema();
    let run = s.start_routine_run("revision-diaria", None).unwrap();
    let p = s.create_propuesta(run.id, "Revisar a mano").unwrap();
    assert!(p.accion.is_none());
    let p = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Web).await.unwrap();
    assert_eq!(p.estado, EstadoPropuesta::Aceptada);
    assert!(p.ejecucion.is_none());
    assert!(sis.mutaciones().is_empty());
    assert!(acciones(&s).contains(&"propuesta_decidida".to_string()));
}

#[tokio::test]
async fn ejecutar_directo_registra_en_el_diario_como_la_via_de_propuestas() {
    let (mut s, _d) = store();
    let t = tarea(&mut s);
    let sis = sistema();
    let r = ejecutar_directo(&mut s, &sis, "usuario", Canal::Web, "ea_cambiar_estado_tarea", &json!({ "id": t, "estado": "en_progreso" }))
        .await
        .unwrap();
    assert_eq!(r.tarea, Some(t));
    let acc = acciones(&s);
    assert!(acc.contains(&"tarea_iniciada".to_string()));
    assert!(acc.contains(&"accion_ejecutada".to_string()));

    ejecutar_directo(&mut s, &sis, "usuario", Canal::Web, "ea_reiniciar_servicio", &json!({ "servicio": "ea-mcp" }))
        .await
        .unwrap();
    assert_eq!(sis.mutaciones(), vec!["reiniciar:ea-mcp"]);

    let r = ejecutar_directo(&mut s, &sis, "usuario", Canal::Web, "ea_encolar_rutina", &json!({ "slug": "revision-diaria" }))
        .await
        .unwrap();
    let nueva = s.get_task(r.tarea.unwrap()).unwrap();
    assert_eq!((nueva.assigned_to.as_str(), nueva.origin), ("orquestador", TaskOrigin::Boton));
    // No se encola dos veces la misma rutina.
    assert!(ejecutar_directo(&mut s, &sis, "usuario", Canal::Web, "ea_encolar_rutina", &json!({ "slug": "revision-diaria" }))
        .await
        .is_err());
}

#[test]
fn invariantes_del_catalogo() {
    // Solo ea_crear_tarea y ea_escribir_nota (la región propia de cada agente)
    // son mutantes de ejecución directa (excepciones razonadas).
    let directas_mutantes: Vec<_> = CATALOGO
        .iter()
        .filter(|h| h.riesgo.es_mutante() && h.ejecucion == Ejecucion::Directa)
        .map(|h| h.nombre)
        .collect();
    assert_eq!(directas_mutantes, ["ea_crear_tarea", "ea_escribir_nota"]);
    // Los irreversibles son siempre supervisados y nunca van al modelo local.
    for h in CATALOGO.iter().filter(|h| h.riesgo == Riesgo::Irreversible) {
        assert_eq!(h.ejecucion, Ejecucion::Supervisada);
        assert!(!h.local, "{}", h.nombre);
    }
    // Todas las lecturas están en la proyección local.
    assert!(CATALOGO.iter().filter(|h| h.riesgo == Riesgo::Lectura).all(|h| h.local));
    let mut nombres: Vec<_> = CATALOGO.iter().map(|h| h.nombre).collect();
    nombres.sort();
    nombres.dedup();
    assert_eq!(nombres.len(), CATALOGO.len());
}

#[test]
fn autorizacion_por_actor() {
    let h = |n| buscar(n).unwrap();
    // Lo desatendido es de solo lectura, sin excepciones.
    for herr in CATALOGO {
        assert_eq!(
            autorizar_mcp("orquestador", herr).is_ok(),
            herr.riesgo == Riesgo::Lectura,
            "{}",
            herr.nombre
        );
    }
    // El modelo local: lecturas y como mucho proponer cambios de estado.
    assert!(autorizar_mcp("modelo-local", h("ea_status")).is_ok());
    assert!(autorizar_mcp("modelo-local", h("ea_cambiar_estado_tarea")).is_ok());
    for n in ["ea_eliminar_rutina", "ea_reiniciar_servicio", "ea_encolar_rutina", "ea_crear_tarea"] {
        assert!(autorizar_mcp("modelo-local", h(n)).is_err(), "{n}");
    }
    assert!(autorizar_mcp("claude-code", h("ea_eliminar_rutina")).is_ok());
}

#[test]
fn el_canal_web_nunca_confirma_irreversibles() {
    assert!(Canal::Web.puede_confirmar(Riesgo::Reversible).is_ok());
    assert!(Canal::Web.puede_confirmar(Riesgo::Irreversible).is_err());
    assert!(Canal::Terminal.puede_confirmar(Riesgo::Irreversible).is_ok());
}

#[test]
fn los_campos_declarados_casan_con_los_argumentos_que_se_validan() {
    for h in CATALOGO.iter().filter(|h| h.ejecucion == Ejecucion::Supervisada) {
        let campos = campos(h.nombre);
        assert!(!campos.is_empty(), "{} no declara campos", h.nombre);

        let valor = |c: &Campo| match (c.entero, c.opciones.first(), c.nombre) {
            (true, _, _) => json!(1),
            (_, Some(o), _) => json!(o),
            (_, _, "ruta") => json!("claude-code/tema/nota.md"),
            (_, _, "destino") => json!("tema/nota.md"),
            _ => json!("revision-diaria"),
        };
        // Solo los obligatorios, y luego todos: ambas formas deben validar.
        for solo_obligatorios in [true, false] {
            let args: serde_json::Map<String, Value> = campos
                .iter()
                .filter(|c| c.obligatorio || !solo_obligatorios)
                .map(|c| (c.nombre.to_string(), valor(c)))
                .collect();
            validar_argumentos(h.nombre, &Value::Object(args))
                .unwrap_or_else(|e| panic!("{}: {e}", h.nombre));
        }
    }
}
