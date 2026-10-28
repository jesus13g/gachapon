//! Detector de Claude Code: cada sesión viva (`~/.claude/sessions`, ver
//! `crate::sesiones`) es una máquina, y cada subagente que apunta el hook
//! `ea hook subagentes` es una máquina pequeña colgada de ella.

use super::familia::familia;
use super::{AgenteVivo, EstadoAgente};
use crate::sesiones::Sesion;

/// Cuánto sigue en escena un subagente terminado: lo justo para verlo
/// apagarse aunque haya durado menos que un sondeo.
pub const GRACIA_MS: u64 = 15_000;

const AGENTE: &str = ea_ajustes::AgenteId::ClaudeCode.as_str();

/// `modelo` da el modelo de una sesión (de su transcripción); `rol`, el que
/// ocupa Claude Code según los ajustes, si ocupa alguno.
pub fn detectar(
    sesiones: Vec<Sesion>,
    modelo: impl Fn(&Sesion) -> Option<String>,
    rol: Option<&str>,
    ahora_ms: u64,
) -> Vec<AgenteVivo> {
    let mut v = Vec::new();
    for s in sesiones {
        let id = format!("claude:{}", if s.sesion.is_empty() { s.pid.to_string() } else { s.sesion.clone() });
        let modelo_sesion = modelo(&s);
        for sub in &s.subagentes {
            let estado = match sub.hasta {
                None => EstadoAgente::Trabajando,
                Some(h) if ahora_ms.saturating_sub(h) < GRACIA_MS => EstadoAgente::Terminado,
                Some(_) => continue,
            };
            // `inherit` (o nada): el del padre.
            let modelo = Some(sub.modelo.clone()).filter(|m| !m.is_empty() && m != "inherit").or_else(|| modelo_sesion.clone());
            v.push(AgenteVivo {
                id: format!("{id}/{}", sub.id),
                agente: Some(AGENTE.into()),
                nombre: if sub.tipo.is_empty() { "subagente".into() } else { sub.tipo.clone() },
                familia: familia(Some(AGENTE), modelo.as_deref(), false),
                modelo,
                local: false,
                rol: Some("subagente".into()),
                padre: Some(id.clone()),
                estado,
                actividad: Some(sub.descripcion.clone()).filter(|d| !d.is_empty()),
                trabajo: None,
                proyecto: Some(s.cwd.clone()),
                desde: Some(sub.desde),
                fuente: "subagente",
                equipo: None,
                pistas: Vec::new(),
            });
        }
        v.push(AgenteVivo {
            familia: familia(Some(AGENTE), modelo_sesion.as_deref(), false),
            modelo: modelo_sesion,
            agente: Some(AGENTE.into()),
            nombre: "Claude Code".into(),
            local: false,
            rol: rol.map(Into::into),
            padre: None,
            estado: if s.ocupada { EstadoAgente::Trabajando } else { EstadoAgente::Esperando },
            actividad: Some(s.nombre.clone()).filter(|n| !n.is_empty()),
            trabajo: None,
            desde: Some(s.desde),
            fuente: "claude-code",
            equipo: None,
            pistas: vec![s.cwd.clone(), s.sesion.clone()],
            proyecto: Some(s.cwd),
            id,
        });
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sesiones::Subagente;

    fn sesion(id: &str, ocupada: bool, subs: Vec<Subagente>) -> Sesion {
        Sesion { pid: 7, sesion: id.into(), nombre: "f5".into(), cwd: "/p/x".into(), ocupada, desde: 1, actualizada: 2, subagentes: subs }
    }
    fn sub(id: &str, modelo: &str, hasta: Option<u64>) -> Subagente {
        Subagente { id: id.into(), descripcion: format!("hace {id}"), tipo: "Explore".into(), modelo: modelo.into(), desde: 10, hasta }
    }

    #[test]
    fn la_sesion_es_la_maquina_y_sus_subagentes_cuelgan_de_ella() {
        let ahora = 100_000;
        let s = sesion("s1", true, vec![
            sub("a", "haiku", None),
            sub("b", "inherit", Some(ahora - 1_000)),
            sub("c", "", Some(ahora - GRACIA_MS - 1)),
        ]);
        let v = detectar(vec![s], |_| Some("claude-opus-5-5".into()), Some("planificador"), ahora);
        let ids: Vec<_> = v.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["claude:s1/a", "claude:s1/b", "claude:s1"], "el terminado hace rato ya no sale");
        let padre = &v[2];
        assert_eq!((padre.estado, padre.rol.as_deref(), padre.familia), (EstadoAgente::Trabajando, Some("planificador"), "claude"));
        assert_eq!(padre.modelo.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(v[0].padre.as_deref(), Some("claude:s1"));
        assert_eq!((v[0].estado, v[0].modelo.as_deref()), (EstadoAgente::Trabajando, Some("haiku")));
        assert_eq!((v[1].estado, v[1].modelo.as_deref()), (EstadoAgente::Terminado, Some("claude-opus-5-5")), "inherit: el del padre");
        assert_eq!(padre.pistas, ["/p/x", "s1"]);
    }

    #[test]
    fn una_sesion_libre_espera_y_sin_id_usa_el_pid() {
        let v = detectar(vec![sesion("", false, vec![])], |_| None, None, 0);
        assert_eq!((v[0].id.as_str(), v[0].estado, v[0].modelo.as_deref()), ("claude:7", EstadoAgente::Esperando, None));
    }
}
