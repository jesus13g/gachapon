use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Barrier};
use std::time::Duration;

/// Un directorio absoluto que existe en cada sistema (`/tmp` no es absoluta en Windows).
#[cfg(windows)]
macro_rules! tmp {
    () => {
        "C:/Windows"
    };
}
#[cfg(not(windows))]
macro_rules! tmp {
    () => {
        "/tmp"
    };
}

use ea_bus::{
    transicion, Bus, CandadoWorktree, ConfigOrquestador, ConfigSkills, EjecutadoPor, ErrorBus, Estado, EstadoTrabajo, Evento, Fase,
    Informe, Modo, Ocupante, Orden, ResultadoInforme, Rol, TipoAgente, Topes,
};
use ea_store::Store;

fn orden(wt: &Path) -> Orden {
    Orden {
        id: String::new(),
        modo: Modo::Obra,
        titulo: "prueba".into(),
        worktree: wt.to_string_lossy().into(),
        objetivo: "haz algo".into(),
        criterio_exito: None,
        fases: vec![Fase {
            numero: 1,
            titulo: "uno".into(),
            instrucciones: "i".into(),
            entregable: "e".into(),
            criterio: "true".into(),
            skills: vec![],
        }],
        revisar_cada: None,
        topes: Topes::default(),
        rutina: None,
        notas_revision: vec![],
        skills: vec![],
        equipo: None,
        asignado_a: None,
        revisor: None,
        worktree_propio: false,
        creada_en: String::new(),
        creada_por: "claude-code".into(),
    }
}

fn informe() -> Informe {
    Informe { evidencia: "ok".into(), fases_hechas: 1, duracion_s: 3, ..Informe::nuevo(ResultadoInforme::Terminado) }
}

fn yo(trabajo: &str) -> Ocupante {
    Ocupante { agente: "qwen-code".into(), trabajo: trabajo.into(), pid: 0, desde: "ahora".into(), rol: Some(Rol::Ejecutor) }
}

#[test]
fn crear_tomar_entregar_y_cerrar_con_historial_en_sqlite() {
    let d = tempfile::tempdir().unwrap();
    let store = Store::open_at(&d.path().join("ea.db")).unwrap();
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap().con_store(store);
    let id = bus.crear_trabajo(orden(d.path())).unwrap();
    assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::EnCola);
    assert_eq!(bus.listar_cola(Rol::Ejecutor).unwrap().len(), 1);
    assert_eq!(bus.tomar(Rol::Ejecutor).unwrap(), Some(id.clone()));
    assert_eq!(bus.tomar(Rol::Ejecutor).unwrap(), None);
    let e = bus.aplicar(&id, Evento::Tomar).unwrap();
    assert_eq!((e.estado, e.iteracion), (EstadoTrabajo::Ejecutando, 0));
    assert_eq!(bus.anotar_iteracion(&id).unwrap(), Some(1));
    assert_eq!(bus.leer_estado(&id).unwrap().iteracion, 1);
    bus.aplicar(&id, Evento::Entregar(informe())).unwrap();
    assert_eq!(bus.leer_informe(&id).unwrap().unwrap().evidencia, "ok");
    // entregar encola en claude-code por sí solo
    assert_eq!(bus.listar_cola(Rol::Planificador).unwrap()[0].motivo, "terminado");
    bus.aplicar(&id, Evento::Cerrar).unwrap();
    // cerrar limpia la entrada de claude-code
    assert!(bus.listar_cola(Rol::Planificador).unwrap().is_empty());
    assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::Cerrado);
    assert_eq!(bus.listar_trabajos().unwrap(), vec![id.clone()]);
    let lineas = std::fs::read_to_string(d.path().join("estado/trabajos").join(&id).join("historial.jsonl")).unwrap();
    assert_eq!(lineas.lines().count(), 4); // crear, tomar, entregar, cerrar

    // el historial en SQLite
    let s = Store::open_at(&d.path().join("ea.db")).unwrap();
    assert_eq!(s.obtener_trabajo(&id).unwrap().unwrap().estado, "cerrado");
    assert_eq!(s.listar_fases(&id).unwrap()[0].estado, "hecha");
    let eventos = s.list_events(&Default::default()).unwrap();
    assert!(eventos.iter().any(|e| e.action == "bus_cerrar" && e.actor == "orquestador"));
}

#[test]
fn dos_hilos_no_toman_el_mismo_trabajo() {
    let d = tempfile::tempdir().unwrap();
    let raiz = d.path().join("estado");
    let bus = Bus::abrir_en(&raiz).unwrap();
    let n = 20;
    for _ in 0..n {
        bus.crear_trabajo(orden(d.path())).unwrap();
    }
    let barrera = Arc::new(Barrier::new(4));
    let hilos: Vec<_> = (0..4)
        .map(|_| {
            let (r, b) = (raiz.clone(), barrera.clone());
            std::thread::spawn(move || {
                let bus = Bus::abrir_en(&r).unwrap();
                b.wait();
                let mut mios = vec![];
                while let Some(id) = bus.tomar(Rol::Ejecutor).unwrap() {
                    mios.push(id);
                }
                mios
            })
        })
        .collect();
    let todos: Vec<String> = hilos.into_iter().flat_map(|h| h.join().unwrap()).collect();
    assert_eq!(todos.len(), n);
    assert_eq!(todos.iter().collect::<HashSet<_>>().len(), n);
}

#[test]
fn un_segundo_candado_espera_y_caduca_y_se_libera_al_soltar() {
    let d = tempfile::tempdir().unwrap();
    let raiz = d.path().join("estado");
    let wt = d.path().join("wt");
    std::fs::create_dir_all(&wt).unwrap();
    let c = CandadoWorktree::adquirir_en(&raiz, &wt, yo("t1"), Duration::from_secs(1)).unwrap();
    let o = CandadoWorktree::ocupante_en(&raiz, &wt).unwrap();
    assert_eq!((o.trabajo.as_str(), o.pid), ("t1", std::process::id()));
    assert_eq!(ea_bus::listar_semaforos_en(&raiz).len(), 1);
    let t = std::time::Instant::now();
    let r = CandadoWorktree::adquirir_en(&raiz, &wt, yo("t2"), Duration::from_millis(150));
    assert!(matches!(r, Err(ErrorBus::Ocupado)));
    assert!(t.elapsed() >= Duration::from_millis(150));
    drop(c);
    assert!(CandadoWorktree::ocupante_en(&raiz, &wt).is_none());
    assert!(ea_bus::listar_semaforos_en(&raiz).is_empty());
    let _c2 = CandadoWorktree::adquirir_en(&raiz, &wt, yo("t2"), Duration::from_millis(150)).unwrap();
}

