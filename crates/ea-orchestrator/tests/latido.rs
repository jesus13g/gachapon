//! U-07: el binario real del orquestador, con todo su estado en un directorio
//! temporal, escribe su latido en `orquestador.json` del bus y en el diario
//! deja solo su arranque y, tras SIGTERM, su parada; ningún `heartbeat`.
#![cfg(unix)]

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn esperar<T>(que: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let fin = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < fin, "no llegó: {que}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn acciones(db: &Path) -> Vec<String> {
    let s = ea_store::Store::open_at(db).unwrap();
    let mut v: Vec<String> = s.list_events(&Default::default()).unwrap().into_iter().map(|e| e.action).collect();
    v.reverse();
    v
}

fn parar(hijo: &mut Child) {
    let ok = Command::new("kill").args(["-TERM", &hijo.id().to_string()]).status().unwrap().success();
    assert!(ok, "kill -TERM");
    let fin = Instant::now() + Duration::from_secs(40);
    while hijo.try_wait().unwrap().is_none() {
        if Instant::now() > fin {
            let _ = hijo.kill();
            panic!("el orquestador no paró con SIGTERM");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn el_latido_va_al_fichero_del_bus_y_el_diario_solo_guarda_arranque_y_parada() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    let mut hijo =
        ea_pruebas::cli::ea(env!("CARGO_BIN_EXE_ea-orchestrator"), r).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("lanzar ea-orchestrator");
    let pid = hijo.id();
    let estado = r.join("estado");
    let latido = esperar("el latido", || ea_bus::latido_orquestador(&estado));
    assert_eq!(latido.pid, pid);
    assert!(latido.sandbox.is_some(), "{latido:?}");
    assert!(ea_bus::estado_orquestador(&estado, 60).vivo);
    esperar("el arranque en el diario", || acciones(&r.join("ea.db")).contains(&"orquestador_arrancado".to_string()).then_some(()));
    parar(&mut hijo);
    let v = acciones(&r.join("ea.db"));
    assert_eq!(v.iter().filter(|a| a.as_str() == "orquestador_arrancado").count(), 1, "{v:?}");
    assert_eq!(v.last().map(String::as_str), Some("orquestador_parado"), "{v:?}");
    assert!(!v.iter().any(|a| a == "heartbeat"), "{v:?}");
}
