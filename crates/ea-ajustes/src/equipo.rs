//! Validación de los miembros de un equipo (spec 019, RF-002).
//!
//! Un equipo es un grupo de agentes del catálogo con un rol cada uno dentro
//! del equipo. La regla es única para la CLI, la web y el orquestador.

use std::ffi::OsStr;

use serde::{Deserialize, Serialize};

use crate::agentes::{buscar_binario, ficha, Rol};
use crate::fuentes::Ajustes;

/// El papel de un agente dentro de un equipo. El revisor es un planificador
/// que solo revisa: no crea trabajo ni cierra ni reformula.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RolEquipo {
    Planificador,
    Ejecutor,
    Revisor,
}

impl RolEquipo {
    pub fn as_str(self) -> &'static str {
        match self {
            RolEquipo::Planificador => "planificador",
            RolEquipo::Ejecutor => "ejecutor",
            RolEquipo::Revisor => "revisor",
        }
    }

    pub fn desde(texto: &str) -> Option<RolEquipo> {
        match texto.trim() {
            "planificador" => Some(RolEquipo::Planificador),
            "ejecutor" => Some(RolEquipo::Ejecutor),
            "revisor" => Some(RolEquipo::Revisor),
            _ => None,
        }
    }

    /// El rol del catálogo que el agente tiene que admitir para tener este.
    pub fn rol_base(self) -> Rol {
        match self {
            RolEquipo::Ejecutor => Rol::Ejecutor,
            RolEquipo::Planificador | RolEquipo::Revisor => Rol::Planificador,
        }
    }
}

/// Un miembro de un equipo: el id del agente del catálogo y su rol en él.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Miembro {
    pub agente: String,
    pub rol: RolEquipo,
}

/// Comprueba los miembros de un equipo contra el catálogo y los ajustes:
/// - cada agente es del catálogo y aparece una sola vez;
/// - cada uno admite el rol que se le pide (`ejecutor` → Ejecutor;
///   `planificador` y `revisor` → Planificador);
/// - el ejecutor tiene un perfil de modelo que existe y es local (art. VII);
/// - hay al menos un ejecutor y un planificador.
///
/// Devuelve el motivo del primer problema. La disponibilidad del binario es
/// aparte ([`comprobar_disponibles`]).
pub fn validar_equipo(miembros: &[Miembro], a: &Ajustes) -> Result<(), String> {
    let mut vistos: Vec<&str> = Vec::new();
    for m in miembros {
        let f = ficha(&m.agente).ok_or_else(|| format!("{:?} no es un agente del catálogo", m.agente))?;
        if vistos.contains(&m.agente.as_str()) {
            return Err(format!("{} aparece más de una vez: un agente ocupa un solo rol en el equipo", f.nombre));
        }
        vistos.push(&m.agente);
        if !f.admite(m.rol.rol_base()) {
            return Err(format!("{} no puede ser {}", f.nombre, m.rol.as_str()));
        }
        if m.rol == RolEquipo::Ejecutor {
            let perfil = a.agentes.get(&m.agente).map(|g| g.modelo.as_str()).unwrap_or_default();
            if perfil.is_empty() {
                return Err(format!("{} es ejecutor y necesita un perfil de modelo local (agentes.{}.modelo)", f.nombre, m.agente));
            }
            match a.modelos.get(perfil) {
                None => return Err(format!("{}: no existe el perfil de modelo {perfil:?}", f.nombre)),
                Some(p) if p.remoto => {
                    return Err(format!("{} es ejecutor y su perfil {perfil:?} es remoto: el ejecutor solo usa modelos locales", f.nombre));
                }
                Some(_) => {}
            }
        }
    }
    if !miembros.iter().any(|m| m.rol == RolEquipo::Ejecutor) {
        return Err("el equipo necesita al menos un ejecutor".into());
    }
    if !miembros.iter().any(|m| m.rol == RolEquipo::Planificador) {
        return Err("el equipo necesita al menos un planificador".into());
    }
    Ok(())
}