#[test]
fn una_transicion_invalida_falla_y_no_toca_el_estado() {
    assert!(transicion(EstadoTrabajo::EnCola, &Evento::Cerrar).is_err());
    assert!(transicion(EstadoTrabajo::Cerrado, &Evento::Cancelar).is_err());
    assert_eq!(transicion(EstadoTrabajo::EnEspera, &Evento::Reformular).unwrap(), EstadoTrabajo::EnCola);
    let d = tempfile::tempdir().unwrap();
    let bus = Bus::abrir_en(d.path()).unwrap();
    let id = bus.crear_trabajo(orden(d.path())).unwrap();
    let antes: Estado = bus.leer_estado(&id).unwrap();
    assert!(matches!(bus.aplicar(&id, Evento::Entregar(informe())), Err(ErrorBus::TransicionInvalida { .. })));
    assert_eq!(bus.leer_estado(&id).unwrap(), antes);
}

#[test]
fn la_escritura_es_atomica_y_no_quedan_temporales() {
    let d = tempfile::tempdir().unwrap();
    let bus = Bus::abrir_en(d.path()).unwrap();
    let id = bus.crear_trabajo(orden(d.path())).unwrap();
    bus.aplicar(&id, Evento::Tomar).unwrap();
    bus.aplicar(&id, Evento::Entregar(informe())).unwrap();
    fn recorre(p: &Path, malos: &mut Vec<String>) {
        for e in std::fs::read_dir(p).unwrap().flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.ends_with(".tmp") || n.contains(".tomada-") {
                malos.push(n);
            }
            if e.path().is_dir() {
                recorre(&e.path(), malos);
            }
        }
    }
    let mut malos = vec![];
    recorre(d.path(), &mut malos);
    assert!(malos.is_empty(), "{malos:?}");
}

#[test]
fn la_configuracion_del_ejecutor_sale_de_los_roles() {
    let d = tempfile::tempdir().unwrap();
    let f = ea_ajustes::Fuentes::aislada(d.path());
    let c = ConfigOrquestador::desde_ajustes(&ea_ajustes::cargar_de(&f).ajustes);
    assert_eq!(c, ConfigOrquestador::desde_ajustes(&ea_ajustes::Ajustes::por_defecto(&f)));
    assert_eq!(ConfigOrquestador::default().ejecutor.id, "qwen-code");
    assert_eq!((c.ejecutor.id.as_str(), c.ejecutor.tipo, c.ejecutor.bin.as_str()), ("qwen-code", TipoAgente::QwenCode, "qwen"));
    assert_eq!((c.ejecutor.modelo.nombre.as_str(), c.ejecutor.modelo.base_url.as_str()), ("qwen3-coder-30b", "http://127.0.0.1:8080/v1"));
    assert_eq!(c.techo_topes.turnos, 100);
    assert_eq!(c.skills.planificador, "claude-code");
    assert!(c.avisos_escritorio);

    // Otro ejecutor y otro planificador: el perfil sigue al agente.
    std::fs::create_dir_all(&f.dir_config).unwrap();
    std::fs::write(
        f.dir_config.join("ajustes.toml"),
        "[roles]\nplanificador = \"opencode\"\nejecutor = \"codex\"\n[modelos.otro]\nnombre = \"otro-7b\"\nbase_url = \"http://127.0.0.1:8081/v1\"\n[agentes.codex]\nmodelo = \"otro\"\n",
    )
    .unwrap();
    let c = ConfigOrquestador::desde_ajustes(&ea_ajustes::cargar_de(&f).ajustes);
    assert_eq!((c.ejecutor.id.as_str(), c.ejecutor.tipo, c.ejecutor.bin.as_str()), ("codex", TipoAgente::Codex, "codex"));
    assert_eq!(c.ejecutor.ejecutado_por(), EjecutadoPor { agente: "codex".into(), modelo: "otro-7b".into() });
    assert_eq!(c.skills.planificador, "opencode");

    // El `orquestador.toml` heredado sigue valiendo (lo lee `ea-ajustes`).
    std::fs::remove_file(f.dir_config.join("ajustes.toml")).unwrap();
    std::fs::write(f.dir_config.join("orquestador.toml"), "modelo = \"otro\"\n[topes]\nturnos = 5\n").unwrap();
    let c = ConfigOrquestador::desde_ajustes(&ea_ajustes::cargar_de(&f).ajustes);
    assert_eq!((c.ejecutor.modelo.nombre.as_str(), c.techo_topes.turnos, c.techo_topes.tiempo_s), ("otro", 5, 3600));
}

#[test]
fn el_trabajo_anota_quien_lo_ejecuta() {
    let d = tempfile::tempdir().unwrap();
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap();
    for rol in Rol::TODOS {
        assert!(bus.raiz().join("colas").join(rol.as_str()).is_dir());
    }
    let id = bus.crear_trabajo(orden(d.path())).unwrap();
    assert_eq!(bus.tomar(Rol::Ejecutor).unwrap(), Some(id.clone()));
    let qwen = EjecutadoPor { agente: "qwen-code".into(), modelo: "qwen3-coder-30b".into() };
    let e = bus.tomar_con(&id, qwen.clone()).unwrap();
    assert_eq!((e.estado, e.ejecutado_por.as_ref()), (EstadoTrabajo::Ejecutando, Some(&qwen)));
    // La entrega copia al informe quién lo hizo si el ejecutor no lo dice.
    bus.aplicar(&id, Evento::Entregar(informe())).unwrap();
    assert_eq!(bus.leer_informe(&id).unwrap().unwrap().ejecutado_por, Some(qwen.clone()));
    // Reformular conserva el último hasta que otro lo vuelva a tomar.
    let e = bus.reformular(&id, "otra vez", None).unwrap();
    assert_eq!(e.ejecutado_por, Some(qwen.clone()));
    bus.tomar(Rol::Ejecutor).unwrap();
    let codex = EjecutadoPor { agente: "codex".into(), modelo: "qwen3-coder-30b".into() };
    bus.tomar_con(&id, codex.clone()).unwrap();
    let mut inf = informe();
    inf.ejecutado_por = Some(EjecutadoPor { agente: "opencode".into(), modelo: "m".into() });
    bus.aplicar(&id, Evento::Entregar(inf.clone())).unwrap();
    assert_eq!(bus.leer_informe(&id).unwrap().unwrap().ejecutado_por, inf.ejecutado_por, "el del ejecutor manda");
    assert_eq!(bus.leer_estado(&id).unwrap().ejecutado_por, Some(codex.clone()));

    let historial = std::fs::read_to_string(bus.raiz().join("trabajos").join(&id).join("historial.jsonl")).unwrap();
    let lineas: Vec<serde_json::Value> = historial.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let tomas: Vec<_> = lineas.iter().filter(|l| l["evento"] == "tomar").map(|l| l["ejecutado_por"]["agente"].clone()).collect();
    assert_eq!(tomas, ["qwen-code", "codex"]);
    assert!(lineas[0].get("ejecutado_por").is_none(), "al crear aún no lo ha tomado nadie");
}

