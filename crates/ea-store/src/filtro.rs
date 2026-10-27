//! El filtro del diario tal como llega de la CLI, la web y el MCP (en texto)
//! y las fechas: la «fecha flexible» de los filtros y el RFC 3339 de las
//! marcas de tiempo. Cada superficie solo traduce su entrada y sus errores.

use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::OffsetDateTime;

use crate::models::EventOutcome;
use crate::repository::events::EventFilter;

/// Por qué no vale un filtro del diario. Los textos son los de siempre: la
/// web y el MCP los enseñan tal cual y la CLI, con la causa de la fecha.
#[derive(Debug, thiserror::Error)]
pub enum ErrorFiltro {
    #[error("fecha inválida: '{texto}' (usa RFC3339 o AAAA-MM-DD)")]
    Fecha {
        texto: String,
        #[source]
        causa: time::error::Parse,
    },
    #[error("valor de resultado inválido: {0}")]
    Resultado(String),
    #[error("acción no válida: «{0}» (solo [a-z_])")]
    Accion(String),
    #[error("antes_de no válido: «{0}» (un id de evento)")]
    AntesDe(String),
}

/// Los filtros de un listado del diario, en texto, tal como los escribe quien
/// pregunta. Un campo vacío es un valor (la web quita antes los suyos).
#[derive(Debug, Clone, Default)]
pub struct FiltroDiario {
    pub trabajo: Option<String>,
    pub actor: Option<String>,
    /// Fecha mínima: RFC 3339 o `AAAA-MM-DD` ([`fecha_flexible`]).
    pub desde: Option<String>,
    /// Fecha máxima, como `desde`.
    pub hasta: Option<String>,
    /// `ok` o `error`.
    pub resultado: Option<String>,
    /// Prefijo de la acción, la familia de eventos (`bus_`, `mcp_`…): solo `[a-z_]`.
    pub accion: Option<String>,
    /// Cursor: solo los eventos anteriores al de este id (un entero > 0).
    pub antes_de: Option<String>,
    pub limite: Option<i64>,
}

impl FiltroDiario {
    /// El filtro validado. `limite_max` recorta el límite a `1..=limite_max`;
    /// con `None`, el límite va tal cual.
    pub fn a_filtro(self, limite_max: Option<i64>) -> Result<EventFilter, ErrorFiltro> {
        let outcome = self
            .resultado
            .map(|r| r.parse::<EventOutcome>().map_err(|_| ErrorFiltro::Resultado(r)))
            .transpose()?;
        if let Some(a) = &self.accion
            && !prefijo_accion_valido(a)
        {
            return Err(ErrorFiltro::Accion(a.clone()));
        }
        let before_id = self
            .antes_de
            .map(|a| a.parse::<i64>().ok().filter(|n| *n > 0).ok_or(ErrorFiltro::AntesDe(a)))
            .transpose()?;
        Ok(EventFilter {
            trabajo: self.trabajo,
            actor: self.actor,
            since: self.desde.as_deref().map(fecha_flexible).transpose()?,
            until: self.hasta.as_deref().map(fecha_flexible).transpose()?,
            exclude_action: None,
            outcome,
            action_prefix: self.accion,
            before_id,
            limit: self.limite.map(|l| limite_max.map_or(l, |m| l.clamp(1, m))),
        })
    }
}

/// Un prefijo de acción del diario: letras minúsculas y `_`, como los nombres
/// de las acciones, y no vacío.
pub fn prefijo_accion_valido(prefijo: &str) -> bool {
    !prefijo.is_empty() && prefijo.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}

/// Una fecha-hora RFC 3339 completa o una fecha `AAAA-MM-DD` (medianoche
/// UTC), para que los filtros `desde`/`hasta` sean cómodos de escribir a mano.
pub fn fecha_flexible(texto: &str) -> Result<OffsetDateTime, ErrorFiltro> {
    if let Ok(dt) = OffsetDateTime::parse(texto, &Rfc3339) {
        return Ok(dt);
    }
    time::Date::parse(texto, &format_description!("[year]-[month]-[day]"))
        .map(|d| d.midnight().assume_utc())
        .map_err(|causa| ErrorFiltro::Fecha { texto: texto.to_string(), causa })
}

/// `ts` en RFC 3339 (`2026-01-01T00:00:00Z`); vacío si no se puede escribir
/// así (un año fuera de 0..=9999).
pub fn rfc3339(ts: OffsetDateTime) -> String {
    ts.format(&Rfc3339).unwrap_or_default()
}

