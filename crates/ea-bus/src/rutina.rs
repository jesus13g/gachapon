//! Definición versionada de una rutina (`<slug>.toml`): entradas, pasos,
//! criterio de éxito y salida esperada. Se registra con una versión que solo
//! puede crecer; lanzarla crea un trabajo de modo `rutina` en el bus.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{ErrorBus, Resultado};
use crate::tipos::{Modo, Orden, RutinaRef, Topes};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CabeceraRutina {
    pub slug: String,
    pub version: u32,
    #[serde(default)]
    pub descripcion: String,
    pub worktree: String,
    pub criterio_exito: String,
    #[serde(default)]
    pub salida_esperada: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Paso {
    pub texto: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rutina {
    pub rutina: CabeceraRutina,
    /// Clave = valor por defecto; `{clave}` se sustituye en pasos y criterio.
    #[serde(default)]
    pub entradas: BTreeMap<String, String>,
    #[serde(default)]
    pub pasos: Vec<Paso>,
    #[serde(default)]
    pub topes: Option<Topes>,
}

/// Los slugs acaban en una ruta de fichero: solo `[A-Za-z0-9_-]`.
pub fn slug_valido(slug: &str) -> bool {
    !slug.is_empty() && slug.len() <= 64 && slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Tamaño máximo, en bytes, del TOML de una rutina al registrarla
/// (RF-016 de la spec 004).
pub const MAX_DEFINICION: usize = 64 * 1024;

/// Como mucho tantas entradas por ejecución (RF-013 de la spec 004).
pub const MAX_ENTRADAS: usize = 32;
/// Tamaño máximo, en bytes, del valor de una entrada.
pub const MAX_VALOR_ENTRADA: usize = 1024;

/// La política única de las entradas que se pasan al lanzar, la misma para
/// la CLI, la web y `ea_encolar_rutina` (todas pasan por [`Rutina::a_orden`]).
///
/// Ojo: `{clave}` se sustituye tal cual, también en `criterio_exito`, que el
/// sistema ejecuta con `sh -c`. Un valor con `;` o `$(…)` es código de shell
/// igual que uno con un salto de línea; esto no lo hace seguro. La defensa es
/// que solo el usuario da entradas (su terminal, el panel con la clave o una
/// propuesta que acepta tras ver el criterio resultante). Los caracteres de
/// control se rechazan porque romperían la estructura de la orden (un salto
/// de línea podría fingir una sección «## Pasos») o esconderían parte del
/// criterio en una vista previa.
pub fn validar_entradas(entradas: &[(String, String)]) -> Resultado<()> {
    if entradas.len() > MAX_ENTRADAS {
        return Err(ErrorBus::Argumentos(format!("demasiadas entradas ({}; como mucho {MAX_ENTRADAS})", entradas.len())));
    }
    for (k, v) in entradas {
        if v.len() > MAX_VALOR_ENTRADA {
            return Err(ErrorBus::Argumentos(format!(
                "la entrada {k:?} es demasiado larga ({} bytes; como mucho {MAX_VALOR_ENTRADA})",
                v.len()
            )));
        }
        if v.chars().any(char::is_control) {
            return Err(ErrorBus::Argumentos(format!(
                "la entrada {k:?} lleva caracteres de control (saltos de línea, tabuladores…), que no se admiten"
            )));
        }
    }
    Ok(())
}

fn sustituir(texto: &str, entradas: &BTreeMap<String, String>) -> String {
    entradas.iter().fold(texto.to_string(), |t, (k, v)| t.replace(&format!("{{{k}}}"), v))
}

impl Rutina {
    pub fn parsear(texto: &str) -> Resultado<Self> {
        let r: Rutina = toml::from_str(texto).map_err(|e| ErrorBus::Argumentos(format!("rutina inválida: {e}")))?;
        r.validar()?;
        Ok(r)
    }

    pub fn validar(&self) -> Resultado<()> {
        let c = &self.rutina;
        let err = |m: &str| Err(ErrorBus::Argumentos(format!("rutina inválida: {m}")));
        if !slug_valido(&c.slug) {
            return err(&format!("slug inválido: {:?}", c.slug));
        }
        if c.version == 0 {
            return err("la versión empieza en 1");
        }
        if !std::path::Path::new(&c.worktree).is_absolute() {
            return err("worktree debe ser una ruta absoluta");
        }
        if c.criterio_exito.trim().is_empty() {
            return err("falta criterio_exito");
        }
        if self.pasos.is_empty() || self.pasos.iter().any(|p| p.texto.trim().is_empty()) {
            return err("hace falta al menos un paso, y ninguno vacío");
        }
        Ok(())
    }

    /// Orden de trabajo de una ejecución, con las entradas por defecto
    /// sobrescritas por `entradas`. Una entrada que la rutina no declara es
    /// un error (casi siempre una errata), igual que saltarse los límites de
    /// [`validar_entradas`]. No escribe nada: sirve también de vista previa.
    pub fn a_orden(&self, entradas: &[(String, String)]) -> Resultado<Orden> {
        validar_entradas(entradas)?;
        let mut vals = self.entradas.clone();
        for (k, v) in entradas {
            if !vals.contains_key(k) {
                return Err(ErrorBus::Argumentos(format!("la rutina no declara la entrada {k:?}")));
            }
            vals.insert(k.clone(), v.clone());
        }
        let c = &self.rutina;
        let mut obj = format!("Ejecuta la rutina «{}» (versión {}).", c.slug, c.version);
        if !c.descripcion.is_empty() {
            obj.push_str(&format!("\n\n{}", c.descripcion));
        }
        if !vals.is_empty() {
            obj.push_str("\n\n## Entradas\n");
            for (k, v) in &vals {
                obj.push_str(&format!("- {k} = {v}\n"));
            }
        }
        obj.push_str("\n## Pasos, en orden\n");
        for (i, p) in self.pasos.iter().enumerate() {
            obj.push_str(&format!("{}. {}\n", i + 1, sustituir(&p.texto, &vals)));
        }
        if !c.salida_esperada.is_empty() {
            obj.push_str(&format!("\n## Salida esperada\n{}\n", sustituir(&c.salida_esperada, &vals)));
        }
        Ok(Orden {
            id: String::new(),
            modo: Modo::Rutina,
            titulo: format!("Rutina {}", c.slug),
            worktree: c.worktree.clone(),
            objetivo: obj,
            criterio_exito: Some(sustituir(&c.criterio_exito, &vals)),
            fases: vec![],
            revisar_cada: None,
            topes: self.topes.unwrap_or_default(),
            rutina: Some(RutinaRef { slug: c.slug.clone(), version: c.version }),
            notas_revision: vec![],
            skills: vec![],
            creada_en: String::new(),
            creada_por: "rutina".into(),
        })
    }
}