#[test]
fn un_trabajo_de_antes_sin_ejecutado_por_se_sigue_leyendo() {
    let d = tempfile::tempdir().unwrap();
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap();
    let id = bus.crear_trabajo(orden(d.path())).unwrap();
    let dir = bus.raiz().join("trabajos").join(&id);
    // `estado.json` e `informe.json` tal y como los escribía la versión anterior.
    std::fs::write(dir.join("estado.json"), r#"{"estado":"en_revision","fase_actual":null,"iteracion":1,"actualizado_en":"2026-10-01T10:00:00Z","detalle":null}"#).unwrap();
    std::fs::write(
        dir.join("informe.json"),
        r#"{"resultado":"terminado","evidencia":"ok","fases":[{"numero":1,"titulo":"f","resultado":"hecha","diff_stat":""}]}"#,
    )
    .unwrap();
    let e = bus.leer_estado(&id).unwrap();
    assert_eq!((e.estado, e.ejecutado_por.as_ref()), (EstadoTrabajo::EnRevision, None));
    let inf = bus.leer_informe(&id).unwrap().unwrap();
    assert_eq!((inf.ejecutado_por, inf.fases[0].ejecutado_por.clone()), (None, None));
    assert_eq!(ea_bus::tipo_ejecutor(e.ejecutado_por.as_ref()), TipoAgente::QwenCode);
    // Al escribir de nuevo no aparece un `ejecutado_por: null`.
    bus.aplicar(&id, Evento::Cerrar).unwrap();
    assert!(!std::fs::read_to_string(dir.join("estado.json")).unwrap().contains("ejecutado_por"));
    // Las colas y los semáforos de antes usan el nombre del rol en disco.
    assert_eq!(serde_json::from_str::<Rol>("\"modelo-local\"").unwrap(), Rol::Ejecutor);
    assert_eq!(serde_json::to_string(&Rol::Planificador).unwrap(), "\"claude-code\"");
    let viejo: Ocupante = serde_json::from_str(r#"{"agente":"modelo-local","trabajo":"t","pid":1,"desde":"x"}"#).unwrap();
    assert!(viejo.es_ejecutor());
    assert!(yo("t").es_ejecutor());
}

#[test]
fn reformular_anade_notas_y_vuelve_a_encolar() {
    let d = tempfile::tempdir().unwrap();
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap();
    let id = bus.crear_trabajo(orden(d.path())).unwrap();
    assert!(bus.reformular(&id, "no vale", None).is_err()); // aún EnCola
    bus.tomar(Rol::Ejecutor).unwrap();
    bus.aplicar(&id, Evento::Tomar).unwrap();
    bus.aplicar(&id, Evento::Entregar(informe())).unwrap();
    bus.reformular(&id, "falta la prueba", None).unwrap();
    assert_eq!(bus.leer_orden(&id).unwrap().notas_revision, vec!["falta la prueba".to_string()]);
    assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::EnCola);
    assert_eq!(bus.tomar(Rol::Ejecutor).unwrap(), Some(id));
}

/// Obra de tres fases (1, 2, 3) ya completa y en revisión, con su
/// `fases-informe.json` como lo deja el ejecutor y las fases hechas en SQLite.
fn obra_completa_en_revision(d: &Path) -> (Bus, String) {
    let store = Store::open_at(&d.join("ea.db")).unwrap();
    let bus = Bus::abrir_en(&d.join("estado")).unwrap().con_store(store);
    let fase = |n: u32| Fase {
        numero: n,
        titulo: format!("f{n}"),
        instrucciones: "i".into(),
        entregable: "e".into(),
        criterio: "true".into(),
        skills: vec![],
    };
    let id = bus.crear_trabajo(Orden { fases: vec![fase(1), fase(2), fase(3)], ..orden(d) }).unwrap();
    bus.tomar(Rol::Ejecutor).unwrap();
    bus.aplicar(&id, Evento::Tomar).unwrap();
    let fases: Vec<ea_bus::FaseInforme> = (1..=3)
        .map(|n| ea_bus::FaseInforme { numero: n, titulo: format!("f{n}"), resultado: "hecha".into(), diff_stat: String::new(), ejecutado_por: None })
        .collect();
    std::fs::write(bus.raiz().join("trabajos").join(&id).join("fases-informe.json"), serde_json::to_vec(&fases).unwrap()).unwrap();
    for n in 1..=3 {
        bus.registrar_fase(&id, n, "hecha", &serde_json::json!({ "arbol": format!("arbol{n}") })).unwrap();
    }
    let inf = Informe { fases_hechas: 3, obra_completa: true, fases, ..informe() };
    let e = bus.aplicar(&id, Evento::Entregar(inf)).unwrap();
    assert_eq!((e.estado, e.fase_actual), (EstadoTrabajo::EnRevision, Some(4)));
    (bus, id)
}

fn fases_informe(bus: &Bus, id: &str) -> Vec<u32> {
    let b = std::fs::read(bus.raiz().join("trabajos").join(id).join("fases-informe.json")).unwrap();
    serde_json::from_slice::<Vec<ea_bus::FaseInforme>>(&b).unwrap().iter().map(|f| f.numero).collect()
}

fn estados_fases(d: &Path, id: &str) -> Vec<(String, bool)> {
    let s = Store::open_at(&d.join("ea.db")).unwrap();
    s.listar_fases(id).unwrap().into_iter().map(|f| (f.estado, f.informe.is_some())).collect()
}

#[test]
fn reformular_obra_completa_desde_fase_2_rehace_la_2_y_siguientes() {
    let d = tempfile::tempdir().unwrap();
    let (bus, id) = obra_completa_en_revision(d.path());
    let e = bus.reformular(&id, "rehaz la 2 con pruebas", Some(2)).unwrap();
    assert_eq!((e.estado, e.fase_actual), (EstadoTrabajo::EnCola, Some(2)));
    assert_eq!(bus.leer_estado(&id).unwrap().fase_actual, Some(2));
    assert_eq!(fases_informe(&bus, &id), [1]);
    let h = |e: &str, i: bool| (e.to_string(), i);
    assert_eq!(estados_fases(d.path(), &id), [h("hecha", true), h("pendiente", false), h("pendiente", false)]);
    assert_eq!(bus.leer_orden(&id).unwrap().notas_revision, ["rehaz la 2 con pruebas"]);
    assert_eq!(bus.tomar(Rol::Ejecutor).unwrap(), Some(id.clone()));
    // El diario lleva la fase desde la que se rehace.
    let s = Store::open_at(&d.path().join("ea.db")).unwrap();
    let ev = s.list_events_by_trabajo(&id).unwrap().into_iter().find(|e| e.action == "bus_reformular").unwrap();
    assert!(ev.detail.unwrap().contains("\"fase\":2"));
}

#[test]
fn reformular_obra_completa_sin_fase_rehace_la_ultima() {
    let d = tempfile::tempdir().unwrap();
    let (bus, id) = obra_completa_en_revision(d.path());
    let e = bus.reformular(&id, "retoca el final", None).unwrap();
    assert_eq!(e.fase_actual, Some(3));
    assert_eq!(fases_informe(&bus, &id), [1, 2]);
    let estados: Vec<String> = estados_fases(d.path(), &id).into_iter().map(|(e, _)| e).collect();
    assert_eq!(estados, ["hecha", "hecha", "pendiente"]);
}

#[test]
fn reformular_con_fase_fuera_de_rango_o_en_mision_no_escribe_nada() {
    let d = tempfile::tempdir().unwrap();
    let (bus, id) = obra_completa_en_revision(d.path());
    let dir = bus.raiz().join("trabajos").join(&id);
    let antes = |f: &str| std::fs::read(dir.join(f)).unwrap();
    let (orden0, estado0, fases0) = (antes("orden.json"), antes("estado.json"), antes("fases-informe.json"));
    for n in [0, 4] {
        let r = bus.reformular(&id, "no debería quedar", Some(n));
        assert!(matches!(r, Err(ErrorBus::Argumentos(_))), "fase {n}: {r:?}");
    }
    assert_eq!((antes("orden.json"), antes("estado.json"), antes("fases-informe.json")), (orden0, estado0, fases0));
    assert_eq!(estados_fases(d.path(), &id).iter().filter(|(e, _)| e == "hecha").count(), 3);
    assert!(bus.listar_cola(Rol::Ejecutor).unwrap().is_empty());

    // Una misión no tiene fases.
    let m = bus
        .crear_trabajo(Orden { modo: Modo::Mision, fases: vec![], criterio_exito: Some("true".into()), ..orden(d.path()) })
        .unwrap();
    bus.tomar(Rol::Ejecutor).unwrap();
    bus.aplicar(&m, Evento::Tomar).unwrap();
    bus.aplicar(&m, Evento::Entregar(informe())).unwrap();
    assert!(matches!(bus.reformular(&m, "x", Some(1)), Err(ErrorBus::Argumentos(_))));
    assert_eq!(bus.leer_estado(&m).unwrap().estado, EstadoTrabajo::EnRevision);
    assert_eq!(bus.reformular(&m, "x", None).unwrap().estado, EstadoTrabajo::EnCola);
}

#[test]
fn recuperar_devuelve_huerfanos_y_bloquea_lo_interrumpido() {
    let d = tempfile::tempdir().unwrap();
    let raiz = d.path().join("estado");
    let bus = Bus::abrir_en(&raiz).unwrap();
    let a = bus.crear_trabajo(orden(d.path())).unwrap();
    // simula un reclamo huérfano de `a` (sigue EnCola)
    let cola = raiz.join("colas/modelo-local");
    let f = std::fs::read_dir(&cola).unwrap().flatten().next().unwrap().path();
    let nombre = f.file_name().unwrap().to_string_lossy().to_string();
    std::fs::rename(&f, cola.join(format!(".{nombre}.tomada-1"))).unwrap();
    assert!(bus.listar_cola(Rol::Ejecutor).unwrap().is_empty());
    // y un trabajo que se quedó ejecutando
    let b = bus.crear_trabajo(orden(d.path())).unwrap();
    bus.tomar(Rol::Ejecutor).unwrap();
    bus.aplicar(&b, Evento::Tomar).unwrap();
    let rec = bus.recuperar().unwrap();
    assert!(rec.contains(&a) && rec.contains(&b));
    assert_eq!(bus.listar_cola(Rol::Ejecutor).unwrap()[0].trabajo, a);
    assert_eq!(bus.leer_estado(&b).unwrap().estado, EstadoTrabajo::EnRevision);
    let inf = bus.leer_informe(&b).unwrap().unwrap();
    assert_eq!((inf.resultado, inf.motivo.as_deref()), (ResultadoInforme::Bloqueado, Some("interrumpido")));
    assert_eq!(bus.listar_cola(Rol::Planificador).unwrap()[0].motivo, "bloqueado");
    assert!(bus.recuperar().unwrap().is_empty());
}

/// B-04: un trabajo reclamado (su entrada ya no está en la cola) que no se
/// llegó a tomar, porque el proceso cayó entre medias, vuelve a su cola al
/// recuperar, una sola vez.
#[test]
fn recuperar_devuelve_a_la_cola_lo_reclamado_que_no_se_llego_a_tomar() {
    let d = tempfile::tempdir().unwrap();
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap();
    let id = bus.crear_trabajo(orden(d.path())).unwrap();
    assert_eq!(bus.tomar_para("qwen-code").unwrap(), Some(id.clone()));
    assert!(bus.listar_cola(Rol::Ejecutor).unwrap().is_empty());
    assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::EnCola);
    assert_eq!(bus.recuperar().unwrap(), vec![id.clone()]);
    let cola = bus.listar_cola(Rol::Ejecutor).unwrap();
    assert_eq!((cola.len(), cola[0].trabajo.as_str(), cola[0].motivo.as_str()), (1, id.as_str(), "recuperado"));
    // Con su entrada ya en la cola no se repite.
    assert!(bus.recuperar().unwrap().is_empty());
    assert_eq!(bus.listar_cola(Rol::Ejecutor).unwrap().len(), 1);
}

