//! `ea servicios ver|instalar|desinstalar`: los servicios residentes
//! (`ea-mcp`, `ea-orchestrator`, `ea-web`) en el gestor del sistema, con la
//! ruta real de los binarios en vez de la ruta fija de las plantillas
//! (spec 020, decisión 5). `ver` solo cuenta; instalar y desinstalar cambian
//! el sistema, así que exigen una terminal interactiva real y la confirmación
//! escrita del usuario, como el resto de lo mutante de la CLI.

use anyhow::{bail, Result};
use ea_acciones::servicios::{aplicar, correr_orden, plan_desinstalar, plan_instalar, Backend, Entorno, Paso};
use ea_acciones::{Canal, GestorServicios, SERVICIOS_INSTALABLES};

use super::{bloquear, preguntar, respuesta_confirma};

fn gestor(b: Backend) -> &'static str {
    match b {
        Backend::Systemd => "systemd --user (unidades en ~/.config/systemd/user)",
        Backend::Launchd => "launchd (LaunchAgents en ~/Library/LaunchAgents)",
        Backend::Schtasks => "Programador de tareas de Windows (tareas «ecosistema-asistente\\…» al iniciar sesión)",
    }
}

fn mostrar(env: &Entorno, pasos: &[Paso]) {
    println!("Gestor:    {}", gestor(env.backend));
    println!("Binarios:  {}", env.exes_dir.display());
    if env.backend == Backend::Launchd {
        // launchd arranca los agentes con un PATH mínimo: se fija el de ahora.
        println!("PATH:      {}", env.path.as_deref().unwrap_or("(sin PATH: los agentes usarán el de launchd)"));
        println!("           (si lo cambias, vuelve a ejecutar `ea servicios instalar`)");
    }
    if env.backend == Backend::Schtasks {
        println!("Usuario:   {}", env.usuario.as_deref().unwrap_or("(desconocido: la tarea valdrá para cualquier usuario)"));
    }
    println!("Pasos:");
    for p in pasos {
        let nota = match p {
            Paso::Escribir { ruta, .. } | Paso::EscribirUtf16 { ruta, .. } if ruta.exists() => " (sustituye el que hay)",
            _ => "",
        };
        println!("  - {}{nota}", p.describir());
    }
}

/// Qué haría `instalar` y cómo está cada servicio ahora. No cambia nada.
pub fn ver() -> Result<()> {
    let env = Entorno::usuario().map_err(anyhow::Error::msg)?;
    let pasos = plan_instalar(&env).map_err(anyhow::Error::msg)?;
    println!("ea servicios instalar haría esto en {}:\n", ea_plataforma::Sistema::actual().nombre());
    mostrar(&env, &pasos);
    let faltan = env.faltan();
    if !faltan.is_empty() {
        println!("\nFaltan binarios (compílalos con `cargo build --release`):");
        for f in &faltan {
            println!("  - {}", f.display());
        }
    }
    println!("\nEstado ahora:");
    let g = GestorServicios::usuario();
    for n in SERVICIOS_INSTALABLES {
        let e = bloquear(g.estado(n));
        match e {
            Ok(e) if e.cargado => println!("  {n:<16} {}", e.activo),
            Ok(_) => println!("  {n:<16} no instalado"),
            Err(m) => println!("  {n:<16} desconocido ({m})"),
        }
    }
    Ok(())
}

fn con_confirmacion(titulo: &str, env: &Entorno, pasos: &[Paso]) -> Result<()> {
    // Lo primero: una persona delante de un teclado, no un agente con `ea`
    // como subproceso.
    Canal::terminal_interactiva()?;
    println!("{titulo} en {}:\n", ea_plataforma::Sistema::actual().nombre());
    mostrar(env, pasos);
    let r = preguntar("\n  ¿Hacerlo ahora? [s/N]: ")?;
    if !respuesta_confirma(false, "servicios", &r) {
        println!("Cancelado: no se ha hecho nada.");
        return Ok(());
    }
    let hecho = aplicar(pasos, &mut |argv| correr_orden(argv)).map_err(anyhow::Error::msg)?;
    println!("Hecho ({} pasos).", hecho.len());
    Ok(())
}

/// Escribe las unidades con la ruta real y arranca los servicios.
pub fn instalar() -> Result<()> {
    Canal::terminal_interactiva()?;
    let env = Entorno::usuario().map_err(anyhow::Error::msg)?;
    let faltan = env.faltan();
    if !faltan.is_empty() {
        let lista = faltan.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", ");
        bail!("faltan binarios junto a este `ea` ({lista}); compílalos con `cargo build --release`");
    }
    let pasos = plan_instalar(&env).map_err(anyhow::Error::msg)?;
    con_confirmacion("Se instalarán los servicios", &env, &pasos)
}

/// Para y quita los servicios (no toca las rutinas programadas).
pub fn desinstalar() -> Result<()> {
    Canal::terminal_interactiva()?;
    let env = Entorno::usuario().map_err(anyhow::Error::msg)?;
    let pasos = plan_desinstalar(&env).map_err(anyhow::Error::msg)?;
    con_confirmacion("Se desinstalarán los servicios", &env, &pasos)
}
