//! Equipos (spec 019): tipos, colas por agente y entrega al revisor. Todo en
//! directorios temporales.

use std::path::Path;

use ea_ajustes::{Ajustes, Fuentes};
use ea_bus::{
    Bus, ConfigSkills, EjecutadoPor, ErrorBus, EstadoEquipo, EstadoTrabajo, Evento, Fase, Informe, Miembro, Modo, NuevoEquipo,
    Orden, ResultadoInforme, Rol, RolEquipo, Topes,
};
use ea_store::Store;

fn orden(wt: &Path) -> Orden {
    Orden {
        id: String::new(),
        modo: Modo::Mision,
        titulo: "prueba".into(),
        worktree: wt.to_string_lossy().into(),
        objetivo: "haz algo".into(),
        criterio_exito: Some("true".into()),
        fases: vec![],
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

fn m(agente: &str, rol: RolEquipo) -> Miembro {
    Miembro { agente: agente.into(), rol }
}

fn ajustes(d: &Path) -> Ajustes {
    Ajustes::por_defecto(&Fuentes::aislada(&d.join("config")))
}

/// claude-code planifica, qwen-code y codex ejecutan, antigravity revisa.
fn miembros() -> Vec<Miembro> {
    vec![
        m("claude-code", RolEquipo::Planificador),
        m("qwen-code", RolEquipo::Ejecutor),
        m("codex", RolEquipo::Ejecutor),
        m("antigravity", RolEquipo::Revisor),
    ]
}

fn nuevo(miembros: Vec<Miembro>) -> NuevoEquipo {
    NuevoEquipo {
        titulo: "equipo de prueba".into(),
        objetivo: "probar".into(),
        worktree_base: "/tmp/repo".into(),
        miembros,
        creado_por: "ea-cli".into(),
    }
}

struct Entorno {
    d: tempfile::TempDir,
    bus: Bus,
    a: Ajustes,
}

fn entorno() -> Entorno {
    let d = tempfile::tempdir().unwrap();
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap();
    let mut a = ajustes(d.path());
    a.orquestador.paralelo = 4;
    Entorno { d, bus, a }
}

fn error_de(r: Result<impl std::fmt::Debug, ErrorBus>) -> String {
    r.unwrap_err().to_string()
}

#[test]
fn una_orden_antigua_sin_los_campos_nuevos_se_lee() {
    let antigua = r#"{
        "id": "20260101T000000-00000000", "modo": "mision", "titulo": "t", "worktree": "/tmp",
        "objetivo": "o", "criterio_exito": "true", "creada_en": "2026-01-01T00:00:00Z", "creada_por": "claude-code"
    }"#;
    let o: Orden = serde_json::from_str(antigua).unwrap();
    assert_eq!((o.equipo, o.asignado_a, o.revisor, o.worktree_propio), (None, None, None, false));
}

#[test]
fn sin_equipo_la_orden_se_serializa_como_antes() {
    let e = entorno();
    let v = serde_json::to_value(orden(e.d.path())).unwrap();
    for k in ["equipo", "asignado_a", "revisor", "worktree_propio"] {
        assert!(v.get(k).is_none(), "{k} no debe salir sin equipo");
    }
    // con ellos, sí
    let con = Orden { equipo: Some("e".into()), asignado_a: Some("codex".into()), revisor: Some("x".into()), worktree_propio: true, ..orden(e.d.path()) };
    let v = serde_json::to_value(&con).unwrap();
    assert_eq!((v["equipo"].as_str(), v["asignado_a"].as_str(), v["worktree_propio"].as_bool()), (Some("e"), Some("codex"), Some(true)));
    let ida_y_vuelta: Orden = serde_json::from_value(v).unwrap();
    assert_eq!(ida_y_vuelta, con);
}

#[test]
fn despertar_y_dormir_un_equipo_deja_rastro_en_el_diario() {
    let d = tempfile::tempdir().unwrap();
    let store = Store::open_at(&d.path().join("ea.db")).unwrap();
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap().con_store(store).con_actor("ea-cli");
    let mut a = ajustes(d.path());
    a.orquestador.paralelo = 4;
    let eq = bus.crear_equipo(nuevo(miembros()), &a).unwrap();
    assert_eq!(eq.estado, EstadoEquipo::Despierto);
    assert_eq!(bus.leer_equipo(&eq.id).unwrap(), eq);
    assert_eq!(bus.listar_equipos().unwrap(), vec![eq.clone()]);
    assert!(d.path().join("estado/equipos").join(&eq.id).join("equipo.json").is_file());
    let d1 = bus.dormir_equipo(&eq.id).unwrap();
    assert_eq!(d1.estado, EstadoEquipo::Dormido);
    // dormir de nuevo no repite el evento
    assert_eq!(bus.dormir_equipo(&eq.id).unwrap().estado, EstadoEquipo::Dormido);
    let s = Store::open_at(&d.path().join("ea.db")).unwrap();
    let acciones: Vec<String> = s.list_events(&Default::default()).unwrap().into_iter().map(|e| e.action).collect();
    assert_eq!(acciones.iter().filter(|a| *a == "equipo_despertado").count(), 1);
    assert_eq!(acciones.iter().filter(|a| *a == "equipo_dormido").count(), 1);
    assert!(matches!(bus.leer_equipo("no-existe"), Err(ErrorBus::NoExiste(_))));
    assert!(bus.leer_equipo("../x").is_err());
}

#[test]
fn un_equipo_invalido_se_rechaza_sin_escribir_nada() {
    let mut e = entorno();
    let mut v = miembros();
    v.push(m("codex", RolEquipo::Revisor));
    assert!(error_de(e.bus.crear_equipo(nuevo(v), &e.a)).contains("más de una vez"));
    let mut v = miembros();
    v.push(m("opencode", RolEquipo::Ejecutor));
    v[3] = m("antigravity", RolEquipo::Ejecutor);
    assert!(error_de(e.bus.crear_equipo(nuevo(v), &e.a)).contains("no puede ser ejecutor"));
    let sin_ej = vec![m("claude-code", RolEquipo::Planificador), m("antigravity", RolEquipo::Revisor)];
    assert!(error_de(e.bus.crear_equipo(nuevo(sin_ej), &e.a)).contains("al menos un ejecutor"));
    let sin_pl = vec![m("qwen-code", RolEquipo::Ejecutor)];
    assert!(error_de(e.bus.crear_equipo(nuevo(sin_pl), &e.a)).contains("al menos un planificador"));
    e.a.modelos.get_mut("local").unwrap().remoto = true;
    assert!(error_de(e.bus.crear_equipo(nuevo(miembros()), &e.a)).contains("remoto"));
    e.a.modelos.get_mut("local").unwrap().remoto = false;
    let mut vacio = nuevo(miembros());
    vacio.titulo = "  ".into();
    assert!(e.bus.crear_equipo(vacio, &e.a).is_err());
    assert!(e.bus.listar_equipos().unwrap().is_empty());
}

#[test]
fn un_agente_solo_esta_en_un_equipo_despierto() {
    let e = entorno();
    let primero = e.bus.crear_equipo(nuevo(miembros()), &e.a).unwrap();
    let otro = vec![m("opencode", RolEquipo::Ejecutor), m("claude-code", RolEquipo::Planificador)];
    let msg = error_de(e.bus.crear_equipo(nuevo(otro.clone()), &e.a));
    assert!(msg.contains("claude-code") && msg.contains(&primero.id), "{msg}");
    // dormido el primero, el agente queda libre
    e.bus.dormir_equipo(&primero.id).unwrap();
    assert!(e.bus.crear_equipo(nuevo(otro), &e.a).is_ok());
}

#[test]
fn tomar_para_prefiere_la_cola_propia_y_luego_la_del_rol() {
    let e = entorno();
    let (a, b, c) = (
        e.bus.crear_trabajo(orden(e.d.path())).unwrap(), // cola de rol
        e.bus.crear_trabajo(orden(e.d.path())).unwrap(), // cola de rol
        e.bus.crear_trabajo(orden(e.d.path())).unwrap(),
    );
    e.bus.retirar_de_colas(&c).unwrap();
    e.bus.encolar_agente("codex", &c, "propio").unwrap();
    e.bus.encolar_agente("qwen-code", "ajeno", "de otro").unwrap();
    assert_eq!(e.bus.listar_cola_agente("codex").unwrap().len(), 1);
    assert_eq!(e.bus.listar_cola_agente("opencode").unwrap(), vec![]);
    // la cola propia, aunque sea la más nueva
    assert_eq!(e.bus.tomar_para("codex").unwrap(), Some(c));
    // luego la del rol, por orden
    assert_eq!(e.bus.tomar_para("codex").unwrap(), Some(a));
    assert_eq!(e.bus.tomar_para("codex").unwrap(), Some(b));
    assert_eq!(e.bus.tomar_para("codex").unwrap(), None);
    // lo del otro agente no se toca
    assert_eq!(e.bus.listar_cola_agente("qwen-code").unwrap().len(), 1);
}

#[test]
fn las_colas_por_agente_solo_abren_ids_del_catalogo() {
    let e = entorno();
    for malo in ["../../etc", "", "desconocido", "codex/../x"] {
        assert!(e.bus.encolar_agente(malo, "t", "m").is_err(), "{malo:?}");
        assert!(e.bus.listar_cola_agente(malo).is_err(), "{malo:?}");
        assert!(e.bus.tomar_para(malo).is_err(), "{malo:?}");
    }
    assert!(!e.d.path().join("etc").exists());
}

#[test]
fn crear_trabajo_enruta_y_anota_en_el_equipo() {
    let e = entorno();
    let eq = e.bus.crear_equipo(nuevo(miembros()), &e.a).unwrap();
    let para_codex = Orden { equipo: Some(eq.id.clone()), asignado_a: Some("codex".into()), ..orden(e.d.path()) };
    let libre = Orden { equipo: Some(eq.id.clone()), ..orden(e.d.path()) };
    let a = e.bus.crear_trabajo(para_codex).unwrap();
    let b = e.bus.crear_trabajo(libre).unwrap();
    // el asignado no aparece en la cola de rol
    assert_eq!(e.bus.listar_cola_agente("codex").unwrap()[0].trabajo, a);
    let rol: Vec<String> = e.bus.listar_cola(Rol::Ejecutor).unwrap().into_iter().map(|x| x.trabajo).collect();
    assert_eq!(rol, vec![b.clone()]);
    assert_eq!(e.bus.leer_equipo(&eq.id).unwrap().trabajos, vec![a.clone(), b.clone()]);
    // otro ejecutor no ve lo de Codex
    assert_eq!(e.bus.tomar_para("qwen-code").unwrap(), Some(b));
    assert_eq!(e.bus.tomar_para("qwen-code").unwrap(), None);
    assert_eq!(e.bus.tomar_para("codex").unwrap(), Some(a));
}

#[test]
fn crear_trabajo_valida_el_equipo_y_no_deja_rastro() {
    let e = entorno();
    let eq = e.bus.crear_equipo(nuevo(miembros()), &e.a).unwrap();
    let con = |f: &dyn Fn(&mut Orden)| {
        let mut o = Orden { equipo: Some(eq.id.clone()), ..orden(e.d.path()) };
        f(&mut o);
        o
    };
    // asignado que no es ejecutor (planificador, revisor, ajeno al equipo)
    for malo in ["claude-code", "antigravity", "opencode"] {
        assert!(error_de(e.bus.crear_trabajo(con(&|o| o.asignado_a = Some(malo.into())))).contains("no es un ejecutor"));
    }
    // revisor que ni revisa ni planifica en el equipo
    assert!(error_de(e.bus.crear_trabajo(con(&|o| o.revisor = Some("codex".into())))).contains("no es un revisor"));
    // revisor = asignado
    let mismo = con(&|o| {
        o.asignado_a = Some("codex".into());
        o.revisor = Some("codex".into());
    });
    assert!(e.bus.crear_trabajo(mismo).is_err());
    // equipo inexistente y asignado/revisor sin equipo
    assert!(e.bus.crear_trabajo(con(&|o| o.equipo = Some("nada".into()))).is_err());
    assert!(e.bus.crear_trabajo(Orden { asignado_a: Some("codex".into()), ..orden(e.d.path()) }).is_err());
    assert!(e.bus.crear_trabajo(Orden { revisor: Some("antigravity".into()), ..orden(e.d.path()) }).is_err());
    // equipo dormido
    e.bus.dormir_equipo(&eq.id).unwrap();
    assert!(error_de(e.bus.crear_trabajo(con(&|_| {}))).contains("dormido"));
    assert!(e.bus.listar_trabajos().unwrap().is_empty());
    assert!(e.bus.listar_cola(Rol::Ejecutor).unwrap().is_empty());
    assert!(e.bus.listar_cola_agente("codex").unwrap().is_empty());
    assert!(e.bus.leer_equipo(&eq.id).unwrap().trabajos.is_empty());
    // worktree_propio no necesita equipo
    assert!(e.bus.crear_trabajo(Orden { worktree_propio: true, ..orden(e.d.path()) }).is_ok());
}

/// Crea un trabajo para `codex` con revisor, y lo deja ejecutado por `por`.
fn entregar(e: &Entorno, eq: &str, revisor: Option<&str>, por: &str) -> String {
    let o = Orden { equipo: Some(eq.into()), asignado_a: Some("codex".into()), revisor: revisor.map(Into::into), ..orden(e.d.path()) };
    let id = e.bus.crear_trabajo(o).unwrap();
    assert_eq!(e.bus.tomar_para("codex").unwrap(), Some(id.clone()));
    e.bus.tomar_con(&id, EjecutadoPor { agente: por.into(), modelo: "m".into() }).unwrap();
    e.bus.aplicar(&id, Evento::Entregar(informe())).unwrap();
    id
}

#[test]
fn la_entrega_llega_al_revisor_ademas_de_al_planificador() {
    let e = entorno();
    let eq = e.bus.crear_equipo(nuevo(miembros()), &e.a).unwrap();
    let id = entregar(&e, &eq.id, Some("antigravity"), "codex");
    assert_eq!(e.bus.listar_cola(Rol::Planificador).unwrap()[0].trabajo, id);
    let cola = e.bus.listar_cola_revision("antigravity").unwrap();
    assert_eq!((cola[0].trabajo.as_str(), cola[0].motivo.as_str()), (id.as_str(), "terminado"));
    // cerrar limpia las dos
    e.bus.aplicar(&id, Evento::Cerrar).unwrap();
    assert!(e.bus.listar_cola(Rol::Planificador).unwrap().is_empty());
    assert!(e.bus.listar_cola_revision("antigravity").unwrap().is_empty());
}

#[test]
fn si_el_revisor_es_quien_ejecuto_la_entrega_solo_va_a_la_cola_del_rol() {
    let e = entorno();
    let eq = e.bus.crear_equipo(nuevo(miembros()), &e.a).unwrap();
    // claude-code (planificador del equipo) revisa un trabajo que consta ejecutado por él
    let id = entregar(&e, &eq.id, Some("claude-code"), "claude-code");
    assert_eq!(e.bus.listar_cola(Rol::Planificador).unwrap().len(), 1);
    assert!(e.bus.listar_cola_revision("claude-code").unwrap().is_empty());
    let aviso = e.bus.leer_informe(&id).unwrap().unwrap().aviso.unwrap();
    assert!(aviso.contains("claude-code") && aviso.contains("sin revisión independiente"), "{aviso}");
    e.bus.aplicar(&id, Evento::Cancelar).unwrap();
    // sin revisor, como siempre
    let sin = entregar(&e, &eq.id, None, "codex");
    assert_eq!(e.bus.leer_informe(&sin).unwrap().unwrap().aviso, None);
    assert_eq!(e.bus.listar_cola(Rol::Planificador).unwrap()[0].trabajo, sin);
    assert!(e.bus.listar_cola_revision("antigravity").unwrap().is_empty());
}

#[test]
fn reformular_un_trabajo_asignado_vuelve_a_la_cola_de_ese_agente() {
    let e = entorno();
    let eq = e.bus.crear_equipo(nuevo(miembros()), &e.a).unwrap();
    let id = entregar(&e, &eq.id, Some("antigravity"), "codex");
    e.bus.reformular(&id, "falta algo", None).unwrap();
    assert_eq!(e.bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::EnCola);
    assert_eq!(e.bus.listar_cola_agente("codex").unwrap()[0].motivo, "reformulado");
    assert!(e.bus.listar_cola(Rol::Ejecutor).unwrap().is_empty());
    // y la entrega vieja del revisor y del planificador se retiró
    assert!(e.bus.listar_cola(Rol::Planificador).unwrap().is_empty());
    assert!(e.bus.listar_cola_revision("antigravity").unwrap().is_empty());
}

#[test]
fn recuperar_devuelve_a_su_cola_de_agente_el_reclamo_huerfano() {
    let e = entorno();
    let eq = e.bus.crear_equipo(nuevo(miembros()), &e.a).unwrap();
    let o = Orden { equipo: Some(eq.id.clone()), asignado_a: Some("codex".into()), ..orden(e.d.path()) };
    let id = e.bus.crear_trabajo(o).unwrap();
    let cola = e.d.path().join("estado/colas/agentes/codex");
    let f = std::fs::read_dir(&cola).unwrap().flatten().next().unwrap().path();
    let nombre = f.file_name().unwrap().to_string_lossy().into_owned();
    std::fs::rename(&f, cola.join(format!(".{nombre}.tomada-1"))).unwrap();
    assert!(e.bus.listar_cola_agente("codex").unwrap().is_empty());
    assert_eq!(e.bus.recuperar().unwrap(), vec![id.clone()]);
    assert_eq!(e.bus.listar_cola_agente("codex").unwrap()[0].trabajo, id);
}

#[test]
fn los_planificadores_del_equipo_tienen_la_llave_de_las_skills_y_los_revisores_no() {
    let d = tempfile::tempdir().unwrap();
    let sk = tempfile::tempdir().unwrap();
    std::fs::write(sk.path().join("rust.md"), "---\ndescripcion: Rust\n---\nUsa cargo test.\n").unwrap();
    let cfg = ConfigSkills { dir: sk.path().to_string_lossy().into(), ..Default::default() };
    let bus = Bus::abrir_en(&d.path().join("estado")).unwrap().con_skills(cfg);
    let a = ajustes(d.path());
    // codex planifica (admite ambos roles) y antigravity revisa
    let eq = bus
        .crear_equipo(
            nuevo(vec![m("codex", RolEquipo::Planificador), m("qwen-code", RolEquipo::Ejecutor), m("antigravity", RolEquipo::Revisor)]),
            &a,
        )
        .unwrap();
    let con = |por: &str| Orden { equipo: Some(eq.id.clone()), creada_por: por.into(), skills: vec!["rust".into()], ..orden(d.path()) };
    assert!(bus.crear_trabajo(con("codex")).is_ok());
    assert!(matches!(bus.crear_trabajo(con("antigravity")), Err(ErrorBus::Skills(_))));
    // fuera del equipo, codex no la tiene (la llave es del rol planificador)
    let fuera = Orden { creada_por: "codex".into(), skills: vec!["rust".into()], ..orden(d.path()) };
    assert!(matches!(bus.crear_trabajo(fuera), Err(ErrorBus::Skills(_))));
}

#[test]
fn una_obra_con_fases_sigue_funcionando_con_equipo() {
    let e = entorno();
    let eq = e.bus.crear_equipo(nuevo(miembros()), &e.a).unwrap();
    let f = |n| Fase { numero: n, titulo: "t".into(), instrucciones: "i".into(), entregable: "e".into(), criterio: "true".into(), skills: vec![] };
    let o = Orden { modo: Modo::Obra, criterio_exito: None, fases: vec![f(1), f(2)], equipo: Some(eq.id.clone()), ..orden(e.d.path()) };
    let id = e.bus.crear_trabajo(o).unwrap();
    assert_eq!(e.bus.tomar_para("qwen-code").unwrap(), Some(id));
}

#[test]
fn la_revision_dirigida_no_la_toma_un_ejecutor_con_tomar_para() {
    let e = entorno();
    let eq = e.bus.crear_equipo(nuevo(miembros()), &e.a).unwrap();
    let id = entregar(&e, &eq.id, Some("antigravity"), "codex");
    assert_eq!(e.bus.listar_cola_revision("antigravity").unwrap()[0].trabajo, id);
    assert!(e.bus.listar_cola_agente("antigravity").unwrap().is_empty());
    // ni siquiera quien tiene la entrega en su cola de revisión la toma como trabajo
    assert_eq!(e.bus.tomar_para("antigravity").unwrap(), None);
    assert_eq!(e.bus.listar_cola_revision("antigravity").unwrap().len(), 1);
    assert!(e.bus.listar_cola_revision("no-existe").is_err());
    // un reclamo huérfano de la cola de revisión vuelve a ella
    let cola = e.d.path().join("estado/colas/revision/antigravity");
    let f = std::fs::read_dir(&cola).unwrap().flatten().next().unwrap().path();
    let nombre = f.file_name().unwrap().to_string_lossy().into_owned();
    std::fs::rename(&f, cola.join(format!(".{nombre}.tomada-1"))).unwrap();
    assert_eq!(e.bus.recuperar().unwrap(), vec![id.clone()]);
    assert_eq!(e.bus.listar_cola_revision("antigravity").unwrap()[0].trabajo, id);
}

#[test]
fn dormir_un_equipo_devuelve_lo_pendiente_de_sus_ejecutores_a_la_cola_del_rol() {
    let e = entorno();
    let eq = e.bus.crear_equipo(nuevo(miembros()), &e.a).unwrap();
    let o = Orden { equipo: Some(eq.id.clone()), asignado_a: Some("codex".into()), ..orden(e.d.path()) };
    let id = e.bus.crear_trabajo(o).unwrap();
    assert_eq!(e.bus.listar_cola_agente("codex").unwrap().len(), 1);
    e.bus.dormir_equipo(&eq.id).unwrap();
    assert!(e.bus.listar_cola_agente("codex").unwrap().is_empty());
    let cola = e.bus.listar_cola(Rol::Ejecutor).unwrap();
    assert_eq!((cola[0].trabajo.as_str(), cola[0].motivo.as_str()), (id.as_str(), "equipo dormido"));
    // un ejecutor del rol lo toma, queda como ejecutado_por y ya no puede revisarlo
    assert_eq!(e.bus.tomar_para("qwen-code").unwrap(), Some(id.clone()));
    e.bus.tomar_con(&id, EjecutadoPor { agente: "qwen-code".into(), modelo: "m".into() }).unwrap();
    assert_eq!(e.bus.leer_estado(&id).unwrap().ejecutado_por.unwrap().agente, "qwen-code");
}

#[test]
fn reformular_un_asignado_de_un_equipo_dormido_va_a_la_cola_del_rol() {
    let e = entorno();
    let eq = e.bus.crear_equipo(nuevo(miembros()), &e.a).unwrap();
    let id = entregar(&e, &eq.id, Some("antigravity"), "codex");
    e.bus.dormir_equipo(&eq.id).unwrap();
    e.bus.reformular(&id, "otra vez", None).unwrap();
    assert!(e.bus.listar_cola_agente("codex").unwrap().is_empty());
    assert_eq!(e.bus.listar_cola(Rol::Ejecutor).unwrap()[0].trabajo, id);
}

#[test]
fn el_revisor_sigue_sin_poder_ser_quien_ejecuto_aunque_el_equipo_durmiera() {
    let e = entorno();
    let eq = e.bus.crear_equipo(nuevo(miembros()), &e.a).unwrap();
    let o = Orden { equipo: Some(eq.id.clone()), asignado_a: Some("codex".into()), revisor: Some("claude-code".into()), ..orden(e.d.path()) };
    let id = e.bus.crear_trabajo(o).unwrap();
    e.bus.dormir_equipo(&eq.id).unwrap();
    assert_eq!(e.bus.tomar_para("claude-code").unwrap(), Some(id.clone()));
    e.bus.tomar_con(&id, EjecutadoPor { agente: "claude-code".into(), modelo: "m".into() }).unwrap();
    e.bus.aplicar(&id, Evento::Entregar(informe())).unwrap();
    assert!(e.bus.listar_cola_revision("claude-code").unwrap().is_empty());
    assert!(e.bus.leer_informe(&id).unwrap().unwrap().aviso.is_some());
}

#[test]
fn crear_un_equipo_que_deja_ejecutores_sin_trabajador_se_rechaza() {
    let mut e = entorno();
    e.a.orquestador.paralelo = 2;
    e.bus.crear_equipo(nuevo(miembros()), &e.a).unwrap(); // qwen-code y codex: 2
    // otro equipo con otro ejecutor distinto: 3 > 2
    let otro = nuevo(vec![m("opencode", RolEquipo::Ejecutor), m("claude-code", RolEquipo::Planificador)]);
    let msg = error_de(e.bus.crear_equipo(otro, &e.a));
    assert!(msg.contains("orquestador.paralelo"), "{msg}");
    assert_eq!(e.bus.listar_equipos().unwrap().len(), 1);
    e.a.orquestador.paralelo = 3;
    // claude-code ya está en el primer equipo: el rechazo ahora es ese, no el tope
    let otro = nuevo(vec![m("opencode", RolEquipo::Ejecutor), m("claude-code", RolEquipo::Planificador)]);
    assert!(!error_de(e.bus.crear_equipo(otro, &e.a)).contains("orquestador.paralelo"));
}

#[test]
fn dormir_quita_la_presencia_de_planificadores_y_revisores_del_equipo() {
    let e = entorno();
    let eq = e.bus.crear_equipo(nuevo(miembros()), &e.a).unwrap();
    let pid = std::process::id();
    e.bus.anotar_presencia(&ea_bus::Presencia::nueva("claude-code", RolEquipo::Planificador, Some(&eq.id), pid)).unwrap();
    e.bus.anotar_presencia(&ea_bus::Presencia::nueva("antigravity", RolEquipo::Revisor, Some("otro"), pid)).unwrap();
    e.bus.dormir_equipo(&eq.id).unwrap();
    assert_eq!(e.bus.presencia_de("claude-code").unwrap(), None);
    assert!(e.bus.presencia_de("antigravity").unwrap().is_some(), "la de otro equipo no se toca");
}

#[test]
fn la_vista_del_equipo_dice_presencia_cola_y_ultimo_informe() {
    let e = entorno();
    let eq = e.bus.crear_equipo(nuevo(miembros()), &e.a).unwrap();
    e.bus.anotar_presencia(&ea_bus::Presencia::nueva("claude-code", RolEquipo::Planificador, Some(&eq.id), std::process::id())).unwrap();
    // pid 0 nunca cuenta como vivo: una presencia sin pid fiable no es presencia
    e.bus.anotar_presencia(&ea_bus::Presencia::nueva("antigravity", RolEquipo::Revisor, Some(&eq.id), 0)).unwrap();
    let id = entregar(&e, &eq.id, Some("antigravity"), "codex");
    let otro = e.bus.crear_trabajo(Orden { equipo: Some(eq.id.clone()), asignado_a: Some("codex".into()), ..orden(e.d.path()) }).unwrap();
    let v = e.bus.vista_equipo(&eq.id, 10).unwrap();
    let miembro = |a: &str| v.miembros.iter().find(|m| m.agente == a).unwrap();
    assert!(miembro("claude-code").viva && !miembro("antigravity").viva && !miembro("codex").viva);
    assert_eq!(miembro("antigravity").cola, std::slice::from_ref(&id));
    assert_eq!(miembro("codex").cola, std::slice::from_ref(&otro));
    assert_eq!((v.trabajos_total, v.trabajos.len()), (2, 2));
    let t = &v.trabajos[0];
    assert_eq!((t.id.as_str(), t.estado), (id.as_str(), EstadoTrabajo::EnRevision));
    let inf = t.informe.as_ref().unwrap();
    assert_eq!(inf.resultado, ResultadoInforme::Terminado);
    assert_eq!(inf.ejecutado_por.as_ref().unwrap().agente, "codex");
    assert_eq!(v.trabajos[1].informe, None);
    assert_eq!(e.bus.vista_equipo(&eq.id, 1).unwrap().trabajos[0].id, otro, "acotada a los últimos");
    let r = e.bus.resumen_equipos().unwrap();
    assert_eq!((r[0].trabajos, r[0].miembros.iter().filter(|m| m.viva).count()), (2, 1));
}
