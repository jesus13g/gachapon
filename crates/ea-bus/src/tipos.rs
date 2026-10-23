//! Tipos del bus y máquina de estados pura.

use serde::{Deserialize, Serialize};

use crate::error::{ErrorBus, Resultado};

/// El rol de una cola: el planificador (`claude-code` en disco) o el
/// ejecutor (`modelo-local`). Lo define `ea-ajustes` (spec 017, RF-001).
pub use ea_ajustes::Rol;

/// Miembro de un equipo y su rol dentro de él (spec 019). Los define
/// `ea-ajustes`, que valida los equipos.
pub use ea_ajustes::{Miembro, RolEquipo};

/// Quién ejecutó una iteración: el agente (id del catálogo) y el nombre del
/// modelo con el que se tomó el trabajo (spec 017, RF-008). Opcional en todo
/// lo que se guarda, para leer trabajos de antes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EjecutadoPor {
    pub agente: String,
    pub modelo: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modo {
    Mision,
    Obra,
    Rutina,
}

impl Modo {
    pub fn as_str(self) -> &'static str {
        match self {
            Modo::Mision => "mision",
            Modo::Obra => "obra",
            Modo::Rutina => "rutina",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Topes {
    pub turnos: u32,
    pub tiempo_s: u64,
    pub llamadas_herramienta: u32,
    pub iteraciones: u32,
}

impl Default for Topes {
    fn default() -> Self {
        Self { turnos: 30, tiempo_s: 900, llamadas_herramienta: 60, iteraciones: 5 }
    }
}

impl Topes {
    /// Recorta cada tope al del techo (el de los ajustes). Al menos una
    /// iteración: una orden siempre llega a llamar al agente una vez.
    pub fn acotar(self, techo: &Topes) -> Topes {
        Topes {
            turnos: self.turnos.min(techo.turnos),
            tiempo_s: self.tiempo_s.min(techo.tiempo_s),
            llamadas_herramienta: self.llamadas_herramienta.min(techo.llamadas_herramienta),
            iteraciones: self.iteraciones.min(techo.iteraciones).max(1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fase {
    pub numero: u32,
    pub titulo: String,
    pub instrucciones: String,
    pub entregable: String,
    /// Comando de shell que decide si la fase está hecha.
    pub criterio: String,
    /// Skills del ejecutor para esta fase; si está vacío, las de la orden.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RutinaRef {
    pub slug: String,
    pub version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Orden {
    pub id: String,
    pub modo: Modo,
    pub titulo: String,
    /// Ruta absoluta.
    pub worktree: String,
    /// Texto para el agente.
    pub objetivo: String,
    /// Comando de shell opcional; en la obra cada fase tiene el suyo.
    #[serde(default)]
    pub criterio_exito: Option<String>,
    #[serde(default)]
    pub fases: Vec<Fase>,
    #[serde(default)]
    pub revisar_cada: Option<u32>,
    #[serde(default)]
    pub topes: Topes,
    #[serde(default)]
    pub rutina: Option<RutinaRef>,
    /// Notas que añade la revisión al reformular.
    #[serde(default)]
    pub notas_revision: Vec<String>,
    /// Skills del ejecutor que lleva el trabajo (las elige quien tenga la llave).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    /// Equipo para el que se crea el trabajo (spec 019, RF-004).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equipo: Option<String>,
    /// Agente ejecutor concreto del equipo; sin él, lo toma cualquiera.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asignado_a: Option<String>,
    /// Agente del equipo al que llega la entrega para revisarla.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revisor: Option<String>,
    /// El sistema crea un `git worktree` para el trabajo.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub worktree_propio: bool,
    pub creada_en: String,
    pub creada_por: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EstadoEquipo {
    Despierto,
    Dormido,
}

impl EstadoEquipo {
    pub fn as_str(self) -> &'static str {
        match self {
            EstadoEquipo::Despierto => "despierto",
            EstadoEquipo::Dormido => "dormido",
        }
    }
}

/// Un grupo de agentes despertado por el usuario para una tarea
/// (`equipos/<id>/equipo.json`, spec 019, RF-001).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Equipo {
    pub id: String,
    pub titulo: String,
    pub objetivo: String,
    /// Repo sobre el que trabaja el equipo.
    pub worktree_base: String,
    pub miembros: Vec<Miembro>,
    pub estado: EstadoEquipo,
    pub creado_en: String,
    pub creado_por: String,
    /// Ids de los trabajos creados para el equipo, en orden.
    #[serde(default)]
    pub trabajos: Vec<String>,
}

impl Equipo {
    pub fn miembro(&self, agente: &str) -> Option<&Miembro> {
        self.miembros.iter().find(|m| m.agente == agente)
    }

    pub fn con_rol(&self, agente: &str, rol: RolEquipo) -> bool {
        self.miembro(agente).is_some_and(|m| m.rol == rol)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstadoTrabajo {
    EnCola,
    Ejecutando,
    EnRevision,
    EnEspera,
    Cerrado,
    Cancelado,
}

impl EstadoTrabajo {
    pub fn as_str(self) -> &'static str {
        match self {
            EstadoTrabajo::EnCola => "en_cola",
            EstadoTrabajo::Ejecutando => "ejecutando",
            EstadoTrabajo::EnRevision => "en_revision",
            EstadoTrabajo::EnEspera => "en_espera",
            EstadoTrabajo::Cerrado => "cerrado",
            EstadoTrabajo::Cancelado => "cancelado",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Estado {
    pub estado: EstadoTrabajo,
    #[serde(default)]
    pub fase_actual: Option<u32>,
    #[serde(default)]
    pub iteracion: u32,
    pub actualizado_en: String,
    #[serde(default)]
    pub detalle: Option<String>,
    /// El agente y el modelo con los que se tomó por última vez.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ejecutado_por: Option<EjecutadoPor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResultadoInforme {
    Terminado,
    Bloqueado,
}

/// Resultado de una fase en el informe de una obra.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaseInforme {
    pub numero: u32,
    pub titulo: String,
    /// `hecha` o `bloqueada`.
    pub resultado: String,
    #[serde(default)]
    pub diff_stat: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ejecutado_por: Option<EjecutadoPor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Informe {
    pub resultado: ResultadoInforme,
    /// Salida del criterio, recortada.
    #[serde(default)]
    pub evidencia: String,
    #[serde(default)]
    pub commit_inicio: Option<String>,
    #[serde(default)]
    pub diff_stat: Option<String>,
    #[serde(default)]
    pub motivo: Option<String>,
    #[serde(default)]
    pub fases_hechas: u32,
    #[serde(default)]
    pub duracion_s: u64,
    /// Obra: `true` si no queda ninguna fase; `false` si espera la revisión
    /// intermedia (o quedó bloqueada). En misión y rutina, `true` al terminar.
    #[serde(default)]
    pub obra_completa: bool,
    /// Obra: fases hechas hasta ahora, con su resultado.
    #[serde(default)]
    pub fases: Vec<FaseInforme>,
    /// Obra: diff de las fases desde la última revisión (recortado).
    #[serde(default)]
    pub diff: Option<String>,
    /// Con qué agente y modelo se hizo esta iteración.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ejecutado_por: Option<EjecutadoPor>,
    /// Aviso del sistema al revisar (p. ej. el revisor es quien ejecutó).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aviso: Option<String>,
}

// Un evento vive lo que dura una transición: no compensa meter el informe en
// una caja.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evento {
    Tomar,
    Entregar(Informe),
    Reformular,
    Cerrar,
    Cancelar,
    PonerEnEspera,
    /// El usuario confirma el paquete de corrección de una rutina.
    Confirmar,
}

impl Evento {
    pub fn nombre(&self) -> &'static str {
        match self {
            Evento::Tomar => "tomar",
            Evento::Entregar(_) => "entregar",
            Evento::Reformular => "reformular",
            Evento::Cerrar => "cerrar",
            Evento::Cancelar => "cancelar",
            Evento::PonerEnEspera => "poner_en_espera",
            Evento::Confirmar => "confirmar",
        }
    }
}

/// Máquina de estados pura.
pub fn transicion(estado: EstadoTrabajo, evento: &Evento) -> Resultado<EstadoTrabajo> {
    use EstadoTrabajo::*;
    let nuevo = match (estado, evento) {
        (EnCola, Evento::Tomar) => Ejecutando,
        (Ejecutando, Evento::Entregar(_)) => EnRevision,
        (EnRevision | EnEspera, Evento::Reformular) => EnCola,
        (EnRevision | EnEspera, Evento::Cerrar) => Cerrado,
        (Ejecutando | EnRevision, Evento::PonerEnEspera) => EnEspera,
        (EnEspera, Evento::Confirmar) => EnRevision,
        (EnCola | Ejecutando | EnRevision | EnEspera, Evento::Cancelar) => Cancelado,
        _ => {
            return Err(ErrorBus::TransicionInvalida {
                desde: estado.as_str().into(),
                evento: evento.nombre().into(),
            });
        }
    };
    Ok(nuevo)
}

impl Informe {
    /// Informe vacío con el resultado dado; el resto se rellena a mano.
    pub fn nuevo(resultado: ResultadoInforme) -> Self {
        Self {
            resultado,
            evidencia: String::new(),
            commit_inicio: None,
            diff_stat: None,
            motivo: None,
            fases_hechas: 0,
            duracion_s: 0,
            obra_completa: false,
            fases: vec![],
            diff: None,
            ejecutado_por: None,
            aviso: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Topes;

    #[test]
    fn acotar_recorta_cada_tope_al_techo() {
        let pedidos = Topes { turnos: 500, tiempo_s: 9000, llamadas_herramienta: 20, iteraciones: 0 };
        let techo = Topes { turnos: 10, tiempo_s: 120, llamadas_herramienta: 200, iteraciones: 3 };
        let t = pedidos.acotar(&techo);
        assert_eq!(t, Topes { turnos: 10, tiempo_s: 120, llamadas_herramienta: 20, iteraciones: 1 });
        assert_eq!(Topes::default().acotar(&Topes::default()), Topes::default());
    }
}