/// B-05: una rutina interrumpida queda en espera con su paquete de
/// corrección (motivo «interrumpido»), como una que falla, y no en revisión
/// sin paquete ni cola.
#[test]
fn recuperar_deja_en_espera_y_con_paquete_una_rutina_interrumpida() {
    let d = tempfile::tempdir().unwrap();
    let store = Store::open_at(&d.path().join("ea.db")).unwrap();
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap().con_store(store);
    bus.registrar_rutina(RUTINA, &d.path().join("rutinas")).unwrap();
    let id = bus.lanzar_rutina("prueba", &[]).unwrap();
    bus.tomar(Rol::Ejecutor).unwrap();
    bus.aplicar(&id, Evento::Tomar).unwrap();
    assert_eq!(bus.recuperar().unwrap(), vec![id.clone()]);
    assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::EnEspera);
    assert!(bus.listar_cola(Rol::Planificador).unwrap().is_empty());
    let p = std::fs::read_to_string(d.path().join("estado/trabajos").join(&id).join("paquete-correccion.md")).unwrap();
    assert!(p.contains("Motivo: interrumpido") && p.contains("slug = \"prueba\""), "{p}");
    let inf = bus.leer_informe(&id).unwrap().unwrap();
    assert_eq!((inf.resultado, inf.motivo.as_deref()), (ResultadoInforme::Bloqueado, Some("interrumpido")));
}

