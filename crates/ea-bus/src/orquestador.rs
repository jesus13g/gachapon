//! El latido del orquestador (U-07): `<estado>/orquestador.json`, escrito con
//! `fs_atomico` cada `orquestador.intervalo_heartbeat_s`. Dice cuándo latió,
//! cuántos de sus trabajadores tienen la presencia al día y el sandbox del
//! ejecutor (spec 020, «sandbox visible»). Las interfaces (`ea status`, la
//! TUI, `ea_status` y el panel) solo lo leen; en el diario el orquestador deja
//! su arranque y su parada, no cada latido.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use crate::bus::{Bus, ahora_rfc3339};
use crate::equipo::RolEquipo;
use crate::error::Resultado;
use crate::fs_atomico::escribir_json;

/// El fichero del latido, en la raíz del bus.
pub const FICHERO_LATIDO: &str = "orquestador.json";

/// Cuánto puede adelantarse un latido al reloj de quien lo lee.
const MARGEN: Duration = Duration::seconds(5);

/// Aislamiento del ejecutor que el orquestador declara en su latido.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxInfo {
    /// `Linux`, `macOS`, `Windows` u «otro sistema».
    pub sistema: String,
    /// `bwrap`, `seatbelt`, `ninguno` o `no disponible`.
    pub sandbox: String,
    pub disponible: bool,
    /// Por qué no puede arrancar (solo si `disponible` es falso).
    #[serde(default)]
    pub motivo: Option<String>,
}

/// Lo que guarda `orquestador.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatidoOrquestador {
    /// RFC 3339: cuándo latió.
    pub ts: String,
    /// El proceso del orquestador.
    pub pid: u32,
    /// Sus trabajadores (presencias de ejecutor con su pid) vivos al latir:
    /// los que siguen renovando su presencia.
    pub trabajadores: usize,
    #[serde(default)]
    pub sandbox: Option<SandboxInfo>,
}

/// Estado del orquestador según su último latido.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EstadoOrquestador {
    /// `true` si el último latido no es más antiguo que el umbral (ni está
    /// fechado en el futuro más allá de un margen: un reloj mal puesto).
    pub vivo: bool,
    /// `None` si nunca ha latido (no es lo mismo que «caído»).
    pub ultimo_latido: Option<OffsetDateTime>,
    /// Los trabajadores que contó el último latido.
    pub trabajadores: Option<usize>,
    pub sandbox: Option<SandboxInfo>,
}

impl EstadoOrquestador {
    /// Con el latido leído (o ninguno), a la hora `ahora` y con `umbral_s`, la
    /// antigüedad máxima tolerada (el doble del intervalo).
    pub fn calcular(latido: Option<LatidoOrquestador>, ahora: OffsetDateTime, umbral_s: i64) -> Self {
        let Some(l) = latido else { return Self { vivo: false, ultimo_latido: None, trabajadores: None, sandbox: None } };
        let ts = OffsetDateTime::parse(&l.ts, &Rfc3339).ok();
        let vivo = ts.is_some_and(|t| t <= ahora + MARGEN && (ahora - t).whole_seconds() <= umbral_s);
        Self { vivo, ultimo_latido: ts, trabajadores: Some(l.trabajadores), sandbox: l.sandbox }
    }

    /// «vivo», «sin señal reciente» o «sin señal (ningún heartbeat
    /// registrado todavía)»: lo que dicen `ea status`, la TUI y `ea_status`.
    /// Nunca «caído»: sin latido no se sabe más.
    pub fn descripcion(&self) -> &'static str {
        match (self.vivo, self.ultimo_latido) {
            (true, _) => "vivo",
            (false, Some(_)) => "sin señal reciente",
            (false, None) => "sin señal (ningún heartbeat registrado todavía)",
        }
    }
}

impl Bus {
    /// Late: reescribe `orquestador.json` con la hora, el pid de este proceso,
    /// cuántos de sus trabajadores siguen vivos y el sandbox.
    pub fn latir_orquestador(&self, sandbox: Option<SandboxInfo>) -> Resultado<LatidoOrquestador> {
        let pid = std::process::id();
        let trabajadores = self.presencias().iter().filter(|(p, viva)| *viva && p.pid == pid && p.rol == RolEquipo::Ejecutor).count();
        let l = LatidoOrquestador { ts: ahora_rfc3339(), pid, trabajadores, sandbox };
        escribir_json(&self.raiz.join(FICHERO_LATIDO), &l)?;
        Ok(l)
    }
}

