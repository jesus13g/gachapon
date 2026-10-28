//! Rutinas: listar, ver, lanzar, registrar y validar, versiones, programar
//! su timer, retirar y restaurar.

use crate::comun::*;

#[tokio::test]
async fn rutinas_listar_ver_y_lanzar() {
    let f = montar();

    let (_, lista) = get(&f, "/api/rutinas").await;
    assert_eq!(lista[0]["slug"], "revision-diaria");
    assert_eq!(lista[0]["version"], 1);

    let (s, r) = get(&f, "/api/rutinas/revision-diaria").await;
    assert_eq!(s, StatusCode::OK);
    assert!(r["contenido"].as_str().unwrap().contains("revisa"));

    assert_eq!(lista[0]["entradas"]["salida"], "informe.md");
    let trabajos = || bus(&f).listar_trabajos().unwrap().len();

    // Vista previa: lo sustituido, sin crear trabajo.
    let (s, p) = post(&f, "/api/rutinas/revision-diaria/previa", json!({ "entradas": { "salida": "otro.md" } })).await;
    assert_eq!(s, StatusCode::OK, "{p}");
    assert_eq!(p["criterio"], "test -f otro.md");
    assert!(p["objetivo"].as_str().unwrap().contains("revisa y escribe otro.md"));
    assert_eq!(p["version"], 1);
    let (s, p) = post(&f, "/api/rutinas/revision-diaria/previa", json!({})).await;
    assert_eq!((s, p["criterio"].as_str()), (StatusCode::OK, Some("test -f informe.md")));
    let (s, _) = post(&f, "/api/rutinas/revision-diaria/previa", json!({ "entradas": { "noexiste": "1" } })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = post(&f, "/api/rutinas/revision-diaria/previa", json!({ "entradas": { "salida": "a\nrm x" } })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "sin saltos de línea");
    let (s, _) = post(&f, "/api/rutinas/no-existe/previa", json!({})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // La vista previa pasa por la guardia de la clave como toda escritura.
    let (s, _) = enviar_con_clave(&f, Method::POST, "/api/rutinas/revision-diaria/previa", "localhost:8092", Some(json!({})), true, None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(trabajos(), 0, "la vista previa no crea trabajo");

    // Una entrada no declarada: 400 y ningún trabajo.
    let (s, e) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({ "entradas": { "noexiste": "1" } })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(e["error"].as_str().unwrap().contains("noexiste"));
    let (s, _) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({ "otra": 1 })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "campo desconocido en el cuerpo");
    assert_eq!(trabajos(), 0);

    let (s, t) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({ "entradas": { "salida": "otro.md" } })).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(t["rutina"], "revision-diaria");
    let id = t["trabajo"].as_str().unwrap().to_string();
    let (_, j) = get(&f, &format!("/api/trabajos/{id}")).await;
    assert_eq!(j["estado"]["estado"], "en_cola");
    let orden = bus(&f).leer_orden(&id).unwrap();
    assert_eq!(orden.criterio_exito.as_deref(), Some("test -f otro.md"));
    assert!(orden.objetivo.contains("revisa y escribe otro.md"));
    assert!(!orden.objetivo.contains("informe.md"));
    let (s, _) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT, "ya hay una ejecución en curso");
    // Cuerpo vacío: los valores por defecto (lo que ya hacía la página).
    bus(&f).aplicar(&id, ea_bus::Evento::Cancelar).unwrap();
    let (s, t) = enviar(&f, Method::POST, "/api/rutinas/revision-diaria/lanzar", "localhost:8092", None, true).await;
    assert_eq!(s, StatusCode::CREATED, "{t}");
    let id: String = serde_json::from_str::<Value>(&t).unwrap()["trabajo"].as_str().unwrap().into();
    assert_eq!(bus(&f).leer_orden(&id).unwrap().criterio_exito.as_deref(), Some("test -f informe.md"));

    let (s, _) = get(&f, "/api/rutinas/no-existe").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = post(&f, "/api/rutinas/no-existe/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = get(&f, "/api/rutinas/a.b").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = post(&f, "/api/rutinas/a.b/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn la_ficha_de_una_rutina_lista_sus_ejecuciones_mas_recientes_primero() {
    let f = montar();
    let bus = bus(&f);

    // Sin trabajos: la fila no tiene última y la ficha no tiene ejecuciones.
    let (_, l) = get(&f, "/api/rutinas").await;
    assert!(l[0]["ultima"].is_null(), "{l}");
    let (_, r) = get(&f, "/api/rutinas/revision-diaria").await;
    assert_eq!(r["ejecuciones"], json!([]));

    // Doce ejecuciones (v1 las seis primeras, v2 el resto), intercaladas con
    // una misión y con otra rutina. Los ids fijan el orden (empiezan por la
    // fecha); se crean desordenados para que no cuente el orden de creación.
    let de_rutina = |n: u32, slug: &str, version: u32| {
        let mut o = orden(ea_bus::Modo::Rutina, slug, tmp!());
        o.id = format!("20260101T0000{n:02}-0000aaaa");
        o.rutina = Some(ea_bus::RutinaRef { slug: slug.into(), version });
        o
    };
    for n in (1..=12).rev() {
        bus.crear_trabajo(de_rutina(n, "revision-diaria", if n <= 6 { 1 } else { 2 })).unwrap();
    }
    let mut otra = de_rutina(13, "otra-rutina", 1);
    otra.id = "20260101T000013-0000bbbb".into();
    bus.crear_trabajo(otra).unwrap();
    let mut mision = orden(ea_bus::Modo::Mision, "Una misión", tmp!());
    mision.id = "20260101T000014-0000cccc".into();
    bus.crear_trabajo(mision).unwrap();
    // La más reciente entregó informe; la anterior se canceló.
    let ultimo = "20260101T000012-0000aaaa";
    bus.aplicar(ultimo, ea_bus::Evento::Tomar).unwrap();
    let informe = ea_bus::Informe { duracion_s: 42, ..ea_bus::Informe::nuevo(ea_bus::ResultadoInforme::Bloqueado) };
    bus.aplicar(ultimo, ea_bus::Evento::Entregar(informe)).unwrap();
    bus.aplicar("20260101T000011-0000aaaa", ea_bus::Evento::Cancelar).unwrap();
    let estado_ultimo = bus.leer_estado(ultimo).unwrap().estado.as_str();

    let (s, r) = get(&f, "/api/rutinas/revision-diaria").await;
    assert_eq!(s, StatusCode::OK);
    let ej = r["ejecuciones"].as_array().unwrap();
    let ids: Vec<&str> = ej.iter().map(|e| e["trabajo"].as_str().unwrap()).collect();
    let esperados: Vec<String> = (3..=12).rev().map(|n| format!("20260101T0000{n:02}-0000aaaa")).collect();
    assert_eq!(ids, esperados, "las diez más recientes, la más nueva primero, solo de esta rutina");
    assert_eq!(ej[0]["estado"], estado_ultimo);
    assert_eq!(ej[0]["version"], 2);
    assert_eq!(ej[0]["resultado"], "bloqueado");
    assert_eq!(ej[0]["duracion_s"], 42);
    assert_eq!(ej[1]["estado"], "cancelado");
    assert!(ej[1]["resultado"].is_null() && ej[1]["duracion_s"].is_null());
    assert_eq!(ej[2]["estado"], "en_cola");
    assert_eq!(ej[9]["version"], 1, "la v1 de las primeras");
    assert!(ej[0]["creado"].as_str().is_some_and(|c| !c.is_empty()));
    assert!(ej[0]["actualizado"].as_str().is_some_and(|c| !c.is_empty()));

    // La fila lleva solo la última, la misma que encabeza la ficha.
    let (_, l) = get(&f, "/api/rutinas").await;
    assert_eq!(l[0]["ultima"]["trabajo"], ultimo);
    assert_eq!(l[0]["ultima"]["estado"], estado_ultimo);
    assert_eq!(l[0]["ultima"]["resultado"], "bloqueado");

    // Solo lectura: ni trabajos nuevos ni cambios de estado.
    assert_eq!(bus.listar_trabajos().unwrap().len(), 14);
    assert_eq!(bus.leer_estado(ultimo).unwrap().estado.as_str(), estado_ultimo);
}

// ------------------------------------------- registrar rutinas desde la web

/// Lo que hay registrado y en `rutinas_dir` de una rutina: (versión, creada_por, fichero).
async fn registro(f: &Fixture, slug: &str) -> (Option<(i64, String)>, Option<String>) {
    let fila = f.state.store.lock().await.ultima_rutina(slug).unwrap().map(|r| (r.version, r.creada_por));
    let fichero = std::fs::read_to_string(f.state.config.rutinas_dir.join(format!("{slug}.toml"))).ok();
    (fila, fichero)
}

async fn registros_en_el_diario(f: &Fixture) -> Vec<ea_store::Event> {
    let todos = f.state.store.lock().await.list_events(&ea_store::EventFilter { limit: Some(500), ..Default::default() }).unwrap();
    todos.into_iter().filter(|e| e.action == "rutina_registrada").collect()
}

#[tokio::test]
async fn registrar_desde_la_web_exige_version_mayor_y_toml_valido() {
    let f = montar();
    let antes = registro(&f, "revision-diaria").await;
    for (toml, motivo) in [
        (RUTINA_BUS.to_string(), "la nueva debe ser mayor"),
        (RUTINA_BUS.replace("version = 1", "version = 0"), "la versión empieza en 1"),
        (RUTINA_BUS.replace("version = 1", "version = 2").replace(&format!("\"{}\"", ea_pruebas::repo_comun()), "\"tmp\""), "worktree debe ser una ruta absoluta"),
        (RUTINA_BUS.replace("version = 1", "version = 2").replace("test -f {salida}", " "), "falta criterio_exito"),
        ("[rutina]\nslug = ".into(), "rutina inválida"),
        (format!("{}#{}", RUTINA_BUS.replace("version = 1", "version = 2"), "x".repeat(ea_bus::MAX_DEFINICION)), "demasiado larga"),
    ] {
        let (s, v) = post(&f, "/api/rutinas/registrar", json!({ "toml": toml })).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{motivo}: {v}");
        assert!(v["error"].as_str().unwrap().contains(motivo), "{motivo}: {v}");
    }
    // Cuerpos que no son {toml}: 400 también, sin escribir.
    for cuerpo in [json!({}), json!({ "toml": *RUTINA_BUS, "creada_por": "otro" }), json!({ "toml": 3 })] {
        let (s, v) = post(&f, "/api/rutinas/registrar", cuerpo).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    }
    assert_eq!(registro(&f, "revision-diaria").await, antes, "nada escrito");
    assert!(registros_en_el_diario(&f).await.is_empty());

    // La versión 2 sí: como `ea rutina registrar` (SQLite + rutinas_dir), con el usuario de actor.
    let v2 = RUTINA_BUS.replace("version = 1", "version = 2").replace("test -f {salida}", "test -s {salida}");
    let (s, v) = post(&f, "/api/rutinas/registrar", json!({ "toml": v2 })).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v, json!({ "rutina": "revision-diaria", "version": 2, "anterior": 1 }));
    assert_eq!(registro(&f, "revision-diaria").await, (Some((2, "usuario".into())), Some(v2.clone())));
    let (_, d) = get(&f, "/api/rutinas/revision-diaria").await;
    assert_eq!(d["version"], 2);
    let ev = registros_en_el_diario(&f).await;
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].actor, "usuario");
    let det: Value = serde_json::from_str(ev[0].detail.as_deref().unwrap()).unwrap();
    // Lo deja el bus (U-05), por cualquier camino: sin canal, con el tamaño.
    assert_eq!((det["rutina"].as_str(), det["version"].as_i64(), det["anterior"].as_i64()), (Some("revision-diaria"), Some(2), Some(1)));
    assert_eq!(det["bytes"].as_u64(), Some(v2.len() as u64));
    // Registrar no lanza nada.
    let (_, t) = get(&f, "/api/trabajos").await;
    assert_eq!(t.as_array().map(Vec::len), Some(0), "{t}");
}

