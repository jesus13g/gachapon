//! Equipos de agentes (spec 019): listar, despertar y dormir, presencia y
//! trabajos para un equipo.

use crate::comun::*;

/// Binarios de mentira para los cinco agentes del catálogo y unos ajustes que
/// los usan, con `orquestador.paralelo = paralelo`: así la prueba no depende
/// del PATH real. Devuelve el repo git donde se despiertan los equipos.
fn preparar_equipos(f: &Fixture, paralelo: u32) -> std::path::PathBuf {
    let bins = f._dir.path().join("bins");
    std::fs::create_dir_all(&bins).unwrap();
    let mut toml = format!("[orquestador]\nparalelo = {paralelo}\n");
    for id in ea_ajustes::IDS_AGENTES {
        // Un binario falso que `buscar_binario` da por ejecutable en cada sistema.
        let p = bins.join(if cfg!(windows) { format!("{id}.cmd") } else { id.to_string() });
        std::fs::write(&p, if cfg!(windows) { "@echo off\r\n" } else { "#!/bin/sh\n" }).unwrap();
        #[cfg(unix)]
        ea_plataforma::fs::poner_permisos_unix(&p, 0o755).unwrap();
        toml += &format!("[agentes.{id}]\nbin = \"{}\"\n", p.display().to_string().replace('\\', "/"));
    }
    let fichero = f.state.config.ajustes.fichero();
    std::fs::create_dir_all(fichero.parent().unwrap()).unwrap();
    std::fs::write(&fichero, toml).unwrap();
    repo_git(f)
}

fn miembros(lista: &[(&str, &str)]) -> Value {
    Value::Array(lista.iter().map(|(a, r)| json!({ "agente": a, "rol": r })).collect())
}

fn despertar_cuerpo(wt: &std::path::Path, lista: &[(&str, &str)], terminal: bool) -> Value {
    json!({ "titulo": "Equipo de prueba", "objetivo": "Probar los equipos", "worktree": wt, "miembros": miembros(lista), "terminal": terminal })
}

const BASICO: [(&str, &str); 2] = [("claude-code", "planificador"), ("qwen-code", "ejecutor")];

#[tokio::test]
async fn los_equipos_se_listan_vacios_y_con_uno() {
    let f = montar();
    let wt = preparar_equipos(&f, 2);
    let (s, v) = get(&f, "/api/equipos").await;
    assert_eq!((s, v), (StatusCode::OK, json!([])));

    let (s, r) = post(&f, "/api/equipos", despertar_cuerpo(&wt, &BASICO, false)).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    let id = r["equipo"]["id"].as_str().unwrap().to_string();
    assert_eq!(r["equipo"]["estado"], "despierto");
    assert_eq!(r["equipo"]["creado_por"], "usuario");
    // Sin terminal pedida: solo dice cómo abrir la sesión del planificador.
    assert_eq!(r["sesiones"][0]["agente"], "claude-code");
    assert_eq!(r["sesiones"][0]["abierta"], false);
    assert_eq!(r["sesiones"][0]["comando"], format!("ea equipo sesion {id} claude-code"));
    assert!(lanzadas(&f).is_empty());

    let (_, v) = get(&f, "/api/equipos").await;
    assert_eq!(v.as_array().unwrap().len(), 1, "{v}");
    assert_eq!(v[0]["id"], id.as_str());
    assert_eq!(v[0]["estado"], "despierto");
    assert_eq!(v[0]["trabajos"], 0);
    assert_eq!(v[0]["miembros"], json!([
        { "agente": "claude-code", "rol": "planificador", "viva": false },
        { "agente": "qwen-code", "rol": "ejecutor", "viva": false },
    ]));

    let (s, v) = get(&f, &format!("/api/equipos/{id}")).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["titulo"], "Equipo de prueba");
    assert_eq!(v["miembros"][1]["pendientes"], 0);
    assert_eq!(v["trabajos_total"], 0);

    assert_eq!(get(&f, "/api/equipos/no-existe").await.0, StatusCode::NOT_FOUND);
    assert_eq!(get(&f, "/api/equipos/a.b").await.0, StatusCode::BAD_REQUEST);
    // El diario lo cuenta con actor usuario.
    let (_, d) = get(&f, "/api/diario?accion=equipo_").await;
    assert!(d.to_string().contains("equipo_despertado"), "{d}");
}