/// El último latido del orquestador del bus en `raiz`; `None` si no hay o no
/// se puede leer. Solo lee: no crea nada en el bus.
pub fn latido_orquestador(raiz: &Path) -> Option<LatidoOrquestador> {
    serde_json::from_slice(&fs::read(raiz.join(FICHERO_LATIDO)).ok()?).ok()
}

/// Estado del orquestador ahora mismo, según su último latido.
pub fn estado_orquestador(raiz: &Path, umbral_s: i64) -> EstadoOrquestador {
    EstadoOrquestador::calcular(latido_orquestador(raiz), OffsetDateTime::now_utc(), umbral_s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn latido(ts: OffsetDateTime) -> Option<LatidoOrquestador> {
        Some(LatidoOrquestador { ts: ts.format(&Rfc3339).unwrap(), pid: 1, trabajadores: 2, sandbox: None })
    }

    #[test]
    fn vivo_hasta_el_umbral_y_nunca_con_un_latido_del_futuro() {
        let ahora = OffsetDateTime::now_utc();
        let e = EstadoOrquestador::calcular(latido(ahora - Duration::seconds(40)), ahora, 60);
        assert!(e.vivo && e.trabajadores == Some(2), "{e:?}");
        let viejo = EstadoOrquestador::calcular(latido(ahora - Duration::seconds(61)), ahora, 60);
        assert!(!viejo.vivo && viejo.ultimo_latido.is_some());
        assert_eq!(viejo.descripcion(), "sin señal reciente");
        // Un reloj mal puesto al latir no deja «vivo» a un orquestador caído.
        assert!(!EstadoOrquestador::calcular(latido(ahora + Duration::hours(1)), ahora, 60).vivo);
        let nunca = EstadoOrquestador::calcular(None, ahora, 60);
        assert_eq!((nunca.vivo, nunca.ultimo_latido), (false, None));
        assert_eq!(nunca.descripcion(), "sin señal (ningún heartbeat registrado todavía)");
    }

    #[test]
    fn el_latido_va_al_fichero_y_cuenta_los_trabajadores_vivos_de_este_proceso() {
        let d = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(d.path()).unwrap();
        assert!(latido_orquestador(d.path()).is_none());
        assert!(!estado_orquestador(d.path(), 60).vivo);
        let pid = std::process::id();
        bus.anotar_presencia(&crate::Presencia::nueva("qwen-code", RolEquipo::Ejecutor, None, pid)).unwrap();
        // De otro proceso (el pid 0 nunca vive) o de un revisor: no cuentan.
        bus.anotar_presencia(&crate::Presencia::nueva("codex", RolEquipo::Ejecutor, None, 0)).unwrap();
        bus.anotar_presencia(&crate::Presencia::nueva("claude-code", RolEquipo::Revisor, None, pid)).unwrap();
        let info = SandboxInfo { sistema: "Linux".into(), sandbox: "bwrap".into(), disponible: true, motivo: None };
        let l = bus.latir_orquestador(Some(info.clone())).unwrap();
        assert_eq!((l.pid, l.trabajadores), (pid, 1));
        assert_eq!(latido_orquestador(d.path()), Some(l));
        let e = estado_orquestador(d.path(), 60);
        assert!(e.vivo && e.trabajadores == Some(1) && e.sandbox == Some(info), "{e:?}");
    }

    #[test]
    fn un_latido_ilegible_es_sin_senal_y_el_siguiente_lo_rehace() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(FICHERO_LATIDO), "{ roto").unwrap();
        assert!(latido_orquestador(d.path()).is_none());
        assert_eq!(estado_orquestador(d.path(), 60).ultimo_latido, None);
        let bus = Bus::abrir_en(d.path()).unwrap();
        bus.latir_orquestador(None).unwrap();
        let l = bus.latir_orquestador(None).unwrap();
        assert_eq!(latido_orquestador(d.path()), Some(l));
        // Escritura atómica: ningún temporal se queda al lado.
        let ficheros: Vec<_> = std::fs::read_dir(d.path()).unwrap().flatten().filter(|e| e.path().is_file()).map(|e| e.file_name()).collect();
        assert_eq!(ficheros, [FICHERO_LATIDO]);
    }
}
