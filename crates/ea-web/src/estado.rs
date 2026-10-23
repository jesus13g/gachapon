//! Salud de los servicios residentes, preguntada al gestor de servicios del
//! sistema (`systemd --user`, launchd o el Programador de tareas). Todo es de
//! mejor esfuerzo: si el gestor no está o no responde, se devuelve
//! "desconocido" en vez de fallar (§12.4: degradación limpia).

use ea_acciones::GestorServicios;

/// Los únicos servicios que la interfaz puede consultar y reiniciar.
pub const SERVICIOS: [&str; 3] = ["llama-server", "ea-mcp", "ea-orchestrator"];

pub struct Servicio {
    pub nombre: &'static str,
    /// `active`, `inactive`, `failed`… o `desconocido`.
    pub activo: String,
    /// Segundos que lleva activo. Se calcula con el reloj monotónico y no
    /// con `ActiveEnterTimestamp`: esta máquina alterna con Windows y arranca
    /// con la hora de pared desajustada hasta que NTP la corrige, así que las
    /// marcas de pared de los servicios pueden quedar en el futuro. Solo con
    /// systemd; launchd y el Programador de tareas no dan el dato (`None`).
    pub activo_s: Option<i64>,
}

pub async fn servicios(consultar: bool) -> Vec<Servicio> {
    let desconocido = || {
        SERVICIOS
            .iter()
            .map(|&nombre| Servicio { nombre, activo: "desconocido".into(), activo_s: None })
            .collect::<Vec<_>>()
    };
    if !consultar {
        return desconocido();
    }
    let Some(info) = GestorServicios::usuario().panel(&SERVICIOS).await else {
        return desconocido();
    };
    SERVICIOS
        .iter()
        .map(|&nombre| {
            let i = info.iter().find(|i| i.nombre == nombre);
            Servicio {
                nombre,
                activo: i.map_or_else(|| "desconocido".into(), |i| i.activo.clone()),
                activo_s: i.and_then(|i| i.activo_s),
            }
        })
        .collect()
}
