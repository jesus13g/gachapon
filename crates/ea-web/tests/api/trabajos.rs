//! Trabajos: colas y semáforos, acciones del usuario, crear misiones y
//! obras, la ficha con su diff, las skills del ejecutor, los worktrees
//! propios y el texto de los resúmenes.

use crate::comun::*;

#[tokio::test]
async fn trabajos_colas_y_semaforos_de_solo_lectura() {
    let f = montar();
    let bus = bus(&f);

    let (s, v) = get(&f, "/api/trabajos").await;
    assert_eq!((s, v.as_array().unwrap().len()), (StatusCode::OK, 0));

    let id = bus.crear_trabajo(orden(ea_bus::Modo::Mision, "Primera", tmp!())).unwrap();
    let (_, v) = get(&f, "/api/trabajos").await;
    assert_eq!(v[0]["id"], id.as_str());
    assert_eq!(v[0]["modo"], "mision");
    assert_eq!(v[0]["estado"], "en_cola");
    assert!(v[0]["resultado"].is_null());
    assert!(v[0]["ejecutado_por"].is_null(), "nadie lo ha tomado aún");
    assert!(std::path::Path::new(v[0]["worktree"].as_str().unwrap()).ends_with(std::path::Path::new(tmp!()).file_name().unwrap()));

    let (_, c) = get(&f, "/api/colas").await;
    assert_eq!(c["modelo-local"][0]["trabajo"], id.as_str());
    assert_eq!(c["modelo-local"][0]["titulo"], "Primera");
    assert!(c["claude-code"].as_array().unwrap().is_empty());

    let (s, j) = get(&f, &format!("/api/trabajos/{id}")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(j["orden"]["titulo"], "Primera");
    assert_eq!(j["historial"][0]["evento"], "crear");
    assert!(j["informe"].is_null());

    let (s, _) = get(&f, "/api/trabajos/no-existe").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = get(&f, &format!("/api/trabajos/{id}/diff")).await;
    assert_eq!(s, StatusCode::CONFLICT, "sin informe no hay diff");

    let (s, sem) = get(&f, "/api/semaforos").await;
    assert_eq!((s, sem.as_array().unwrap().len()), (StatusCode::OK, 0));
    let _candado = ea_bus::CandadoWorktree::adquirir_en(
        bus.raiz(),
        std::path::Path::new(tmp!()),
        ea_bus::Ocupante { agente: "modelo-local".into(), trabajo: id.clone(), pid: 0, desde: "2026-10-14T10:00:00Z".into(), rol: None },
        std::time::Duration::from_millis(50),
    )
    .unwrap();
    let (_, sem) = get(&f, "/api/semaforos").await;
    assert_eq!(sem[0]["trabajo"], id.as_str());
    assert_eq!(sem[0]["agente"], "modelo-local");

    // Al tomarlo, la lista dice con qué agente y modelo (las tarjetas de Actividad).
    bus.tomar_con(&id, ea_bus::EjecutadoPor { agente: "qwen-code".into(), modelo: "qwen3-coder-30b".into() }).unwrap();
    let (_, l) = get(&f, "/api/trabajos").await;
    assert_eq!(l[0]["ejecutado_por"], json!({ "agente": "qwen-code", "modelo": "qwen3-coder-30b" }));

    // Cancelado con el semáforo aún cogido: «cancelando…» hasta que se suelte.
    let cancelando = |l: &Value| l.as_array().unwrap().iter().find(|t| t["id"] == id.as_str()).unwrap()["cancelando"].clone();
    let (_, l) = get(&f, "/api/trabajos").await;
    assert_eq!(cancelando(&l), json!(false));
    bus.aplicar(&id, ea_bus::Evento::Cancelar).unwrap();
    let (_, l) = get(&f, "/api/trabajos").await;
    assert_eq!(cancelando(&l), json!(true));
    drop(_candado);
    let (_, l) = get(&f, "/api/trabajos").await;
    assert_eq!(cancelando(&l), json!(false));
}

#[tokio::test]
async fn acciones_del_usuario_sobre_los_trabajos() {
    let f = montar();
    let bus = bus(&f);
    let entrega = |id: &str, completa: bool| {
        bus.aplicar(id, ea_bus::Evento::Tomar).unwrap();
        let mut inf = ea_bus::Informe::nuevo(ea_bus::ResultadoInforme::Terminado);
        inf.obra_completa = completa;
        bus.aplicar(id, ea_bus::Evento::Entregar(inf)).unwrap();
    };

    // cerrar
    let a = bus.crear_trabajo(orden(ea_bus::Modo::Mision, "A", tmp!())).unwrap();
    let (s, _) = post(&f, &format!("/api/trabajos/{a}/cerrar"), json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT, "en cola no se cierra");
    entrega(&a, true);
    let (_, c) = get(&f, "/api/colas").await;
    assert_eq!(c["claude-code"][0]["trabajo"], a.as_str());
    let (s, r) = post(&f, &format!("/api/trabajos/{a}/cerrar"), json!({})).await;
    assert_eq!((s, r["estado"].as_str()), (StatusCode::OK, Some("cerrado")));
    let (_, c) = get(&f, "/api/colas").await;
    assert!(c["claude-code"].as_array().unwrap().is_empty());

    // reformular exige nota y vuelve a la cola del modelo local
    let b = bus.crear_trabajo(orden(ea_bus::Modo::Mision, "B", tmp!())).unwrap();
    entrega(&b, true);
    let (s, _) = post(&f, &format!("/api/trabajos/{b}/reformular"), json!({})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, r) = post(&f, &format!("/api/trabajos/{b}/reformular"), json!({ "nota": "falta el caso vacío" })).await;
    assert_eq!((s, r["estado"].as_str()), (StatusCode::OK, Some("en_cola")));
    assert_eq!(bus.leer_orden(&b).unwrap().notas_revision, vec!["falta el caso vacío"]);

    // continuar y cancelar
    let c1 = bus.crear_trabajo(orden(ea_bus::Modo::Obra, "C", tmp!())).unwrap();
    entrega(&c1, false);
    let (s, r) = post(&f, &format!("/api/trabajos/{c1}/continuar"), json!({})).await;
    assert_eq!((s, r["estado"].as_str()), (StatusCode::OK, Some("en_cola")));
    let (s, r) = post(&f, &format!("/api/trabajos/{c1}/cancelar"), json!({})).await;
    assert_eq!((s, r["estado"].as_str()), (StatusCode::OK, Some("cancelado")));

    // confirmar el paquete de corrección de una rutina en espera
    let r1 = bus.crear_trabajo(orden(ea_bus::Modo::Rutina, "R", tmp!())).unwrap();
    let (s, _) = post(&f, &format!("/api/trabajos/{r1}/confirmar"), json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT, "solo en espera");
    bus.aplicar(&r1, ea_bus::Evento::Tomar).unwrap();
    bus.aplicar(&r1, ea_bus::Evento::PonerEnEspera).unwrap();
    std::fs::write(bus.raiz().join("trabajos").join(&r1).join("paquete-correccion.md"), "# Paquete\n").unwrap();
    let (_, j) = get(&f, &format!("/api/trabajos/{r1}")).await;
    assert!(j["paquete"].as_str().unwrap().contains("Paquete"));
    let (s, r) = post(&f, &format!("/api/trabajos/{r1}/confirmar"), json!({ "contexto": "ojo con la ruta" })).await;
    assert_eq!((s, r["estado"].as_str()), (StatusCode::OK, Some("en_revision")));
    let paquete = std::fs::read_to_string(bus.raiz().join("trabajos").join(&r1).join("paquete-correccion.md")).unwrap();
    assert!(paquete.contains("ojo con la ruta"));
    let (_, c) = get(&f, "/api/colas").await;
    assert_eq!(c["claude-code"][0]["trabajo"], r1.as_str());
}

/// Obra de tres fases entregada completa (las tres hechas), en revisión.
fn obra_completa(bus: &ea_bus::Bus) -> String {
    let fase = |n: u32| ea_bus::Fase {
        numero: n,
        titulo: format!("f{n}"),
        instrucciones: "i".into(),
        entregable: "e".into(),
        criterio: "true".into(),
        skills: vec![],
    };
    let o = ea_bus::Orden { criterio_exito: None, fases: vec![fase(1), fase(2), fase(3)], ..orden(ea_bus::Modo::Obra, "O", tmp!()) };
    let id = bus.crear_trabajo(o).unwrap();
    bus.aplicar(&id, ea_bus::Evento::Tomar).unwrap();
    let inf = ea_bus::Informe { fases_hechas: 3, obra_completa: true, ..ea_bus::Informe::nuevo(ea_bus::ResultadoInforme::Terminado) };
    bus.aplicar(&id, ea_bus::Evento::Entregar(inf)).unwrap();
    id
}

#[tokio::test]
async fn reformular_con_fase_fuera_de_rango_da_400() {
    let f = montar();
    let bus = bus(&f);
    let o = obra_completa(&bus);
    let antes = bus.leer_estado(&o).unwrap();
    for fase in [0, 4] {
        let (s, r) = post(&f, &format!("/api/trabajos/{o}/reformular"), json!({ "nota": "rehaz", "fase": fase })).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "fase {fase}: {r}");
    }
    // Una misión no tiene fases.
    let m = bus.crear_trabajo(orden(ea_bus::Modo::Mision, "M", tmp!())).unwrap();
    bus.aplicar(&m, ea_bus::Evento::Tomar).unwrap();
    bus.aplicar(&m, ea_bus::Evento::Entregar(ea_bus::Informe::nuevo(ea_bus::ResultadoInforme::Terminado))).unwrap();
    let (s, _) = post(&f, &format!("/api/trabajos/{m}/reformular"), json!({ "nota": "rehaz", "fase": 1 })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Nada escrito: ni notas ni estado.
    assert_eq!(bus.leer_estado(&o).unwrap(), antes);
    assert!(bus.leer_orden(&o).unwrap().notas_revision.is_empty());
    assert!(bus.leer_orden(&m).unwrap().notas_revision.is_empty());
    assert_eq!(bus.leer_estado(&m).unwrap().estado, ea_bus::EstadoTrabajo::EnRevision);
}

#[tokio::test]
async fn reformular_una_obra_con_fase_deja_la_fase_actual_en_ella() {
    let f = montar();
    let bus = bus(&f);
    let o = obra_completa(&bus);
    let (s, r) = post(&f, &format!("/api/trabajos/{o}/reformular"), json!({ "nota": "rehaz la 2", "fase": 2 })).await;
    assert_eq!((s, r["estado"].as_str()), (StatusCode::OK, Some("en_cola")));
    let (_, j) = get(&f, &format!("/api/trabajos/{o}")).await;
    assert_eq!(j["estado"]["fase_actual"], json!(2));
    assert_eq!(j["orden"]["notas_revision"], json!(["rehaz la 2"]));
    // Sin fase, una obra completa rehace la última.
    let o2 = obra_completa(&bus);
    let (s, _) = post(&f, &format!("/api/trabajos/{o2}/reformular"), json!({ "nota": "retoca" })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(bus.leer_estado(&o2).unwrap().fase_actual, Some(3));
}

// ------------------------------------------------------- crear una misión

#[tokio::test]
async fn crear_una_mision_desde_la_web_la_encola() {
    let f = montar();
    let wt = repo_git(&f);
    // El formulario da los modos, el techo y los topes por defecto.
    let (s, form) = get(&f, "/api/trabajos/nuevo").await;
    assert_eq!(s, StatusCode::OK, "{form}");
    assert_eq!(form["modos"], json!(["mision", "obra"]));
    let techo_tiempo = form["techo"]["tiempo_s"].as_u64().unwrap();
    assert!(form["defecto"]["iteraciones"].as_u64().unwrap() <= form["techo"]["iteraciones"].as_u64().unwrap());

    // Sin canonicalizar: la orden lleva la ruta canónica.
    let (s, r) = post(&f, "/api/trabajos", mision(&format!("{}/./", wt.display()))).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    assert_eq!(r["estado"], "en_cola");
    let id = r["id"].as_str().unwrap().to_string();

    let (_, colas) = get(&f, "/api/colas").await;
    let ml = colas["modelo-local"].as_array().unwrap();
    assert!(ml.iter().any(|e| e["trabajo"] == id.as_str()), "{colas}");

    let (s, d) = get(&f, &format!("/api/trabajos/{id}")).await;
    assert_eq!(s, StatusCode::OK);
    let o = &d["orden"];
    assert_eq!(o["creada_por"], "usuario");
    assert_eq!(o["modo"], "mision");
    assert_eq!(o["worktree"], wt.to_str().unwrap());
    assert_eq!(o["criterio_exito"], "test -f hola.txt");
    assert!(o.get("skills").is_none(), "sin skills: {o}");
    assert_eq!(o["topes"]["iteraciones"], 3);
    // Lo que pasa del techo se recorta, como hace el ejecutor.
    assert_eq!(o["topes"]["tiempo_s"].as_u64().unwrap(), techo_tiempo);
    // El criterio no se ha ejecutado: no hay informe.
    assert!(d["informe"].is_null());

    // El diario apunta la creación con el usuario como actor.
    let (_, ev) = get(&f, "/api/diario").await;
    let creado = ev.as_array().unwrap().iter().find(|e| e["accion"] == "bus_crear").expect("bus_crear en el diario");
    assert_eq!(creado["actor"], "usuario");

    // Y el worktree queda sugerido para la próxima.
    let (_, form) = get(&f, "/api/trabajos/nuevo").await;
    assert!(form["worktrees"].as_array().unwrap().iter().any(|w| w == wt.to_str().unwrap()), "{form}");
}

#[tokio::test]
async fn crear_sin_criterio_o_worktree_invalido_da_400_y_no_escribe() {
    let f = montar();
    let wt = repo_git(&f);
    let wt = wt.to_str().unwrap();
    let sin_repo = f._dir.path().join("sin-repo");
    std::fs::create_dir_all(&sin_repo).unwrap();
    let fichero = f._dir.path().join("fichero.txt");
    std::fs::write(&fichero, "x").unwrap();
    // U-03: un subdirectorio del repo no es su raíz.
    let sub = std::path::Path::new(wt).join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let con = |campo: &str, valor: Value| {
        let mut m = mision(wt);
        m[campo] = valor;
        m
    };
    let sin = |campo: &str| {
        let mut m = mision(wt);
        m.as_object_mut().unwrap().remove(campo);
        m
    };
    let casos = [
        con("criterio", json!("")),
        con("criterio", json!("   ")),
        sin("criterio"),
        con("criterio", json!("x".repeat(2_001))),
        con("worktree", json!("")),
        con("worktree", json!("relativo/a/algo")),
        con("worktree", json!(concat!(tmp!(), "/no/existe/de/ninguna/manera"))),
        con("worktree", json!(sin_repo.to_str().unwrap())),
        con("worktree", json!(fichero.to_str().unwrap())),
        con("worktree", json!(sub.to_str().unwrap())),
        sin("worktree"),
        con("titulo", json!("")),
        con("titulo", json!("dos\nlineas")),
        con("titulo", json!("t".repeat(201))),
        con("objetivo", json!("")),
        con("modo", json!("obra")),
        con("modo", json!("rutina")),
        con("skills", json!(["git"])),
        con("creada_por", json!("claude-code")),
        con("topes", json!({ "iteraciones": 0 })),
        con("topes", json!({ "turnos": 99 })),
        json!("no es un objeto"),
    ];
    for c in casos {
        let (s, r) = post(&f, "/api/trabajos", c.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{c} -> {r}");
        assert!(r["error"].as_str().is_some_and(|e| !e.is_empty()), "mensaje: {r}");
    }
    // Un JSON roto también es 400.
    let (s, _) = enviar(&f, Method::POST, "/api/trabajos", "localhost", None, true).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Nada en el bus: ni trabajos, ni colas, ni eventos de creación.
    let (_, t) = get(&f, "/api/trabajos").await;
    assert_eq!(t.as_array().map(Vec::len), Some(0), "{t}");
    let (_, colas) = get(&f, "/api/colas").await;
    assert_eq!(colas["modelo-local"].as_array().map(Vec::len), Some(0), "{colas}");
    let (_, ev) = get(&f, "/api/diario").await;
    assert!(!acciones_del_diario(&ev).iter().any(|a| a == "bus_crear"));
    let trabajos = std::fs::read_dir(f._dir.path().join("estado/trabajos")).map(|d| d.count()).unwrap_or(0);
    assert_eq!(trabajos, 0);
}

#[tokio::test]
async fn crear_exige_json_host_local_y_clave() {
    let f = montar();
    let wt = repo_git(&f);
    let cuerpo = mision(wt.to_str().unwrap());
    let clave = f.state.clave.texto().to_string();
    // Sin clave o con una mala: 403.
    for c in [None, Some("mala")] {
        let (s, t) = enviar_con_clave(&f, Method::POST, "/api/trabajos", "localhost", Some(cuerpo.clone()), true, c).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{c:?}: {t}");
    }
    // Con la clave pero sin JSON: 415.
    let (s, _) =
        enviar_con_clave(&f, Method::POST, "/api/trabajos", "localhost", Some(cuerpo.clone()), false, Some(&clave)).await;
    assert_eq!(s, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    // Con la clave desde otro host: 403.
    let (s, _) =
        enviar_con_clave(&f, Method::POST, "/api/trabajos", "evil.example", Some(cuerpo.clone()), true, Some(&clave)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (_, t) = get(&f, "/api/trabajos").await;
    assert_eq!(t.as_array().map(Vec::len), Some(0), "{t}");
    // Con todo en regla, se crea.
    let (s, r) =
        enviar_con_clave(&f, Method::POST, "/api/trabajos", "127.0.0.1:8092", Some(cuerpo), true, Some(&clave)).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
}

// --------------------------------------------------------- crear una obra

fn fase(n: u32) -> Value {
    json!({
        "titulo": format!("Fase {n}"),
        "instrucciones": format!("Crea f{n}.txt."),
        "entregable": format!("f{n}.txt"),
        "criterio": format!("test -f f{n}.txt"),
    })
}

fn obra(wt: &str) -> Value {
    json!({
        "modo": "obra",
        "titulo": "Tres ficheros",
        "worktree": wt,
        "objetivo": "Crea f1.txt, f2.txt y f3.txt, uno por fase.",
        "fases": [fase(1), fase(2), fase(3)],
        "revisar_cada": 2,
        "topes": { "iteraciones": 2, "tiempo_s": 999_999 },
    })
}

/// Nada en el bus: ni trabajos, ni colas, ni eventos de creación.
async fn bus_vacio(f: &Fixture) {
    let (_, t) = get(f, "/api/trabajos").await;
    assert_eq!(t.as_array().map(Vec::len), Some(0), "{t}");
    let (_, colas) = get(f, "/api/colas").await;
    assert_eq!(colas["modelo-local"].as_array().map(Vec::len), Some(0), "{colas}");
    let (_, ev) = get(f, "/api/diario").await;
    assert!(!acciones_del_diario(&ev).iter().any(|a| a == "bus_crear"));
    let trabajos = std::fs::read_dir(f._dir.path().join("estado/trabajos")).map(|d| d.count()).unwrap_or(0);
    assert_eq!(trabajos, 0);
}

#[tokio::test]
async fn crear_una_obra_valida_cada_fase() {
    let f = montar();
    let wt = repo_git(&f);
    let wt = wt.to_str().unwrap();
    let con = |campo: &str, valor: Value| {
        let mut o = obra(wt);
        o[campo] = valor;
        o
    };
    let sin = |campo: &str| {
        let mut o = obra(wt);
        o.as_object_mut().unwrap().remove(campo);
        o
    };
    let fase_con = |campo: &str, valor: Value| {
        let mut o = obra(wt);
        o["fases"][1][campo] = valor;
        o
    };
    let fase_sin = |campo: &str| {
        let mut o = obra(wt);
        o["fases"][1].as_object_mut().unwrap().remove(campo);
        o
    };
    let casos = [
        // Una fase sin criterio: la rechaza el bus antes de escribir nada.
        (fase_con("criterio", json!("")), "la fase 2 no tiene criterio"),
        (fase_con("criterio", json!("   ")), "la fase 2 no tiene criterio"),
        (fase_sin("criterio"), "la fase 2 no tiene criterio"),
        // revisar_cada se elige expresamente: sin él no hay valor por defecto.
        (sin("revisar_cada"), "revisar_cada"),
        (con("revisar_cada", json!(0)), "revisar_cada"),
        (con("revisar_cada", json!(4)), "revisar_cada"),
        (con("revisar_cada", json!(-1)), "revisar_cada"),
        (con("revisar_cada", json!("2")), "revisar_cada"),
        (sin("fases"), "al menos una fase"),
        (con("fases", json!([])), "al menos una fase"),
        (con("fases", json!((1..=21).map(fase).collect::<Vec<_>>())), "como mucho"),
        (con("criterio", json!("test -f todo")), "cada fase"),
        (fase_con("titulo", json!("")), "fase 2: titulo"),
        (fase_con("titulo", json!("dos\nlineas")), "fase 2: titulo"),
        (fase_con("instrucciones", json!(" ")), "fase 2: instrucciones"),
        (fase_sin("entregable"), "entregable"),
        (fase_con("criterio", json!("x".repeat(2_001))), "fase 2: criterio"),
        // Ni skills ni número por fase: no se eligen desde la web.
        (fase_con("skills", json!(["git"])), "skills"),
        (fase_con("numero", json!(7)), "numero"),
        (con("fases", json!("no es una lista")), "petición inválida"),
        (con("skills", json!(["git"])), "skills"),
        // Una misión no lleva fases ni revisar_cada.
        (con("modo", json!("mision")), "modo 'obra'"),
    ];
    for (c, motivo) in casos {
        let (s, r) = post(&f, "/api/trabajos", c.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{c} -> {r}");
        assert!(r["error"].as_str().is_some_and(|e| e.contains(motivo)), "{motivo}: {r}");
    }
    bus_vacio(&f).await;

    // Una obra válida: 201, en cola, en la fase 1, con las fases numeradas
    // por su orden y sin criterio general.
    let (s, r) = post(&f, "/api/trabajos", obra(wt)).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    assert_eq!(r["estado"], "en_cola");
    let id = r["id"].as_str().unwrap().to_string();
    let (_, d) = get(&f, &format!("/api/trabajos/{id}")).await;
    assert_eq!(d["estado"]["fase_actual"], 1, "{d}");
    let o = &d["orden"];
    assert_eq!(o["modo"], "obra");
    assert_eq!(o["creada_por"], "usuario");
    assert_eq!(o["revisar_cada"], 2);
    assert!(o["criterio_exito"].is_null(), "{o}");
    assert!(o.get("skills").is_none(), "sin skills: {o}");
    assert_eq!(o["topes"]["iteraciones"], 2);
    let fases = o["fases"].as_array().unwrap();
    assert_eq!(fases.len(), 3);
    for (i, fz) in fases.iter().enumerate() {
        assert_eq!(fz["numero"], i + 1);
        assert_eq!(fz["criterio"], format!("test -f f{}.txt", i + 1));
        assert!(fz.get("skills").is_none(), "{fz}");
    }
    // Lo mismo en el orden.json del bus.
    let disco: Value = serde_json::from_str(
        &std::fs::read_to_string(f._dir.path().join("estado/trabajos").join(&id).join("orden.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(disco["fases"], o["fases"]);
    // El criterio no se ha ejecutado y el diario apunta al usuario.
    assert!(d["informe"].is_null());
    let (_, ev) = get(&f, "/api/diario").await;
    let creado = ev.as_array().unwrap().iter().find(|e| e["accion"] == "bus_crear").expect("bus_crear en el diario");
    assert_eq!(creado["actor"], "usuario");
    let (_, lista) = get(&f, "/api/trabajos").await;
    assert_eq!(lista[0]["fases_total"], 3);

    // `revisar_cada: null` (solo al final) es una elección válida.
    let (s, r) = post(&f, "/api/trabajos", con("revisar_cada", Value::Null)).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    let (_, d) = get(&f, &format!("/api/trabajos/{}", r["id"].as_str().unwrap())).await;
    assert!(d["orden"]["revisar_cada"].is_null());
}

#[tokio::test]
async fn crear_en_un_repo_sin_commits_da_400() {
    let f = montar();
    let vacio = repo_git_vacio(&f, "sin-commits");
    let vacio = vacio.to_str().unwrap();
    for cuerpo in [mision(vacio), obra(vacio)] {
        let (s, r) = post(&f, "/api/trabajos", cuerpo).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
        assert!(r["error"].as_str().unwrap().contains("el repo no tiene ningún commit"), "{r}");
    }
    bus_vacio(&f).await;
}

// ------------------------------------------------- ficha completa (T6, H18)

/// `git` en `wt` con el índice que se pida (para las instantáneas sin tocar
/// el índice del repo, como hace el ejecutor). Devuelve stdout recortado.
fn git_en(wt: &std::path::Path, indice: Option<&std::path::Path>, args: &[&str]) -> String {
    let mut c = std::process::Command::new("git");
    c.arg("-C").arg(wt).args(["-c", "user.name=prueba", "-c", "user.email=prueba@localhost", "-c", "commit.gpgsign=false"]);
    if let Some(i) = indice {
        c.env("GIT_INDEX_FILE", i);
    }
    let o = c.args(args).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// Instantánea del árbol de trabajo con un índice aparte, como al cerrar una fase.
fn instantanea(wt: &std::path::Path) -> String {
    let idx = wt.parent().unwrap().join("indice-instantanea");
    let _ = std::fs::remove_file(&idx);
    git_en(wt, Some(&idx), &["add", "-A"]);
    git_en(wt, Some(&idx), &["write-tree"])
}

#[tokio::test]
async fn diff_de_una_fase_usa_las_instantaneas() {
    let f = montar();
    let wt = f._dir.path().join("obra");
    std::fs::create_dir_all(&wt).unwrap();
    git_en(&wt, None, &["init", "-q"]);
    std::fs::write(wt.join("base.txt"), "base\n").unwrap();
    git_en(&wt, None, &["add", "-A"]);
    git_en(&wt, None, &["commit", "-q", "-m", "inicio"]);
    let inicio = git_en(&wt, None, &["rev-parse", "HEAD"]);

    let fase = |n: u32| -> ea_bus::Fase {
        serde_json::from_value(json!({
            "numero": n, "titulo": format!("fase {n}"), "instrucciones": format!("haz la {n}"),
            "entregable": format!("f{n}.txt"), "criterio": format!("test -f f{n}.txt"),
        }))
        .unwrap()
    };
    let mut o = orden(ea_bus::Modo::Obra, "Obra en tres fases", wt.to_str().unwrap());
    o.criterio_exito = None;
    o.fases = vec![fase(1), fase(2), fase(3)];
    let store = f.state.store.lock().await.duplicar().unwrap();
    let bus = bus(&f).con_store(store);
    let id = bus.crear_trabajo(o).unwrap();

    // La obra cierra las fases 1 y 2 con su instantánea; la 3 sigue pendiente.
    std::fs::write(wt.join("f1.txt"), "uno\n").unwrap();
    let a1 = instantanea(&wt);
    let hecha = |diff_stat: &str, arbol: &str| ea_bus::InformeFase { diff_stat: Some(diff_stat.into()), arbol: Some(arbol.into()), motivo: None };
    bus.registrar_fase(&id, 1, ea_bus::EstadoFase::Hecha, &hecha(" f1.txt | 1 +", &a1)).unwrap();
    std::fs::write(wt.join("f2.txt"), "dos\n").unwrap();
    std::fs::write(wt.join("base.txt"), "base cambiada\n").unwrap();
    let a2 = instantanea(&wt);
    bus.registrar_fase(&id, 2, ea_bus::EstadoFase::Hecha, &hecha(" 2 files changed", &a2)).unwrap();

    // Sin informe todavía: la fase 2 ya tiene base (la instantánea de la 1);
    // la 1 no (le falta commit_inicio) y lo dice con un 404 claro.
    let (s, d) = get(&f, &format!("/api/trabajos/{id}/diff?fase=2")).await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert!(d["commit_inicio"].is_null());
    let (s, e) = get(&f, &format!("/api/trabajos/{id}/diff?fase=1")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert!(e["error"].as_str().unwrap().contains("commit_inicio"), "{e}");

    bus.escribir_informe(
        &id,
        &serde_json::from_value(json!({ "resultado": "terminado", "commit_inicio": inicio })).unwrap(),
    )
    .unwrap();

    let (s, d1) = get(&f, &format!("/api/trabajos/{id}/diff?fase=1")).await;
    assert_eq!(s, StatusCode::OK, "{d1}");
    let t1 = d1["diff"].as_str().unwrap();
    assert_eq!((d1["fase"].as_u64(), d1["truncado"].as_bool()), (Some(1), Some(false)));
    assert!(t1.contains("f1.txt") && t1.contains("+uno"), "{t1}");
    assert!(!t1.contains("f2.txt") && !t1.contains("base cambiada"), "solo lo de la fase 1: {t1}");

    let (_, d2) = get(&f, &format!("/api/trabajos/{id}/diff?fase=2")).await;
    let t2 = d2["diff"].as_str().unwrap();
    assert!(t2.contains("f2.txt") && t2.contains("+base cambiada"), "{t2}");
    assert!(!t2.contains("f1.txt"), "solo lo de la fase 2: {t2}");

    // Sin instantánea: 404 con el motivo. Fase 0 o no entera: 400.
    let (s, e) = get(&f, &format!("/api/trabajos/{id}/diff?fase=3")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert!(e["error"].as_str().unwrap().contains("no hay instantánea de árbol de la fase 3"), "{e}");
    let (s, _) = get(&f, &format!("/api/trabajos/{id}/diff?fase=0")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = get(&f, &format!("/api/trabajos/{id}/diff?fase=dos")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = get(&f, "/api/trabajos/no-existe/diff?fase=1").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // Sin fase, el diff completo de siempre.
    let (s, d) = get(&f, &format!("/api/trabajos/{id}/diff")).await;
    assert_eq!((s, d["fase"].is_null()), (StatusCode::OK, true));
    assert!(d["diff"].as_str().unwrap().contains("f2.txt"));

    // La ficha: estado de cada fase, si tiene diff y el resumen del agente.
    let (_, j) = get(&f, &format!("/api/trabajos/{id}")).await;
    let fases = j["fases"].as_array().unwrap();
    assert_eq!(fases.len(), 3);
    assert_eq!((fases[0]["estado"].as_str(), fases[0]["instantanea"].as_bool()), (Some("hecha"), Some(true)));
    assert_eq!(fases[1]["diff_stat"], " 2 files changed");
    assert_eq!((fases[2]["estado"].as_str(), fases[2]["instantanea"].as_bool()), (Some("pendiente"), Some(false)));
    assert_eq!(j["orden"]["fases"][2]["criterio"], "test -f f3.txt");
    assert_eq!(j["orden"]["creada_por"], "test");
    assert!(j["resumen_agente"].is_null());
    let dir = bus.raiz().join("trabajos").join(&id);
    std::fs::write(dir.join("salida-qwen.json"), r#"[{"type":"result","result":"  hecho: f1 y f2  "}]"#).unwrap();
    let (_, j) = get(&f, &format!("/api/trabajos/{id}")).await;
    assert_eq!(j["resumen_agente"], "hecho: f1 y f2");

    // Todo esto es lectura: el trabajo sigue donde estaba.
    assert_eq!(j["estado"]["estado"], "en_cola");
}

#[tokio::test]
async fn el_diff_es_el_de_git_mas_los_ficheros_nuevos() {
    let f = montar();
    let wt = f._dir.path().join("repo");
    std::fs::create_dir_all(&wt).unwrap();
    git_en(&wt, None, &["init", "-q"]);
    git_en(&wt, None, &["config", "core.autocrlf", "false"]);
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    git_en(&wt, None, &["add", "-A"]);
    git_en(&wt, None, &["commit", "-q", "-m", "inicio"]);
    let inicio = git_en(&wt, None, &["rev-parse", "HEAD"]);
    // Un cambio y ficheros nuevos de todo tipo; `__pycache__` no sale.
    std::fs::write(wt.join("a.txt"), "a\nb\n").unwrap();
    std::fs::write(wt.join("nuevo.txt"), "hola\n").unwrap();
    std::fs::write(wt.join("sin-salto.txt"), "x").unwrap();
    std::fs::write(wt.join("datos.bin"), [0xff, 0xfe, 0x00, 0x01]).unwrap();
    std::fs::write(wt.join("canción.txt"), "ñ\n").unwrap();
    std::fs::create_dir_all(wt.join("sub dir")).unwrap();
    std::fs::write(wt.join("sub dir/espacio.txt"), "e\n").unwrap();
    std::fs::create_dir_all(wt.join("__pycache__")).unwrap();
    std::fs::write(wt.join("__pycache__/m.pyc"), "pyc").unwrap();

    let bus = bus(&f);
    let id = bus.crear_trabajo(orden(ea_bus::Modo::Mision, "Diff", wt.to_str().unwrap())).unwrap();
    bus.escribir_informe(&id, &serde_json::from_value(json!({ "resultado": "terminado", "commit_inicio": inicio })).unwrap())
        .unwrap();
    let (s, d) = get(&f, &format!("/api/trabajos/{id}/diff")).await;
    assert_eq!((s, d["truncado"].as_bool()), (StatusCode::OK, Some(false)), "{d}");
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(&wt)
        .args(["diff", &inicio, "--", ".", ea_bus::EXCLUIR_DIFF])
        .output()
        .unwrap();
    let esperado = String::from_utf8_lossy(&o.stdout).into_owned()
        + "\n=== fichero nuevo sin seguimiento: canción.txt ===\nñ\n"
        + "\n=== fichero nuevo sin seguimiento: datos.bin (binario o ilegible) ===\n"
        + "\n=== fichero nuevo sin seguimiento: nuevo.txt ===\nhola\n"
        + "\n=== fichero nuevo sin seguimiento: sin-salto.txt ===\nx"
        + "\n=== fichero nuevo sin seguimiento: sub dir/espacio.txt ===\ne\n";
    assert!(esperado.contains("+b\n"), "{esperado}");
    assert_eq!(d["diff"].as_str().unwrap(), esperado);
}

// ------------------------------------------------------- skills del ejecutor

fn catalogo_de_prueba(f: &Fixture) {
    let dir = f._dir.path().join("config/qwen-skills");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("rust.md"), "---\ndescripcion: Cargo\n---\ncargo test -p\n").unwrap();
    std::fs::write(dir.join("shell.md"), "---\ndescripcion: Shell\n---\nset -euo pipefail\n").unwrap();
}

#[tokio::test]
async fn el_catalogo_web_dice_quien_tiene_la_llave() {
    let f = montar();
    catalogo_de_prueba(&f);
    let (s, c) = get(&f, "/api/skills").await;
    assert_eq!(s, StatusCode::OK, "{c}");
    let nombres: Vec<_> = c["skills"].as_array().unwrap().iter().map(|k| k["nombre"].as_str().unwrap()).collect();
    assert_eq!(nombres, ["rust", "shell"]);
    assert_eq!(c["skills"][0]["descripcion"], "Cargo");
    assert!(c["skills"][0].get("cuerpo").is_none(), "el cuerpo no sale: {c}");
    assert_eq!(c["max_por_trabajo"], 3);
    assert_eq!(c["pueden_elegir"], json!(["claude-code"]));
    assert_eq!(c["usuario_tiene_llave"], false);
}

#[tokio::test]
async fn probar_skills_desde_la_web_reutiliza_la_validacion() {
    let f = montar();
    catalogo_de_prueba(&f);
    // Sin `por`, el actor es `usuario`, que no tiene la llave.
    let (s, r) = post(&f, "/api/skills/probar", json!({ "skills": ["rust"] })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
    assert!(r["error"].as_str().unwrap().contains("llave"), "{r}");
    // Como lo vería el planificador: bien, y con el recuento.
    let (s, r) = post(&f, "/api/skills/probar", json!({ "skills": ["rust", "shell", "rust"], "por": "claude-code" })).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["skills"], json!(["rust", "shell"]));
    assert!(r["caracteres"].as_u64().unwrap() > 0);
    // Inexistente y por encima del tope.
    let (s, r) = post(&f, "/api/skills/probar", json!({ "skills": ["nada"], "por": "claude-code" })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(r["error"].as_str().unwrap().contains("no existe"), "{r}");
    let (s, r) = post(&f, "/api/skills/probar", json!({ "skills": ["a", "b", "c", "d"], "por": "claude-code" })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(r["error"].as_str().unwrap().contains("máximo"), "{r}");
    // Una lista vacía no tiene nada que validar; un campo desconocido, 4xx.
    let (s, _) = post(&f, "/api/skills/probar", json!({ "skills": [] })).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = post(&f, "/api/skills/probar", json!({ "skills": [], "otro": 1 })).await;
    assert!(s.is_client_error());
}

/// Worktrees propios (spec 019, anexo A): listarlos, verlos en la ficha del
/// trabajo y quitar el de un trabajo terminado con simulación y revalidación;
/// la rama se conserva y un worktree con cambios no se toca (sin --force).
#[tokio::test]
async fn los_worktrees_propios_se_listan_y_se_limpian_con_simulacion() {
    let f = montar();
    let bus = bus(&f);
    let repo = repo_git(&f);
    let (s, v) = get(&f, "/api/worktrees").await;
    assert_eq!((s, v), (StatusCode::OK, json!([])));

    let mut o = orden(ea_bus::Modo::Mision, "Con worktree", repo.to_str().unwrap());
    o.worktree_propio = true;
    let id = bus.crear_trabajo(o.clone()).unwrap().to_string();
    let ruta = bus.preparar_worktree(&id, &o).unwrap();

    let (_, v) = get(&f, "/api/worktrees").await;
    assert_eq!(v.as_array().unwrap().len(), 1, "{v}");
    assert_eq!(v[0]["trabajo"], id.as_str());
    assert_eq!(v[0]["rama"], format!("ea/{id}"));
    assert_eq!(v[0]["limpio"], true);
    assert_eq!(v[0]["estado_trabajo"], "en_cola");
    let (_, d) = get(&f, &format!("/api/trabajos/{id}")).await;
    assert_eq!(d["worktree"]["rama"], format!("ea/{id}"), "{d}");

    let pedir = json!({ "herramienta": "ea_limpiar_worktree", "argumentos": { "trabajo": id } });
    // Sin terminar, no se limpia.
    let (s, r) = post(&f, "/api/acciones/simular", pedir.clone()).await;
    assert_eq!(s, StatusCode::CONFLICT, "{r}");
    bus.aplicar(&id, ea_bus::Evento::Cancelar).unwrap();

    // Con cambios sin guardar, tampoco: nunca se fuerza.
    std::fs::write(ruta.join("nuevo.txt"), "x").unwrap();
    let (s, r) = post(&f, "/api/acciones/simular", pedir.clone()).await;
    assert_eq!(s, StatusCode::CONFLICT, "{r}");
    assert!(r.to_string().contains("nunca se fuerza"), "{r}");
    std::fs::remove_file(ruta.join("nuevo.txt")).unwrap();

    let (s, sim) = post(&f, "/api/acciones/simular", pedir).await;
    assert_eq!(s, StatusCode::OK, "{sim}");
    assert!(ruta.exists(), "simular no cambia nada");
    let (s, r) = post(
        &f,
        "/api/acciones/ejecutar",
        json!({ "herramienta": sim["herramienta"], "argumentos": sim["argumentos"], "precondicion": sim["precondicion"] }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert!(!ruta.exists());
    let (_, v) = get(&f, "/api/worktrees").await;
    assert_eq!(v, json!([]));
    let rama = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["rev-parse", "--verify", "--quiet", &format!("refs/heads/ea/{id}")])
        .status()
        .unwrap();
    assert!(rama.success(), "la rama se conserva");
}

/// El texto exacto de los trabajos de `/api/trabajos` y de su cuenta en `/api/estado`.
#[tokio::test]
async fn los_resumenes_de_trabajos_dicen_lo_mismo_que_siempre() {
    let f = montar();
    let bus = ea_bus::Bus::abrir_en(&f.state.config.estado_dir).unwrap();
    let ids = ea_pruebas::sembrar_trabajos(&bus, tmp!());

    let (s, estado) = enviar(&f, Method::GET, "/api/estado", "localhost:8092", None, false).await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        estado.contains(r#""trabajos":{"en_cola":1,"ejecutando":1,"en_revision":1,"en_espera":0,"cerrado":1,"cancelado":0}"#),
        "{estado}"
    );

    let (s, mut lista) = enviar(&f, Method::GET, "/api/trabajos", "localhost:8092", None, false).await;
    assert_eq!(s, StatusCode::OK);
    for (n, id) in ids.iter().enumerate() {
        lista = lista.replace(&bus.leer_estado(id).unwrap().actualizado_en, &format!("<actualizado {}>", n + 1));
    }
    let fila = |id: &str, modo: &str, titulo: &str, estado: &str, fase: &str, fases: u32, detalle: &str, resultado: &str, completa: &str, n: u32| {
        format!(
            r#"{{"id":"{id}","modo":"{modo}","titulo":"{titulo}","estado":"{estado}","fase_actual":{fase},"fases_total":{fases},"iteracion":0,"detalle":{detalle},"resultado":{resultado},"obra_completa":{completa},"cancelando":false,"rutina":null,"worktree":"{}","creado":"2026-01-01T00:00:00Z","actualizado":"<actualizado {n}>","ejecutado_por":null}}"#,
            tmp!()
        )
    };
    let esperado = [
        fila(&ids[3], "mision", "misión 4", "en_revision", "null", 0, r#""no pasa""#, r#""bloqueado""#, "false", 4),
        fila(&ids[2], "obra", "una obra", "ejecutando", "1", 2, "null", "null", "null", 3),
        fila(&ids[1], "mision", "misión 2", "en_cola", "null", 0, "null", "null", "null", 2),
        fila(&ids[0], "mision", "misión 1", "cerrado", "null", 0, "null", r#""terminado""#, "false", 1),
    ];
    assert_eq!(lista, format!("[{}]", esperado.join(",")));
}
