//! Vistas de solo lectura de los equipos (spec 019, RF-014/RF-015): lo que
//! enseñan `ea equipo`, `ea status`, `ea-mcp` y `ea-web`, con la presencia de
//! cada miembro y el último informe de cada trabajo. También el resumen de
//! cada trabajo y la cuenta por estado, que la CLI, la TUI, la web y el MCP
//! solo formatean.

use serde::Serialize;
use time::OffsetDateTime;

use crate::bus::{Bus, Cola};
use crate::error::Resultado;
use crate::presencia::Presencia;
use crate::equipo::RolEquipo;
use crate::tipos::{EjecutadoPor, Equipo, EstadoEquipo, EstadoTrabajo, Informe, Modo, ResultadoInforme, RutinaRef};

/// Un miembro del equipo y si está despierto ahora.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VistaMiembro {
    pub agente: String,
    pub rol: RolEquipo,
    /// La presencia anotada (viva o caducada); `None` si nunca la hubo.
    pub presencia: Option<Presencia>,
    /// La presencia sigue viva: su proceso existe y su latido es reciente.
    pub viva: bool,
    /// Trabajo en que está ahora (solo si la presencia está viva).
    pub trabajo_actual: Option<String>,
    /// Entradas pendientes: en su cola de órdenes (ejecutor) o de revisión.
    pub pendientes: usize,
    /// Los ids de esos trabajos (los más antiguos primero, hasta [`MAX_COLA`]).
    pub cola: Vec<String>,
}

/// Cuántos ids de cola enseña la vista por miembro.
pub const MAX_COLA: usize = 20;

/// Último informe de un trabajo, en una línea.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LineaInforme {
    pub resultado: ResultadoInforme,
    pub ejecutado_por: Option<EjecutadoPor>,
    /// Motivo si lo hay; si no, la primera línea de la evidencia (recortada).
    pub resumen: String,
    pub aviso: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VistaTrabajoEquipo {
    pub id: String,
    pub titulo: String,
    pub estado: EstadoTrabajo,
    pub asignado_a: Option<String>,
    pub revisor: Option<String>,
    /// Quién lo tomó por última vez.
    pub ejecutado_por: Option<EjecutadoPor>,
    pub informe: Option<LineaInforme>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VistaEquipo {
    pub id: String,
    pub titulo: String,
    pub objetivo: String,
    pub worktree_base: String,
    pub estado: EstadoEquipo,
    pub creado_en: String,
    pub creado_por: String,
    pub miembros: Vec<VistaMiembro>,
    /// Los últimos trabajos del equipo (los más recientes al final).
    pub trabajos: Vec<VistaTrabajoEquipo>,
    /// Cuántos trabajos tiene el equipo en total.
    pub trabajos_total: usize,
}

/// Una fila de la lista de equipos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResumenEquipo {
    pub id: String,
    pub titulo: String,
    pub objetivo: String,
    pub estado: EstadoEquipo,
    pub worktree_base: String,
    pub miembros: Vec<MiembroResumen>,
    pub trabajos: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MiembroResumen {
    pub agente: String,
    pub rol: RolEquipo,
    pub viva: bool,
}

/// Lo que enseñan de un trabajo las listas de la CLI, la TUI, la web y el
/// MCP: su orden, su estado y su último informe, leídos de una vez.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumenTrabajo {
    pub id: String,
    pub modo: Modo,
    pub titulo: String,
    pub estado: EstadoTrabajo,
    pub fase_actual: Option<u32>,
    /// Fases de la orden (0 si no es una obra).
    pub fases_total: usize,
    pub iteracion: u32,
    pub detalle: Option<String>,
    pub ejecutado_por: Option<EjecutadoPor>,
    pub rutina: Option<RutinaRef>,
    pub worktree: String,
    pub creada_en: String,
    pub creada_por: String,
    pub actualizado_en: String,
    /// El último informe; `None` si aún no hay o no se puede leer.
    pub informe: Option<Informe>,
}

const MAX_RESUMEN: usize = 160;

fn una_linea(texto: &str) -> String {
    let l = texto.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let mut s: String = l.chars().take(MAX_RESUMEN).collect();
    if l.chars().count() > MAX_RESUMEN {
        s.push('…');
    }
    s
}

impl Bus {
    /// El resumen del trabajo `id`; `None` si su orden o su estado no se
    /// pueden leer.
    pub fn resumen_trabajo(&self, id: &str) -> Option<ResumenTrabajo> {
        let orden = self.leer_orden(id).ok()?;
        let estado = self.leer_estado(id).ok()?;
        Some(ResumenTrabajo {
            id: id.into(),
            modo: orden.modo,
            titulo: orden.titulo,
            estado: estado.estado,
            fase_actual: estado.fase_actual,
            fases_total: orden.fases.len(),
            iteracion: estado.iteracion,
            detalle: estado.detalle,
            ejecutado_por: estado.ejecutado_por,
            rutina: orden.rutina,
            worktree: orden.worktree,
            creada_en: orden.creada_en,
            creada_por: orden.creada_por,
            actualizado_en: estado.actualizado_en,
            informe: self.leer_informe(id).ok().flatten(),
        })
    }

