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

use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use comfy_table::Table;
use ea_ajustes::ficha;
use ea_bus::{Bus, EstadoEquipo, Latido, Miembro, NuevoEquipo, Presencia, RolEquipo, VistaEquipo, INTERVALO_LATIDO_S};

use super::limpio;

/// Cuántos trabajos del equipo enseña `ver`.
const TRABAJOS_VISTOS: usize = 20;

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
    ea_acciones::terminal_interactiva()?;
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
    let worktree = ea_plataforma::rutas::canonica(worktree).with_context(|| format!("el worktree {worktree:?} no existe"))?;
    if !worktree.is_dir() {
        bail!("el worktree {} no es una carpeta", worktree.display());
    }
    let ajustes = ea_ajustes::cargar();
    let path = std::env::var_os("PATH");
    let (lista, omitidos) = if todos {
        ea_bus::repartir_todos(&ajustes, path.as_deref())?
    } else {
        (miembros.iter().map(|m| ea_bus::parse_miembro(m)).collect::<Result<Vec<_>, _>>()?, vec![])
    };
    ea_bus::comprobar_disponibles(&lista, &ajustes, path.as_deref())?;
    let equipo = super::trabajo::bus()?.crear_equipo(
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
    ea_acciones::terminal_interactiva()?;
    let e = super::trabajo::bus()?.dormir_equipo(id)?;
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
        t.add_row(vec![e.id.clone(), e.estado.as_str().into(), limpio(&e.titulo), miembros.join(", "), e.trabajos.to_string()]);
    }
    println!("{t}");
    Ok(())
}

pub fn ver(id: &str, json: bool) -> Result<()> {
    let v = Bus::abrir()?.vista_equipo(id, TRABAJOS_VISTOS)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    // Los títulos, informes y avisos de los trabajos los escriben los agentes.
    print!("{}", limpio(&texto_equipo(&v)));
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
/// terminal que abre `despertar`. Anota la presencia con su pid y la mantiene
/// con un [`Latido`] (deja de latir si el equipo duerme), ejecuta el agente con
/// su prompt y quita la presencia al terminar.
pub fn sesion(id: &str, agente: &str) -> Result<()> {
    ea_acciones::terminal_interactiva()?;
    let bus = super::trabajo::bus()?;
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
    let presencia = Presencia::nueva(agente, miembro.rol, Some(id), std::process::id());
    let equipo_id = id.to_string();
    let latido = Latido::anotar(&bus, &presencia, Duration::from_secs(INTERVALO_LATIDO_S), move |b| {
        b.leer_equipo(&equipo_id).is_ok_and(|e| e.estado == EstadoEquipo::Despierto)
    })?;
    let ajustes = ea_ajustes::cargar();
    let bin = ajustes.agentes.get(agente).map(|g| g.bin.clone()).unwrap_or_default();
    let ficha = ficha(agente).expect("con la presencia anotada, es del catálogo");
    let argv = ficha.argv_interactivo(&bin, &prompt_sesion(&equipo, &miembro));
    let estado = Command::new(&argv[0]).args(&argv[1..]).current_dir(&equipo.worktree_base).status();
    latido.soltar()?;
    estado.with_context(|| format!("no se pudo lanzar {}", argv[0]))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
