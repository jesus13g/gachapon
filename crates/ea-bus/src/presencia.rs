//! Presencia de los agentes (spec 019, RF-010): `agentes/<agente-id>/presencia.json`,
//! escrito con `fs_atomico`. Dice quién está despierto, en qué trabaja y desde
//! cuándo. Caduca si el proceso ya no vive o si el último latido es viejo.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::bus::{ahora_rfc3339, Bus};
use crate::error::{ErrorBus, Resultado};
use crate::fs_atomico::escribir_json;
use crate::tipos::RolEquipo;

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

/// El proceso existe (Linux: `/proc/<pid>`). El pid 0 nunca cuenta.
fn pid_vivo(pid: u32) -> bool {
    pid != 0 && std::path::Path::new(&format!("/proc/{pid}")).exists()
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