/// Ahora, en UTC y RFC 3339: las marcas del diario, del bus y de las vistas.
pub fn ahora_rfc3339() -> String {
    rfc3339(OffsetDateTime::now_utc())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    /// Lo que aceptaban (y rechazaban) `ea diario listar --desde`, la web y
    /// `ea_listar_diario` antes de compartir esta función.
    #[test]
    fn la_fecha_flexible_acepta_rfc3339_y_aaaa_mm_dd() {
        for (texto, esperado) in [
            ("2026-10-27T12:34:56Z", datetime!(2026-10-27 12:34:56 UTC)),
            ("2026-10-27T12:34:56.5Z", datetime!(2026-10-27 12:34:56.5 UTC)),
            ("2026-10-27T12:34:56+02:00", datetime!(2026-10-27 10:34:56 UTC)),
            ("2026-10-27T12:34:56-05:30", datetime!(2026-10-27 18:04:56 UTC)),
            ("2026-10-27t12:34:56z", datetime!(2026-10-27 12:34:56 UTC)),
            ("2026-10-27 12:34:56Z", datetime!(2026-10-27 12:34:56 UTC)),
            ("2026-10-27", datetime!(2026-10-27 00:00:00 UTC)),
            ("+2026-10-27", datetime!(2026-10-27 00:00:00 UTC)),
            ("0000-01-01", datetime!(0000-01-01 00:00:00 UTC)),
            ("9999-12-31", datetime!(9999-12-31 00:00:00 UTC)),
        ] {
            assert_eq!(fecha_flexible(texto).unwrap(), esperado, "{texto}");
        }
        for texto in [
            "",
            "2026-10-27T12:34:56",
            "2026-10-27T12:34Z",
            "2026-10-27Z",
            "2026-1-7",
            "20261027",
            " 2026-10-27",
            "2026-02-30",
            "2026-10-27T24:00:00Z",
            "ayer",
        ] {
            let e = fecha_flexible(texto).unwrap_err();
            assert_eq!(e.to_string(), format!("fecha inválida: '{texto}' (usa RFC3339 o AAAA-MM-DD)"));
            assert!(std::error::Error::source(&e).is_some(), "la causa de {texto:?} se conserva");
        }
    }

    #[test]
    fn el_filtro_valida_y_recorta_el_limite_solo_si_se_le_pide() {
        let f = FiltroDiario {
            trabajo: Some("t".into()),
            actor: Some(String::new()),
            desde: Some("2026-01-01".into()),
            resultado: Some("error".into()),
            accion: Some("bus_".into()),
            antes_de: Some("7".into()),
            limite: Some(5000),
            ..Default::default()
        }
        .a_filtro(Some(1000))
        .unwrap();
        assert_eq!((f.trabajo.as_deref(), f.actor.as_deref()), (Some("t"), Some("")));
        assert_eq!((f.since, f.until), (Some(datetime!(2026-01-01 00:00:00 UTC)), None));
        assert_eq!((f.outcome, f.action_prefix.as_deref(), f.before_id), (Some(EventOutcome::Error), Some("bus_"), Some(7)));
        assert_eq!(f.limit, Some(1000));
        let limite = |l, max| FiltroDiario { limite: l, ..Default::default() }.a_filtro(max).unwrap().limit;
        assert_eq!(limite(Some(0), Some(1000)), Some(1));
        assert_eq!(limite(Some(0), None), Some(0));
        assert_eq!(limite(Some(-3), None), Some(-3));
        assert_eq!(limite(None, Some(1000)), None);
    }

    #[test]
    fn cada_error_del_filtro_dice_lo_de_siempre() {
        let error = |f: FiltroDiario| f.a_filtro(None).unwrap_err().to_string();
        assert_eq!(error(FiltroDiario { resultado: Some("otro".into()), ..Default::default() }), "valor de resultado inválido: otro");
        for a in ["BUS", "bus-", ""] {
            assert_eq!(error(FiltroDiario { accion: Some(a.into()), ..Default::default() }), format!("acción no válida: «{a}» (solo [a-z_])"));
        }
        for a in ["0", "-1", "x", ""] {
            assert_eq!(error(FiltroDiario { antes_de: Some(a.into()), ..Default::default() }), format!("antes_de no válido: «{a}» (un id de evento)"));
        }
        assert_eq!(error(FiltroDiario { hasta: Some("ayer".into()), ..Default::default() }), "fecha inválida: 'ayer' (usa RFC3339 o AAAA-MM-DD)");
        // El orden de siempre: el resultado antes que la acción, el cursor y las fechas.
        let todo_mal = FiltroDiario {
            desde: Some("x".into()),
            resultado: Some("otro".into()),
            accion: Some("X".into()),
            antes_de: Some("0".into()),
            ..Default::default()
        };
        assert_eq!(error(todo_mal), "valor de resultado inválido: otro");
    }

    #[test]
    fn rfc3339_escribe_la_marca_de_siempre() {
        assert_eq!(rfc3339(datetime!(2026-01-01 00:00:00 UTC)), "2026-01-01T00:00:00Z");
        assert_eq!(rfc3339(datetime!(2026-01-01 00:00:00.25 +02:00)), "2026-01-01T00:00:00.25+02:00");
        assert!(OffsetDateTime::parse(&ahora_rfc3339(), &Rfc3339).is_ok());
    }
}
