//! Contrato con la app de escritorio (`escritorio/`, spec 021): lo que lee del
//! panel sin tipos (`escritorio/src/{api,vigia}.rs`) y la clave que comparten.
//! Las pruebas de `escritorio/` no ven la API: si algo de aquí cambia, la
//! bandeja, los avisos o la clave dejan de funcionar sin que falle nada allí.
//! Un cambio a propósito se hace a la vez en los dos lados.

use crate::comun::*;

/// Como pregunta el escritorio (`api::obtener`): GET a `127.0.0.1:<puerto>`,
/// sin clave, y la respuesta como JSON.
async fn como_el_escritorio(f: &Fixture, ruta: &str) -> (StatusCode, Value) {
    let (s, t) = enviar_con_clave(f, Method::GET, ruta, "127.0.0.1:8092", None, false, None).await;
    (s, serde_json::from_str(&t).unwrap_or(Value::Null))
}

/// `vigia::estado_de`: `orquestador.vivo` (booleano) y `servicios[].activo`
/// (texto; «active» y «desconocido» cuentan como sanos).
#[tokio::test]
async fn el_estado_lleva_lo_que_mira_la_bandeja() {
    let f = montar();
    let (s, e) = como_el_escritorio(&f, "/api/estado").await;
    assert_eq!(s, StatusCode::OK, "{e}");
    assert_eq!(e["orquestador"]["vivo"], false);
    let servicios = e["servicios"].as_array().expect("servicios es una lista");
    assert!(!servicios.is_empty());
    // Sin gestor de servicios (`systemctl: false`), «desconocido»: no degrada la bandeja.
    assert!(servicios.iter().all(|s| s["activo"] == "desconocido"), "{e}");

    bus(&f).latir_orquestador(None).unwrap();
    let (_, e) = como_el_escritorio(&f, "/api/estado").await;
    assert_eq!(e["orquestador"]["vivo"], true, "{e}");
}

/// `vigia::VigiaTrabajos::observar`: una lista (no un objeto) y de cada fila
/// `id`, `estado` y `titulo` como texto; los estados finales que avisan
/// (`en_revision`, `en_espera`, `cerrado`) se llaman así en el bus.
#[tokio::test]
async fn los_trabajos_son_una_lista_con_id_estado_y_titulo() {
    let f = montar();
    let ids = ea_pruebas::sembrar_trabajos(&bus(&f), tmp!());
    let (s, v) = como_el_escritorio(&f, "/api/trabajos").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let filas = v.as_array().expect("/api/trabajos es una lista");
    let mut vistos: Vec<(&str, &str, &str)> = filas
        .iter()
        .map(|t| (t["id"].as_str().unwrap(), t["estado"].as_str().unwrap(), t["titulo"].as_str().unwrap()))
        .collect();
    vistos.sort();
    assert_eq!(
        vistos,
        [
            (ids[0].as_str(), "cerrado", "misión 1"),
            (ids[1].as_str(), "en_cola", "misión 2"),
            (ids[2].as_str(), "ejecutando", "una obra"),
            (ids[3].as_str(), "en_revision", "misión 4"),
        ]
    );
    for final_ in ["en_revision", "en_espera", "cerrado"] {
        assert!(ea_bus::EstadoTrabajo::TODOS.iter().any(|e| e.as_str() == final_), "{final_}");
    }
}

/// `clave::leer` y `wsl::leer_clave`: la clave está en
/// `ea_ajustes::fichero_clave_web` (`DIR_WEB`/`FICHERO_CLAVE` del directorio
/// de ejecución) y, recortada, la acepta `ea_ajustes::clave_web_valida`.
#[test]
fn la_clave_se_guarda_donde_y_como_la_lee_el_escritorio() {
    let d = tempfile::tempdir().unwrap();
    let clave = ea_web::clave::ClaveSesion::nueva().unwrap();
    let fichero = ea_web::clave::guardar(d.path(), &clave).unwrap();
    assert_eq!(fichero, ea_ajustes::fichero_clave_web(d.path()));
    assert_eq!(fichero, d.path().join(ea_ajustes::DIR_WEB).join(ea_ajustes::FICHERO_CLAVE));
    let leida = std::fs::read_to_string(&fichero).unwrap();
    assert!(ea_ajustes::clave_web_valida(leida.trim()), "{leida}");
    assert_eq!(leida.trim(), clave.texto());
}

/// El escritorio abre `http://127.0.0.1:<puerto>/` y deja la clave en
/// `sessionStorage['ea-clave']` (`clave::guion_inicio`): la página sale con
/// ese Host, la lee de ahí y la manda en la cabecera que exige la API.
#[tokio::test]
async fn el_panel_toma_la_clave_que_deja_el_escritorio() {
    let f = montar();
    let (s, html) = enviar_con_clave(&f, Method::GET, "/", "127.0.0.1:8092", None, false, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(html.contains("cliente.js"), "{html}");
    let (s, js) = enviar_con_clave(&f, Method::GET, "/cliente.js", "127.0.0.1:8092", None, false, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(js.contains("sessionStorage.getItem('ea-clave')"), "la página ya no lee la clave del escritorio");
    assert!(js.contains("'X-EA-Clave'"));
    assert!("X-EA-Clave".eq_ignore_ascii_case(ea_web::clave::CABECERA));
}
