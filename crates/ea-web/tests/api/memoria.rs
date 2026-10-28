//! La memoria de Claude Code: listar, leer, crear, guardar, renombrar y
//! eliminar, con versión y rastro en el diario.

use crate::comun::*;

const MEMORIA_EJEMPLO: &str = "---\nname: refactor\ndescription: \"Qué se refactorizó\"\nmetadata:\n  node_type: memory\n  type: project\n---\n\nCuerpo con [[otra]].\n";

/// Un proyecto de Claude Code con dos memorias, el índice y una transcripción
/// que cuenta su cwd. Además, uno sin `memory/` y otro con ella vacía.
fn montar_memoria(f: &Fixture) -> std::path::PathBuf {
    let raiz = f._dir.path().join("proyectos");
    let mem = raiz.join("-home-u-proy/memory");
    std::fs::create_dir_all(&mem).unwrap();
    std::fs::write(mem.join("refactor.md"), MEMORIA_EJEMPLO).unwrap();
    std::fs::write(mem.join("MEMORY.md"), "- [Refactor](refactor.md) — gancho\n").unwrap();
    std::fs::write(mem.join("notas.txt"), "no es markdown").unwrap();
    std::fs::write(
        raiz.join("-home-u-proy/sesion.jsonl"),
        "{\"type\":\"x\"}\nbasura\n{\"cwd\":\"/home/u/proy\",\"type\":\"user\"}\n",
    )
    .unwrap();
    std::fs::create_dir_all(raiz.join("-sin-memoria")).unwrap();
    std::fs::create_dir_all(raiz.join("-vacia/memory")).unwrap();
    mem
}

#[tokio::test]
async fn la_memoria_lista_solo_proyectos_con_memorias() {
    let f = montar();
    let (_, v) = get(&f, "/api/memoria").await;
    assert_eq!(v["proyectos"].as_array().unwrap().len(), 0, "sin directorio");
    montar_memoria(&f);
    let (s, v) = get(&f, "/api/memoria").await;
    assert_eq!(s, StatusCode::OK);
    let ps = v["proyectos"].as_array().unwrap();
    assert_eq!(ps.len(), 1, "{v}");
    assert_eq!((ps[0]["id"].as_str(), ps[0]["cwd"].as_str()), (Some("-home-u-proy"), Some("/home/u/proy")));
    let fs = ps[0]["ficheros"].as_array().unwrap();
    let nombres: Vec<_> = fs.iter().map(|x| x["nombre"].as_str().unwrap()).collect();
    assert_eq!(nombres, ["MEMORY.md", "refactor.md"], "el índice primero y solo .md");
    assert_eq!(fs[1]["titulo"], "refactor");
    assert_eq!(fs[1]["descripcion"], "Qué se refactorizó");
    assert_eq!(fs[1]["tipo"], "project");
    assert!(fs[0]["titulo"].is_null());
}

#[tokio::test]
async fn la_memoria_se_lee_con_su_version() {
    let f = montar();
    montar_memoria(&f);
    let (s, v) = get(&f, "/api/memoria/-home-u-proy/refactor.md").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["contenido"], MEMORIA_EJEMPLO);
    assert_eq!(v["version"].as_str().unwrap().len(), 16);
}

#[tokio::test]
async fn guardar_la_memoria_cambia_el_disco_y_deja_rastro_en_el_diario() {
    let f = montar();
    let mem = montar_memoria(&f);
    let (_, v) = get(&f, "/api/memoria/-home-u-proy/refactor.md").await;
    let cuerpo = json!({ "contenido": "---\nname: nuevo\n---\ntexto\n", "version": v["version"] });
    let (s, nueva) = post(&f, "/api/memoria/-home-u-proy/refactor.md", cuerpo).await;
    assert_eq!(s, StatusCode::OK, "{nueva}");
    assert_eq!(std::fs::read_to_string(mem.join("refactor.md")).unwrap(), "---\nname: nuevo\n---\ntexto\n");
    assert_ne!(nueva["version"], v["version"]);
    assert_eq!(nueva["contenido"], "---\nname: nuevo\n---\ntexto\n");
    let temporales = std::fs::read_dir(&mem).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).count();
    assert_eq!(temporales, 0, "sin temporales");
    let (_, d) = get(&f, "/api/diario").await;
    assert!(d.as_array().unwrap().iter().any(|e| e["accion"] == "memoria_editada" && e["actor"] == "usuario"), "{d}");
    // Con la versión nueva se puede volver a guardar.
    let (s, _) = post(&f, "/api/memoria/-home-u-proy/refactor.md", json!({ "contenido": "x", "version": nueva["version"] })).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn guardar_con_una_version_vieja_da_conflicto_y_no_toca_el_disco() {
    let f = montar();
    let mem = montar_memoria(&f);
    let (_, v) = get(&f, "/api/memoria/-home-u-proy/refactor.md").await;
    std::fs::write(mem.join("refactor.md"), "cambiada a mano").unwrap();
    let (s, e) = post(&f, "/api/memoria/-home-u-proy/refactor.md", json!({ "contenido": "mío", "version": v["version"] })).await;
    assert_eq!(s, StatusCode::CONFLICT, "{e}");
    assert_eq!(std::fs::read_to_string(mem.join("refactor.md")).unwrap(), "cambiada a mano");
}

