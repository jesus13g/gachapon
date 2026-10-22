use super::*;

fn roles() -> Roles {
    Roles { planificador: "claude-code".into(), ejecutor: "qwen-code".into() }
}

fn maquina(id: &str, agente: &str, fuente: &'static str, rol: &str, pistas: &[&str]) -> AgenteVivo {
    AgenteVivo {
        id: id.into(),
        agente: Some(agente.into()),
        nombre: agente.into(),
        familia: familia::familia(Some(agente), None, false),
        modelo: None,
        local: false,
        rol: Some(rol.into()),
        padre: None,
        estado: EstadoAgente::Esperando,
        actividad: None,
        trabajo: None,
        proyecto: None,
        desde: Some(1),
        fuente,
        equipo: None,
        pistas: pistas.iter().map(|p| p.to_string()).collect(),
    }
}

fn claude(id: &str, cwd: &str, sesion: &str) -> AgenteVivo {
    maquina(id, "claude-code", "claude-code", "planificador", &[cwd, sesion])
}

fn qwen() -> AgenteVivo {
    maquina("agente:qwen-code", "qwen-code", "presencia", "ejecutor", &[])
}

fn trabajo(id: &str, estado: &str, worktree: &str) -> TrabajoBus {
    TrabajoBus { id: id.into(), estado: estado.into(), titulo: format!("trabajo {id}"), worktree: worktree.into(), ..Default::default() }
}

fn lugar<'a>(e: &'a Ecosistema, t: &str) -> &'a Tarea {
    e.tareas.iter().find(|x| x.trabajo == t).unwrap()
}

#[test]
fn sin_agentes_no_hay_maquinas_y_los_trabajos_esperan_en_el_bus() {
    let e = componer(vec![], &[trabajo("t1", "en_cola", "/p"), trabajo("t2", "en_revision", "/p")], &[], &[], &roles());
    assert!(e.agentes.is_empty());
    assert_eq!(lugar(&e, "t1").lugar, "bus");
    assert_eq!(lugar(&e, "t2").lugar, "bus");
}

#[test]
fn cada_estado_tiene_su_lugar_y_los_cerrados_no_salen() {
    let ag = vec![claude("claude:s1", "/home/u/proy", "s1"), qwen()];
    let ts = [
        trabajo("cola", "en_cola", "/home/u/proy"),
        trabajo("run", "ejecutando", "/home/u/proy/sub"),
        trabajo("rev", "en_revision", "/home/u/proy"),
        trabajo("esp", "en_espera", "/home/u/proy"),
        trabajo("fin", "cerrado", "/home/u/proy"),
    ];
    let sems = [SemaforoBus { agente: "qwen-code".into(), trabajo: "run".into() }];
    let e = componer(ag, &ts, &sems, &[], &roles());
    assert_eq!(e.tareas.len(), 4, "cerrados y cancelados van a la bandeja y la papelera");
    assert_eq!(lugar(&e, "cola").lugar, "agente:qwen-code");
    assert_eq!(lugar(&e, "run").lugar, "agente:qwen-code");
    assert_eq!(lugar(&e, "rev").lugar, "claude:s1");
    assert_eq!(lugar(&e, "esp").lugar, "usuario");
    assert_eq!(lugar(&e, "run").planificador.as_deref(), Some("claude:s1"));
    // El semáforo enciende al ejecutor con el título de su trabajo.
    let q = e.agentes.iter().find(|a| a.id == "agente:qwen-code").unwrap();
    assert_eq!((q.estado, q.trabajo.as_deref(), q.actividad.as_deref()), (EstadoAgente::Trabajando, Some("run"), Some("trabajo run")));
}

#[test]
fn con_varias_sesiones_gana_la_que_reconoce_el_worktree() {
    let ag = vec![
        claude("claude:a", "/home/u", "a"),
        claude("claude:b", "/home/u/proy", "b"),
        claude("claude:c", "/otra", "sesion-c"),
        qwen(),
    ];
    let ts = [
        trabajo("hondo", "en_revision", "/home/u/proy/x"),
        trabajo("arriba", "en_revision", "/home/u/otro"),
        trabajo("scratch", "en_revision", "/tmp/claude-1000/proj/sesion-c/scratchpad/humo"),
        trabajo("nadie", "en_revision", "/srv/nada"),
        trabajo("prefijo", "en_revision", "/home/u/proyecto"),
    ];
    let e = componer(ag, &ts, &[], &[], &roles());
    assert_eq!(lugar(&e, "hondo").lugar, "claude:b", "el cwd más cercano");
    assert_eq!(lugar(&e, "arriba").lugar, "claude:a");
    assert_eq!(lugar(&e, "scratch").lugar, "claude:c", "el worktree lleva el id de la sesión");
    assert_eq!(lugar(&e, "nadie").lugar, "bus", "ambiguo: no se inventa dueño");
    assert_eq!(lugar(&e, "prefijo").lugar, "claude:a", "/home/u/proy no es padre de /home/u/proyecto");
}