/// B-09: el espejo de SQLite es secundario, como el diario. Con una base que
/// no acepta escrituras en `trabajos` ni en `trabajo_fases`, crear y aplicar
/// salen bien y el bus (estado, historial y colas) queda escrito.
#[test]
fn un_fallo_del_espejo_sqlite_no_hace_fallar_al_bus() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("ea.db");
    let store = Store::open_at(&db).unwrap();
    let c = rusqlite::Connection::open(&db).unwrap();
    for tabla in ["trabajos", "trabajo_fases"] {
        for op in ["INSERT", "UPDATE"] {
            c.execute_batch(&format!("CREATE TRIGGER no_{op}_{tabla} BEFORE {op} ON {tabla} BEGIN SELECT RAISE(ABORT, 'solo lectura'); END;"))
                .unwrap();
        }
    }
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap().con_store(store);
    let id = bus.crear_trabajo(orden(d.path())).unwrap();
    assert_eq!(bus.listar_cola(Rol::Ejecutor).unwrap()[0].trabajo, id);
    assert_eq!(bus.tomar(Rol::Ejecutor).unwrap(), Some(id.clone()));
    assert_eq!(bus.aplicar(&id, Evento::Tomar).unwrap().estado, EstadoTrabajo::Ejecutando);
    assert_eq!(bus.aplicar(&id, Evento::Entregar(informe())).unwrap().estado, EstadoTrabajo::EnRevision);
    assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::EnRevision);
    assert_eq!(bus.listar_cola(Rol::Planificador).unwrap()[0].trabajo, id);
    assert_eq!(bus.reformular(&id, "otra", None).unwrap().estado, EstadoTrabajo::EnCola);
    // Sin fila en el espejo, pero con el diario.
    let s = Store::open_at(&db).unwrap();
    assert!(s.obtener_trabajo(&id).unwrap().is_none());
    let acciones: Vec<String> = s.list_events_by_trabajo(&id).unwrap().into_iter().map(|e| e.action).collect();
    for a in ["bus_crear", "bus_tomar", "bus_entregar", "bus_reformular"] {
        assert!(acciones.iter().any(|x| x == a), "{a}: {acciones:?}");
    }
}

/// F-21: dos confirmaciones a la vez del mismo paquete dejan el contexto una
/// sola vez: validar y añadir van bajo el bloqueo del trabajo.
#[test]
fn dos_confirmar_a_la_vez_anaden_el_contexto_una_sola_vez() {
    let d = tempfile::tempdir().unwrap();
    let raiz = d.path().join("estado");
    let bus = Bus::abrir_en(&raiz).unwrap();
    let mut o = orden(d.path());
    o.modo = Modo::Rutina;
    o.fases.clear();
    o.criterio_exito = Some("true".into());
    let id = bus.crear_trabajo(o).unwrap();
    bus.tomar(Rol::Ejecutor).unwrap();
    bus.aplicar(&id, Evento::Tomar).unwrap();
    bus.aplicar(&id, Evento::PonerEnEspera).unwrap();
    let dir = raiz.join("trabajos").join(&id);
    std::fs::write(dir.join("paquete-correccion.md"), "# paquete\n").unwrap();
    // Otro proceso tiene el bloqueo del trabajo mientras llegan las dos.
    let bloqueo = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(".estado.lock")).unwrap();
    bloqueo.lock().unwrap();
    let hilos: Vec<_> = (0..2)
        .map(|_| {
            let (raiz, id) = (raiz.clone(), id.clone());
            std::thread::spawn(move || Bus::abrir_en(&raiz).unwrap().confirmar(&id, "ojo con el disco").is_ok())
        })
        .collect();
    std::thread::sleep(Duration::from_millis(300));
    bloqueo.unlock().unwrap();
    let ok: Vec<bool> = hilos.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(ok.iter().filter(|b| **b).count(), 1, "{ok:?}");
    let p = std::fs::read_to_string(dir.join("paquete-correccion.md")).unwrap();
    assert_eq!(p.matches("ojo con el disco").count(), 1, "{p}");
    assert_eq!(bus.listar_cola(Rol::Planificador).unwrap().len(), 1);
}

#[test]
fn cerrar_y_cancelar_retiran_el_trabajo_de_todas_las_colas() {
    let d = tempfile::tempdir().unwrap();
    let bus = Bus::abrir_en(d.path()).unwrap();
    let a = bus.crear_trabajo(orden(d.path())).unwrap();
    let b = bus.crear_trabajo(orden(d.path())).unwrap();
    bus.tomar(Rol::Ejecutor).unwrap();
    bus.tomar(Rol::Ejecutor).unwrap();
    for id in [&a, &b] {
        bus.aplicar(id, Evento::Tomar).unwrap();
        bus.aplicar(id, Evento::Entregar(informe())).unwrap();
        bus.encolar(Rol::Ejecutor, id, "extra").unwrap();
    }
    assert_eq!(bus.listar_cola(Rol::Planificador).unwrap().len(), 2);
    bus.aplicar(&a, Evento::Cerrar).unwrap();
    bus.aplicar(&b, Evento::Cancelar).unwrap();
    assert!(bus.listar_cola(Rol::Planificador).unwrap().is_empty());
    assert!(bus.listar_cola(Rol::Ejecutor).unwrap().is_empty());
}

/// B-06: una obra numera sus fases 1, 2, …, N en el orden en que van. Con
/// 10/20/30 (o repetidas, o desordenadas) el ejecutor confundía la cuenta de
/// fases hechas con su número y se saltaba fases; ahora no se crea.
#[test]
fn una_obra_con_las_fases_mal_numeradas_no_se_crea() {
    let d = tempfile::tempdir().unwrap();
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap();
    let fase = |n: u32| Fase { numero: n, ..orden(d.path()).fases[0].clone() };
    let obra = |numeros: &[u32]| Orden { fases: numeros.iter().map(|n| fase(*n)).collect(), ..orden(d.path()) };
    for malos in [&[10, 20, 30][..], &[1, 1, 2], &[0, 1, 2], &[2, 1, 3], &[1, 3]] {
        let e = bus.crear_trabajo(obra(malos)).unwrap_err();
        assert!(matches!(&e, ErrorBus::Argumentos(m) if m.contains("las fases deben numerarse 1, 2, …, N")), "{malos:?}: {e}");
    }
    sin_rastro(&bus);
    let id = bus.crear_trabajo(obra(&[1, 2, 3])).unwrap();
    assert_eq!(bus.leer_orden(&id).unwrap().fases.iter().map(|f| f.numero).collect::<Vec<_>>(), [1, 2, 3]);
}

