//! Los ajustes: ver con origen, cambiar desde la web (solo lo que la web
//! puede) y el rol de los agentes.

use crate::comun::*;

fn clave_de<'a>(v: &'a Value, clave: &str) -> &'a Value {
    v["secciones"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["claves"].as_array().unwrap())
        .find(|c| c["clave"] == clave)
        .unwrap_or_else(|| panic!("falta {clave}"))
}

#[tokio::test]
async fn los_ajustes_se_ven_con_origen_y_edicion() {
    let f = montar();
    let (s, v) = get(&f, "/api/ajustes").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["existe"], false);
    assert_eq!(v["version"], "");
    assert!(v["fichero"].as_str().unwrap().starts_with(f._dir.path().to_str().unwrap()));
    assert_eq!(v["cliente"], json!({ "sondeo_ms": 3000, "diario_limite": 300 }));
    let ids: Vec<&str> = v["secciones"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["roles", "agentes", "modelos", "agente", "orquestador", "red", "rutas", "web", "escritorio", "biblioteca"]);
    let turnos = clave_de(&v, "agente.techo_topes.turnos");
    assert_eq!((turnos["valor"].clone(), turnos["origen"]["tipo"].clone()), (json!(100), json!("defecto")));
    assert_eq!((turnos["edicion"].clone(), turnos["endurecible"].clone()), (json!("web"), json!(true)));
    assert_eq!(clave_de(&v, "agente.sandbox")["edicion"], "terminal");
    assert_eq!(clave_de(&v, "red.puerto_web")["reinicio"], json!(["ea-web"]));
}