#[test]
fn una_sola_sesion_del_planificador_se_queda_lo_que_no_reconoce() {
    let e = componer(vec![claude("claude:s", "/home/u/proy", "s")], &[trabajo("t", "en_revision", "/srv/otro")], &[], &[], &roles());
    let t = lugar(&e, "t");
    assert_eq!((t.lugar.as_str(), t.ejecutor.as_deref()), ("claude:s", None), "sin ejecutor encendido");
}

#[test]
fn el_revisor_y_el_asignado_de_un_equipo_mandan() {
    let ag = vec![
        claude("claude:s", "/p", "s"),
        qwen(),
        maquina("agente:codex", "codex", "presencia", "ejecutor", &[]),
        maquina("agente:opencode", "opencode", "presencia", "revisor", &[]),
    ];
    let mut t = trabajo("t", "en_cola", "/p");
    t.asignado_a = Some("codex".into());
    t.revisor = Some("opencode".into());
    let mut r = t.clone();
    r.id = "r".into();
    r.estado = "en_revision".into();
    let e = componer(ag, &[t, r], &[], &[], &roles());
    assert_eq!(lugar(&e, "t").lugar, "agente:codex");
    assert_eq!(lugar(&e, "r").lugar, "agente:opencode");
    assert_eq!(lugar(&e, "r").planificador.as_deref(), Some("agente:opencode"));
}

#[test]
fn la_presencia_de_claude_se_funde_con_su_unica_sesion() {
    let mut p = maquina("agente:claude-code", "claude-code", "presencia", "revisor", &[]);
    p.equipo = Some("eq".into());
    let e = componer(vec![claude("claude:s", "/p", "s"), p.clone()], &[], &[], &[], &roles());
    assert_eq!(e.agentes.len(), 1);
    assert_eq!((e.agentes[0].rol.as_deref(), e.agentes[0].equipo.as_deref()), (Some("revisor"), Some("eq")));

    // Con dos sesiones no se sabe cuál es: se descarta la presencia sin tocar ninguna.
    let e = componer(vec![claude("claude:a", "/p", "a"), claude("claude:b", "/q", "b"), p], &[], &[], &[], &roles());
    assert_eq!(e.agentes.len(), 2);
    assert!(e.agentes.iter().all(|a| a.rol.as_deref() == Some("planificador")));
}

#[test]
fn las_senales_solo_salen_de_una_maquina_inequivoca() {
    let ev = |id, actor: &str| EventoBus { id, actor: actor.into(), accion: "mcp_leer_nota".into(), trabajo: None, error: false };
    let eventos = [ev(1, "claude-code"), ev(2, "orquestador"), ev(3, "usuario"), ev(4, "modelo-local"), ev(5, "desconocido")];
    let e = componer(vec![claude("claude:s", "/p", "s"), qwen()], &[], &[], &eventos, &roles());
    let de: Vec<_> = e.senales.iter().map(|s| s.de.as_deref()).collect();
    assert_eq!(de, [Some("claude:s"), None, Some("usuario"), Some("agente:qwen-code"), None]);

    let e = componer(vec![claude("claude:a", "/p", "a"), claude("claude:b", "/q", "b")], &[], &[], &eventos[..1], &roles());
    assert_eq!(e.senales[0].de, None, "con dos sesiones no se sabe cuál llamó");
}

#[test]
fn planificadores_primero_y_subagentes_al_final() {
    let mut sub = maquina("claude:s/a", "claude-code", "subagente", "subagente", &[]);
    sub.padre = Some("claude:s".into());
    let e = componer(vec![qwen(), sub, claude("claude:s", "/p", "s")], &[], &[], &[], &roles());
    let ids: Vec<_> = e.agentes.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["claude:s", "agente:qwen-code", "claude:s/a"]);
}
