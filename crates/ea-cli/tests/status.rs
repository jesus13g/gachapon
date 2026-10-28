//! `ea status` con el binario real sobre un bus y una base temporales: el
//! texto exacto, con los trabajos por estado, las colas y el orquestador.

use std::path::Path;

use ea_bus::Bus;
use ea_pruebas::sembrar_trabajos;

fn status(dir: &Path) -> String {
    let o = ea_pruebas::cli::ea(env!("CARGO_BIN_EXE_ea"), dir).arg("status").output().expect("lanzar ea");
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
    sembrar_trabajos(&Bus::abrir_en(&d.path().join("estado")).unwrap(), &d.path().to_string_lossy());

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

    // Un latido viejo en el diario (de antes de U-07) sale entre los últimos
    // eventos, pero ya no dice nada del orquestador.
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
    let formato = time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second] UTC");
    let hora = ts.format(formato).unwrap();
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
        format!("{cabecera}Orquestador: sin señal (ningún heartbeat registrado todavía)\n{roles}{BUS}\nÚltimos eventos:\n{eventos}")
    );

    // Con el latido en el fichero del bus: vivo, con su hora y sus trabajadores.
    let l = Bus::abrir_en(&d.path().join("estado")).unwrap().latir_orquestador(None).unwrap();
    let t = time::OffsetDateTime::parse(&l.ts, &time::format_description::well_known::Rfc3339).unwrap();
    let hora = t.format(formato).unwrap();
    assert_eq!(
        status(d.path()),
        format!("{cabecera}Orquestador: vivo (heartbeat {hora}, 0 trabajadores)\n{roles}{BUS}\nÚltimos eventos:\n{eventos}")
    );
}
