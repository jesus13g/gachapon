//! Tipos del bus y máquina de estados pura.

use serde::{Deserialize, Serialize};

use crate::equipo::{Miembro, RolEquipo};
use crate::error::{ErrorBus, Resultado};

/// El rol de una cola: el planificador (`claude-code` en disco) o el
/// ejecutor (`modelo-local`). Lo define `ea-ajustes` (spec 017, RF-001).
pub use ea_ajustes::Rol;

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
    /// Segundos que puede durar cada corrida del criterio de éxito. Una orden
    /// sin él (las de antes) lleva el de por defecto.
    pub tiempo_criterio_s: u64,
}

impl Default for Topes {
    fn default() -> Self {
        Self { turnos: 30, tiempo_s: 900, llamadas_herramienta: 60, iteraciones: 5, tiempo_criterio_s: 120 }
    }
}

/// El techo de los ajustes (`agente.techo_topes`) como topes.
impl From<&ea_ajustes::TechoTopes> for Topes {
    fn from(t: &ea_ajustes::TechoTopes) -> Self {
        Self {
            turnos: t.turnos,
            tiempo_s: t.tiempo_s,
            llamadas_herramienta: t.llamadas_herramienta,
            iteraciones: t.iteraciones,
            tiempo_criterio_s: t.tiempo_criterio_s,
        }
    }
}

impl Topes {
    /// Recorta cada tope al del techo (el de los ajustes). Al menos una
    /// iteración: una orden siempre llega a llamar al agente una vez; y al
    /// menos un segundo de criterio.
    pub fn acotar(self, techo: &Topes) -> Topes {
        Topes {
            turnos: self.turnos.min(techo.turnos),
            tiempo_s: self.tiempo_s.min(techo.tiempo_s),
            llamadas_herramienta: self.llamadas_herramienta.min(techo.llamadas_herramienta),
            iteraciones: self.iteraciones.min(techo.iteraciones).max(1),
            tiempo_criterio_s: self.tiempo_criterio_s.min(techo.tiempo_criterio_s).max(1),
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

impl Orden {
    /// Una misión: alcanzar `objetivo` en `worktree` hasta que pase `criterio`.
    /// El resto, por defecto (topes, sin fases, equipo ni skills); el id y la
    /// fecha los pone [`crate::Bus::crear_trabajo`].
    pub fn mision(
        titulo: impl Into<String>,
        worktree: impl Into<String>,
        objetivo: impl Into<String>,
        criterio: impl Into<String>,
        creada_por: impl Into<String>,
    ) -> Orden {
        Orden {
            id: String::new(),
            modo: Modo::Mision,
            titulo: titulo.into(),
            worktree: worktree.into(),
            objetivo: objetivo.into(),
            criterio_exito: Some(criterio.into()),
            fases: vec![],
            revisar_cada: None,
            topes: Topes::default(),
            rutina: None,
            notas_revision: vec![],
            skills: vec![],
            equipo: None,
            asignado_a: None,
            revisor: None,
            worktree_propio: false,
            creada_en: String::new(),
            creada_por: creada_por.into(),
        }
    }

    /// Una obra: `fases` (numeradas 1, 2, …, N, cada una con su criterio) con
    /// revisión cada `revisar_cada` (`None`: solo al final). El resto, como en
    /// [`Orden::mision`].
    pub fn obra(
        titulo: impl Into<String>,
        worktree: impl Into<String>,
        objetivo: impl Into<String>,
        fases: Vec<Fase>,
        revisar_cada: Option<u32>,
        creada_por: impl Into<String>,
    ) -> Orden {
        Orden { modo: Modo::Obra, criterio_exito: None, fases, revisar_cada, ..Orden::mision(titulo, worktree, objetivo, String::new(), creada_por) }
    }
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
    /// Todos, en el orden del ciclo de vida (el de `ea status` y el panel).
    pub const TODOS: [EstadoTrabajo; 6] = [
        EstadoTrabajo::EnCola,
        EstadoTrabajo::Ejecutando,
        EstadoTrabajo::EnRevision,
        EstadoTrabajo::EnEspera,
        EstadoTrabajo::Cerrado,
        EstadoTrabajo::Cancelado,
    ];

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

impl ResultadoInforme {
    /// Como en disco: `terminado` o `bloqueado`.
    pub fn as_str(self) -> &'static str {
        match self {
            ResultadoInforme::Terminado => "terminado",
            ResultadoInforme::Bloqueado => "bloqueado",
        }
    }
}

/// El estado de una fase de obra: el de su fila en SQLite (`pendiente` al
/// crear, `hecha` o `bloqueada` al cerrarla, `cancelada` si se cancela la
/// obra) y el resultado de cada fase del informe (`hecha` o `bloqueada`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EstadoFase {
    Pendiente,
    Hecha,
    Bloqueada,
    Cancelada,
}

impl EstadoFase {
    /// Como en disco.
    pub fn as_str(self) -> &'static str {
        match self {
            EstadoFase::Pendiente => "pendiente",
            EstadoFase::Hecha => "hecha",
            EstadoFase::Bloqueada => "bloqueada",
            EstadoFase::Cancelada => "cancelada",
        }
    }
}

/// El informe de una fase en SQLite (`trabajo_fases.informe`). El ejecutor
/// guarda `{diff_stat, arbol}` al cerrar una fase hecha y `{motivo,
/// diff_stat}` al bloquearse (por este orden: el de los `json!` de antes con
/// `preserve_order`, que es como se compila el workspace); la entrega de una
/// obra guarda el [`Informe`] entero en las fases que aún no estaban hechas.
/// Se leen los dos esquemas: lo que falta queda en `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InformeFase {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motivo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_stat: Option<String>,
    /// La instantánea del árbol al cerrar la fase.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arbol: Option<String>,
}

