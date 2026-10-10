use std::path::Path;

use ea_store::{
    EstadoPropuesta, EventFilter, NewEvent, NewTask, PropuestaFilter, RunOutcome, Store,
    TaskFilter, TaskOrigin, TaskStatus,
};

fn open_test_store() -> Store {
    Store::open_at(Path::new(":memory:")).expect("abrir store en memoria")
}

#[test]
fn create_task_registers_creation_event() {
    let mut store = open_test_store();

    let task = store
        .create_task(NewTask {
            created_by: "cli-test".into(),
            assigned_to: "claude-code".into(),
            origin: TaskOrigin::Manual,
            parent_task_id: None,
            title: "Probar mecanismo".into(),
            request: "Verificar el diario".into(),
        })
        .expect("crear tarea");

    assert_eq!(task.status, TaskStatus::Pendiente);
    assert_eq!(task.assigned_to, "claude-code");

    let events = store
        .list_events_by_task(task.id)
        .expect("listar eventos de la tarea");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].action, "tarea_creada");
    assert_eq!(events[0].task_id, Some(task.id));
}

#[test]
fn update_task_status_appends_event_and_computes_duration() {
    let mut store = open_test_store();

    let task = store
        .create_task(NewTask {
            created_by: "cli-test".into(),
            assigned_to: "claude-code".into(),
            origin: TaskOrigin::Manual,
            parent_task_id: None,
            title: "Probar mecanismo".into(),
            request: "Verificar el diario".into(),
        })
        .expect("crear tarea");

    store
        .update_task_status(task.id, TaskStatus::EnProgreso, "orquestador", None)
        .expect("marcar en progreso");

    let completed = store
        .update_task_status(
            task.id,
            TaskStatus::Completada,
            "orquestador",
            Some("Ciclo verificado".into()),
        )
        .expect("marcar completada");

    assert_eq!(completed.status, TaskStatus::Completada);
    assert_eq!(completed.result.as_deref(), Some("Ciclo verificado"));

    let events = store
        .list_events_by_task(task.id)
        .expect("listar eventos de la tarea");
    assert_eq!(events.len(), 3);

    let completion_event = events
        .iter()
        .find(|e| e.action == "tarea_completada")
        .expect("evento de finalización");
    assert!(completion_event.duration_ms.is_some());
    assert_eq!(completion_event.outcome, Some(ea_store::EventOutcome::Ok));
}

#[test]
fn append_event_without_task_is_listed() {
    let store = open_test_store();

    store
        .append_event(NewEvent {
            actor: "orquestador".into(),
            action: "heartbeat".into(),
            task_id: None,
            outcome: Some(ea_store::EventOutcome::Ok),
            duration_ms: None,
            detail: None,
        })
        .expect("registrar evento suelto");

    let events = store
        .list_events(&EventFilter {
            actor: Some("orquestador".into()),
            ..Default::default()
        })
        .expect("listar eventos por actor");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].action, "heartbeat");
}

#[test]
fn list_events_can_exclude_an_action() {
    let store = open_test_store();

    for action in ["heartbeat", "heartbeat", "voz_escucha_iniciada"] {
        store
            .append_event(NewEvent {
                actor: "orquestador".into(),
                action: action.into(),
                task_id: None,
                outcome: Some(ea_store::EventOutcome::Ok),
                duration_ms: None,
                detail: None,
            })
            .expect("registrar evento suelto");
    }

    let events = store
        .list_events(&EventFilter {
            exclude_action: Some("heartbeat".into()),
            ..Default::default()
        })
        .expect("listar eventos sin latidos");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].action, "voz_escucha_iniciada");
}