#[tokio::test]
async fn validar_no_registra_nada() {
    let f = montar();
    let antes = registro(&f, "revision-diaria").await;
    let v2 = RUTINA_BUS.replace("version = 1", "version = 2").replace("test -f {salida}", "test -s {salida}");
    let (s, v) = post(&f, "/api/rutinas/validar", json!({ "toml": v2 })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!((v["rutina"].as_str(), v["version"].as_u64()), (Some("revision-diaria"), Some(2)));
    assert_eq!(v["anterior"]["version"], 1);
    assert_eq!(v["anterior"]["definicion"], *RUTINA_BUS);
    // La vista previa de R1 con las entradas por defecto.
    assert_eq!(v["previa"]["criterio"], "test -s informe.md");
    assert!(v["previa"]["objetivo"].as_str().unwrap().contains("revisa y escribe informe.md"));
    // Los mismos errores que registrar.
    let (s, v) = post(&f, "/api/rutinas/validar", json!({ "toml": *RUTINA_BUS })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().contains("la nueva debe ser mayor"), "{v}");
    // Una nueva: sin anterior.
    let nueva = RUTINA_BUS.replace("revision-diaria", "otra");
    let (s, v) = post(&f, "/api/rutinas/validar", json!({ "toml": nueva })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["anterior"].is_null());

    assert_eq!(registro(&f, "revision-diaria").await, antes);
    assert_eq!(registro(&f, "otra").await, (None, None));
    assert!(registros_en_el_diario(&f).await.is_empty());
    let (_, t) = get(&f, "/api/trabajos").await;
    assert_eq!(t.as_array().map(Vec::len), Some(0), "{t}");
}

#[tokio::test]
async fn crear_una_rutina_nueva_desde_la_web() {
    let f = montar();
    let toml = &concat!("# Una rutina nueva\n[rutina]\nslug = \"nueva\"\nversion = 1\ndescripcion = \"De prueba\"\nworktree = \"", tmp!(), "\"\ncriterio_exito = \"true\"\n\n[[pasos]]\ntexto = \"nada\"\n")
        .replace(tmp!(), ea_pruebas::repo_comun());
    let (s, v) = post(&f, "/api/rutinas/registrar", json!({ "toml": toml })).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v, json!({ "rutina": "nueva", "version": 1, "anterior": null }));
    assert_eq!(registro(&f, "nueva").await, (Some((1, "usuario".into())), Some(toml.into())));
    let (_, l) = get(&f, "/api/rutinas").await;
    assert!(l.as_array().unwrap().iter().any(|r| r["slug"] == "nueva" && r["descripcion"] == "De prueba"), "{l}");
    let (s, d) = get(&f, "/api/rutinas/nueva").await;
    assert_eq!((s, d["contenido"].as_str()), (StatusCode::OK, Some(toml.as_str())));
    // Y se puede lanzar como cualquier otra.
    let (s, v) = post(&f, "/api/rutinas/nueva/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
}

#[tokio::test]
async fn validar_y_registrar_sin_clave_dan_403() {
    let f = montar();
    let antes = registro(&f, "revision-diaria").await;
    let v2 = RUTINA_BUS.replace("version = 1", "version = 2");
    for ruta in ["/api/rutinas/validar", "/api/rutinas/registrar"] {
        for clave in [None, Some("mala")] {
            let (s, t) =
                enviar_con_clave(&f, Method::POST, ruta, "localhost:8092", Some(json!({ "toml": v2 })), true, clave).await;
            assert_eq!(s, StatusCode::FORBIDDEN, "{ruta} con {clave:?}: {t}");
        }
        // Con la clave pero sin JSON, tampoco.
        let (s, _) = enviar(&f, Method::POST, ruta, "localhost:8092", Some(json!({ "toml": v2 })), false).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{ruta}");
    }
    assert_eq!(registro(&f, "revision-diaria").await, antes);
    assert!(registros_en_el_diario(&f).await.is_empty());
    // Las rutas fijas no tapan la ficha de una rutina que se llame así.
    let (s, _) = get(&f, "/api/rutinas/validar").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn las_versiones_de_una_rutina_se_listan_y_se_leen() {
    let f = montar();
    // v1 la deja `montar` (creada por «test»); v2 se registra desde la web.
    let v2 = RUTINA_BUS.replace("version = 1", "version = 2").replace("revisa y", "revisa con calma y");
    let (s, v) = post(&f, "/api/rutinas/registrar", json!({ "toml": v2 })).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");

    let (s, l) = get(&f, "/api/rutinas/revision-diaria/versiones").await;
    assert_eq!(s, StatusCode::OK, "{l}");
    let l = l.as_array().unwrap();
    let resumen: Vec<_> =
        l.iter().map(|v| (v["version"].as_i64().unwrap(), v["creada_por"].as_str().unwrap())).collect();
    assert_eq!(resumen, vec![(2, "usuario"), (1, "test")]);
    assert!(l.iter().all(|v| v.get("contenido").is_none() && v.get("definicion").is_none()), "{l:?}");
    assert_eq!(l[1]["creada_en"], "2026-01-01T00:00:00Z");

    for (n, toml) in [(1, RUTINA_BUS.to_string()), (2, v2.clone())] {
        let (s, d) = get(&f, &format!("/api/rutinas/revision-diaria/versiones/{n}")).await;
        assert_eq!(s, StatusCode::OK, "{d}");
        assert_eq!((d["version"].as_i64(), d["contenido"].as_str()), (Some(n), Some(toml.as_str())));
        assert_eq!(d["slug"], "revision-diaria");
    }

    // Lo que no existe da 404 con un mensaje claro; lo mal formado, 400.
    let (s, e) = get(&f, "/api/rutinas/revision-diaria/versiones/3").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert!(e.to_string().contains("no tiene versión 3"), "{e}");
    let (s, _) = get(&f, "/api/rutinas/no-existe/versiones").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = get(&f, "/api/rutinas/no-existe/versiones/1").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = get(&f, "/api/rutinas/revision-diaria/versiones/dos").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Las rutas fijas de R5 no tapan estas ni la ficha.
    let (s, _) = get(&f, "/api/rutinas/validar/versiones").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = get(&f, "/api/rutinas/revision-diaria").await;
    assert_eq!(s, StatusCode::OK);
}

// ------------------------------------------- programar una rutina (R4)

#[cfg(unix)]
#[tokio::test]
async fn programar_desde_la_web_pide_simulacion_y_revalida() {
    let f = montar();
    let timer = f._dir.path().join("unidades/ea-rutina@revision-diaria.timer");

    // La consola ofrece la herramienta con las rutinas registradas como dominio.
    let (_, cat) = get(&f, "/api/acciones").await;
    let h = cat.as_array().unwrap().iter().find(|h| h["nombre"] == "ea_programar_rutina").unwrap().clone();
    assert_eq!(h["confirmable"], true);
    assert_eq!(h["campos"][0]["opciones"], json!(["revision-diaria"]));
    assert_eq!(h["campos"][1]["nombre"], "calendario");

    let programar = |cal: &str| {
        json!({ "herramienta": "ea_programar_rutina", "argumentos": { "slug": "revision-diaria", "calendario": cal } })
    };
    // Simular no escribe nada y enseña las próximas ejecuciones.
    let (s, sim) = post(&f, "/api/acciones/simular", programar("*-*-* 08:00:00")).await;
    assert_eq!(s, StatusCode::OK, "{sim}");
    let texto = sim["texto"].as_str().unwrap();
    assert!(texto.contains("OnCalendar=*-*-* 08:00:00") && texto.contains("Sat 2026-10-24"), "{texto}");
    assert_eq!(sim["confirmable"], true);
    assert!(!timer.exists());

    // Calendarios malos (400) y rutinas sin registrar (409): nada escrito.
    for malo in ["daily\nExecStart=/bin/sh", "muy malo", "%h", ""] {
        let (s, _) = post(&f, "/api/acciones/simular", programar(malo)).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{malo:?}");
    }
    let (s, _) = post(
        &f,
        "/api/acciones/simular",
        json!({ "herramienta": "ea_programar_rutina", "argumentos": { "slug": "no-existe", "calendario": "daily" } }),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert!(!timer.exists());

    // El mundo cambia entre simular y guardar: la revalidación lo para.
    std::fs::create_dir_all(timer.parent().unwrap()).unwrap();
    std::fs::write(&timer, "[Timer]\nOnCalendar=daily\n").unwrap();
    let ejecutar = |sim: &Value| {
        json!({ "herramienta": sim["herramienta"], "argumentos": sim["argumentos"], "precondicion": sim["precondicion"] })
    };
    let (s, r) = post(&f, "/api/acciones/ejecutar", ejecutar(&sim)).await;
    assert_eq!(s, StatusCode::CONFLICT, "{r}");
    assert_eq!(std::fs::read_to_string(&timer).unwrap(), "[Timer]\nOnCalendar=daily\n");

    // Simulado otra vez sobre lo que hay, se guarda.
    let (_, sim) = post(&f, "/api/acciones/simular", programar("*-*-* 08:00:00")).await;
    assert!(sim["texto"].as_str().unwrap().contains("OnCalendar=daily"), "dice el horario actual: {sim}");
    let (s, r) = post(&f, "/api/acciones/ejecutar", ejecutar(&sim)).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert!(std::fs::read_to_string(&timer).unwrap().contains("\nOnCalendar=*-*-* 08:00:00\n"));
    let ordenes = std::fs::read_to_string(f._dir.path().join("ordenes.log")).unwrap();
    assert!(ordenes.contains("systemctl\n--user\nenable\nea-rutina@revision-diaria.timer\n"), "{ordenes}");
    let (_, rs) = get(&f, "/api/rutinas").await;
    assert_eq!(rs[0]["calendario"], "*-*-* 08:00:00");

    // Desprogramar: igual, simular y ejecutar lo simulado.
    let quitar = json!({ "herramienta": "ea_desprogramar_rutina", "argumentos": { "slug": "revision-diaria" } });
    let (s, sim) = post(&f, "/api/acciones/simular", quitar).await;
    assert_eq!(s, StatusCode::OK, "{sim}");
    let (s, r) = post(&f, "/api/acciones/ejecutar", ejecutar(&sim)).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert!(!timer.exists());
    let (_, rs) = get(&f, "/api/rutinas").await;
    assert_eq!(rs[0]["calendario"], Value::Null);

    // Todo queda en el diario: el intento parado y los dos que salieron.
    let ev = f.state.store.lock().await.list_events(&ea_store::EventFilter { limit: Some(50), ..Default::default() }).unwrap();
    let de = |a: &str| ev.iter().filter(|e| e.action == a && e.detail.as_deref().unwrap_or("").contains("_rutina")).count();
    assert_eq!((de("accion_fallida"), de("accion_ejecutada")), (1, 2));
}

/// R6 (spec 004, RF-011): retirar manda el TOML a la papelera de
/// `rutinas_dir` y desprograma el timer; la rutina retirada no se lista
/// (salvo con `?retiradas=1`) ni se lanza, y la web y la simulación de
/// `ea_encolar_rutina` por la capa de MCP (`SoloConsulta`) dan la misma
/// respuesta. Restaurar la devuelve, sin volver a programar el timer.
#[cfg(unix)]
#[tokio::test]
async fn una_rutina_retirada_no_se_lista_ni_se_lanza_y_se_restaura() {
    let f = montar();
    let dir = f.state.config.rutinas_dir.clone();
    let timer = f._dir.path().join("unidades/ea-rutina@revision-diaria.timer");
    let ejecutar = |sim: &Value| {
        json!({ "herramienta": sim["herramienta"], "argumentos": sim["argumentos"], "precondicion": sim["precondicion"] })
    };
    let accion = |h: &str| json!({ "herramienta": h, "argumentos": { "slug": "revision-diaria" } });

    // Con horario, para ver que retirar lo quita.
    let programar =
        json!({ "herramienta": "ea_programar_rutina", "argumentos": { "slug": "revision-diaria", "calendario": "daily" } });
    let (_, sim) = post(&f, "/api/acciones/simular", programar).await;
    let (s, r) = post(&f, "/api/acciones/ejecutar", ejecutar(&sim)).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert!(timer.exists());

    // Retirar es reversible: se confirma desde la web, y la simulación dice
    // que desprograma el timer.
    let (s, sim) = post(&f, "/api/acciones/simular", accion("ea_retirar_rutina")).await;
    assert_eq!(s, StatusCode::OK, "{sim}");
    assert_eq!((sim["riesgo"].as_str(), sim["confirmable"].as_bool()), (Some("reversible"), Some(true)));
    assert!(sim["texto"].as_str().unwrap().contains("desprograma su timer (OnCalendar=daily)"), "{sim}");
    let (s, r) = post(&f, "/api/acciones/ejecutar", ejecutar(&sim)).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert!(!timer.exists(), "retirar desprograma el timer");
    assert!(!dir.join("revision-diaria.toml").exists());
    assert!(ea_bus::ruta_retirada(&dir, "revision-diaria").unwrap().exists());

    // No se lista, salvo pidiendo las retiradas; la ficha dice dónde está.
    let (_, rs) = get(&f, "/api/rutinas").await;
    assert!(rs.as_array().unwrap().is_empty(), "{rs}");
    let (_, rs) = get(&f, "/api/rutinas?retiradas=1").await;
    assert_eq!((rs[0]["slug"].as_str(), rs[0]["ubicacion"].as_str()), (Some("revision-diaria"), Some("retirada")));
    let (_, d) = get(&f, "/api/rutinas/revision-diaria").await;
    assert_eq!(d["ubicacion"], "retirada");

    // No se lanza: la web (409) y la simulación de la capa MCP, con el mismo texto.
    let motivo = ea_bus::Ubicacion::Retirada.motivo_no_lanzable("revision-diaria").unwrap();
    let (s, e) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(e["error"], motivo);
    let (s, e_web) = post(&f, "/api/acciones/simular", accion("ea_encolar_rutina")).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let mcp = ea_acciones::SoloConsulta::usuario(dir.clone(), f.state.boveda.clone(), f.state.config.estado_dir.clone());
    let args = json!({ "slug": "revision-diaria" });
    let h = ea_acciones::observar(&mcp, "ea_encolar_rutina", &args).await.unwrap();
    let e_mcp =
        ea_acciones::simular_con(&*f.state.store.lock().await, &mcp, &h, "ea_encolar_rutina", &args).unwrap_err();
    assert_eq!(e_web["error"].as_str().unwrap(), e_mcp.to_string());
    assert!(e_mcp.to_string().contains(&motivo));
    let (_, ts) = get(&f, "/api/trabajos").await;
    assert!(ts.as_array().unwrap().is_empty(), "ningún trabajo creado");

    // Restaurar: vuelve a listarse y a lanzarse, sin timer.
    let (s, sim) = post(&f, "/api/acciones/simular", accion("ea_restaurar_rutina")).await;
    assert_eq!(s, StatusCode::OK, "{sim}");
    assert!(sim["texto"].as_str().unwrap().contains("No vuelve a programar su timer"), "{sim}");
    let (s, r) = post(&f, "/api/acciones/ejecutar", ejecutar(&sim)).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert!(dir.join("revision-diaria.toml").exists() && !timer.exists());
    let (_, rs) = get(&f, "/api/rutinas").await;
    assert_eq!(rs[0]["ubicacion"], "activa");
    let (_, rs) = get(&f, "/api/rutinas?retiradas=1").await;
    assert!(rs.as_array().unwrap().is_empty());
    let (s, _) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::CREATED);

    let ev = f.state.store.lock().await.list_events(&ea_store::EventFilter { limit: Some(50), ..Default::default() }).unwrap();
    let hechas = |h: &str| {
        ev.iter().filter(|e| e.action == "accion_ejecutada" && e.detail.as_deref().unwrap_or("").contains(h)).count()
    };
    assert_eq!((hechas("ea_retirar_rutina"), hechas("ea_restaurar_rutina")), (1, 1));
}
