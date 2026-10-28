//! Presencia de los agentes (spec 019, RF-010): `agentes/<agente-id>/presencia.json`,
//! escrito con `fs_atomico`. Dice quién está despierto, en qué trabaja y desde
//! cuándo. Caduca si el proceso ya no vive o si el último latido es viejo.
//! Quien la sostiene (un trabajador del orquestador o una sesión de equipo)
//! la mantiene con un [`Latido`].

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::bus::{ahora_rfc3339, Bus};
use crate::error::{ErrorBus, Resultado};
use crate::fs_atomico::escribir_json;
use crate::equipo::RolEquipo;

/// Una presencia sin latido en estos segundos está caducada.
pub const CADUCIDAD_LATIDO_S: i64 = 90;
/// Cada cuánto debería latir quien escribe una presencia.
pub const INTERVALO_LATIDO_S: u64 = 15;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Presencia {
    pub agente: String,
    pub rol: RolEquipo,
    #[serde(default)]
    pub equipo: Option<String>,
    /// Trabajo en el que está ahora, si lo hay.
    #[serde(default)]
    pub trabajo: Option<String>,
    /// RFC 3339: cuándo se despertó.
    pub desde: String,
    /// RFC 3339: el último latido.
    pub latido: String,
    /// Proceso que la sostiene (el orquestador o la terminal de la sesión).
    pub pid: u32,
}

impl Presencia {
    /// Presencia recién despertada: `desde` y `latido` son ahora.
    pub fn nueva(agente: &str, rol: RolEquipo, equipo: Option<&str>, pid: u32) -> Self {
        let ahora = ahora_rfc3339();
        Self { agente: agente.into(), rol, equipo: equipo.map(Into::into), trabajo: None, desde: ahora.clone(), latido: ahora, pid }
    }

    /// Viva a las `ahora`: el pid existe y el latido tiene menos de
    /// [`CADUCIDAD_LATIDO_S`] segundos. Un latido ilegible cuenta como caducado.
    pub fn viva_en(&self, ahora: OffsetDateTime) -> bool {
        let reciente = OffsetDateTime::parse(&self.latido, &Rfc3339)
            .is_ok_and(|l| (ahora - l).whole_seconds() < CADUCIDAD_LATIDO_S);
        reciente && pid_vivo(self.pid)
    }
}

/// El proceso existe. El pid 0 nunca cuenta.
fn pid_vivo(pid: u32) -> bool {
    pid != 0 && ea_plataforma::proceso::vive(pid)
}

impl Bus {
    fn dir_agente(&self, agente: &str) -> Resultado<PathBuf> {
        if ea_ajustes::ficha(agente).is_none() {
            return Err(ErrorBus::Argumentos(format!("{agente:?} no es un agente del catálogo")));
        }
        Ok(self.raiz.join("agentes").join(agente))
    }

    /// Escribe (o reescribe) la presencia del agente.
    pub fn anotar_presencia(&self, p: &Presencia) -> Resultado<()> {
        escribir_json(&self.dir_agente(&p.agente)?.join("presencia.json"), p)
    }

