//! Salud de los servicios residentes, preguntada a `systemd --user`. Todo
//! es de mejor esfuerzo: si `systemctl` no está o no responde, se devuelve
//! "desconocido" en vez de fallar (§12.4: degradación limpia).

use std::collections::HashMap;
use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

/// Los únicos servicios que la interfaz puede consultar y reiniciar.
pub const SERVICIOS: [&str; 3] = ["llama-server", "ea-mcp", "ea-orchestrator"];

const LIMITE: Duration = Duration::from_secs(3);

pub struct Servicio {
    pub nombre: &'static str,
    /// `active`, `inactive`, `failed`… o `desconocido`.
    pub activo: String,
    /// Segundos que lleva activo. Se calcula con el reloj monotónico y no
    /// con `ActiveEnterTimestamp`: esta máquina alterna con Windows y arranca
    /// con la hora de pared desajustada hasta que NTP la corrige, así que las
    /// marcas de pared de los servicios pueden quedar en el futuro.
    pub activo_s: Option<i64>,
}

/// `systemctl --user show` con marcas de tiempo unix (`@1790000000`): una
/// sola invocación para varias unidades, un mapa por unidad.
async fn show(unidades: &[String], props: &[&str]) -> Option<Vec<HashMap<String, String>>> {
    let mut cmd = Command::new("systemctl");
    cmd.args(["--user", "--timestamp=unix", "show"]);
    cmd.args(unidades);
    for p in props {
        cmd.arg("-p").arg(p);
    }
    cmd.kill_on_drop(true);

    let salida = timeout(LIMITE, cmd.output()).await.ok()?.ok()?;
    if !salida.status.success() {
        return None;
    }
    let texto = String::from_utf8_lossy(&salida.stdout);
    Some(
        texto
            .split("\n\n")
            .map(|bloque| {
                bloque
                    .lines()
                    .filter_map(|l| l.split_once('='))
                    .map(|(k, v)| (k.to_string(), v.trim().to_string()))
                    .collect::<HashMap<_, _>>()
            })
            .filter(|m| !m.is_empty())
            .collect(),
    )
}

fn monotonico_us() -> Option<i64> {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `ts` es un `timespec` válido y escribible durante la llamada.
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    (rc == 0).then(|| ts.tv_sec as i64 * 1_000_000 + ts.tv_nsec as i64 / 1_000)
}

/// Segundos desde un `ActiveEnterTimestampMonotonic` (µs desde el arranque;
/// 0 si la unidad nunca ha estado activa).
fn segundos_activo(valor: Option<&String>) -> Option<i64> {
    let desde: i64 = valor?.parse().ok().filter(|&us| us > 0)?;
    Some(((monotonico_us()? - desde) / 1_000_000).max(0))
}

fn marca_unix(valor: Option<&String>) -> Option<i64> {
    valor?.strip_prefix('@')?.parse().ok()
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

    let unidades: Vec<String> = SERVICIOS.iter().map(|s| format!("{s}.service")).collect();
    let Some(bloques) = show(&unidades, &["Id", "ActiveState", "ActiveEnterTimestampMonotonic"]).await else {
        return desconocido();
    };

    SERVICIOS
        .iter()
        .map(|&nombre| {
            let id = format!("{nombre}.service");
            let bloque = bloques.iter().find(|b| b.get("Id") == Some(&id));
            Servicio {
                nombre,
                activo: bloque
                    .and_then(|b| b.get("ActiveState").cloned())
                    .unwrap_or_else(|| "desconocido".into()),
                activo_s: bloque.and_then(|b| segundos_activo(b.get("ActiveEnterTimestampMonotonic"))),
            }
        })
        .collect()
}

/// Próxima ejecución del timer de una rutina (marca unix), si está
/// instalado y programado. `slug` ya debe estar validado por el llamador.
pub async fn proxima_ejecucion(slug: &str) -> Option<i64> {
    let unidad = vec![format!("ea-rutina@{slug}.timer")];
    let bloques = show(&unidad, &["NextElapseUSecRealtime"]).await?;
    marca_unix(bloques.first()?.get("NextElapseUSecRealtime"))
}