/// Todos los agentes del equipo tienen su binario en `path` (o en la ruta
/// indicada en `agentes.<id>.bin`). Aparte de [`validar_equipo`] para que las
/// pruebas no dependan del `PATH` real.
pub fn comprobar_disponibles(miembros: &[Miembro], a: &Ajustes, path: Option<&OsStr>) -> Result<(), String> {
    for m in miembros {
        let bin = a.agentes.get(&m.agente).map(|g| g.bin.as_str()).unwrap_or_default();
        if buscar_binario(bin, path).is_none() {
            let nombre = ficha(&m.agente).map_or(m.agente.as_str(), |f| f.nombre);
            return Err(format!("{nombre} no está disponible: no se encuentra el binario {bin:?}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Fuentes;

    fn ajustes() -> (tempfile::TempDir, Ajustes) {
        let d = tempfile::tempdir().unwrap();
        let a = Ajustes::por_defecto(&Fuentes::aislada(d.path()));
        (d, a)
    }

    fn m(agente: &str, rol: RolEquipo) -> Miembro {
        Miembro { agente: agente.into(), rol }
    }

    fn bien() -> Vec<Miembro> {
        vec![m("claude-code", RolEquipo::Planificador), m("qwen-code", RolEquipo::Ejecutor), m("codex", RolEquipo::Ejecutor)]
    }

    #[test]
    fn un_equipo_correcto_pasa_y_el_revisor_es_opcional() {
        let (_d, a) = ajustes();
        assert_eq!(validar_equipo(&bien(), &a), Ok(()));
        let mut con_revisor = bien();
        con_revisor.push(m("antigravity", RolEquipo::Revisor));
        assert_eq!(validar_equipo(&con_revisor, &a), Ok(()));
    }

    #[test]
    fn rechaza_agente_repetido_desconocido_y_rol_no_admitido() {
        let (_d, a) = ajustes();
        let mut v = bien();
        v.push(m("codex", RolEquipo::Revisor));
        assert!(validar_equipo(&v, &a).unwrap_err().contains("más de una vez"));
        let mut v = bien();
        v.push(m("otro", RolEquipo::Revisor));
        assert!(validar_equipo(&v, &a).unwrap_err().contains("catálogo"));
        // Antigravity solo admite Planificador: no puede ejecutar
        let mut v = bien();
        v.push(m("antigravity", RolEquipo::Ejecutor));
        assert!(validar_equipo(&v, &a).unwrap_err().contains("no puede ser ejecutor"));
        // Claude Code solo admite Planificador
        let v = vec![m("claude-code", RolEquipo::Ejecutor), m("qwen-code", RolEquipo::Planificador)];
        assert!(validar_equipo(&v, &a).is_err());
    }

    #[test]
    fn el_ejecutor_necesita_modelo_local() {
        let (_d, mut a) = ajustes();
        a.modelos.get_mut("local").unwrap().remoto = true;
        assert!(validar_equipo(&bien(), &a).unwrap_err().contains("remoto"));
        a.modelos.get_mut("local").unwrap().remoto = false;
        a.agentes.get_mut("codex").unwrap().modelo = String::new();
        assert!(validar_equipo(&bien(), &a).unwrap_err().contains("perfil de modelo local"));
        a.agentes.get_mut("codex").unwrap().modelo = "fantasma".into();
        assert!(validar_equipo(&bien(), &a).unwrap_err().contains("no existe el perfil"));
    }

    #[test]
    fn exige_un_ejecutor_y_un_planificador() {
        let (_d, a) = ajustes();
        let sin_ej = vec![m("claude-code", RolEquipo::Planificador), m("antigravity", RolEquipo::Revisor)];
        assert!(validar_equipo(&sin_ej, &a).unwrap_err().contains("ejecutor"));
        let sin_pl = vec![m("qwen-code", RolEquipo::Ejecutor), m("antigravity", RolEquipo::Revisor)];
        assert!(validar_equipo(&sin_pl, &a).unwrap_err().contains("planificador"));
        assert!(validar_equipo(&[], &a).is_err());
    }

    #[test]
    fn la_disponibilidad_se_comprueba_aparte() {
        let (d, mut a) = ajustes();
        let bin = d.path().join("falso");
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&bin, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        for id in ["claude-code", "qwen-code", "codex"] {
            a.agentes.get_mut(id).unwrap().bin = bin.to_string_lossy().into_owned();
        }
        assert_eq!(comprobar_disponibles(&bien(), &a, None), Ok(()));
        a.agentes.get_mut("codex").unwrap().bin = "no-existe-nunca".into();
        assert!(comprobar_disponibles(&bien(), &a, None).unwrap_err().contains("Codex"));
    }

    #[test]
    fn el_rol_se_escribe_en_minusculas() {
        assert_eq!(serde_json::to_string(&m("codex", RolEquipo::Revisor)).unwrap(), r#"{"agente":"codex","rol":"revisor"}"#);
        assert_eq!(RolEquipo::desde("ejecutor"), Some(RolEquipo::Ejecutor));
        assert_eq!(RolEquipo::desde("modelo-local"), None);
    }
}
