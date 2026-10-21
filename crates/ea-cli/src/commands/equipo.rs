//! `ea equipo`: equipos de agentes (spec 019). Despertar y dormir exigen una
//! terminal interactiva real, como `ea ajustes establecer`: ningún agente
//! puede despertar equipos. Listar y ver son de solo lectura.
//!
//! Despertar a un planificador o a un revisor abre una sesión interactiva del
//! agente en una terminal real (el lanzador de la 018). Esa terminal no
//! ejecuta el agente directamente, sino `ea equipo sesion <equipo> <agente>`
//! (oculto): ese proceso es el que anota la presencia con SU pid, la mantiene
//! con latidos y la quita al terminar. Así el pid es fiable (el lanzador de
//! terminales no devuelve el de la ventana) y la presencia caduca sola si la
//! terminal se cierra. Sin terminal (`--sin-terminal`, o si no hay lanzador)
//! esos miembros no tienen presencia hasta que alguien abra la sesión con ese
//! mismo comando.

use std::ffi::OsStr;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use comfy_table::Table;
use ea_ajustes::{ficha, Ajustes};
use ea_bus::{Bus, EstadoEquipo, Miembro, NuevoEquipo, Presencia, RolEquipo, VistaEquipo, INTERVALO_LATIDO_S};

/// Cuántos trabajos del equipo enseña `ver`.
const TRABAJOS_VISTOS: usize = 20;

fn bus() -> Result<Bus> {
    super::trabajo::bus()
}

/// `agente:rol` → miembro. El rol es `planificador`, `ejecutor` o `revisor`.
pub fn parse_miembro(texto: &str) -> Result<Miembro> {
    ea_ajustes::parse_miembro(texto).map_err(|e| anyhow!(e))
}

/// «Despertar a todos»: el reparto compartido con la web
/// ([`ea_ajustes::repartir_todos`]).
pub fn repartir_todos(a: &Ajustes, path: Option<&OsStr>) -> Result<(Vec<Miembro>, Vec<String>)> {
    ea_ajustes::repartir_todos(a, path).map_err(|e| anyhow!(e))
}

/// El argv que abre la sesión de un agente con un prompt inicial; interactivo
/// (el prompt no cierra la sesión). Sin herramientas ni opciones extra: es la
/// configuración habitual del agente.
pub fn argv_agente(agente: &str, bin: &str, prompt: &str) -> Vec<String> {
    let (b, p) = (bin.to_string(), prompt.to_string());
    match agente {
        "qwen-code" | "antigravity" => vec![b, "-i".into(), p],
        "opencode" => vec![b, "--prompt".into(), p],
        _ => vec![b, p],
    }
}

/// El prompt inicial de una sesión de equipo: corto, apunta al equipo y a su
/// cola.
pub fn prompt_sesion(equipo: &ea_bus::Equipo, miembro: &Miembro) -> String {
    let nombre = ficha(&miembro.agente).map_or(miembro.agente.as_str(), |f| f.nombre);
    let id = &equipo.id;
    let tarea = match miembro.rol {
        RolEquipo::Revisor => format!(
            "Eres revisor: tu cola de revisión sale en tu fila de `ea equipo ver {id}`. Revisa cada entrega con `ea trabajo ver <id>` y `ea trabajo diff <id>`; solo propones, no cierras ni reformulas."
        ),
        _ => format!(
            "Eres planificador: reparte el trabajo con `ea trabajo crear --equipo {id} --asignar <ejecutor> [--revisor <agente>] [--worktree-propio] ...` y revisa las entregas de tus ejecutores."
        ),
    };
    format!(
        "Eres {nombre} en el equipo «{}» ({id}) del ecosistema. Objetivo: {}. Lee `ea equipo ver {id}` (miembros, presencia, trabajos y colas). {tarea} El procedimiento está en `agentes/flujo/equipo.md` del repo (skill `equipo` o `ea-equipo`).",
        equipo.titulo, equipo.objetivo
    )
}

#[allow(clippy::too_many_arguments)]
pub fn despertar(miembros: &[String], todos: bool, titulo: &str, objetivo: &str, worktree: &str, sin_terminal: bool) -> Result<()> {
    // Lo primero: una persona delante de un teclado, no un agente con `ea`
    // como subproceso.
    ea_acciones::Canal::terminal_interactiva()?;
    despertar_sin_tty(miembros, todos, titulo, objetivo, worktree, sin_terminal, &mut |argv| {
        ea_acciones::terminal::lanzar_en_terminal(argv)
    })
}