    /// Cuántos trabajos hay en cada estado: solo los estados con alguno, en
    /// el orden en que aparecen del trabajo más antiguo al más nuevo (el de
    /// `ea_status`). Un trabajo ilegible no cuenta; un bus ilegible, vacío.
    pub fn conteo_por_estado(&self) -> Vec<(EstadoTrabajo, usize)> {
        let mut cuenta: Vec<(EstadoTrabajo, usize)> = Vec::new();
        for id in self.listar_trabajos().unwrap_or_default() {
            let Ok(e) = self.leer_estado(&id) else { continue };
            match cuenta.iter_mut().find(|(estado, _)| *estado == e.estado) {
                Some((_, n)) => *n += 1,
                None => cuenta.push((e.estado, 1)),
            }
        }
        cuenta
    }

    /// Los equipos con la presencia de sus miembros, del más antiguo al más
    /// reciente.
    pub fn resumen_equipos(&self) -> Resultado<Vec<ResumenEquipo>> {
        let ahora = OffsetDateTime::now_utc();
        Ok(self
            .listar_equipos()?
            .into_iter()
            .map(|e| {
                let miembros = e
                    .miembros
                    .iter()
                    .map(|m| MiembroResumen { agente: m.agente.clone(), rol: m.rol, viva: self.viva_en_equipo(&e, &m.agente, ahora) })
                    .collect();
                ResumenEquipo {
                    id: e.id,
                    titulo: e.titulo,
                    objetivo: e.objetivo,
                    estado: e.estado,
                    worktree_base: e.worktree_base,
                    miembros,
                    trabajos: e.trabajos.len(),
                }
            })
            .collect())
    }

    /// Presencia viva del agente y anotada para ESTE equipo.
    fn viva_en_equipo(&self, e: &Equipo, agente: &str, ahora: OffsetDateTime) -> bool {
        e.estado == EstadoEquipo::Despierto
            && self
                .presencia_de(agente)
                .ok()
                .flatten()
                .is_some_and(|p| p.equipo.as_deref() == Some(e.id.as_str()) && p.viva_en(ahora))
    }

    /// El equipo con sus miembros y sus últimos `max_trabajos` trabajos.
    pub fn vista_equipo(&self, id: &str, max_trabajos: usize) -> Resultado<VistaEquipo> {
        let equipo = self.leer_equipo(id)?;
        let ahora = OffsetDateTime::now_utc();
        let miembros = equipo
            .miembros
            .iter()
            .map(|m| {
                let presencia = self.presencia_de(&m.agente).ok().flatten().filter(|p| p.equipo.as_deref() == Some(id));
                let viva = self.viva_en_equipo(&equipo, &m.agente, ahora);
                let cola = match m.rol {
                    RolEquipo::Ejecutor => Cola::Agente(&m.agente),
                    _ => Cola::Revision(&m.agente),
                };
                let entradas = self.listar_cola(cola).unwrap_or_default();
                let pendientes = entradas.len();
                let cola = entradas.into_iter().take(MAX_COLA).map(|e| e.trabajo).collect();
                let trabajo_actual = if viva { presencia.as_ref().and_then(|p| p.trabajo.clone()) } else { None };
                VistaMiembro { agente: m.agente.clone(), rol: m.rol, presencia, viva, trabajo_actual, pendientes, cola }
            })
            .collect();
        let desde = equipo.trabajos.len().saturating_sub(max_trabajos);
        let trabajos = equipo.trabajos[desde..].iter().filter_map(|t| self.vista_trabajo_equipo(t)).collect();
        let trabajos_total = equipo.trabajos.len();
        Ok(VistaEquipo {
            id: equipo.id,
            titulo: equipo.titulo,
            objetivo: equipo.objetivo,
            worktree_base: equipo.worktree_base,
            estado: equipo.estado,
            creado_en: equipo.creado_en,
            creado_por: equipo.creado_por,
            miembros,
            trabajos,
            trabajos_total,
        })
    }

    fn vista_trabajo_equipo(&self, id: &str) -> Option<VistaTrabajoEquipo> {
        let orden = self.leer_orden(id).ok()?;
        let estado = self.leer_estado(id).ok()?;
        let informe = self.leer_informe(id).ok().flatten().map(|i| LineaInforme {
            resultado: i.resultado,
            ejecutado_por: i.ejecutado_por.clone(),
            resumen: una_linea(i.motivo.as_deref().filter(|m| !m.trim().is_empty()).unwrap_or(&i.evidencia)),
            aviso: i.aviso,
        });
        Some(VistaTrabajoEquipo {
            id: id.into(),
            titulo: orden.titulo,
            estado: estado.estado,
            asignado_a: orden.asignado_a,
            revisor: orden.revisor,
            ejecutado_por: estado.ejecutado_por,
            informe,
        })
    }
}