/// Resultado de una fase en el informe de una obra.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaseInforme {
    pub numero: u32,
    pub titulo: String,
    /// `hecha` o `bloqueada`.
    pub resultado: EstadoFase,
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
            return Err(ErrorBus::TransicionInvalida { desde: estado.as_str().into(), evento: evento.nombre().into() });
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
    use super::{EstadoFase, InformeFase, Topes};

    #[test]
    fn el_informe_de_una_fase_se_escribe_como_antes_y_lee_los_dos_esquemas() {
        let hecha = InformeFase { diff_stat: Some("s".into()), arbol: Some("a1".into()), ..Default::default() };
        assert_eq!(serde_json::to_string(&hecha).unwrap(), r#"{"diff_stat":"s","arbol":"a1"}"#);
        let bloqueada = InformeFase { motivo: Some("m".into()), diff_stat: Some("s".into()), arbol: None };
        assert_eq!(serde_json::to_string(&bloqueada).unwrap(), r#"{"motivo":"m","diff_stat":"s"}"#);
        // El de la entrega de una obra: el informe entero, sin árbol.
        let entera: InformeFase = serde_json::from_str(r#"{"resultado":"terminado","diff_stat":null,"motivo":null,"fases":[]}"#).unwrap();
        assert_eq!(entera, InformeFase::default());
        let fases = [EstadoFase::Pendiente, EstadoFase::Hecha, EstadoFase::Bloqueada, EstadoFase::Cancelada];
        for e in fases {
            assert_eq!(serde_json::to_string(&e).unwrap(), format!("\"{}\"", e.as_str()));
        }
    }

    #[test]
    fn acotar_recorta_cada_tope_al_techo() {
        let pedidos = Topes { turnos: 500, tiempo_s: 9000, llamadas_herramienta: 20, iteraciones: 0, tiempo_criterio_s: 600 };
        let techo = Topes { turnos: 10, tiempo_s: 120, llamadas_herramienta: 200, iteraciones: 3, tiempo_criterio_s: 300 };
        let t = pedidos.acotar(&techo);
        assert_eq!(t, Topes { turnos: 10, tiempo_s: 120, llamadas_herramienta: 20, iteraciones: 1, tiempo_criterio_s: 300 });
        assert_eq!(Topes::default().acotar(&Topes::default()), Topes::default());
        assert_eq!(Topes { tiempo_criterio_s: 0, ..Topes::default() }.acotar(&techo).tiempo_criterio_s, 1);
    }

    /// U-09: el tope del criterio vale 120 s por defecto y una orden escrita
    /// antes de existir (sin el campo) sigue valiendo, con ese valor.
    #[test]
    fn el_tope_del_criterio_es_120_s_y_las_ordenes_de_antes_siguen_valiendo() {
        assert_eq!(Topes::default().tiempo_criterio_s, 120);
        let t: Topes = serde_json::from_str(r#"{"turnos": 7, "tiempo_s": 60, "llamadas_herramienta": 9, "iteraciones": 2}"#).unwrap();
        assert_eq!(t, Topes { turnos: 7, tiempo_s: 60, llamadas_herramienta: 9, iteraciones: 2, tiempo_criterio_s: 120 });
        let t: Topes = serde_json::from_str(r#"{"tiempo_criterio_s": 30}"#).unwrap();
        assert_eq!(t, Topes { tiempo_criterio_s: 30, ..Topes::default() });
        let techo = Topes::from(&ea_ajustes::TECHO_DURO);
        assert_eq!(techo.tiempo_criterio_s, ea_ajustes::TECHO_DURO.tiempo_criterio_s);
    }
}