/// [`despertar`] sin la comprobación de TTY y con el lanzador de terminal
/// inyectado (para probarlo).
pub fn despertar_sin_tty(
    miembros: &[String],
    todos: bool,
    titulo: &str,
    objetivo: &str,
    worktree: &str,
    sin_terminal: bool,
    abrir: &mut dyn FnMut(&[String]) -> Result<(), String>,
) -> Result<()> {
    if todos == !miembros.is_empty() {
        bail!("indica --todos o uno o más --miembro agente:rol (no las dos cosas)");
    }
    let worktree = std::fs::canonicalize(worktree).with_context(|| format!("el worktree {worktree:?} no existe"))?;
    if !worktree.is_dir() {
        bail!("el worktree {} no es una carpeta", worktree.display());
    }
    let ajustes = ea_ajustes::cargar();
    let path = std::env::var_os("PATH");
    let (lista, omitidos) = if todos {
        repartir_todos(&ajustes, path.as_deref())?
    } else {
        (miembros.iter().map(|m| parse_miembro(m)).collect::<Result<Vec<_>>>()?, vec![])
    };
    ea_ajustes::comprobar_disponibles(&lista, &ajustes, path.as_deref()).map_err(|e| anyhow!(e))?;
    let equipo = bus()?.crear_equipo(
        NuevoEquipo {
            titulo: titulo.into(),
            objetivo: objetivo.into(),
            worktree_base: worktree.to_string_lossy().into(),
            miembros: lista,
            creado_por: "ea-cli".into(),
        },
        &ajustes,
    )?;
    println!("Equipo {} despierto: {}", equipo.id, equipo.titulo);
    for m in &equipo.miembros {
        println!("  {:<12} {}", m.agente, m.rol.as_str());
    }
    for o in omitidos {
        println!("  (omitido: {o})");
    }
    let exe = std::env::current_exe().context("no se pudo saber qué binario `ea` está corriendo")?.display().to_string();
    for m in equipo.miembros.iter().filter(|m| m.rol != RolEquipo::Ejecutor) {
        let argv = vec![exe.clone(), "equipo".into(), "sesion".into(), equipo.id.clone(), m.agente.clone()];
        if sin_terminal {
            println!("  sesión de {} sin abrir; para abrirla: {}", m.agente, argv.join(" "));
            continue;
        }
        match abrir(&argv) {
            Ok(()) => println!("  sesión de {} abierta en una terminal", m.agente),
            Err(e) => println!("  no se pudo abrir la sesión de {} ({e}); para abrirla a mano: {}", m.agente, argv.join(" ")),
        }
    }
    println!("Los ejecutores empiezan a trabajar cuando el orquestador lo recoge (ea-orchestrator en marcha).");
    Ok(())
}

pub fn dormir(id: &str) -> Result<()> {
    ea_acciones::Canal::terminal_interactiva()?;
    let e = bus()?.dormir_equipo(id)?;
    println!("Equipo {} dormido: lo pendiente de sus ejecutores vuelve a la cola de modelo-local.", e.id);
    Ok(())
}

pub fn listar(json: bool) -> Result<()> {
    let v = Bus::abrir()?.resumen_equipos()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    if v.is_empty() {
        println!("(ningún equipo)");
        return Ok(());
    }
    let mut t = Table::new();
    t.set_header(vec!["Equipo", "Estado", "Título", "Miembros (* despierto)", "Trabajos"]);
    for e in &v {
        let miembros: Vec<String> =
            e.miembros.iter().map(|m| format!("{}:{}{}", m.agente, m.rol.as_str(), if m.viva { "*" } else { "" })).collect();
        t.add_row(vec![e.id.clone(), estado_equipo(e.estado).into(), e.titulo.clone(), miembros.join(", "), e.trabajos.to_string()]);
    }
    println!("{t}");
    Ok(())
}

fn estado_equipo(e: EstadoEquipo) -> &'static str {
    e.as_str()
}

pub fn ver(id: &str, json: bool) -> Result<()> {
    let v = Bus::abrir()?.vista_equipo(id, TRABAJOS_VISTOS)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    print!("{}", texto_equipo(&v));
    Ok(())
}

