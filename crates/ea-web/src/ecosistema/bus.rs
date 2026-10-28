//! Detector del bus: los agentes del catálogo despiertos según su presencia
//! (`agentes/<id>/presencia.json`: el ejecutor del orquestador y los miembros
//! de un equipo) y, si un semáforo está cogido por un agente sin presencia
//! (un orquestador de antes de la 019), ese agente también.

use ea_ajustes::Ajustes;
use ea_bus::Presencia;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::familia::familia;
use super::{AgenteVivo, EstadoAgente, SemaforoBus};

pub fn detectar(presencias: &[(Presencia, bool)], semaforos: &[SemaforoBus], ajustes: &Ajustes) -> Vec<AgenteVivo> {
    let mut v: Vec<AgenteVivo> = presencias
        .iter()
        .filter(|(_, viva)| *viva)
        .map(|(p, _)| {
            let mut a = maquina(&p.agente, ajustes, "presencia");
            a.rol = Some(p.rol.as_str().into());
            a.equipo = p.equipo.clone();
            a.trabajo = p.trabajo.clone();
            a.desde = ms(&p.desde);
            if a.trabajo.is_some() {
                a.estado = EstadoAgente::Trabajando;
            }
            a
        })
        .collect();
    for s in semaforos {
        if !v.iter().any(|a| a.agente.as_deref() == Some(&s.agente)) {
            let mut a = maquina(&s.agente, ajustes, "semaforo");
            a.rol = ajustes.rol_de(&s.agente).map(|r| r.nombre().into());
            v.push(a);
        }
    }
    v
}

/// La máquina de un agente del catálogo, encendida y esperando, con el modelo
/// de su perfil en los ajustes.
fn maquina(agente: &str, ajustes: &Ajustes, fuente: &'static str) -> AgenteVivo {
    let perfil = ajustes.modelo_de(agente);
    let modelo = perfil.map(|m| m.nombre.clone()).filter(|m| !m.is_empty());
    let local = perfil.is_some_and(|m| !m.remoto && !m.base_url.is_empty());
    AgenteVivo {
        id: format!("agente:{agente}"),
        agente: Some(agente.into()),
        nombre: ea_ajustes::ficha(agente).map_or_else(|| agente.to_string(), |f| f.nombre.to_string()),
        familia: familia(Some(agente), modelo.as_deref(), local),
        modelo,
        local,
        rol: None,
        padre: None,
        estado: EstadoAgente::Esperando,
        actividad: None,
        trabajo: None,
        proyecto: None,
        desde: None,
        fuente,
        equipo: None,
        pistas: Vec::new(),
    }
}

fn ms(rfc3339: &str) -> Option<u64> {
    OffsetDateTime::parse(rfc3339, &Rfc3339).ok().map(|t| (t.unix_timestamp_nanos() / 1_000_000) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ea_ajustes::Fuentes;
    use ea_bus::RolEquipo;

    #[test]
    fn solo_las_presencias_vivas_y_los_semaforos_sin_presencia() {
        let d = tempfile::tempdir().unwrap();
        let a = Ajustes::por_defecto(&Fuentes::aislada(d.path()));
        let mut viva = Presencia::nueva("qwen-code", RolEquipo::Ejecutor, None, 1);
        viva.trabajo = Some("t1".into());
        let muerta = Presencia::nueva("codex", RolEquipo::Ejecutor, Some("eq"), 1);
        let sems = [SemaforoBus { agente: "qwen-code".into(), trabajo: "t1".into() }, SemaforoBus { agente: "opencode".into(), trabajo: "t2".into() }];
        let v = detectar(&[(viva, true), (muerta, false)], &sems, &a);
        let ids: Vec<_> = v.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, ["agente:qwen-code", "agente:opencode"]);
        assert_eq!((v[0].estado, v[0].rol.as_deref(), v[0].familia, v[0].local), (EstadoAgente::Trabajando, Some("ejecutor"), "qwen", true));
        assert!(v[0].desde.is_some());
        assert_eq!((v[1].fuente, v[1].nombre.as_str()), ("semaforo", "OpenCode"));
    }
}