/// El worktree de una orden es una ruta absoluta, la cree quien la cree: una
/// relativa se resolvería contra el directorio del proceso que la ejecuta.
#[test]
fn una_orden_con_el_worktree_relativo_no_se_crea() {
    let d = tempfile::tempdir().unwrap();
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap();
    for malo in [".", "repo", "~/repo", ""] {
        let e = bus.crear_trabajo(Orden { worktree: malo.into(), ..orden(d.path()) }).unwrap_err();
        assert!(matches!(&e, ErrorBus::Argumentos(m) if m.contains("ruta absoluta")), "{malo:?}: {e}");
    }
    sin_rastro(&bus);
}

/// Los constructores dejan una misión y una obra válidas con el resto de
/// campos por defecto.
#[test]
fn orden_mision_y_obra_rellenan_lo_demas_por_defecto() {
    let d = tempfile::tempdir().unwrap();
    let wt = d.path().to_string_lossy().into_owned();
    let m = Orden::mision("t", &wt, "o", "true", "claude-code");
    assert_eq!((m.modo, m.criterio_exito.as_deref(), m.fases.len(), m.topes), (Modo::Mision, Some("true"), 0, Topes::default()));
    assert!(m.id.is_empty() && m.creada_en.is_empty() && m.equipo.is_none() && !m.worktree_propio);
    let fases = orden(d.path()).fases;
    let o = Orden::obra("t", &wt, "o", fases.clone(), Some(1), "claude-code");
    assert_eq!((o.modo, o.criterio_exito.as_deref(), &o.fases, o.revisar_cada), (Modo::Obra, None, &fases, Some(1)));
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap();
    bus.crear_trabajo(m).unwrap();
    bus.crear_trabajo(o).unwrap();
}

#[test]
fn el_criterio_es_obligatorio() {
    let d = tempfile::tempdir().unwrap();
    let bus = Bus::abrir_en(d.path()).unwrap();
    let mut m = orden(d.path());
    m.modo = Modo::Mision;
    m.fases.clear();
    assert!(bus.crear_trabajo(m.clone()).is_err());
    m.criterio_exito = Some("  ".into());
    assert!(bus.crear_trabajo(m.clone()).is_err());
    m.criterio_exito = Some("true".into());
    assert!(bus.crear_trabajo(m).is_ok());
    let mut o = orden(d.path());
    o.fases[0].criterio = String::new();
    assert!(bus.crear_trabajo(o).is_err());
}

/// Catálogo temporal con dos skills y una configuración que apunta a él.
fn skills_de_prueba() -> (tempfile::TempDir, ConfigSkills) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("rust.md"), "---\ndescripcion: Rust\n---\nUsa cargo test.\n").unwrap();
    std::fs::write(d.path().join("tests.md"), "---\ndescripcion: tests\n---\nPrueba primero.\n").unwrap();
    let cfg = ConfigSkills { dir: d.path().to_string_lossy().into(), ..Default::default() };
    (d, cfg)
}

/// Un rechazo de skills no deja rastro: ni trabajo ni entrada en la cola.
fn sin_rastro(bus: &Bus) {
    assert!(bus.listar_trabajos().unwrap().is_empty());
    assert!(bus.listar_cola(Rol::Ejecutor).unwrap().is_empty());
}

#[test]
fn crear_trabajo_rechaza_skills_sin_llave() {
    let d = tempfile::tempdir().unwrap();
    let (_sk, cfg) = skills_de_prueba();
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap().con_skills(cfg);
    let mut o = orden(d.path());
    o.creada_por = "ea-cli".into();
    o.skills = vec!["rust".into()];
    let e = bus.crear_trabajo(o.clone()).unwrap_err();
    assert!(matches!(&e, ErrorBus::Skills(m) if m.contains("llave")), "{e}");
    sin_rastro(&bus);
    // también si la skill va en una fase
    o.skills.clear();
    o.fases[0].skills = vec!["rust".into()];
    let e = bus.crear_trabajo(o.clone()).unwrap_err();
    assert!(matches!(&e, ErrorBus::Skills(m) if m.contains("fase 1") && m.contains("llave")), "{e}");
    sin_rastro(&bus);
    // sin skills no hace falta llave
    o.fases[0].skills.clear();
    bus.crear_trabajo(o).unwrap();
}

#[test]
fn crear_trabajo_rechaza_skills_inexistentes_o_deshabilitadas() {
    let d = tempfile::tempdir().unwrap();
    let (_sk, mut cfg) = skills_de_prueba();
    cfg.habilitadas = vec!["rust".into()];
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap().con_skills(cfg);
    let mut o = orden(d.path());
    o.skills = vec!["no-existe".into()];
    let e = bus.crear_trabajo(o.clone()).unwrap_err();
    assert!(matches!(&e, ErrorBus::Skills(m) if m.contains("no existe")), "{e}");
    o.skills = vec!["tests".into()];
    let e = bus.crear_trabajo(o.clone()).unwrap_err();
    assert!(matches!(&e, ErrorBus::Skills(m) if m.contains("no está habilitada")), "{e}");
    sin_rastro(&bus);
    o.skills = vec!["rust".into()];
    let id = bus.crear_trabajo(o).unwrap();
    assert_eq!(bus.leer_orden(&id).unwrap().skills, ["rust"]);
}

#[test]
fn crear_trabajo_rechaza_skills_que_exceden_los_topes() {
    let d = tempfile::tempdir().unwrap();
    let (_sk, mut cfg) = skills_de_prueba();
    cfg.max_por_trabajo = 1;
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap().con_skills(cfg.clone());
    let mut o = orden(d.path());
    o.fases[0].skills = vec!["rust".into(), "tests".into()];
    let e = bus.crear_trabajo(o.clone()).unwrap_err();
    assert!(matches!(&e, ErrorBus::Skills(m) if m.contains("el máximo es 1")), "{e}");
    cfg.max_por_trabajo = 3;
    cfg.max_caracteres = 10;
    let bus = bus.con_skills(cfg);
    let e = bus.crear_trabajo(o).unwrap_err();
    assert!(matches!(&e, ErrorBus::Skills(m) if m.contains("caracteres")), "{e}");
    sin_rastro(&bus);
}

