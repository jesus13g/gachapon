use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Barrier};
use std::time::Duration;

use ea_bus::{
    transicion, Agente, Bus, CandadoWorktree, ConfigOrquestador, ConfigSkills, ErrorBus, Estado, EstadoTrabajo, Evento, Fase, Informe,
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
            skills: vec![],
        }],
        revisar_cada: None,
        topes: Topes::default(),
        rutina: None,
        notas_revision: vec![],
        skills: vec![],
        creada_en: String::new(),
        creada_por: "claude-code".into(),
    }
}

fn informe() -> Informe {
    Informe { evidencia: "ok".into(), fases_hechas: 1, duracion_s: 3, ..Informe::nuevo(ResultadoInforme::Terminado) }
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
    assert_eq!((e.estado, e.iteracion), (EstadoTrabajo::Ejecutando, 0));
    assert_eq!(bus.anotar_iteracion(&id).unwrap(), 1);
    assert_eq!(bus.leer_estado(&id).unwrap().iteracion, 1);
    bus.aplicar(&id, Evento::Entregar(informe())).unwrap();
    assert_eq!(bus.leer_informe(&id).unwrap().unwrap().evidencia, "ok");
    // entregar encola en claude-code por sí solo
    assert_eq!(bus.listar_cola(Agente::ClaudeCode).unwrap()[0].motivo, "terminado");
    bus.aplicar(&id, Evento::Cerrar).unwrap();
    // cerrar limpia la entrada de claude-code
    assert!(bus.listar_cola(Agente::ClaudeCode).unwrap().is_empty());
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
    assert_eq!(c.techo_topes.turnos, 100);
    assert!(c.avisos_escritorio);
    let p = d.path().join("o.toml");
    std::fs::write(&p, "modelo = \"otro\"\n[topes]\nturnos = 5\n").unwrap();
    let c = ConfigOrquestador::cargar_de(&p);
    // el `[topes]` antiguo es ahora el techo; lo que falta, el techo duro
    assert_eq!((c.modelo.as_str(), c.techo_topes.turnos, c.techo_topes.tiempo_s, c.qwen_bin.as_str()), ("otro", 5, 3600, "qwen"));
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

#[test]
fn cerrar_y_cancelar_retiran_el_trabajo_de_todas_las_colas() {
    let d = tempfile::tempdir().unwrap();
    let bus = Bus::abrir_en(d.path()).unwrap();
    let a = bus.crear_trabajo(orden(d.path())).unwrap();
    let b = bus.crear_trabajo(orden(d.path())).unwrap();
    bus.tomar(Agente::ModeloLocal).unwrap();
    bus.tomar(Agente::ModeloLocal).unwrap();
    for id in [&a, &b] {
        bus.aplicar(id, Evento::Tomar).unwrap();
        bus.aplicar(id, Evento::Entregar(informe())).unwrap();
        bus.encolar(Agente::ModeloLocal, id, "extra").unwrap();
    }
    assert_eq!(bus.listar_cola(Agente::ClaudeCode).unwrap().len(), 2);
    bus.aplicar(&a, Evento::Cerrar).unwrap();
    bus.aplicar(&b, Evento::Cancelar).unwrap();
    assert!(bus.listar_cola(Agente::ClaudeCode).unwrap().is_empty());
    assert!(bus.listar_cola(Agente::ModeloLocal).unwrap().is_empty());
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
    assert!(bus.listar_cola(Agente::ModeloLocal).unwrap().is_empty());
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

const RUTINA: &str = r#"
[rutina]
slug = "prueba"
version = 1
descripcion = "d"
worktree = "/tmp"
criterio_exito = "test -f {fichero}"
salida_esperada = "nada"
[entradas]
fichero = "a.txt"
[[pasos]]
texto = "crea {fichero}"
"#;

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

#[test]
fn confirmar_encola_en_claude_code_con_el_contexto() {
    let d = tempfile::tempdir().unwrap();
    let bus = Bus::abrir_en(d.path()).unwrap();
    let mut o = orden(d.path());
    o.modo = Modo::Rutina;
    o.fases.clear();
    o.criterio_exito = Some("true".into());
    let id = bus.crear_trabajo(o).unwrap();
    bus.tomar(Agente::ModeloLocal).unwrap();
    assert!(bus.confirmar(&id, "").is_err()); // no está en espera
    bus.aplicar(&id, Evento::Tomar).unwrap();
    bus.aplicar(&id, Evento::PonerEnEspera).unwrap();
    std::fs::write(d.path().join("trabajos").join(&id).join("paquete-correccion.md"), "# paquete\n").unwrap();
    assert_eq!(bus.confirmar(&id, "ojo con el disco").unwrap().estado, EstadoTrabajo::EnRevision);
    let c = bus.listar_cola(Agente::ClaudeCode).unwrap();
    assert_eq!((c.len(), c[0].motivo.as_str()), (1, "corrección de rutina"));
    let p = std::fs::read_to_string(d.path().join("trabajos").join(&id).join("paquete-correccion.md")).unwrap();
    assert!(p.contains("ojo con el disco"));
}