#[tokio::test]
async fn despertar_y_dormir_exigen_la_clave() {
    let f = montar();
    let wt = preparar_equipos(&f, 2);
    let cuerpo = despertar_cuerpo(&wt, &BASICO, false);
    for clave in [None, Some("mala")] {
        let (s, t) = enviar_con_clave(&f, Method::POST, "/api/equipos", "localhost:8092", Some(cuerpo.clone()), true, clave).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{t}");
    }
    assert_eq!(get(&f, "/api/equipos").await.1, json!([]), "sin clave no se despierta nada");
    let (_, r) = post(&f, "/api/equipos", cuerpo).await;
    let id = r["equipo"]["id"].as_str().unwrap().to_string();
    let (s, _) = enviar_con_clave(&f, Method::POST, &format!("/api/equipos/{id}/dormir"), "localhost:8092", Some(json!({})), true, None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(get(&f, &format!("/api/equipos/{id}")).await.1["estado"], "despierto");
}

#[tokio::test]
async fn despertar_con_terminal_abre_una_por_planificador_y_revisor() {
    let f = montar();
    let wt = preparar_equipos(&f, 2);
    let lista = [("claude-code", "planificador"), ("antigravity", "revisor"), ("qwen-code", "ejecutor")];
    let (s, r) = post(&f, "/api/equipos", despertar_cuerpo(&wt, &lista, true)).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    let id = r["equipo"]["id"].as_str().unwrap();
    assert_eq!(r["sesiones"].as_array().unwrap().len(), 2, "{r}");
    assert!(r["sesiones"].as_array().unwrap().iter().all(|x| x["abierta"] == true && x["error"].is_null()));
    // Solo planificador y revisor; argv cerrado `ea equipo sesion <id> <agente>`.
    let ea = f._dir.path().join("bin/ea").display().to_string();
    let esperado = |agente: &str| vec![ea.clone(), "equipo".into(), "sesion".into(), id.into(), agente.into()];
    assert_eq!(lanzadas(&f), vec![esperado("claude-code"), esperado("antigravity")]);
    let (_, d) = get(&f, "/api/diario?accion=equipo_sesion").await;
    assert_eq!(d.as_array().unwrap().len(), 2, "{d}");
    assert!(d.to_string().contains("equipo_sesion_abierta"));
}

#[tokio::test]
async fn despertar_a_todos_reparte_segun_lo_que_admite_cada_agente() {
    let f = montar();
    let wt = preparar_equipos(&f, 3);
    let (s, r) = post(&f, "/api/equipos", json!({ "titulo": "Todos", "objetivo": "Todos juntos", "worktree": wt, "todos": true })).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    let m = r["equipo"]["miembros"].as_array().unwrap();
    let rol = |a: &str| m.iter().find(|x| x["agente"] == a).unwrap()["rol"].clone();
    assert_eq!(m.len(), 5);
    assert_eq!((rol("claude-code"), rol("antigravity")), (json!("planificador"), json!("planificador")));
    assert_eq!((rol("qwen-code"), rol("codex"), rol("opencode")), (json!("ejecutor"), json!("ejecutor"), json!("ejecutor")));
    assert_eq!(r["omitidos"], json!([]));
}

#[tokio::test]
async fn despertar_rechaza_lo_que_la_cli_rechaza() {
    let f = montar();
    let wt = preparar_equipos(&f, 1);
    let rechaza = |cuerpo: Value, texto: &'static str| {
        let f = &f;
        async move {
            let (s, r) = post(f, "/api/equipos", cuerpo).await;
            assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
            let e = r["error"].as_str().unwrap_or_default().to_string();
            assert!(e.contains(texto), "{texto:?} en {e:?}");
        }
    };
    // agente repetido
    rechaza(despertar_cuerpo(&wt, &[("claude-code", "planificador"), ("claude-code", "revisor"), ("qwen-code", "ejecutor")], false), "más de una vez").await;
    // rol que el agente no admite, rol inventado y agente fuera del catálogo
    rechaza(despertar_cuerpo(&wt, &[("claude-code", "planificador"), ("antigravity", "ejecutor")], false), "no puede ser ejecutor").await;
    rechaza(despertar_cuerpo(&wt, &[("claude-code", "jefe"), ("qwen-code", "ejecutor")], false), "rol de equipo").await;
    rechaza(despertar_cuerpo(&wt, &[("nadie", "planificador"), ("qwen-code", "ejecutor")], false), "catálogo").await;
    // sin ejecutor, o sin miembros, o con las dos formas a la vez
    rechaza(despertar_cuerpo(&wt, &[("claude-code", "planificador")], false), "ejecutor").await;
    rechaza(despertar_cuerpo(&wt, &[], false), "todos").await;
    let mut ambos = despertar_cuerpo(&wt, &BASICO, false);
    ambos["todos"] = json!(true);
    rechaza(ambos, "no las dos cosas").await;
    // tope orquestador.paralelo = 1: dos ejecutores no caben
    rechaza(despertar_cuerpo(&wt, &[("claude-code", "planificador"), ("qwen-code", "ejecutor"), ("codex", "ejecutor")], false), "orquestador.paralelo").await;
    // campos sueltos, título vacío y worktree que no existe
    let mut extra = despertar_cuerpo(&wt, &BASICO, false);
    extra["creado_por"] = json!("claude-code");
    rechaza(extra, "creado_por").await;
    let mut sin_titulo = despertar_cuerpo(&wt, &BASICO, false);
    sin_titulo["titulo"] = json!("  ");
    rechaza(sin_titulo, "titulo").await;
    let mut sin_wt = despertar_cuerpo(&wt, &BASICO, false);
    sin_wt["worktree"] = json!(concat!(tmp!(), "/no/existe/de/verdad"));
    rechaza(sin_wt, "no existe").await;
    // un binario que falta
    let fichero = f.state.config.ajustes.fichero();
    let t = std::fs::read_to_string(&fichero).unwrap();
    std::fs::write(&fichero, t.replace("bins/qwen-code", "bins/no-esta")).unwrap();
    rechaza(despertar_cuerpo(&wt, &BASICO, false), "no está disponible").await;
    std::fs::write(&fichero, t).unwrap();

    assert_eq!(get(&f, "/api/equipos").await.1, json!([]), "nada se ha escrito");
    assert!(lanzadas(&f).is_empty());

    // Un agente solo está en un equipo despierto; dormirlo lo libera.
    let (s, r) = post(&f, "/api/equipos", despertar_cuerpo(&wt, &BASICO, false)).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    let id = r["equipo"]["id"].as_str().unwrap().to_string();
    rechaza(despertar_cuerpo(&wt, &[("antigravity", "planificador"), ("qwen-code", "ejecutor")], false), "ya está en el equipo despierto").await;
    let (s, d) = post(&f, &format!("/api/equipos/{id}/dormir"), json!({})).await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert_eq!(d["estado"], "dormido");
    assert_eq!(get(&f, "/api/equipos").await.1[0]["estado"], "dormido");
    let (s, _) = post(&f, &format!("/api/equipos/{id}/dormir"), json!({})).await;
    assert_eq!(s, StatusCode::OK, "dormir uno dormido no hace nada");
    assert_eq!(post(&f, "/api/equipos/no-existe/dormir", json!({})).await.0, StatusCode::NOT_FOUND);
    let (s, r) = post(&f, "/api/equipos", despertar_cuerpo(&wt, &[("antigravity", "planificador"), ("qwen-code", "ejecutor")], false)).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
}

