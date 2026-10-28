//! Skills del ejecutor: ficheros `<nombre>.md` cortos en un catálogo propio
//! (`qwen-skills`, por su origen; no son las skills del planificador).
//! Ninguna está activa por defecto: quien crea el trabajo elige cuáles lleva,
//! y el ejecutor inyecta solo esas, porque el contexto de un modelo local es
//! escaso.
//!
//! Formato de cada skill:
//!
//! ```text
//! ---
//! descripcion: una línea que lee el planificador para decidir
//! ---
//! cuerpo que lee el ejecutor
//! ```
//!
//! Qué skills hay disponibles, cuántas caben y quién puede elegirlas lo
//! define el usuario en `[agente.skills]` de `ajustes.toml` (o en la sección
//! heredada `[skills]` de `orquestador.toml`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{ErrorBus, Resultado};
use crate::tipos::{Orden, Rol};

/// `[agente.skills]` de los ajustes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ConfigSkills {
    /// Catálogo de skills (`agente.skills.dir` de los ajustes, que nunca está
    /// vacío: por defecto `<config>/qwen-skills`).
    pub dir: String,
    /// Skills que se pueden usar. Vacío: todas las del catálogo.
    pub habilitadas: Vec<String>,
    /// Máximo de skills en un trabajo (o en una fase de obra).
    pub max_por_trabajo: usize,
    /// Máximo de caracteres que suman sus cuerpos.
    pub max_caracteres: usize,
    /// La llave: quién puede elegir skills. `planificador` (o su nombre en
    /// disco, `claude-code`) es el rol; otro valor, un `creada_por` literal.
    pub pueden_elegir: Vec<String>,
    /// El agente que ocupa ahora el rol de planificador (`roles.planificador`).
    #[serde(skip)]
    pub planificador: String,
}