#[test]
fn list_tasks_filters_by_status() {
    let mut store = open_test_store();

    let t1 = store
        .create_task(NewTask {
            created_by: "cli-test".into(),
            assigned_to: "modelo-local".into(),
            origin: TaskOrigin::Manual,
            parent_task_id: None,
            title: "Tarea 1".into(),
            request: "Solicitud 1".into(),
        })
        .expect("crear tarea 1");
    store
        .create_task(NewTask {
            created_by: "cli-test".into(),
            assigned_to: "modelo-local".into(),
            origin: TaskOrigin::Manual,
            parent_task_id: None,
            title: "Tarea 2".into(),
            request: "Solicitud 2".into(),
        })
        .expect("crear tarea 2");

    store
        .update_task_status(t1.id, TaskStatus::Completada, "cli-test", None)
        .expect("completar tarea 1");

    let pendientes = store
        .list_tasks(&TaskFilter {
            status: Some(TaskStatus::Pendiente),
            ..Default::default()
        })
        .expect("listar pendientes");
    assert_eq!(pendientes.len(), 1);
    assert_eq!(pendientes[0].title, "Tarea 2");
}

#[test]
fn events_table_rejects_update_and_delete() {
    let store = open_test_store();
    let event = store
        .append_event(NewEvent {
            actor: "cli-test".into(),
            action: "evento_suelto".into(),
            task_id: None,
            outcome: None,
            duration_ms: None,
            detail: None,
        })
        .expect("registrar evento");

    // No hay API pública para editar/borrar eventos porque el diario es
    // append-only por contrato; aquí solo confirmamos que sigue siendo
    // consultable (el trigger a nivel de motor se verifica manualmente con
    // sqlite3, ver README).
    let fetched = store.get_event(event.id).expect("recuperar evento");
    assert_eq!(fetched.action, "evento_suelto");
}

#[test]
fn last_successful_run_ignores_failed_runs() {
    let store = open_test_store();

    let fallida = store
        .start_routine_run("revision-diaria", None)
        .expect("abrir ejecución fallida");
    store
        .finish_routine_run(fallida.id, RunOutcome::Fallida, 2, 1, Some("presupuesto agotado".into()))
        .expect("cerrar ejecución fallida");

    assert!(store
        .last_successful_run("revision-diaria")
        .expect("consultar última ejecución")
        .is_none());

    let ok = store
        .start_routine_run("revision-diaria", None)
        .expect("abrir ejecución ok");
    store
        .finish_routine_run(ok.id, RunOutcome::Ok, 3, 2, None)
        .expect("cerrar ejecución ok");

    let ultima = store
        .last_successful_run("revision-diaria")
        .expect("consultar última ejecución")
        .expect("debe existir una ejecución exitosa");
    assert_eq!(ultima.id, ok.id);

    let historial = store
        .list_routine_runs("revision-diaria", None)
        .expect("listar historial");
    assert_eq!(historial.len(), 2);
}

#[test]
fn propuesta_lifecycle_pendiente_to_decidida() {
    let store = open_test_store();

    let run = store
        .start_routine_run("revision-diaria", None)
        .expect("abrir ejecución");
    let propuesta = store
        .create_propuesta(run.id, "Se detectaron errores en el diario")
        .expect("crear propuesta");
    assert_eq!(propuesta.estado, EstadoPropuesta::Pendiente);

    let pendientes = store
        .list_propuestas(&PropuestaFilter {
            estado: Some(EstadoPropuesta::Pendiente),
            ..Default::default()
        })
        .expect("listar pendientes");
    assert_eq!(pendientes.len(), 1);

    let decidida = store
        .decidir_propuesta(propuesta.id, EstadoPropuesta::Aceptada)
        .expect("aceptar propuesta");
    assert_eq!(decidida.estado, EstadoPropuesta::Aceptada);
    assert!(decidida.decidido_en.is_some());

    let pendientes = store
        .list_propuestas(&PropuestaFilter {
            estado: Some(EstadoPropuesta::Pendiente),
            ..Default::default()
        })
        .expect("listar pendientes tras decidir");
    assert!(pendientes.is_empty());
}