    /// La presencia anotada del agente, viva o no; `None` si no hay.
    pub fn presencia_de(&self, agente: &str) -> Resultado<Option<Presencia>> {
        match fs::read(self.dir_agente(agente)?.join("presencia.json")) {
            Ok(b) => Ok(serde_json::from_slice(&b).ok()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Actualiza el latido (ahora) y el trabajo de una presencia ya anotada.
    pub fn latir(&self, agente: &str, trabajo: Option<&str>) -> Resultado<()> {
        let mut p = self.presencia_de(agente)?.ok_or_else(|| ErrorBus::NoExiste(format!("presencia de {agente}")))?;
        p.latido = ahora_rfc3339();
        p.trabajo = trabajo.map(Into::into);
        self.anotar_presencia(&p)
    }

    /// Borra la presencia; si no había, no hace nada.
    pub fn quitar_presencia(&self, agente: &str) -> Resultado<()> {
        match fs::remove_file(self.dir_agente(agente)?.join("presencia.json")) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Todas las presencias anotadas, con si siguen vivas ahora, por agente.
    pub fn presencias(&self) -> Vec<(Presencia, bool)> {
        self.presencias_en(OffsetDateTime::now_utc())
    }

    /// Como [`Bus::presencias`] a una hora dada (para probar sin dormir).
    pub fn presencias_en(&self, ahora: OffsetDateTime) -> Vec<(Presencia, bool)> {
        let Ok(rd) = fs::read_dir(self.raiz.join("agentes")) else { return vec![] };
        let mut v: Vec<Presencia> = rd
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter_map(|a| self.presencia_de(&a).ok().flatten())
            .collect();
        v.sort_by(|a, b| a.agente.cmp(&b.agente));
        v.into_iter().map(|p| { let viva = p.viva_en(ahora); (p, viva) }).collect()
    }
}

/// Una presencia que late mientras vive el valor. [`Latido::anotar`] la
/// escribe y un hilo aparte la renueva cada `cada` con el trabajo actual
/// ([`Latido::fijar`]); al soltarlo, deja de latir y la quita si sigue siendo
/// suya (el mismo pid). El hilo deja de latir antes, y la presencia caduca
/// sola, si ya no es suya o si `sigue` dice que no. Su `Bus` (sin historial:
/// solo toca ficheros) y el trabajo actual van bajo un mismo `Mutex`, que
/// además ordena las escrituras de `presencia.json`.
pub struct Latido {
    comun: Arc<Comun>,
    parar: Option<mpsc::Sender<()>>,
    hilo: Option<JoinHandle<()>>,
}

struct Comun {
    agente: String,
    pid: u32,
    estado: Mutex<(Bus, Option<String>)>,
}

impl Comun {
    fn es_suya(&self, bus: &Bus) -> bool {
        bus.presencia_de(&self.agente).ok().flatten().is_some_and(|p| p.pid == self.pid)
    }
}

impl Latido {
    /// Anota `presencia` y empieza a latir. `sigue` se mira (con el `Bus` del
    /// hilo) antes de cada latido.
    pub fn anotar(bus: &Bus, presencia: &Presencia, cada: Duration, sigue: impl Fn(&Bus) -> bool + Send + 'static) -> Resultado<Self> {
        let propio = Bus::abrir_en(bus.raiz())?;
        bus.anotar_presencia(presencia)?;
        let comun = Arc::new(Comun { agente: presencia.agente.clone(), pid: presencia.pid, estado: Mutex::new((propio, None)) });
        let (tx, rx) = mpsc::channel::<()>();
        let c = Arc::clone(&comun);
        let hilo = std::thread::spawn(move || {
            while let Err(RecvTimeoutError::Timeout) = rx.recv_timeout(cada) {
                let g = c.estado.lock().unwrap_or_else(|e| e.into_inner());
                if !sigue(&g.0) || !c.es_suya(&g.0) {
                    return;
                }
                if let Err(e) = g.0.latir(&c.agente, g.1.as_deref()) {
                    tracing::debug!(agente = %c.agente, error = %e, "no se pudo latir");
                }
            }
        });
        Ok(Self { comun, parar: Some(tx), hilo: Some(hilo) })
    }

    /// Cambia el trabajo actual y late ya.
    pub fn fijar(&self, trabajo: Option<&str>) {
        let mut g = self.comun.estado.lock().unwrap_or_else(|e| e.into_inner());
        g.1 = trabajo.map(Into::into);
        if let Err(e) = g.0.latir(&self.comun.agente, trabajo) {
            tracing::debug!(agente = %self.comun.agente, error = %e, "no se pudo latir");
        }
    }

    /// Como soltarlo, pero con el error de quitar la presencia.
    pub fn soltar(mut self) -> Resultado<()> {
        self.detener()
    }

    fn detener(&mut self) -> Resultado<()> {
        let Some(hilo) = self.hilo.take() else { return Ok(()) };
        drop(self.parar.take());
        let _ = hilo.join();
        let g = self.comun.estado.lock().unwrap_or_else(|e| e.into_inner());
        // Solo la propia: otra sesión del mismo agente no se pisa.
        if g.0.presencia_de(&self.comun.agente)?.is_some_and(|p| p.pid == self.comun.pid) {
            g.0.quitar_presencia(&self.comun.agente)?;
        }
        Ok(())
    }
}

impl Drop for Latido {
    fn drop(&mut self) {
        if let Err(e) = self.detener() {
            tracing::warn!(agente = %self.comun.agente, error = %e, "no se pudo quitar la presencia");
        }
    }
}