#[tokio::test]
async fn dormir_devuelve_lo_pendiente_a_la_cola_del_ejecutor() {
    let f = montar();
    let wt = preparar_equipos(&f, 2);
    let (_, r) = post(&f, "/api/equipos", despertar_cuerpo(&wt, &BASICO, false)).await;
    let id = r["equipo"]["id"].as_str().unwrap().to_string();
    let (s, t) = post(&f, "/api/trabajos", json!({ "titulo": "Asignado", "worktree": wt, "objetivo": "x", "criterio": "true",
        "equipo": id, "asignado_a": "qwen-code" })).await;
    assert_eq!(s, StatusCode::CREATED, "{t}");
    let (_, v) = get(&f, &format!("/api/equipos/{id}")).await;
    assert_eq!(v["miembros"][1]["pendientes"], 1, "{v}");
    assert_eq!(v["trabajos"][0]["asignado_a"], "qwen-code");
    assert_eq!(get(&f, "/api/equipos").await.1[0]["trabajos"], 1);
    let (_, colas) = get(&f, "/api/colas").await;
    assert_eq!(colas["modelo-local"].as_array().unwrap().len(), 0, "{colas}");
    post(&f, &format!("/api/equipos/{id}/dormir"), json!({})).await;
    let (_, colas) = get(&f, "/api/colas").await;
    assert_eq!(colas["modelo-local"].as_array().unwrap().len(), 1, "{colas}");
    let (_, v) = get(&f, &format!("/api/equipos/{id}")).await;
    assert_eq!(v["miembros"][1]["pendientes"], 0);
}