impl Default for ConfigSkills {
    fn default() -> Self {
        Self {
            dir: String::new(),
            habilitadas: vec![],
            max_por_trabajo: 3,
            max_caracteres: 6000,
            pueden_elegir: vec!["claude-code".into()],
            planificador: "claude-code".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skill {
    pub nombre: String,
    pub descripcion: String,
    pub cuerpo: String,
    pub habilitada: bool,
}

impl ConfigSkills {
    pub fn ruta_catalogo(&self) -> PathBuf {
        ea_ajustes::expandir_tilde(&self.dir)
    }

    fn habilitada(&self, nombre: &str) -> bool {
        self.habilitadas.is_empty() || self.habilitadas.iter().any(|h| h == nombre)
    }

    /// Todas las skills del catálogo, ordenadas por nombre.
    pub fn catalogo(&self) -> Resultado<Vec<Skill>> {
        catalogo_en(&self.ruta_catalogo(), self)
    }

    /// `creada_por` tiene la llave (spec 017, RF-012): si la tiene el rol
    /// planificador, vale el nombre del rol (`planificador` o `claude-code`) y
    /// el id del agente que lo ocupa al crear el trabajo.
    pub fn tiene_llave(&self, creada_por: &str) -> bool {
        let rol = |v: &str| Rol::desde(v) == Some(Rol::Planificador);
        self.pueden_elegir.iter().any(|p| {
            p == creada_por || (rol(p) && (rol(creada_por) || (!self.planificador.is_empty() && creada_por == self.planificador)))
        })
    }

    /// Comprueba que `creada_por` tiene la llave y que las skills existen,
    /// están habilitadas y caben. Devuelve las skills en el orden pedido.
    pub fn resolver(&self, creada_por: &str, nombres: &[String]) -> Resultado<Vec<Skill>> {
        if nombres.is_empty() {
            return Ok(vec![]);
        }
        let err = |m: String| Err(ErrorBus::Skills(m));
        if !self.tiene_llave(creada_por) {
            return err(format!(
                "{creada_por:?} no tiene la llave para elegir skills (pueden_elegir = {:?})",
                self.pueden_elegir
            ));
        }
        if nombres.len() > self.max_por_trabajo {
            return err(format!("{} skills; el máximo es {}", nombres.len(), self.max_por_trabajo));
        }
        let catalogo = self.catalogo()?;
        let mut elegidas: Vec<Skill> = Vec::new();
        for n in nombres {
            if elegidas.iter().any(|s| &s.nombre == n) {
                continue;
            }
            match catalogo.iter().find(|s| &s.nombre == n) {
                None => return err(format!("no existe la skill {n:?} en {}", self.ruta_catalogo().display())),
                Some(s) if !s.habilitada => return err(format!("la skill {n:?} no está habilitada")),
                Some(s) => elegidas.push(s.clone()),
            }
        }
        let total: usize = elegidas.iter().map(|s| s.cuerpo.chars().count()).sum();
        if total > self.max_caracteres {
            return err(format!("las skills suman {total} caracteres; el máximo es {}", self.max_caracteres));
        }
        Ok(elegidas)
    }

    /// Valida las skills de la orden y las de cada fase (llave, existencia,
    /// habilitación y topes). Es lo que hace `Bus::crear_trabajo` al crear,
    /// para que todos los puntos de entrada lo hereden.
    pub fn validar_orden(&self, orden: &Orden) -> Resultado<()> {
        self.resolver(&orden.creada_por, &orden.skills)?;
        for f in &orden.fases {
            self.resolver(&orden.creada_por, &f.skills).map_err(|e| match e {
                ErrorBus::Skills(m) => ErrorBus::Skills(format!("fase {}: {m}", f.numero)),
                otro => otro,
            })?;
        }
        Ok(())
    }
}

/// La orden pide alguna skill, en ella o en alguna de sus fases.
pub fn pide_skills(orden: &Orden) -> bool {
    !orden.skills.is_empty() || orden.fases.iter().any(|f| !f.skills.is_empty())
}

/// Texto que se añade al system prompt del ejecutor; vacío si no hay skills.
pub fn bloque_prompt(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut t = String::from("# Skills para esta tarea\nSigue estas pautas además de las instrucciones de la tarea.\n");
    for s in skills {
        t.push_str(&format!("\n## {}\n{}\n", s.nombre, s.cuerpo.trim()));
    }
    t
}

fn catalogo_en(dir: &Path, cfg: &ConfigSkills) -> Resultado<Vec<Skill>> {
    let entradas = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut skills: Vec<Skill> = entradas
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .filter_map(|p| {
            let nombre = p.file_stem()?.to_str()?.to_string();
            let texto = std::fs::read_to_string(&p).ok()?;
            let (descripcion, cuerpo) = partir(&texto);
            let habilitada = cfg.habilitada(&nombre);
            Some(Skill { nombre, descripcion, cuerpo, habilitada })
        })
        .collect();
    skills.sort_by(|a, b| a.nombre.cmp(&b.nombre));
    Ok(skills)
}

/// Separa la cabecera `---\ndescripcion: …\n---` del cuerpo.
fn partir(texto: &str) -> (String, String) {
    let Some(resto) = texto.strip_prefix("---\n") else {
        return (String::new(), texto.trim().to_string());
    };
    let Some((cabecera, cuerpo)) = resto.split_once("\n---\n") else {
        return (String::new(), texto.trim().to_string());
    };
    let descripcion = cabecera
        .lines()
        .find_map(|l| l.strip_prefix("descripcion:"))
        .map(|d| d.trim().to_string())
        .unwrap_or_default();
    (descripcion, cuerpo.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(dir: &Path) -> ConfigSkills {
        ConfigSkills { dir: dir.to_string_lossy().into(), ..Default::default() }
    }

    fn catalogo_de_prueba() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("rust.md"), "---\ndescripcion: código Rust\n---\nUsa cargo.\n").unwrap();
        std::fs::write(d.path().join("tests.md"), "---\ndescripcion: tests\n---\nPrueba primero.\n").unwrap();
        std::fs::write(d.path().join("suelta.md"), "sin cabecera").unwrap();
        std::fs::write(d.path().join("nota.txt"), "no es skill").unwrap();
        d
    }

    #[test]
    fn lee_el_catalogo_con_descripciones() {
        let d = catalogo_de_prueba();
        let c = cfg(d.path()).catalogo().unwrap();
        let nombres: Vec<_> = c.iter().map(|s| s.nombre.as_str()).collect();
        assert_eq!(nombres, ["rust", "suelta", "tests"]);
        assert_eq!(c[0].descripcion, "código Rust");
        assert_eq!(c[0].cuerpo, "Usa cargo.");
        assert_eq!(c[1].cuerpo, "sin cabecera");
    }

    #[test]
    fn resuelve_y_arma_el_bloque() {
        let d = catalogo_de_prueba();
        let s = cfg(d.path()).resolver("claude-code", &["tests".into(), "rust".into(), "tests".into()]).unwrap();
        assert_eq!(s.len(), 2);
        let b = bloque_prompt(&s);
        assert!(b.find("## tests").unwrap() < b.find("## rust").unwrap());
        assert!(b.contains("Usa cargo."));
        assert!(bloque_prompt(&[]).is_empty());
    }

    #[test]
    fn sin_llave_no_se_eligen_skills() {
        let d = catalogo_de_prueba();
        let c = cfg(d.path());
        assert!(c.resolver("ea-cli", &["rust".into()]).is_err());
        // sin skills no hace falta llave
        assert!(c.resolver("ea-cli", &[]).unwrap().is_empty());
    }

    #[test]
    fn la_llave_es_del_rol_planificador() {
        let d = catalogo_de_prueba();
        let mut c = cfg(d.path());
        c.planificador = "codex".into();
        for quien in ["claude-code", "planificador", "codex"] {
            assert!(c.resolver(quien, &["rust".into()]).is_ok(), "{quien}");
        }
        assert!(c.resolver("opencode", &["rust".into()]).is_err(), "otro agente no es el planificador");
        assert!(c.resolver("ejecutor", &["rust".into()]).is_err());
        c.pueden_elegir = vec!["planificador".into()];
        assert!(c.resolver("codex", &["rust".into()]).is_ok(), "«planificador» en la llave equivale a «claude-code»");
        c.pueden_elegir = vec![];
        assert!(c.resolver("claude-code", &["rust".into()]).is_err(), "sin llave nadie elige");
    }

    #[test]
    fn respeta_habilitadas_y_topes() {
        let d = catalogo_de_prueba();
        let mut c = cfg(d.path());
        assert!(c.resolver("claude-code", &["nada".into()]).is_err());
        c.habilitadas = vec!["rust".into()];
        assert!(c.resolver("claude-code", &["tests".into()]).is_err());
        assert!(!c.catalogo().unwrap().iter().find(|s| s.nombre == "tests").unwrap().habilitada);
        c.habilitadas = vec![];
        c.max_por_trabajo = 1;
        assert!(c.resolver("claude-code", &["rust".into(), "tests".into()]).is_err());
        c.max_por_trabajo = 3;
        c.max_caracteres = 5;
        assert!(c.resolver("claude-code", &["rust".into()]).is_err());
    }
}