#[tokio::test]
async fn la_memoria_rechaza_nombres_maliciosos() {
    let f = montar();
    let mem = montar_memoria(&f);
    let fuera = f._dir.path().join("secreto.md");
    std::fs::write(&fuera, "fuera").unwrap();
    ea_plataforma::fs::enlazar(&fuera, &mem.join("enlace.md")).unwrap();
    std::fs::write(mem.join(".oculto.md"), "oculto").unwrap();
    let rutas = [
        "/api/memoria/%2E%2E/refactor.md",
        "/api/memoria/-home-u-proy/%2E%2E",
        "/api/memoria/x%2F..%2Fy/refactor.md",
        "/api/memoria/-home-u-proy/..%2F..%2Fsecreto.md",
        "/api/memoria/-home-u-proy/notas.txt",
        "/api/memoria/-home-u-proy/.oculto.md",
        "/api/memoria/-home-u-proy/enlace.md",
    ];
    for r in rutas {
        let (s, _) = get(&f, r).await;
        assert!(matches!(s, StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND), "GET {r}: {s}");
        let (s, _) = post(&f, r, json!({ "contenido": "pwned", "version": "0" })).await;
        assert!(matches!(s, StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND), "POST {r}: {s}");
    }
    assert_eq!(std::fs::read_to_string(&fuera).unwrap(), "fuera");
    assert_eq!(std::fs::read_to_string(mem.join(".oculto.md")).unwrap(), "oculto");
    assert_eq!(std::fs::read_to_string(mem.join("notas.txt")).unwrap(), "no es markdown");
}

