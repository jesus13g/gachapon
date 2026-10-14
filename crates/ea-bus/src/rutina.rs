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
    /// un error (casi siempre una errata).
    pub fn a_orden(&self, entradas: &[(String, String)]) -> Resultado<Orden> {
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
            creada_en: String::new(),
            creada_por: "rutina".into(),
        })
    }
}
