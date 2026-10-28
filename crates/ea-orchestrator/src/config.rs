//! Configuración del ejecutor: la parte de los ajustes (`ea-ajustes`) que
//! necesita para lanzar al agente que ocupa el rol de ejecutor. Los modelos
//! se nombran en `ajustes.toml` (perfiles `modelos.*`, o los heredados
//! `modelo.*` y `orquestador.toml`); en el código solo aparecen en los
//! valores por defecto de `ea-ajustes`.

use ea_ajustes::{Ajustes, Fuentes, PerfilModelo, Rol};
use ea_bus::{ConfigSkills, EjecutadoPor, TipoAgente, Topes};

/// El agente que ocupa el rol de ejecutor, con su binario y su modelo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerfilEjecutor {
    /// Id del catálogo (`qwen-code`, `codex`, `opencode`).
    pub id: String,
    pub tipo: TipoAgente,
    pub bin: String,
    /// Siempre local: `ea-ajustes` rechaza un ejecutor con modelo remoto.
    pub modelo: PerfilModelo,
}

impl PerfilEjecutor {
    /// Lo que se anota en el trabajo al tomarlo.
    pub fn ejecutado_por(&self) -> EjecutadoPor {
        EjecutadoPor { agente: self.id.clone(), modelo: self.modelo.nombre.clone() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigOrquestador {
    pub ejecutor: PerfilEjecutor,
    /// Techo de los topes de cualquier orden.
    pub techo_topes: Topes,
    pub avisos_escritorio: bool,
    /// Ejecuta al agente dentro de bubblewrap (solo el worktree es escribible).
    pub sandbox: bool,
    /// Catálogo de skills del ejecutor, cuáles se pueden usar y quién las elige.
    pub skills: ConfigSkills,
}

fn ejecutor(a: &Ajustes) -> PerfilEjecutor {
    let (id, config) = a.agente_de(Rol::Ejecutor);
    // `validar_roles` garantiza que el ejecutor es de uno de estos tipos y
    // que tiene un perfil local; si no, el de fábrica.
    match (TipoAgente::desde(id), a.modelo_de(id)) {
        (Some(tipo), Some(modelo)) => {
            PerfilEjecutor { id: id.to_string(), tipo, bin: config.bin.clone(), modelo: modelo.clone() }
        }
        _ => ejecutor(&Ajustes::por_defecto(&Fuentes::sistema())),
    }
}

impl ConfigOrquestador {
    pub fn desde_ajustes(a: &Ajustes) -> Self {
        Self {
            ejecutor: ejecutor(a),
            techo_topes: Topes::from(&a.agente.techo_topes),
            avisos_escritorio: a.agente.avisos_escritorio,
            sandbox: a.agente.sandbox,
            skills: ConfigSkills::desde_ajustes(a),
        }
    }

    /// La configuración de un ejecutor concreto (spec 019, RF-008): la misma
    /// que [`ConfigOrquestador::desde_ajustes`] pero con la ficha y el perfil
    /// de modelo local del agente `id` (`agentes.<id>.bin`), no los de
    /// `roles.ejecutor`. `None` si `id` no es un agente que pueda ejecutar o
    /// su perfil de modelo no existe o es remoto.
    pub fn para_agente(a: &Ajustes, id: &str) -> Option<Self> {
        let tipo = TipoAgente::desde(id)?;
        let modelo = a.modelo_de(id).filter(|m| !m.remoto)?;
        let bin = a.agentes.get(id)?.bin.clone();
        Some(Self { ejecutor: PerfilEjecutor { id: id.to_string(), tipo, bin, modelo: modelo.clone() }, ..Self::desde_ajustes(a) })
    }
}

impl Default for ConfigOrquestador {
    fn default() -> Self {
        Self::desde_ajustes(&Ajustes::por_defecto(&Fuentes::sistema()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_configuracion_del_ejecutor_sale_de_los_roles() {
        let d = tempfile::tempdir().unwrap();
        let f = Fuentes::aislada(d.path());
        let c = ConfigOrquestador::desde_ajustes(&ea_ajustes::cargar_de(&f).ajustes);
        assert_eq!(c, ConfigOrquestador::desde_ajustes(&Ajustes::por_defecto(&f)));
        assert_eq!(ConfigOrquestador::default().ejecutor.id, "qwen-code");
        assert_eq!((c.ejecutor.id.as_str(), c.ejecutor.tipo, c.ejecutor.bin.as_str()), ("qwen-code", TipoAgente::QwenCode, "qwen"));
        assert_eq!((c.ejecutor.modelo.nombre.as_str(), c.ejecutor.modelo.base_url.as_str()), ("qwen3-coder-30b", "http://127.0.0.1:8080/v1"));
        assert_eq!(c.techo_topes.turnos, 100);
        assert_eq!(c.skills.planificador, "claude-code");
        assert!(c.avisos_escritorio);

        // Otro ejecutor y otro planificador: el perfil sigue al agente.
        std::fs::create_dir_all(&f.dir_config).unwrap();
        std::fs::write(
            f.dir_config.join("ajustes.toml"),
            "[roles]\nplanificador = \"opencode\"\nejecutor = \"codex\"\n[modelos.otro]\nnombre = \"otro-7b\"\nbase_url = \"http://127.0.0.1:8081/v1\"\n[agentes.codex]\nmodelo = \"otro\"\n",
        )
        .unwrap();
        let c = ConfigOrquestador::desde_ajustes(&ea_ajustes::cargar_de(&f).ajustes);
        assert_eq!((c.ejecutor.id.as_str(), c.ejecutor.tipo, c.ejecutor.bin.as_str()), ("codex", TipoAgente::Codex, "codex"));
        assert_eq!(c.ejecutor.ejecutado_por(), EjecutadoPor { agente: "codex".into(), modelo: "otro-7b".into() });
        assert_eq!(c.skills.planificador, "opencode");

        // El `orquestador.toml` heredado sigue valiendo (lo lee `ea-ajustes`).
        std::fs::remove_file(f.dir_config.join("ajustes.toml")).unwrap();
        std::fs::write(f.dir_config.join("orquestador.toml"), "modelo = \"otro\"\n[topes]\nturnos = 5\n").unwrap();
        let c = ConfigOrquestador::desde_ajustes(&ea_ajustes::cargar_de(&f).ajustes);
        assert_eq!((c.ejecutor.modelo.nombre.as_str(), c.techo_topes.turnos, c.techo_topes.tiempo_s), ("otro", 5, 3600));
    }
}