#[tokio::test]
async fn no_se_crean_memorias_desde_la_web() {
    let f = montar();
    let mem = montar_memoria(&f);
    let (s, _) = post(&f, "/api/memoria/-home-u-proy/nueva.md", json!({ "contenido": "x", "version": "0" })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert!(!mem.join("nueva.md").exists());
    let (s, _) = post(&f, "/api/memoria/-no-existe/refactor.md", json!({ "contenido": "x", "version": "0" })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn crear_una_memoria_escribe_cabecera_e_indice_y_deja_rastro() {
    let f = montar();
    let mem = montar_memoria(&f);
    let cuerpo = json!({ "fichero": "mi-nota", "titulo": "Mi nota", "descripcion": "Para recordar", "tipo": "user", "cuerpo": "Texto." });
    let (s, m) = post(&f, "/api/memoria/-home-u-proy/crear", cuerpo.clone()).await;
    assert_eq!(s, StatusCode::CREATED, "{m}");
    assert_eq!(m["nombre"], "mi-nota.md");
    let en_disco = std::fs::read_to_string(mem.join("mi-nota.md")).unwrap();
    assert_eq!(en_disco, "---\nname: Mi nota\ndescription: Para recordar\nmetadata:\n  type: user\n---\n\nTexto.\n");
    assert_eq!(
        std::fs::read_to_string(mem.join("MEMORY.md")).unwrap(),
        "- [Refactor](refactor.md) — gancho\n- [Mi nota](mi-nota.md) — Para recordar\n"
    );
    // Aparece en el listado con su título y tipo.
    let (_, l) = get(&f, "/api/memoria").await;
    let fichero = l["proyectos"][0]["ficheros"].as_array().unwrap().iter().find(|x| x["nombre"] == "mi-nota.md").unwrap().clone();
    assert_eq!((fichero["titulo"].as_str(), fichero["tipo"].as_str()), (Some("Mi nota"), Some("user")));
    let (_, d) = get(&f, "/api/diario").await;
    assert!(d.as_array().unwrap().iter().any(|e| e["accion"] == "memoria_creada" && e["actor"] == "usuario"), "{d}");
    // No pisa: 409 y nada cambia.
    let antes = std::fs::read_to_string(mem.join("MEMORY.md")).unwrap();
    let otra = json!({ "fichero": "mi-nota.md", "titulo": "Otra", "descripcion": "x", "tipo": "project" });
    let (s, _) = post(&f, "/api/memoria/-home-u-proy/crear", otra).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(std::fs::read_to_string(mem.join("mi-nota.md")).unwrap(), en_disco);
    assert_eq!(std::fs::read_to_string(mem.join("MEMORY.md")).unwrap(), antes);
}

#[tokio::test]
async fn crear_una_memoria_rechaza_lo_invalido() {
    let f = montar();
    let mem = montar_memoria(&f);
    let base = json!({ "fichero": "ok", "titulo": "T", "descripcion": "D", "tipo": "project" });
    let con = |k: &str, v: Value| {
        let mut b = base.clone();
        b[k] = v;
        b
    };
    for malo in [
        con("fichero", json!("../fuera")),
        con("fichero", json!("a/b")),
        con("fichero", json!("Mayus")),
        con("fichero", json!("MEMORY.md")),
        con("fichero", json!("")),
        con("tipo", json!("otro")),
        con("titulo", json!("")),
        con("descripcion", json!("dos\nlíneas")),
    ] {
        let (s, _) = post(&f, "/api/memoria/-home-u-proy/crear", malo.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{malo}");
    }
    let (s, _) = post(&f, "/api/memoria/%2E%2E/crear", base.clone()).await;
    assert!(s == StatusCode::BAD_REQUEST || s == StatusCode::NOT_FOUND, "{s}");
    let (s, _) = post(&f, "/api/memoria/-no-existe/crear", base.clone()).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = enviar(&f, Method::POST, "/api/memoria/-home-u-proy/crear", "localhost:8092", Some(base), false).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(std::fs::read_to_string(mem.join("MEMORY.md")).unwrap(), "- [Refactor](refactor.md) — gancho\n");
    assert!(!mem.join("ok.md").exists());
    assert!(!f._dir.path().join("proyectos/fuera.md").exists());
}

#[tokio::test]
async fn guardar_la_memoria_exige_json_y_un_tamano_razonable() {
    let f = montar();
    let mem = montar_memoria(&f);
    let (s, _) = enviar(&f, Method::POST, "/api/memoria/-home-u-proy/refactor.md", "localhost:8092", Some(json!({})), false).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (_, v) = get(&f, "/api/memoria/-home-u-proy/refactor.md").await;
    let enorme = "a".repeat(256 * 1024 + 1);
    let (s, _) = post(&f, "/api/memoria/-home-u-proy/refactor.md", json!({ "contenido": enorme, "version": v["version"] })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(std::fs::read_to_string(mem.join("refactor.md")).unwrap(), MEMORIA_EJEMPLO);
}

#[tokio::test]
async fn la_memoria_se_lee_con_su_ruta_absoluta() {
    let f = montar();
    let mem = montar_memoria(&f);
    let (_, v) = get(&f, "/api/memoria/-home-u-proy/refactor.md").await;
    // Como `Path`, no como texto: en Windows la barra y la contrabarra valen igual.
    assert_eq!(std::path::PathBuf::from(v["ruta"].as_str().unwrap()), mem.join("refactor.md"));
}

#[tokio::test]
async fn eliminar_una_memoria_la_borra_y_guarda_su_contenido_en_el_diario() {
    let f = montar();
    let mem = montar_memoria(&f);
    let (_, v) = get(&f, "/api/memoria/-home-u-proy/refactor.md").await;
    let (s, e) = post(&f, "/api/memoria/-home-u-proy/refactor.md/eliminar", json!({ "version": v["version"] })).await;
    assert_eq!(s, StatusCode::OK, "{e}");
    assert!(!mem.join("refactor.md").exists());
    let (_, d) = get(&f, "/api/diario").await;
    let ev = d.as_array().unwrap().iter().find(|e| e["accion"] == "memoria_eliminada").expect("evento");
    assert_eq!(ev["actor"], "usuario");
    assert!(ev.to_string().contains("Cuerpo con"), "el contenido queda en el diario: {ev}");
}

#[tokio::test]
async fn eliminar_exige_la_version_y_no_toca_el_indice_ni_fuera() {
    let f = montar();
    let mem = montar_memoria(&f);
    let (_, v) = get(&f, "/api/memoria/-home-u-proy/refactor.md").await;
    std::fs::write(mem.join("refactor.md"), "cambiada a mano").unwrap();
    let (s, _) = post(&f, "/api/memoria/-home-u-proy/refactor.md/eliminar", json!({ "version": v["version"] })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert!(mem.join("refactor.md").exists());

    let (_, i) = get(&f, "/api/memoria/-home-u-proy/MEMORY.md").await;
    let (s, _) = post(&f, "/api/memoria/-home-u-proy/MEMORY.md/eliminar", json!({ "version": i["version"] })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(mem.join("MEMORY.md").exists());

    let fuera = f._dir.path().join("secreto.md");
    std::fs::write(&fuera, "fuera").unwrap();
    ea_plataforma::fs::enlazar(&fuera, &mem.join("enlace.md")).unwrap();
    for r in ["/api/memoria/-home-u-proy/enlace.md/eliminar", "/api/memoria/-home-u-proy/..%2F..%2Fsecreto.md/eliminar"] {
        let (s, _) = post(&f, r, json!({ "version": memoria_version("fuera") })).await;
        assert!(matches!(s, StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND), "{r}: {s}");
    }
    assert!(fuera.exists() && mem.join("enlace.md").exists());

    let (s, _) = enviar(&f, Method::POST, "/api/memoria/-home-u-proy/refactor.md/eliminar", "localhost:8092", Some(json!({})), false).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

fn memoria_version(contenido: &str) -> String {
    ea_web::memoria::version(contenido.as_bytes())
}

#[tokio::test]
async fn renombrar_mueve_la_memoria_y_reescribe_los_enlaces() {
    let f = montar();
    let mem = montar_memoria(&f);
    std::fs::write(mem.join("otra.md"), "Ver [[refactor|el refactor]] y [[refactorizar]].\n").unwrap();
    let (_, v) = get(&f, "/api/memoria/-home-u-proy/refactor.md").await;
    let (s, r) = post(&f, "/api/memoria/-home-u-proy/refactor.md/renombrar", json!({ "nuevo": "rehecho.md", "version": v["version"] })).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["memoria"]["nombre"], "rehecho.md");
    assert_eq!(r["memoria"]["contenido"], MEMORIA_EJEMPLO);
    assert!(!mem.join("refactor.md").exists());
    let mut act: Vec<_> = r["actualizadas"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
    act.sort();
    assert_eq!(act, ["MEMORY.md", "otra.md"]);
    assert_eq!(std::fs::read_to_string(mem.join("MEMORY.md")).unwrap(), "- [Refactor](rehecho.md) — gancho\n");
    assert_eq!(std::fs::read_to_string(mem.join("otra.md")).unwrap(), "Ver [[rehecho|el refactor]] y [[refactorizar]].\n");
    let (_, d) = get(&f, "/api/diario").await;
    assert!(d.as_array().unwrap().iter().any(|e| e["accion"] == "memoria_renombrada"), "{d}");
}

#[tokio::test]
async fn renombrar_no_pisa_ni_acepta_nombres_malos() {
    let f = montar();
    let mem = montar_memoria(&f);
    std::fs::write(mem.join("otra.md"), "otra").unwrap();
    let (_, v) = get(&f, "/api/memoria/-home-u-proy/refactor.md").await;
    let ruta = "/api/memoria/-home-u-proy/refactor.md/renombrar";
    let (s, _) = post(&f, ruta, json!({ "nuevo": "otra.md", "version": v["version"] })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(std::fs::read_to_string(mem.join("otra.md")).unwrap(), "otra");
    assert_eq!(std::fs::read_to_string(mem.join("refactor.md")).unwrap(), MEMORIA_EJEMPLO);
    for nuevo in ["../x.md", ".oculto.md", "x.txt", "MEMORY.md", "a/b.md", "refactor.md"] {
        let (s, _) = post(&f, ruta, json!({ "nuevo": nuevo, "version": v["version"] })).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{nuevo}");
    }
    let (s, _) = post(&f, ruta, json!({ "nuevo": "nueva.md", "version": "0" })).await;
    assert_eq!(s, StatusCode::CONFLICT, "versión vieja");
    let (_, i) = get(&f, "/api/memoria/-home-u-proy/MEMORY.md").await;
    let (s, _) = post(&f, "/api/memoria/-home-u-proy/MEMORY.md/renombrar", json!({ "nuevo": "indice.md", "version": i["version"] })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(mem.join("refactor.md").exists() && mem.join("MEMORY.md").exists() && !mem.join("nueva.md").exists());
}

#[tokio::test]
async fn mostrar_una_memoria_que_no_existe_da_404() {
    let f = montar();
    montar_memoria(&f);
    let (s, _) = post(&f, "/api/memoria/-home-u-proy/no-esta.md/mostrar", json!({})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}