#[tokio::test]
async fn cambiar_un_ajuste_escribe_el_fichero_y_lo_apunta() {
    let f = montar();
    let (s, v) = post(&f, "/api/ajustes", json!({ "version": "", "cambios": { "web.sondeo_ms": 5000 } })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["aplicados"][0], json!({ "clave": "web.sondeo_ms", "antes": 3000, "despues": 5000, "aplica": "caliente", "reinicio": [] }));
    assert_eq!(v["ajustes"]["cliente"]["sondeo_ms"], 5000);
    let fichero = f._dir.path().join("config").join("ajustes.toml");
    assert!(std::fs::read_to_string(&fichero).unwrap().contains("sondeo_ms = 5000"));
    let (_, d) = get(&f, "/api/diario").await;
    let ev = d.as_array().unwrap().iter().find(|e| e["accion"] == "ajustes_cambiados").expect("evento");
    assert_eq!(ev["actor"], "usuario");
    // El detalle, byte a byte (es el mismo que deja la terminal, con su canal).
    let detalle =
        format!(r#"{{"canal":"web","fichero":{},"cambios":[{{"clave":"web.sondeo_ms","antes":3000,"despues":5000}}]}}"#, json!(fichero.display().to_string()));
    assert_eq!(ev["detalle"], detalle);
    // versión vieja: 409 y el fichero no cambia
    let (s, _) = post(&f, "/api/ajustes", json!({ "version": "", "cambios": { "web.sondeo_ms": 6000 } })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    // null restablece
    let version = v["ajustes"]["version"].clone();
    let (s, v) = post(&f, "/api/ajustes", json!({ "version": version, "cambios": { "web.sondeo_ms": null } })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["ajustes"]["cliente"]["sondeo_ms"], 3000);
}

#[tokio::test]
async fn la_web_no_afloja_ni_toca_lo_de_terminal() {
    let f = montar();
    let (s, _) = post(&f, "/api/ajustes", json!({ "version": "", "cambios": { "agente.sandbox": false } })).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = post(&f, "/api/ajustes", json!({ "version": "", "cambios": { "agente.skills.pueden_elegir": [] } })).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, v) = post(&f, "/api/ajustes", json!({ "version": "", "cambios": { "agente.techo_topes.turnos": 20 } })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let version = v["ajustes"]["version"].clone();
    let (s, _) = post(&f, "/api/ajustes", json!({ "version": version, "cambios": { "agente.techo_topes.turnos": 50 } })).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = post(&f, "/api/ajustes", json!({ "version": version, "cambios": { "modelo.base_url": "http://192.168.1.2:8080/v1" } })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = post(&f, "/api/ajustes", json!({ "version": version, "cambios": { "no.existe": 1 } })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // roles (017): el ejecutor se cambia desde aquí; un agente no ocupa los dos
    let (s, _) = post(&f, "/api/ajustes", json!({ "version": version, "cambios": { "agentes.codex.bin": "/tmp/x" } })).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, v) = post(&f, "/api/ajustes", json!({ "version": version, "cambios": { "roles.ejecutor": "codex" } })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["aplicados"][0]["clave"], "roles.ejecutor");
    let version = v["ajustes"]["version"].clone();
    let (s, v) = post(&f, "/api/ajustes", json!({ "version": version, "cambios": { "roles.planificador": "codex" } })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let cuerpo = json!({ "version": version, "cambios": { "web.sondeo_ms": 5000 } });
    let (s, _) = enviar(&f, Method::POST, "/api/ajustes", "localhost:8092", Some(cuerpo.clone()), false).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = enviar(&f, Method::POST, "/api/ajustes", "evil.example", Some(cuerpo), true).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (_, v) = get(&f, "/api/ajustes").await;
    assert_eq!(clave_de(&v, "agente.techo_topes.turnos")["valor"], 20);
    assert_eq!(clave_de(&v, "agente.sandbox")["valor"], true);
}

#[tokio::test]
async fn los_agentes_dicen_su_rol_y_el_cambio_de_ejecutor_se_ve_en_el_estado() {
    let f = montar();
    // Un endpoint cerrado: la prueba no pregunta al llama-server de verdad.
    let fichero = f.state.config.ajustes.fichero();
    std::fs::create_dir_all(fichero.parent().unwrap()).unwrap();
    std::fs::write(&fichero, "[modelos.local]\nbase_url = \"http://127.0.0.1:9/v1\"\n").unwrap();
    let (s, v) = get(&f, "/api/agentes").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let ag = |v: &Value, id: &str| v["agentes"].as_array().unwrap().iter().find(|a| a["id"] == id).unwrap().clone();
    let ids: Vec<&str> = v["agentes"].as_array().unwrap().iter().map(|a| a["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ea_ajustes::IDS_AGENTES);
    assert_eq!(ag(&v, "qwen-code")["rol"], "ejecutor");
    assert_eq!(ag(&v, "qwen-code")["modelo"]["nombre"], "qwen3-coder-30b");
    assert_eq!(ag(&v, "qwen-code")["modelo_accesible"], false);
    assert_eq!(ag(&v, "claude-code")["rol"], "planificador");
    assert_eq!(ag(&v, "claude-code")["modelo_accesible"], Value::Null);
    assert_eq!(ag(&v, "antigravity")["admite"], json!(["planificador"]));
    assert_eq!(v["fijados"], json!({}));

    let (s, r) = post(&f, "/api/ajustes", json!({ "version": v["version"], "cambios": { "roles.ejecutor": "codex" } })).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let (_, v) = get(&f, "/api/agentes").await;
    assert_eq!(ag(&v, "codex")["rol"], "ejecutor");
    assert_eq!(ag(&v, "qwen-code")["rol"], Value::Null);
    let (_, e) = get(&f, "/api/estado").await;
    let ejecutor = e["agentes"].as_array().unwrap().iter().find(|a| a["rol"] == "ejecutor").unwrap().clone();
    assert_eq!((ejecutor["nombre"].clone(), ejecutor["agente"].clone()), (json!("modelo-local"), json!("codex")));
    assert_eq!((ejecutor["agente_nombre"].clone(), ejecutor["modelo"].clone()), (json!("Codex"), json!("qwen3-coder-30b")));
    let (_, d) = get(&f, "/api/diario?accion=ajustes_").await;
    assert!(d.to_string().contains("roles.ejecutor"), "{d}");
}