/// La ficha de un equipo para la terminal.
pub fn texto_equipo(v: &VistaEquipo) -> String {
    let mut s = format!(
        "equipo    {} ({})\ntitulo    {}\nobjetivo  {}\nworktree  {}\n\nmiembros\n",
        v.id,
        v.estado.as_str(),
        v.titulo,
        v.objetivo,
        v.worktree_base
    );
    for m in &v.miembros {
        let presencia = match (&m.presencia, m.viva) {
            (Some(_), true) => match &m.trabajo_actual {
                Some(t) => format!("despierto, en {t}"),
                None => "despierto, libre".into(),
            },
            (Some(p), false) => format!("caducada (último latido {})", p.latido),
            (None, _) => "sin presencia".into(),
        };
        let cola = if m.cola.is_empty() {
            String::new()
        } else {
            let que = if m.rol == RolEquipo::Ejecutor { "órdenes" } else { "revisión" };
            format!(" · {que} en cola: {}", m.cola.join(", "))
        };
        s += &format!("  {:<12} {:<12} {presencia}{cola}\n", m.agente, m.rol.as_str());
    }
    s += &format!("\ntrabajos ({} de {})\n", v.trabajos.len(), v.trabajos_total);
    if v.trabajos.is_empty() {
        s += "  (ninguno)\n";
    }
    for t in &v.trabajos {
        let mut extra = String::new();
        if let Some(a) = &t.asignado_a {
            extra += &format!(" · asignado a {a}");
        }
        if let Some(r) = &t.revisor {
            extra += &format!(" · revisor {r}");
        }
        s += &format!("  {}  {:<12} {}{extra}\n", t.id, t.estado.as_str(), t.titulo);
        if let Some(i) = &t.informe {
            let por = i.ejecutado_por.as_ref().map_or(String::new(), |e| format!(" por {} ({})", e.agente, e.modelo));
            let resumen = if i.resumen.trim().is_empty() { String::new() } else { format!(" · {}", i.resumen) };
            s += &format!("      informe: {:?}{por}{resumen}\n", i.resultado);
            if let Some(a) = &i.aviso {
                s += &format!("      aviso: {a}\n");
            }
        }
    }
    s
}