#[tokio::test]
async fn los_agentes_traen_su_presencia() {
    let f = montar();
    let fichero = f.state.config.ajustes.fichero();
    std::fs::create_dir_all(fichero.parent().unwrap()).unwrap();
    std::fs::write(&fichero, "[modelos.local]\nbase_url = \"http://127.0.0.1:9/v1\"\n").unwrap();
    let ag = |v: &Value, id: &str| v["agentes"].as_array().unwrap().iter().find(|a| a["id"] == id).unwrap().clone();
    let (_, v) = get(&f, "/api/agentes").await;
    assert!(ag(&v, "qwen-code")["presencia"].is_null(), "{v}");

    let bus = ea_bus::Bus::abrir_en(&f.state.config.estado_dir).unwrap();
    // Viva: el pid de esta prueba existe y el latido es de ahora.
    let mut p = ea_bus::Presencia::nueva("qwen-code", ea_bus::RolEquipo::Ejecutor, Some("e1"), std::process::id());
    p.trabajo = Some("20261021-101500-ab12".into());
    bus.anotar_presencia(&p).unwrap();
    // Caducada: ningún proceso tiene el pid 0.
    bus.anotar_presencia(&ea_bus::Presencia::nueva("claude-code", ea_bus::RolEquipo::Planificador, None, 0)).unwrap();
    let (_, v) = get(&f, "/api/agentes").await;
    let q = ag(&v, "qwen-code");
    assert_eq!(q["presencia"]["viva"], true, "{q}");
    assert_eq!(q["presencia"]["equipo"], "e1");
    assert_eq!(q["presencia"]["trabajo"], "20261021-101500-ab12");
    assert_eq!(q["presencia"]["rol"], "ejecutor");
    assert_eq!(ag(&v, "claude-code")["presencia"]["viva"], false);
    assert!(ag(&v, "codex")["presencia"].is_null());
    // El catálogo sigue siendo el de siempre.
    assert_eq!(q["rol"], "ejecutor");
}

#[tokio::test]
async fn crear_un_trabajo_para_un_equipo_valida_como_el_bus() {
    let f = montar();
    let wt = preparar_equipos(&f, 2);
    let cuerpo = |extra: Value| {
        let mut m = mision(wt.to_str().unwrap());
        for (k, v) in extra.as_object().unwrap() {
            m[k] = v.clone();
        }
        m
    };
    // Sin equipo, asignado y revisor no valen.
    let (s, r) = post(&f, "/api/trabajos", cuerpo(json!({ "asignado_a": "qwen-code" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
    assert!(r["error"].as_str().unwrap().contains("necesitan un equipo"), "{r}");
    let (s, _) = post(&f, "/api/trabajos", cuerpo(json!({ "equipo": "no-existe" }))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let lista = [("claude-code", "planificador"), ("antigravity", "revisor"), ("qwen-code", "ejecutor"), ("codex", "ejecutor")];
    let (_, e) = post(&f, "/api/equipos", despertar_cuerpo(&wt, &lista, false)).await;
    let id = e["equipo"]["id"].as_str().unwrap().to_string();
    // El asignado tiene que ser un ejecutor del equipo, y el revisor, no el mismo.
    let (s, r) = post(&f, "/api/trabajos", cuerpo(json!({ "equipo": id, "asignado_a": "opencode" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
    let (s, r) = post(&f, "/api/trabajos", cuerpo(json!({ "equipo": id, "asignado_a": "claude-code" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
    let (s, r) = post(&f, "/api/trabajos", cuerpo(json!({ "equipo": id, "revisor": "qwen-code" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
    // Los selectores del formulario mandan "" cuando no se elige nada.
    let (s, r) = post(&f, "/api/trabajos", cuerpo(json!({ "equipo": "", "asignado_a": "", "revisor": " " }))).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");

    let (s, r) = post(&f, "/api/trabajos", cuerpo(json!({ "equipo": id, "asignado_a": "codex", "revisor": "antigravity", "worktree_propio": true }))).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    let t = r["id"].as_str().unwrap().to_string();
    let (_, d) = get(&f, &format!("/api/trabajos/{t}")).await;
    assert_eq!(d["orden"]["equipo"], id.as_str());
    assert_eq!(d["orden"]["asignado_a"], "codex");
    assert_eq!(d["orden"]["revisor"], "antigravity");
    assert_eq!(d["orden"]["worktree_propio"], true);
    // Va a la cola de su ejecutor, no a la del rol, y el equipo lo anota.
    let (_, v) = get(&f, &format!("/api/equipos/{id}")).await;
    let codex = v["miembros"].as_array().unwrap().iter().find(|m| m["agente"] == "codex").unwrap().clone();
    assert_eq!(codex["cola"], json!([t.as_str()]));
    assert_eq!(v["trabajos"][0]["id"], t.as_str());
    assert_eq!(v["trabajos"][0]["revisor"], "antigravity");
    // Un equipo dormido no admite trabajo.
    post(&f, &format!("/api/equipos/{id}/dormir"), json!({})).await;
    let (s, r) = post(&f, "/api/trabajos", cuerpo(json!({ "equipo": id }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
    assert!(r["error"].as_str().unwrap().contains("dormido"), "{r}");
}
