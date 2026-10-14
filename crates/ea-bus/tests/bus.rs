use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Barrier};
use std::time::Duration;

use ea_bus::{
    transicion, Agente, Bus, CandadoWorktree, ConfigOrquestador, ErrorBus, Estado, EstadoTrabajo, Evento, Fase, Informe,
    Modo, Ocupante, Orden, ResultadoInforme, Topes,
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
        }],
        revisar_cada: None,
        topes: Topes::default(),
        rutina: None,
        notas_revision: vec![],
        creada_en: String::new(),
        creada_por: "claude-code".into(),
    }
}

fn informe() -> Informe {
    Informe {
        resultado: ResultadoInforme::Terminado,
        evidencia: "ok".into(),
        commit_inicio: None,
        diff_stat: None,
        motivo: None,
        fases_hechas: 1,
        duracion_s: 3,
    }
}

fn yo(trabajo: &str) -> Ocupante {
    Ocupante { agente: "modelo-local".into(), trabajo: trabajo.into(), pid: 0, desde: "ahora".into() }
}

#[test]
fn crear_tomar_entregar_y_cerrar_con_historial_en_sqlite() {
    let d = tempfile::tempdir().unwrap();
    let store = Store::open_at(&d.path().join("ea.db")).unwrap();
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap().con_store(store);
    let id = bus.crear_trabajo(orden(d.path())).unwrap();
    assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::EnCola);
    assert_eq!(bus.listar_cola(Agente::ModeloLocal).unwrap().len(), 1);
    assert_eq!(bus.tomar(Agente::ModeloLocal).unwrap(), Some(id.clone()));
    assert_eq!(bus.tomar(Agente::ModeloLocal).unwrap(), None);
    let e = bus.aplicar(&id, Evento::Tomar).unwrap();
    assert_eq!((e.estado, e.iteracion), (EstadoTrabajo::Ejecutando, 1));
    bus.aplicar(&id, Evento::Entregar(informe())).unwrap();
    assert_eq!(bus.leer_informe(&id).unwrap().unwrap().evidencia, "ok");
    // entregar encola en claude-code por sí solo
    assert_eq!(bus.listar_cola(Agente::ClaudeCode).unwrap()[0].motivo, "terminado");
    assert_eq!(bus.tomar(Agente::ClaudeCode).unwrap(), Some(id.clone()));
    bus.aplicar(&id, Evento::Cerrar).unwrap();
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
                while let Some(id) = bus.tomar(Agente::ModeloLocal).unwrap() {
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
fn sin_configuracion_se_usan_los_valores_por_defecto() {
    let d = tempfile::tempdir().unwrap();
    let c = ConfigOrquestador::cargar_de(&d.path().join("no-existe.toml"));
    assert_eq!(c, ConfigOrquestador::default());
    assert_eq!(c.base_url, "http://127.0.0.1:8080/v1");
    assert_eq!(c.modelo, "qwen3-coder-30b");
    assert_eq!(c.topes.turnos, 30);
    assert!(c.avisos_escritorio);
    let p = d.path().join("o.toml");
    std::fs::write(&p, "modelo = \"otro\"\n[topes]\nturnos = 5\n").unwrap();
    let c = ConfigOrquestador::cargar_de(&p);
    assert_eq!((c.modelo.as_str(), c.topes.turnos, c.topes.tiempo_s, c.qwen_bin.as_str()), ("otro", 5, 900, "qwen"));
}

#[test]
fn reformular_anade_notas_y_vuelve_a_encolar() {
    let d = tempfile::tempdir().unwrap();
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap();
    let id = bus.crear_trabajo(orden(d.path())).unwrap();
    assert!(bus.reformular(&id, "no vale").is_err()); // aún EnCola
    bus.tomar(Agente::ModeloLocal).unwrap();
    bus.aplicar(&id, Evento::Tomar).unwrap();
    bus.aplicar(&id, Evento::Entregar(informe())).unwrap();
    bus.reformular(&id, "falta la prueba").unwrap();
    assert_eq!(bus.leer_orden(&id).unwrap().notas_revision, vec!["falta la prueba".to_string()]);
    assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::EnCola);
    assert_eq!(bus.tomar(Agente::ModeloLocal).unwrap(), Some(id));
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
    assert!(bus.listar_cola(Agente::ModeloLocal).unwrap().is_empty());
    // y un trabajo que se quedó ejecutando
    let b = bus.crear_trabajo(orden(d.path())).unwrap();
    bus.tomar(Agente::ModeloLocal).unwrap();
    bus.aplicar(&b, Evento::Tomar).unwrap();
    let rec = bus.recuperar().unwrap();
    assert!(rec.contains(&a) && rec.contains(&b));
    assert_eq!(bus.listar_cola(Agente::ModeloLocal).unwrap()[0].trabajo, a);
    assert_eq!(bus.leer_estado(&b).unwrap().estado, EstadoTrabajo::EnRevision);
    let inf = bus.leer_informe(&b).unwrap().unwrap();
    assert_eq!((inf.resultado, inf.motivo.as_deref()), (ResultadoInforme::Bloqueado, Some("interrumpido")));
    assert_eq!(bus.listar_cola(Agente::ClaudeCode).unwrap()[0].motivo, "bloqueado");
    assert!(bus.recuperar().unwrap().is_empty());
}
