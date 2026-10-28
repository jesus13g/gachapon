//! La biblioteca de herramientas (spec 018): listar, ficha, uso, activar y
//! desactivar, sesiones y categoría, sin secretos en ninguna respuesta.

use crate::comun::*;

/// Lo que no puede salir nunca en una respuesta: está en los ficheros del HOME de prueba.
const SECRETOS: [&str; 7] = [
    "SECRETO_ENV_CLAUDE",
    "SECRETO_ARG_TOKEN",
    "SECRETO_HEADER_CLAUDE",
    "SECRETO_RUTA_URL",
    "SECRETO_CONSULTA_URL",
    "SECRETO_ENV_CODEX",
    "SECRETO_HEADER_OPENCODE",
];

const SKILL: &str = "claude-code/skill/usuario/propia";
const MCP_CLAUDE: &str = "claude-code/mcp/usuario/remoto-claude";
const MCP_CODEX: &str = "codex/mcp/usuario/docs";
const MCP_CODEX_APAGADO: &str = "codex/mcp/usuario/apagado";

fn ids(v: &Value) -> Vec<String> {
    v["elementos"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn la_biblioteca_lista_y_filtra() {
    let f = montar();
    let (st, v) = get(&f, "/api/biblioteca").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let todos = ids(&v);
    for id in [SKILL, MCP_CLAUDE, MCP_CODEX, MCP_CODEX_APAGADO, "opencode/mcp/usuario/remoto-oc"] {
        assert!(todos.iter().any(|x| x == id), "falta {id}: {todos:?}");
    }
    assert_eq!(v["total"].as_u64().unwrap() as usize, todos.len());
    assert!(v["categorias"].as_array().unwrap().iter().any(|c| c == "usuario"), "{v}");
    assert!(v["avisos"].is_array());
    assert!(v["agentes_sesion"].as_array().unwrap().iter().any(|a| a == "claude-code"));

    let (_, v) = get(&f, "/api/biblioteca?agente=codex").await;
    assert!(ids(&v).iter().all(|i| i.starts_with("codex/")) && !ids(&v).is_empty(), "{v}");
    let (_, v) = get(&f, "/api/biblioteca?agente=claude-code&tipo=skill").await;
    assert!(ids(&v).iter().all(|i| i.starts_with("claude-code/skill/")), "{v}");
    assert!(ids(&v).contains(&SKILL.to_string()));
    let (_, v) = get(&f, "/api/biblioteca?agente=codex&activado=no").await;
    assert_eq!(ids(&v), vec![MCP_CODEX_APAGADO.to_string()], "{v}");
    let (_, v) = get(&f, "/api/biblioteca?categoria=usuario").await;
    assert!(ids(&v).contains(&SKILL.to_string()) && ids(&v).iter().all(|i| todos.contains(i)), "{v}");
    assert!(v["elementos"].as_array().unwrap().iter().all(|e| e["categoria"] == "usuario"), "{v}");
    // Las categorías son las de todo el inventario, no las de lo filtrado.
    let (_, v) = get(&f, "/api/biblioteca?categoria=otra").await;
    assert!(v["elementos"].as_array().unwrap().is_empty());
    assert!(!v["categorias"].as_array().unwrap().is_empty());
    for mala in ["tipo=nada", "activado=quizas"] {
        let (st, _) = get(&f, &format!("/api/biblioteca?{mala}")).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{mala}");
    }
}

#[tokio::test]
async fn la_ficha_trae_el_elemento_y_su_uso() {
    let f = montar();
    // El id lleva «/»: la página lo codifica, pero sin codificar también vale.
    for ruta in [
        "/api/biblioteca/claude-code%2Fskill%2Fusuario%2Fpropia".to_string(),
        format!("/api/biblioteca/{SKILL}"),
    ] {
        let (st, v) = get(&f, &ruta).await;
        assert_eq!(st, StatusCode::OK, "{ruta}: {v}");
        assert_eq!(v["id"], SKILL);
        assert_eq!(v["mecanismo"], "mover");
        assert_eq!(v["activado"], "si");
        assert_eq!(v["cambio"]["posible"], true);
        assert_eq!(v["uso"]["total"], 4, "{v}");
        assert_eq!(v["uso"]["por"][0]["actor"], "claude-code");
        assert!(v["sesion"].as_array().unwrap().iter().any(|a| a == "claude-code"));
    }
    // Un MCP de Claude Code solo se acota en sesión: lo dice, sin botón de activar.
    let (_, v) = get(&f, "/api/biblioteca/claude-code%2Fmcp%2Fusuario%2Fremoto-claude").await;
    assert_eq!(v["cambio"]["posible"], false);
    assert!(v["cambio"]["motivo"].as_str().unwrap().contains("sesión"), "{v}");
    assert_eq!(v["def_mcp"]["transporte"], "http");
    assert!(v["uso"].is_null(), "sin uso: {v}");
    let (st, _) = get(&f, "/api/biblioteca/claude-code%2Fskill%2Fusuario%2Fno-existe").await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn el_uso_es_un_informe_con_cache() {
    let f = montar();
    let (st, v) = get(&f, "/api/biblioteca/uso").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let u = v["usos"].as_array().unwrap().iter().find(|u| u["id"] == SKILL).expect("uso de la skill");
    assert_eq!(u["total"], 4);
    assert!(u["ultimo"].is_string());
    assert!(v["actividad"].is_array() && v["avisos"].is_array() && v["estadisticas"].is_object());
    // La segunda vez sale de la memoria: mismo informe.
    let (_, v2) = get(&f, "/api/biblioteca/uso").await;
    assert_eq!(v, v2);
}

#[tokio::test]
async fn ninguna_respuesta_de_la_biblioteca_lleva_secretos() {
    let f = montar();
    let mut todo = String::new();
    for ruta in [
        "/api/biblioteca".to_string(),
        "/api/biblioteca/uso".to_string(),
        "/api/biblioteca/claude-code%2Fmcp%2Fusuario%2Fremoto-claude".to_string(),
        "/api/biblioteca/claude-code%2Fmcp%2Fusuario%2Fstdio-claude".to_string(),
        "/api/biblioteca/codex%2Fmcp%2Fusuario%2Fdocs".to_string(),
        "/api/biblioteca/opencode%2Fmcp%2Fusuario%2Fremoto-oc".to_string(),
    ] {
        let (st, t) = enviar(&f, Method::GET, &ruta, "localhost:8092", None, false).await;
        assert_eq!(st, StatusCode::OK, "{ruta}");
        todo += &t;
    }
    // Sí salen los nombres de las variables y cabeceras, nunca sus valores.
    assert!(todo.contains("API_KEY") && todo.contains("Authorization"), "se esperaban los nombres");
    // Y las simulaciones de sesión (con el comando) tampoco.
    for (agente, hs) in [
        ("claude-code", json!([MCP_CLAUDE, "claude-code/mcp/usuario/stdio-claude", SKILL])),
        ("codex", json!([MCP_CODEX])),
        ("opencode", json!(["opencode/mcp/usuario/remoto-oc"])),
    ] {
        let (_, t) = enviar(
            &f,
            Method::POST,
            "/api/biblioteca/simular",
            "localhost:8092",
            Some(json!({ "herramienta": "ea_lanzar_sesion", "argumentos": { "agente": agente, "herramientas": hs } })),
            true,
        )
        .await;
        todo += &t;
    }
    sin_secretos(&todo, &SECRETOS);
}

#[tokio::test]
async fn desactivar_y_activar_pasa_por_simulacion_y_se_revalida() {
    let f = montar();
    let skill = f._dir.path().join("home/.claude/skills/propia");
    let guardada = f._dir.path().join("estado-biblioteca/biblioteca/desactivadas/claude-code/skill/propia");
    assert!(skill.is_dir());

    let (st, sim) = post(
        &f,
        "/api/biblioteca/simular",
        json!({ "herramienta": "ea_desactivar_herramienta", "argumentos": { "elemento": SKILL } }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{sim}");
    assert_eq!(sim["riesgo"], "reversible");
    assert_eq!(sim["confirmable"], true);
    assert!(sim["texto"].as_str().unwrap().contains("moviendo"), "{sim}");
    assert!(skill.is_dir(), "simular no toca nada");

    // Si el mundo cambia entre simular y ejecutar, no se hace nada.
    let (st, e) = post(
        &f,
        "/api/biblioteca/ejecutar",
        json!({ "herramienta": "ea_desactivar_herramienta", "argumentos": sim["argumentos"], "precondicion": { "x": 1 } }),
    )
    .await;
    assert!(skill.is_dir(), "{st} {e}");

    let (st, r) = post(
        &f,
        "/api/biblioteca/ejecutar",
        json!({ "herramienta": "ea_desactivar_herramienta", "argumentos": sim["argumentos"], "precondicion": sim["precondicion"] }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert!(!skill.exists(), "la skill se movió");
    assert!(guardada.exists(), "y está en el almacén del ecosistema");
    assert!(r["evento"].is_number(), "queda en el diario: {r}");

    // La biblioteca lo refleja: la skill guardada sigue en el inventario, desactivada.
    let (_, v) = get(&f, "/api/biblioteca/claude-code%2Fskill%2Fusuario%2Fpropia").await;
    assert_eq!(v["activado"], "no", "{v}");

    // Activar lo deshace.
    let (_, sim) = post(
        &f,
        "/api/biblioteca/simular",
        json!({ "herramienta": "ea_activar_herramienta", "argumentos": { "elemento": SKILL } }),
    )
    .await;
    let (st, r) = post(
        &f,
        "/api/biblioteca/ejecutar",
        json!({ "herramienta": "ea_activar_herramienta", "argumentos": sim["argumentos"], "precondicion": sim["precondicion"] }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert!(skill.is_dir() && !guardada.exists());
}

#[tokio::test]
async fn lo_que_no_se_puede_cambiar_se_rechaza() {
    let f = montar();
    // Un MCP de Claude Code (~/.claude.json) solo se acota en sesión.
    let (st, e) = post(
        &f,
        "/api/biblioteca/simular",
        json!({ "herramienta": "ea_desactivar_herramienta", "argumentos": { "elemento": MCP_CLAUDE } }),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    // Solo las tres herramientas de la sección; ni otras ni un directorio a pedido.
    for cuerpo in [
        json!({ "herramienta": "ea_reiniciar_servicio", "argumentos": { "servicio": "ea-mcp" } }),
        json!({ "herramienta": "ea_lanzar_sesion", "argumentos": { "agente": "codex", "herramientas": [MCP_CODEX], "directorio": tmp!() } }),
    ] {
        let (st, _) = post(&f, "/api/biblioteca/simular", cuerpo).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn proponer_y_aceptar_una_sesion_abre_la_terminal_sin_secretos() {
    let f = montar();
    let args = json!({ "agente": "claude-code", "herramientas": [MCP_CLAUDE, SKILL] });
    let (st, sim) =
        post(&f, "/api/biblioteca/simular", json!({ "herramienta": "ea_lanzar_sesion", "argumentos": args })).await;
    assert_eq!(st, StatusCode::OK, "{sim}");
    let texto = sim["texto"].as_str().unwrap();
    assert!(texto.contains("claude") && texto.contains("--strict-mcp-config"), "{sim}");
    sin_secretos(&sim.to_string(), &SECRETOS);
    assert!(f.terminales.lock().unwrap().is_empty(), "simular no abre nada");

    let (st, r) = post(
        &f,
        "/api/biblioteca/ejecutar",
        json!({ "herramienta": "ea_lanzar_sesion", "argumentos": sim["argumentos"], "precondicion": sim["precondicion"] }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    sin_secretos(&r.to_string(), &SECRETOS);
    let abiertas = f.terminales.lock().unwrap().clone();
    assert_eq!(abiertas.len(), 1, "una terminal");
    assert!(abiertas[0].iter().any(|a| a == "claude"), "{abiertas:?}");
    sin_secretos(&format!("{abiertas:?}"), &SECRETOS);
    // Los secretos solo están en los ficheros efímeros, con permisos 0600.
    let mcp = std::fs::read_dir(f._dir.path().join("runtime/ea-biblioteca/sesiones"))
        .unwrap()
        .flatten()
        .flat_map(|d| std::fs::read_dir(d.path()).unwrap().flatten())
        .map(|e| e.path())
        .find(|p| std::fs::read_to_string(p).is_ok_and(|t| t.contains("SECRETO_HEADER_CLAUDE")))
        .expect("el fichero efímero con la definición completa");
    if let Some(m) = ea_plataforma::fs::permisos_unix(&std::fs::metadata(mcp).unwrap()) {
        assert_eq!(m & 0o777, 0o600);
    }
}

#[tokio::test]
async fn la_categoria_propia_va_por_ajustes() {
    let f = montar();
    let clave = |aj: &Value| {
        aj["secciones"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s["claves"].as_array().unwrap().iter())
            .find(|c| c["clave"] == "biblioteca.categorias")
            .expect("la clave está en la vista de ajustes")
            .clone()
    };
    let (_, aj) = get(&f, "/api/ajustes").await;
    let c = clave(&aj);
    assert_eq!(c["tipo"], "mapa", "{c}");
    assert_eq!(c["edicion"], "web");
    let (st, r) = post(
        &f,
        "/api/ajustes",
        json!({ "version": aj["version"], "cambios": { "biblioteca.categorias": { SKILL: "trabajo" } } }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let (_, v) = get(&f, "/api/biblioteca?categoria=trabajo").await;
    assert_eq!(ids(&v), vec![SKILL.to_string()], "{v}");
    assert!(v["categorias"].as_array().unwrap().iter().any(|c| c == "trabajo"));
    let (_, aj) = get(&f, "/api/ajustes").await;
    assert_eq!(clave(&aj)["valor"][SKILL], "trabajo");
}
