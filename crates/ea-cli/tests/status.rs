//! `ea status` con el binario real sobre un bus y una base temporales: el
//! texto exacto, con los trabajos por estado, las colas y el orquestador.

use std::path::Path;
use std::process::{Command, Stdio};

use ea_bus::{Bus, Evento, Fase, Orden};
use serde_json::json;

/// Cuatro trabajos que aparecen en un orden distinto del de los estados: una
/// misión cerrada, otra en cola, una obra en curso y una misión bloqueada en
/// revisión.
fn sembrar(bus: &Bus, wt: &str) {
    let ids = ["20260101T000001-00000001", "20260101T000002-00000002", "20260101T000003-00000003", "20260101T000004-00000004"];
    let fase = |n: u32| Fase { numero: n, titulo: format!("fase {n}"), instrucciones: "i".into(), entregable: "e".into(), criterio: "true".into(), skills: vec![] };
    for (i, id) in ids.iter().enumerate() {
        let base = if i == 2 {
            Orden::obra("una obra", wt, "o", vec![fase(1), fase(2)], None, "prueba")
        } else {
            Orden::mision(format!("misión {}", i + 1), wt, "o", "true", "prueba")
        };
        bus.crear_trabajo(Orden { id: id.to_string(), creada_en: "2026-01-01T00:00:00Z".into(), ..base }).unwrap();
    }
    for id in [ids[0], ids[2], ids[3]] {
        bus.retirar_de_colas(id).unwrap();
        bus.aplicar(id, Evento::Tomar).unwrap();
    }
    let informe = |v: serde_json::Value| serde_json::from_value(v).unwrap();
    bus.aplicar(ids[0], Evento::Entregar(informe(json!({ "resultado": "terminado", "duracion_s": 12 })))).unwrap();
    bus.aplicar(ids[0], Evento::Cerrar).unwrap();
    bus.aplicar(ids[3], Evento::Entregar(informe(json!({ "resultado": "bloqueado", "motivo": "no pasa", "duracion_s": 30 })))).unwrap();
}

fn status(dir: &Path) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_ea"))
        .arg("status")
        .env("EA_CONFIG_DIR", dir.join("config"))
        .env("EA_DB_PATH", dir.join("ea.db"))
        .env("EA_ESTADO_DIR", dir.join("estado"))
        .env("EA_BOVEDA_DIR", dir.join("boveda"))
        .stdin(Stdio::null())
        .output()
        .expect("lanzar ea");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().replace(&dir.join("ea.db").display().to_string(), "<db>")
}

const BUS: &str = "Trabajos por estado:
+-------------+----------+
| Estado      | Cantidad |
+========================+
| en_cola     | 1        |
|-------------+----------|
| ejecutando  | 1        |
|-------------+----------|
| en_revision | 1        |
|-------------+----------|
| en_espera   | 0        |
|-------------+----------|
| cerrado     | 1        |
|-------------+----------|
| cancelado   | 0        |
+-------------+----------+
Cola planificador (claude-code): 1
Cola ejecutor (modelo-local): 1
Semáforos: ningún worktree ocupado
";

#[test]
fn ea_status_dice_lo_mismo_que_siempre() {
    let d = tempfile::tempdir().unwrap();
    sembrar(&Bus::abrir_en(&d.path().join("estado")).unwrap(), &d.path().to_string_lossy());

    let cabecera = "Ecosistema de asistente — estado
---------------------------------
";
    let roles = "Propuestas pendientes: 0
Base de datos: <db>
planificador: Claude Code (su modelo propio)
ejecutor: Qwen Code (qwen3-coder-30b)

";
    assert_eq!(
        status(d.path()),
        format!("{cabecera}Orquestador: sin señal (ningún heartbeat registrado todavía)\n{roles}{BUS}\nÚltimos eventos:\n  (sin eventos todavía)\n")
    );

    // Con un latido: vivo, con su hora, y el latido entre los últimos eventos.
    let store = ea_store::Store::open_at(&d.path().join("ea.db")).unwrap();
    store
        .append_event(ea_store::NewEvent {
            actor: "orquestador".into(),
            action: "heartbeat".into(),
            trabajo: None,
            outcome: None,
            duration_ms: None,
            detail: None,
        })
        .unwrap();
    let ts = store.list_events(&Default::default()).unwrap()[0].ts;
    let hora = ts.format(time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second] UTC")).unwrap();
    let eventos = format!(
        "+-------------------------+-------------+-----------+---------+
| Cuándo                  | Actor       | Acción    | Trabajo |
+=============================================================+
| {hora} | orquestador | heartbeat |         |
+-------------------------+-------------+-----------+---------+
"
    );
    assert_eq!(
        status(d.path()),
        format!("{cabecera}Orquestador: vivo (heartbeat {hora})\n{roles}{BUS}\nÚltimos eventos:\n{eventos}")
    );
}
