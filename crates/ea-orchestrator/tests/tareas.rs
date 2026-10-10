//! El sondeo ejecuta las tareas del modelo local contra un doble de `ea-proxy`.

use std::sync::Mutex;

use ea_orchestrator::tareas::{ejecutar_pendientes, Completador};
use ea_store::{NewTask, Store, TaskOrigin, TaskStatus};

struct Doble {
    falla: bool,
    vistas: Mutex<Vec<String>>,
    /// Estado al que alguien pasa la tarea mientras el modelo "responde".
    entre_tanto: Option<(std::path::PathBuf, i64)>,
}

impl Doble {
    fn nuevo(falla: bool) -> Self {
        Self { falla, vistas: Mutex::new(Vec::new()), entre_tanto: None }
    }
}

impl Completador for Doble {
    async fn completar(&self, texto: &str) -> anyhow::Result<String> {
        self.vistas.lock().unwrap().push(texto.to_string());
        if let Some((db, id)) = &self.entre_tanto {
            let mut otro = Store::open_at(db).unwrap();
            otro.update_task_status(*id, TaskStatus::Cancelada, "usuario", None).unwrap();
        }
        if self.falla {
            anyhow::bail!("ea-proxy respondió 500");
        }
        Ok(format!("respuesta a «{texto}»"))
    }
}

fn tarea(s: &mut Store, para: &str, origen: TaskOrigin, peticion: &str) -> i64 {
    s.create_task(NewTask {
        created_by: "usuario".into(),
        assigned_to: para.into(),
        origin: origen,
        parent_task_id: None,
        title: peticion.into(),
        request: peticion.into(),
    })
    .unwrap()
    .id
}

#[tokio::test]
async fn ejecuta_solo_las_pendientes_del_modelo_local_y_en_orden() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open_at(&dir.path().join("t.db")).unwrap();
    let a = tarea(&mut s, "modelo-local", TaskOrigin::Boton, "primera");
    let de_claude = tarea(&mut s, "claude-code", TaskOrigin::Boton, "no es suya");
    let de_voz = tarea(&mut s, "modelo-local", TaskOrigin::Voz, "la lleva el turno de voz");
    let b = tarea(&mut s, "modelo-local", TaskOrigin::Agente, "segunda");

    let doble = Doble::nuevo(false);
    assert_eq!(ejecutar_pendientes(&mut s, &doble).await.unwrap(), 2);
    assert_eq!(*doble.vistas.lock().unwrap(), ["primera", "segunda"]);

    for (id, peticion) in [(a, "primera"), (b, "segunda")] {
        let t = s.get_task(id).unwrap();
        assert_eq!(t.status, TaskStatus::Completada);
        assert_eq!(t.result.as_deref(), Some(format!("respuesta a «{peticion}»").as_str()));
    }
    assert_eq!(s.get_task(de_claude).unwrap().status, TaskStatus::Pendiente);
    assert_eq!(s.get_task(de_voz).unwrap().status, TaskStatus::Pendiente);

    let acciones: Vec<String> =
        s.list_events_by_task(a).unwrap().into_iter().rev().map(|e| e.action).collect();
    assert_eq!(acciones, ["tarea_creada", "tarea_iniciada", "modelo_respuesta", "tarea_completada"]);

    assert_eq!(ejecutar_pendientes(&mut s, &doble).await.unwrap(), 0, "no se repiten");
}

#[tokio::test]
async fn un_fallo_del_modelo_deja_la_tarea_fallida_con_el_motivo() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open_at(&dir.path().join("t.db")).unwrap();
    let id = tarea(&mut s, "modelo-local", TaskOrigin::Boton, "x");

    ejecutar_pendientes(&mut s, &Doble::nuevo(true)).await.unwrap();
    let t = s.get_task(id).unwrap();
    assert_eq!(t.status, TaskStatus::Fallida);
    assert_eq!(t.result.as_deref(), Some("ea-proxy respondió 500"));
}

#[tokio::test]
async fn una_cancelacion_durante_la_respuesta_se_respeta() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let mut s = Store::open_at(&db).unwrap();
    let id = tarea(&mut s, "modelo-local", TaskOrigin::Boton, "x");

    let doble = Doble { entre_tanto: Some((db, id)), ..Doble::nuevo(false) };
    ejecutar_pendientes(&mut s, &doble).await.unwrap();
    let t = s.get_task(id).unwrap();
    assert_eq!(t.status, TaskStatus::Cancelada);
    assert_eq!(t.result, None);
}