const RUTINA: &str = concat!(r#"
[rutina]
slug = "prueba"
version = 1
descripcion = "d"
worktree = ""#, tmp!(), r#""
criterio_exito = "test -f {fichero}"
salida_esperada = "nada"
[entradas]
fichero = "a.txt"
[[pasos]]
texto = "crea {fichero}"
"#);

#[test]
fn registrar_rutina_exige_version_mayor_y_lanzar_sustituye_entradas() {
    let d = tempfile::tempdir().unwrap();
    let store = Store::open_at(&d.path().join("ea.db")).unwrap();
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap().con_store(store);
    let dir = d.path().join("rutinas");
    bus.registrar_rutina(RUTINA, &dir).unwrap();
    assert!(dir.join("prueba.toml").exists());
    assert!(bus.registrar_rutina(RUTINA, &dir).is_err()); // misma versión
    assert!(bus.registrar_rutina(&RUTINA.replace("version = 1", "version = 0"), &dir).is_err());
    bus.registrar_rutina(&RUTINA.replace("version = 1", "version = 2"), &dir).unwrap();
    let id = bus.lanzar_rutina("prueba", &[("fichero".into(), "b.txt".into())]).unwrap();
    let o = bus.leer_orden(&id).unwrap();
    assert_eq!(o.modo, Modo::Rutina);
    assert_eq!(o.criterio_exito.as_deref(), Some("test -f b.txt"));
    assert_eq!(o.rutina.unwrap().version, 2);
    assert!(o.objetivo.contains("crea b.txt"));
    assert!(bus.lanzar_rutina("prueba", &[("otra".into(), "x".into())]).is_err());
    assert!(bus.lanzar_rutina("no-existe", &[]).is_err());
}

/// B-07: una rutina tiene como mucho una ejecución activa (en cola o
/// ejecutándose), la lance quien la lance: la CLI y su timer, la web o
/// `ea_encolar_rutina`. La siguiente se rechaza con `EnCurso` hasta que la
/// anterior termine; las entradas se validan antes.
#[test]
fn una_rutina_no_se_lanza_dos_veces_mientras_la_anterior_sigue_activa() {
    let d = tempfile::tempdir().unwrap();
    let store = Store::open_at(&d.path().join("ea.db")).unwrap();
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap().con_store(store);
    let dir = d.path().join("rutinas");
    bus.registrar_rutina(RUTINA, &dir).unwrap();
    let id = bus.lanzar_rutina_en(&dir, "prueba", &[]).unwrap();
    let e = bus.lanzar_rutina_en(&dir, "prueba", &[]).unwrap_err();
    assert!(matches!(&e, ErrorBus::EnCurso(s) if s == "prueba"), "{e}");
    assert_eq!(e.to_string(), "la rutina 'prueba' ya tiene una ejecución en curso");
    assert!(matches!(bus.lanzar_rutina("prueba", &[]), Err(ErrorBus::EnCurso(_))));
    // Las entradas que no valen se rechazan antes, con su propio error.
    assert!(matches!(bus.lanzar_rutina("prueba", &[("otra".into(), "x".into())]), Err(ErrorBus::Argumentos(_))));
    // Ejecutándose, también; terminada, ya se puede.
    bus.tomar(Rol::Ejecutor).unwrap();
    bus.aplicar(&id, Evento::Tomar).unwrap();
    assert!(matches!(bus.lanzar_rutina("prueba", &[]), Err(ErrorBus::EnCurso(_))));
    bus.aplicar(&id, Evento::Entregar(informe())).unwrap();
    bus.aplicar(&id, Evento::Cerrar).unwrap();
    bus.lanzar_rutina("prueba", &[]).unwrap();
    assert_eq!(bus.trabajos_activos_de_rutina("prueba").unwrap().len(), 1);
}

/// B-07 entre procesos: de varios lanzamientos a la vez solo uno crea el
/// trabajo (la comprobación y la creación van bajo el mismo `flock`).
#[test]
fn varios_lanzamientos_a_la_vez_de_una_rutina_crean_un_solo_trabajo() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("ea.db");
    let raiz = d.path().join("estado");
    Bus::abrir_en(&raiz).unwrap().con_store(Store::open_at(&db).unwrap()).registrar_rutina(RUTINA, &d.path().join("rutinas")).unwrap();
    let barrera = Arc::new(Barrier::new(8));
    let hilos: Vec<_> = (0..8)
        .map(|_| {
            let (raiz, db, barrera) = (raiz.clone(), db.clone(), barrera.clone());
            std::thread::spawn(move || {
                let bus = Bus::abrir_en(&raiz).unwrap().con_store(Store::open_at(&db).unwrap());
                barrera.wait();
                bus.lanzar_rutina("prueba", &[]).is_ok()
            })
        })
        .collect();
    let creados = hilos.into_iter().map(|h| h.join().unwrap()).filter(|ok| *ok).count();
    assert_eq!(creados, 1);
    assert_eq!(Bus::abrir_en(&raiz).unwrap().listar_trabajos().unwrap().len(), 1);
}

/// La papelera de rutinas (spec 004, RF-011): el fichero decide si una
/// rutina registrada está activa, retirada o sin fichero; SQLite no cambia.
#[test]
fn una_rutina_retirada_no_se_lanza_y_registrarla_la_restaura() {
    use ea_bus::Ubicacion;
    let d = tempfile::tempdir().unwrap();
    let store = Store::open_at(&d.path().join("ea.db")).unwrap();
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap().con_store(store);
    let dir = d.path().join("rutinas");
    bus.registrar_rutina(RUTINA, &dir).unwrap();
    assert_eq!(ea_bus::ubicacion_rutina(&dir, "prueba").unwrap(), Ubicacion::Activa);
    assert!(ea_bus::restaurar_rutina(&dir, "prueba").is_err(), "no está retirada");

    ea_bus::retirar_rutina(&dir, "prueba").unwrap();
    assert_eq!(ea_bus::ubicacion_rutina(&dir, "prueba").unwrap(), Ubicacion::Retirada);
    assert!(dir.join(".papelera/prueba.toml").exists() && !dir.join("prueba.toml").exists());
    assert!(ea_bus::retirar_rutina(&dir, "prueba").is_err(), "ya está retirada");
    let e = bus.lanzar_rutina_en(&dir, "prueba", &[]).unwrap_err();
    assert!(matches!(&e, ErrorBus::NoLanzable(m) if *m == Ubicacion::Retirada.motivo_no_lanzable("prueba").unwrap()), "{e}");
    assert!(bus.listar_trabajos().unwrap().is_empty());

    ea_bus::restaurar_rutina(&dir, "prueba").unwrap();
    assert_eq!(ea_bus::ubicacion_rutina(&dir, "prueba").unwrap(), Ubicacion::Activa);
    bus.lanzar_rutina_en(&dir, "prueba", &[]).unwrap();

    // Registrar una versión nueva de una retirada la vuelve a poner en uso.
    ea_bus::retirar_rutina(&dir, "prueba").unwrap();
    bus.registrar_rutina(&RUTINA.replace("version = 1", "version = 2"), &dir).unwrap();
    assert_eq!(ea_bus::ubicacion_rutina(&dir, "prueba").unwrap(), Ubicacion::Activa);
    assert!(!dir.join(".papelera/prueba.toml").exists());

    // Sin fichero en ningún sitio: tampoco se lanza.
    std::fs::remove_file(dir.join("prueba.toml")).unwrap();
    assert_eq!(ea_bus::ubicacion_rutina(&dir, "prueba").unwrap(), Ubicacion::SinFichero);
    assert!(matches!(bus.lanzar_rutina_en(&dir, "prueba", &[]), Err(ErrorBus::NoLanzable(_))));
    assert!(ea_bus::ubicacion_rutina(&dir, "../x").is_err());
}