/// `ea equipo sesion <equipo> <agente>` (oculto): lo que corre dentro de la
/// terminal que abre `despertar`. Anota la presencia con su pid, late en un
/// hilo, ejecuta el agente con su prompt y quita la presencia al terminar.
pub fn sesion(id: &str, agente: &str) -> Result<()> {
    ea_acciones::Canal::terminal_interactiva()?;
    let bus = bus()?;
    let equipo = bus.leer_equipo(id)?;
    if equipo.estado != EstadoEquipo::Despierto {
        bail!("el equipo {id} está dormido");
    }
    let miembro = equipo.miembro(agente).filter(|m| m.rol != RolEquipo::Ejecutor).cloned();
    let Some(miembro) = miembro else {
        bail!("{agente} no es un planificador ni un revisor del equipo {id}");
    };
    if let Some((p, true)) = bus.presencias().into_iter().find(|(p, _)| p.agente == agente) {
        bail!("{agente} ya está despierto (pid {}, equipo {})", p.pid, p.equipo.as_deref().unwrap_or("—"));
    }
    let pid = std::process::id();
    bus.anotar_presencia(&Presencia::nueva(agente, miembro.rol, Some(id), pid))?;
    let parar = Arc::new(AtomicBool::new(false));
    let latidos = {
        let (parar, id, agente) = (Arc::clone(&parar), id.to_string(), agente.to_string());
        std::thread::spawn(move || {
            while !parar.load(Ordering::Relaxed) {
                for _ in 0..INTERVALO_LATIDO_S * 10 {
                    if parar.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                let Ok(b) = Bus::abrir() else { continue };
                // Si el equipo durmió (o la presencia ya no es de esta sesión), deja de latir.
                let sigue = b.leer_equipo(&id).is_ok_and(|e| e.estado == EstadoEquipo::Despierto)
                    && b.presencia_de(&agente).ok().flatten().is_some_and(|p| p.pid == pid);
                if !sigue {
                    return;
                }
                let _ = b.latir(&agente, None);
            }
        })
    };
    let ajustes = ea_ajustes::cargar();
    let bin = ajustes.agentes.get(agente).map(|g| g.bin.clone()).unwrap_or_default();
    let argv = argv_agente(agente, &bin, &prompt_sesion(&equipo, &miembro));
    let estado = Command::new(&argv[0]).args(&argv[1..]).current_dir(&equipo.worktree_base).status();
    parar.store(true, Ordering::Relaxed);
    let _ = latidos.join();
    // Solo se quita la presencia propia: otra sesión no se pisa.
    if bus.presencia_de(agente)?.is_some_and(|p| p.pid == pid) {
        bus.quitar_presencia(agente)?;
    }
    estado.with_context(|| format!("no se pudo lanzar {}", argv[0]))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ea_ajustes::{Fuentes, IDS_AGENTES};

    fn ajustes() -> (tempfile::TempDir, Ajustes) {
        let d = tempfile::tempdir().unwrap();
        let a = Ajustes::por_defecto(&Fuentes::aislada(d.path()));
        (d, a)
    }

    /// Un directorio con binarios de mentira para los ids dados.
    fn bin(ids: &[&str], a: &Ajustes) -> (tempfile::TempDir, std::ffi::OsString) {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        for id in ids {
            let p = d.path().join(&a.agentes[*id].bin);
            std::fs::write(&p, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = d.path().as_os_str().to_owned();
        (d, path)
    }

    #[test]
    fn parsea_agente_y_rol() {
        assert_eq!(parse_miembro("codex:ejecutor").unwrap(), Miembro { agente: "codex".into(), rol: RolEquipo::Ejecutor });
        assert_eq!(parse_miembro(" antigravity : revisor ").unwrap().rol, RolEquipo::Revisor);
        assert!(parse_miembro("codex").unwrap_err().to_string().contains("agente:rol"));
        assert!(parse_miembro("nadie:ejecutor").unwrap_err().to_string().contains("catálogo"));
        assert!(parse_miembro("codex:jefe").unwrap_err().to_string().contains("rol de equipo"));
    }

    #[test]
    fn todos_reparte_segun_lo_que_admite_cada_agente() {
        let (_d, a) = ajustes();
        let (_b, path) = bin(&IDS_AGENTES.to_vec(), &a);
        let (m, omitidos) = repartir_todos(&a, Some(&path)).unwrap();
        let rol = |id: &str| m.iter().find(|x| x.agente == id).map(|x| x.rol);
        assert_eq!(rol("claude-code"), Some(RolEquipo::Planificador));
        assert_eq!(rol("antigravity"), Some(RolEquipo::Planificador));
        assert_eq!(rol("qwen-code"), Some(RolEquipo::Ejecutor));
        assert_eq!(rol("codex"), Some(RolEquipo::Ejecutor));
        assert_eq!(rol("opencode"), Some(RolEquipo::Ejecutor));
        assert!(omitidos.is_empty());
        // El que ocupa roles.planificador va a planificador aunque admita ejecutor.
        let mut b = a.clone();
        b.roles.planificador = "codex".into();
        let (m, _) = repartir_todos(&b, Some(&path)).unwrap();
        assert_eq!(m.iter().find(|x| x.agente == "codex").unwrap().rol, RolEquipo::Planificador);
    }

    #[test]
    fn todos_omite_lo_que_no_esta_y_dice_lo_imposible() {
        let (_d, a) = ajustes();
        let (_b, path) = bin(&["claude-code", "qwen-code"], &a);
        let (m, omitidos) = repartir_todos(&a, Some(&path)).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(omitidos.len(), 3);
        assert!(omitidos.iter().any(|o| o.contains("Codex") && o.contains("no está disponible")));
        // sin el planificador de los roles
        let (_b2, path) = bin(&["qwen-code"], &a);
        assert!(repartir_todos(&a, Some(&path)).unwrap_err().to_string().contains("planificador"));
        // un ejecutor con modelo remoto es imposible
        let (_b3, path) = bin(&["claude-code", "qwen-code", "codex"], &a);
        let mut r = a.clone();
        let mut remoto = r.modelos[&r.agentes["codex"].modelo].clone();
        remoto.remoto = true;
        r.modelos.insert("remoto".into(), remoto);
        r.agentes.get_mut("codex").unwrap().modelo = "remoto".into();
        let e = repartir_todos(&r, Some(&path)).unwrap_err().to_string();
        assert!(e.contains("codex") && e.contains("remoto") && e.contains("--miembro"), "{e}");
    }

    #[test]
    fn el_argv_de_cada_agente_lleva_el_prompt_interactivo() {
        assert_eq!(argv_agente("claude-code", "claude", "hola"), ["claude", "hola"]);
        assert_eq!(argv_agente("qwen-code", "qwen", "hola"), ["qwen", "-i", "hola"]);
        assert_eq!(argv_agente("opencode", "opencode", "hola"), ["opencode", "--prompt", "hola"]);
        assert_eq!(argv_agente("antigravity", "agy", "hola"), ["agy", "-i", "hola"]);
        assert_eq!(argv_agente("codex", "codex", "hola"), ["codex", "hola"]);
    }

    #[test]
    fn el_prompt_apunta_al_equipo_y_a_la_cola() {
        let e = ea_bus::Equipo {
            id: "e1".into(),
            titulo: "T".into(),
            objetivo: "O".into(),
            worktree_base: "/r".into(),
            miembros: vec![],
            estado: EstadoEquipo::Despierto,
            creado_en: String::new(),
            creado_por: String::new(),
            trabajos: vec![],
        };
        let rev = prompt_sesion(&e, &Miembro { agente: "antigravity".into(), rol: RolEquipo::Revisor });
        assert!(rev.contains("ea equipo ver e1") && rev.contains("cola de revisión") && rev.contains("Antigravity") && rev.contains("O"));
        let pla = prompt_sesion(&e, &Miembro { agente: "claude-code".into(), rol: RolEquipo::Planificador });
        assert!(pla.contains("--equipo e1 --asignar"));
        assert!(pla.contains("agentes/flujo/equipo.md") && rev.contains("agentes/flujo/equipo.md"));
        assert!(!pla.starts_with('-') && !rev.starts_with('-'));
    }
}
