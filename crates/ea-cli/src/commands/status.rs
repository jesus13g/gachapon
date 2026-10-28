use anyhow::Result;
use comfy_table::Table;
use ea_bus::{Bus, EstadoTrabajo, Rol};
use ea_store::{EstadoPropuesta, EventFilter, PropuestaFilter, Store};

use super::fmt_ts;

/// Salud del orquestador según su último latido, el que él mismo escribe en
/// `orquestador.json` del bus (§13.2; U-07). Nunca inventa un latido contra un
/// proceso que no responde: si no hay ninguno, dice que no hay señal, no que
/// esté "caído". Vivo, dice también cuántos trabajadores contó.
pub fn estado_orquestador() -> String {
    let a = ea_ajustes::cargar();
    let estado = ea_bus::estado_orquestador(&a.rutas.estado, a.umbral_latido_s());
    let texto = estado.descripcion();
    match (estado.ultimo_latido, estado.trabajadores) {
        (Some(t), Some(n)) if estado.vivo => format!("{texto} (heartbeat {}, {n} trabajador{})", fmt_ts(t), if n == 1 { "" } else { "es" }),
        (Some(t), _) => format!("{texto} (último heartbeat {})", fmt_ts(t)),
        _ => texto.to_string(),
    }
}

/// Resumen no interactivo del estado del sistema: una sola invocación,
/// apta para mostrarse al iniciar sesión (§12.4 de la especificación).
pub fn mostrar(store: &Store) -> Result<()> {
    println!("Ecosistema de asistente — estado");
    println!("---------------------------------");
    println!("Orquestador: {}", estado_orquestador());
    let pendientes = store
        .list_propuestas(&PropuestaFilter { estado: Some(EstadoPropuesta::Pendiente), limit: None })?
        .len();
    println!("Propuestas pendientes: {pendientes}");
    println!("Base de datos: {}", ea_ajustes::cargar().rutas.db.display());
    for l in lineas_roles(&ea_ajustes::cargar()) {
        println!("{l}");
    }
    println!();

    mostrar_bus();

    println!("\nÚltimos eventos:");
    let events = store.list_events(&EventFilter {
        limit: Some(5),
        ..Default::default()
    })?;
    if events.is_empty() {
        println!("  (sin eventos todavía)");
    } else {
        let mut table = Table::new();
        table.set_header(vec!["Cuándo", "Actor", "Acción", "Trabajo"]);
        for e in events {
            table.add_row(vec![
                fmt_ts(e.ts),
                e.actor,
                e.action,
                e.trabajo.unwrap_or_default(),
            ]);
        }
        println!("{table}");
    }

    Ok(())
}

/// «planificador: Claude Code (el suyo propio)», «ejecutor: Qwen Code
/// (qwen3-coder-30b)»: quién ocupa cada rol ahora.
pub fn lineas_roles(a: &ea_ajustes::Ajustes) -> Vec<String> {
    Rol::TODOS
        .into_iter()
        .map(|r| {
            let (id, _) = a.agente_de(r);
            let nombre = ea_ajustes::ficha(id).map_or(id, |f| f.nombre);
            let modelo = a.modelo_de(id).map_or("su modelo propio".to_string(), |m| m.nombre.clone());
            format!("{}: {nombre} ({modelo})", r.nombre())
        })
        .collect()
}

/// Trabajos por estado, colas y semáforos del bus. Si el bus no se puede
/// leer, lo dice en vez de fallar: `ea status` es un banner de login.
fn mostrar_bus() {
    let Ok(bus) = Bus::abrir() else {
        println!("Bus de trabajos: (no se puede leer el estado)");
        return;
    };
    let cuenta = bus.conteo_por_estado();
    println!("Trabajos por estado:");
    let mut table = Table::new();
    table.set_header(vec!["Estado", "Cantidad"]);
    for estado in EstadoTrabajo::TODOS {
        let n = cuenta.iter().find(|(e, _)| *e == estado).map_or(0, |(_, n)| *n);
        table.add_row(vec![estado.as_str().to_string(), n.to_string()]);
    }
    println!("{table}");

    for a in Rol::TODOS {
        let n = bus.listar_cola(a).map(|c| c.len()).unwrap_or(0);
        println!("Cola {} ({}): {n}", a.nombre(), a.as_str());
    }
    mostrar_presencias(&bus);
    let semaforos = ea_bus::listar_semaforos();
    if semaforos.is_empty() {
        println!("Semáforos: ningún worktree ocupado");
    }
    for (_, o) in semaforos {
        println!("Semáforo: {} ocupa un worktree con el trabajo {} (pid {}, desde {})", o.agente, o.trabajo, o.pid, o.desde);
    }
}

/// Quién está despierto en los equipos y en qué trabaja; vacío si no hay
/// presencias ni equipos despiertos.
fn mostrar_presencias(bus: &Bus) {
    for l in lineas_presencia(bus) {
        println!("{l}");
    }
}

/// Una línea por equipo despierto y por presencia anotada.
pub fn lineas_presencia(bus: &Bus) -> Vec<String> {
    let mut v = Vec::new();
    let equipos = bus.resumen_equipos().unwrap_or_default();
    for e in equipos.iter().filter(|e| e.estado == ea_bus::EstadoEquipo::Despierto) {
        v.push(format!("Equipo {} ({}): {} trabajo(s)", e.id, e.titulo, e.trabajos));
    }
    for (p, viva) in bus.presencias() {
        let donde = p.equipo.as_deref().map_or(String::new(), |e| format!(" en el equipo {e}"));
        let estado = match (viva, &p.trabajo) {
            (true, Some(t)) => format!("despierto, trabajando en {t}"),
            (true, None) => "despierto, libre".into(),
            (false, _) => format!("presencia caducada (último latido {})", p.latido),
        };
        v.push(format!("  {} ({}){donde}: {estado}", p.agente, p.rol.as_str()));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::lineas_roles;
    use ea_ajustes::{Ajustes, Fuentes};

    #[test]
    fn las_lineas_de_roles_dicen_agente_y_modelo() {
        let d = tempfile::tempdir().unwrap();
        let mut a = Ajustes::por_defecto(&Fuentes::aislada(d.path()));
        assert_eq!(lineas_roles(&a), ["planificador: Claude Code (su modelo propio)", "ejecutor: Qwen Code (qwen3-coder-30b)"]);
        a.roles.ejecutor = "codex".into();
        assert_eq!(lineas_roles(&a)[1], "ejecutor: Codex (qwen3-coder-30b)");
    }
}