#[test]
fn las_entradas_tienen_limites_y_no_admiten_caracteres_de_control() {
    let r = ea_bus::Rutina::parsear(RUTINA).unwrap();
    let e = |v: &str| vec![("fichero".to_string(), v.to_string())];
    assert!(r.a_orden(&e("b.txt")).is_ok());
    for malo in ["a\nrm -rf x", "a\rb", "a\tb", "a\u{0}b"] {
        let err = r.a_orden(&e(malo)).unwrap_err().to_string();
        assert!(err.contains("caracteres de control"), "{malo:?}: {err}");
    }
    assert!(r.a_orden(&e(&"x".repeat(ea_bus::MAX_VALOR_ENTRADA))).is_ok());
    assert!(r.a_orden(&e(&"x".repeat(ea_bus::MAX_VALOR_ENTRADA + 1))).unwrap_err().to_string().contains("demasiado larga"));
    let muchas = vec![("fichero".to_string(), "a".to_string()); ea_bus::MAX_ENTRADAS + 1];
    assert!(r.a_orden(&muchas).unwrap_err().to_string().contains("demasiadas"));
}

#[test]
fn confirmar_encola_en_claude_code_con_el_contexto() {
    let d = tempfile::tempdir().unwrap();
    let bus = Bus::abrir_en(d.path()).unwrap();
    let mut o = orden(d.path());
    o.modo = Modo::Rutina;
    o.fases.clear();
    o.criterio_exito = Some("true".into());
    let id = bus.crear_trabajo(o).unwrap();
    bus.tomar(Rol::Ejecutor).unwrap();
    assert!(bus.confirmar(&id, "").is_err()); // no está en espera
    bus.aplicar(&id, Evento::Tomar).unwrap();
    bus.aplicar(&id, Evento::PonerEnEspera).unwrap();
    std::fs::write(d.path().join("trabajos").join(&id).join("paquete-correccion.md"), "# paquete\n").unwrap();
    assert_eq!(bus.confirmar(&id, "ojo con el disco").unwrap().estado, EstadoTrabajo::EnRevision);
    let c = bus.listar_cola(Rol::Planificador).unwrap();
    assert_eq!((c.len(), c[0].motivo.as_str()), (1, "corrección de rutina"));
    let p = std::fs::read_to_string(d.path().join("trabajos").join(&id).join("paquete-correccion.md")).unwrap();
    assert!(p.contains("ojo con el disco"));
}

#[test]
fn anotar_iteracion_no_resucita_un_cancelado() {
    let d = tempfile::tempdir().unwrap();
    let store = Store::open_at(&d.path().join("ea.db")).unwrap();
    let raiz = d.path().join("estado");
    let bus = Bus::abrir_en(&raiz).unwrap().con_store(store);
    let id = bus.crear_trabajo(orden(d.path())).unwrap();
    bus.aplicar(&id, Evento::Tomar).unwrap();
    assert_eq!(bus.anotar_iteracion(&id).unwrap(), Some(1));
    assert!(!bus.esta_cancelado(&id));
    bus.aplicar(&id, Evento::Cancelar).unwrap();
    // El ejecutor no se ha enterado y anota la siguiente: no escribe nada.
    assert_eq!(bus.anotar_iteracion(&id).unwrap(), None);
    let e = bus.leer_estado(&id).unwrap();
    assert_eq!((e.estado, e.iteracion), (EstadoTrabajo::Cancelado, 1));
    assert!(bus.esta_cancelado(&id));

    // A la vez, desde otro hilo (y otro `Bus`, como otro proceso): tras
    // cancelar, ninguna anotación lo devuelve a ejecutando.
    let id2 = bus.crear_trabajo(orden(d.path())).unwrap();
    bus.aplicar(&id2, Evento::Tomar).unwrap();
    let barrera = Arc::new(Barrier::new(2));
    let anotador = {
        let (raiz, id2, barrera) = (raiz.clone(), id2.clone(), barrera.clone());
        std::thread::spawn(move || {
            let bus = Bus::abrir_en(&raiz).unwrap();
            barrera.wait();
            for _ in 0..500 {
                if bus.anotar_iteracion(&id2).unwrap().is_none() {
                    break;
                }
            }
        })
    };
    barrera.wait();
    bus.aplicar(&id2, Evento::Cancelar).unwrap();
    anotador.join().unwrap();
    assert_eq!(bus.leer_estado(&id2).unwrap().estado, EstadoTrabajo::Cancelado);
    assert_eq!(bus.anotar_iteracion(&id2).unwrap(), None);

    // La cancelación en curso queda en el historial y en el diario.
    bus.anotar_cancelacion_en_curso(&id, 1, None).unwrap();
    let historial = std::fs::read_to_string(raiz.join("trabajos").join(&id).join("historial.jsonl")).unwrap();
    assert!(historial.lines().last().unwrap().contains("cancelado_en_curso"));
    let s = Store::open_at(&d.path().join("ea.db")).unwrap();
    let eventos = s.list_events(&Default::default()).unwrap();
    let ev = eventos.iter().find(|e| e.action == "bus_cancelado_en_curso").expect("evento en el diario");
    assert!(ev.detail.as_deref().unwrap().contains(&id));
}

#[test]
fn comprobar_registro_no_escribe_y_aplica_la_misma_politica_que_registrar() {
    let d = tempfile::tempdir().unwrap();
    let store = Store::open_at(&d.path().join("ea.db")).unwrap();
    let (r, anterior) = ea_bus::comprobar_registro(&store, RUTINA).unwrap();
    assert_eq!((r.rutina.slug.as_str(), r.rutina.version), ("prueba", 1));
    assert!(anterior.is_none());
    assert!(store.ultima_rutina("prueba").unwrap().is_none(), "comprobar no registra");

    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap().con_store(store.duplicar().unwrap());
    bus.registrar_rutina(RUTINA, &d.path().join("rutinas")).unwrap();
    let e = ea_bus::comprobar_registro(&store, RUTINA).unwrap_err().to_string();
    assert!(e.contains("la nueva debe ser mayor"), "{e}");
    let (_, anterior) = ea_bus::comprobar_registro(&store, &RUTINA.replace("version = 1", "version = 2")).unwrap();
    assert_eq!(anterior.map(|f| f.version), Some(1));
    // Demasiado larga: ni se interpreta, y registrar la rechaza igual.
    let largo = format!("{}\n#{}", RUTINA.replace("version = 1", "version = 3"), "x".repeat(ea_bus::MAX_DEFINICION));
    assert!(ea_bus::comprobar_registro(&store, &largo).unwrap_err().to_string().contains("demasiado larga"));
    assert!(bus.registrar_rutina(&largo, &d.path().join("rutinas")).is_err());
}
