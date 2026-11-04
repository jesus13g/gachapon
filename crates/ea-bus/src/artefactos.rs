//! Los ficheros de un trabajo además de su orden, su estado y su informe
//! (`trabajos/<id>/`): la salida y las skills del agente, las instantáneas
//! de árbol y las fases informadas de una obra, el paquete de corrección de
//! una rutina y el historial. Todo se escribe de forma atómica (`fs_atomico`).

use std::fs;
use std::path::{Path, PathBuf};

use crate::bus::Bus;
use crate::error::Resultado;
use crate::fs_atomico::{escribir_atomico, escribir_json};
use crate::tipos::FaseInforme;

/// La salida en bruto del agente en el directorio del trabajo (spec 017,
/// RF-010): `salida-agente.json`, o `salida-qwen.json` en los trabajos de
/// antes.
pub const SALIDA_AGENTE: &str = "salida-agente.json";
const SALIDA_ANTIGUA: &str = "salida-qwen.json";
/// Las skills que se le pasaron (`skills-qwen.md` en los trabajos de antes).
pub const SKILLS_AGENTE: &str = "skills-agente.md";
const PAQUETE: &str = "paquete-correccion.md";
/// La instantánea del árbol tras la última fase hecha de una obra.
const ARBOL_ULTIMO: &str = "arbol-ultimo.txt";
/// La instantánea del árbol en la última parada de revisión.
const ARBOL_REVISION: &str = "arbol-revision.txt";
const FASES_INFORME: &str = "fases-informe.json";
const HISTORIAL: &str = "historial.jsonl";

/// Los artefactos de un trabajo ([`Bus::artefactos`]).
#[derive(Debug, Clone)]
pub struct Artefactos {
    dir: PathBuf,
}

impl Bus {
    /// Los artefactos del trabajo `id`; no comprueba que exista.
    pub fn artefactos(&self, id: &str) -> Resultado<Artefactos> {
        Ok(Artefactos { dir: self.dir_trabajo(id)? })
    }
}

impl Artefactos {
    /// `trabajos/<id>/`.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn texto(&self, fichero: &str) -> Option<String> {
        fs::read_to_string(self.dir.join(fichero)).ok()
    }

    pub fn paquete_correccion(&self) -> Option<String> {
        self.texto(PAQUETE)
    }

    pub(crate) fn guardar_paquete_correccion(&self, texto: &str) -> Resultado<()> {
        escribir_atomico(&self.dir.join(PAQUETE), texto.as_bytes())
    }

    /// Añade `texto` al final del paquete (lo crea si no estaba).
    pub(crate) fn anadir_al_paquete_correccion(&self, texto: &str) -> Resultado<()> {
        let anterior = match fs::read_to_string(self.dir.join(PAQUETE)) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e.into()),
        };
        self.guardar_paquete_correccion(&format!("{anterior}{texto}"))
    }

    /// La salida en bruto de la última llamada al agente, con el nombre nuevo
    /// o el antiguo.
    pub fn salida_agente(&self) -> Option<String> {
        [SALIDA_AGENTE, SALIDA_ANTIGUA].iter().find_map(|n| self.texto(n))
    }

    pub fn guardar_salida_agente(&self, bruta: &str) -> Resultado<()> {
        escribir_atomico(&self.dir.join(SALIDA_AGENTE), bruta.as_bytes())
    }

    pub fn guardar_skills_agente(&self, texto: &str) -> Resultado<()> {
        escribir_atomico(&self.dir.join(SKILLS_AGENTE), texto.as_bytes())
    }

    /// Una instantánea guardada, sin espacios a los lados; `None` si no hay
    /// (o está vacía).
    fn arbol(&self, fichero: &str) -> Option<String> {
        self.texto(fichero).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    }

    pub fn arbol_ultimo(&self) -> Option<String> {
        self.arbol(ARBOL_ULTIMO)
    }

    pub fn guardar_arbol_ultimo(&self, arbol: &str) -> Resultado<()> {
        escribir_atomico(&self.dir.join(ARBOL_ULTIMO), arbol.as_bytes())
    }

    pub fn arbol_revision(&self) -> Option<String> {
        self.arbol(ARBOL_REVISION)
    }

    pub fn guardar_arbol_revision(&self, arbol: &str) -> Resultado<()> {
        escribir_atomico(&self.dir.join(ARBOL_REVISION), arbol.as_bytes())
    }

    /// Las fases ya informadas de una obra (`fases-informe.json`); `None` si
    /// no hay fichero. Uno ilegible se trata como vacío, con un aviso.
    pub fn fases_informe(&self) -> Option<Vec<FaseInforme>> {
        let b = fs::read(self.dir.join(FASES_INFORME)).ok()?;
        Some(serde_json::from_slice(&b).unwrap_or_else(|e| {
            tracing::warn!(dir = %self.dir.display(), error = %e, "fases-informe.json ilegible: se trata como vacío");
            Vec::new()
        }))
    }

    pub fn guardar_fases_informe(&self, fases: &[FaseInforme]) -> Resultado<()> {
        escribir_json(&self.dir.join(FASES_INFORME), &fases)
    }

    /// Las líneas legibles de `historial.jsonl`, en orden; vacío si no hay.
    pub fn historial(&self) -> Vec<serde_json::Value> {
        self.texto(HISTORIAL).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
    }

    pub(crate) fn ruta_historial(&self) -> PathBuf {
        self.dir.join(HISTORIAL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tipos::EstadoFase;

    fn artefactos(d: &Path) -> Artefactos {
        Artefactos { dir: d.to_path_buf() }
    }

    #[test]
    fn la_salida_se_lee_con_el_nombre_nuevo_o_el_antiguo() {
        let d = tempfile::tempdir().unwrap();
        let a = artefactos(d.path());
        assert_eq!(a.salida_agente(), None);
        std::fs::write(d.path().join(SALIDA_ANTIGUA), "vieja").unwrap();
        assert_eq!(a.salida_agente().as_deref(), Some("vieja"));
        a.guardar_salida_agente("nueva").unwrap();
        assert_eq!(a.salida_agente().as_deref(), Some("nueva"));
        assert_eq!(std::fs::read_to_string(d.path().join(SALIDA_AGENTE)).unwrap(), "nueva");
    }

    #[test]
    fn las_instantaneas_y_el_paquete_se_escriben_tal_cual() {
        let d = tempfile::tempdir().unwrap();
        let a = artefactos(d.path());
        assert_eq!((a.arbol_ultimo(), a.arbol_revision()), (None, None));
        a.guardar_arbol_ultimo("abc1").unwrap();
        a.guardar_arbol_revision("abc0").unwrap();
        assert_eq!((a.arbol_ultimo().as_deref(), a.arbol_revision().as_deref()), (Some("abc1"), Some("abc0")));
        assert_eq!(std::fs::read_to_string(d.path().join(ARBOL_ULTIMO)).unwrap(), "abc1");
        std::fs::write(d.path().join(ARBOL_ULTIMO), " \n").unwrap();
        assert_eq!(a.arbol_ultimo(), None, "una instantánea vacía no cuenta");

        a.anadir_al_paquete_correccion("\n## Contexto\n").unwrap();
        assert_eq!(a.paquete_correccion().as_deref(), Some("\n## Contexto\n"), "sin paquete, se crea");
        a.guardar_paquete_correccion("# Paquete\n").unwrap();
        a.anadir_al_paquete_correccion("\n## Contexto\n").unwrap();
        assert_eq!(a.paquete_correccion().as_deref(), Some("# Paquete\n\n## Contexto\n"));
        let restos: Vec<_> = std::fs::read_dir(d.path()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).collect();
        assert!(restos.is_empty(), "{restos:?}");
    }

    #[test]
    fn un_fases_informe_ilegible_cuenta_como_vacio_y_sin_fichero_no_hay() {
        let d = tempfile::tempdir().unwrap();
        let a = artefactos(d.path());
        assert_eq!(a.fases_informe(), None);
        let f = FaseInforme { numero: 1, titulo: "f1".into(), resultado: EstadoFase::Hecha, diff_stat: String::new(), ejecutado_por: None };
        a.guardar_fases_informe(std::slice::from_ref(&f)).unwrap();
        assert_eq!(a.fases_informe(), Some(vec![f.clone()]));
        assert_eq!(std::fs::read(d.path().join(FASES_INFORME)).unwrap(), serde_json::to_vec_pretty(&[f]).unwrap());
        std::fs::write(d.path().join(FASES_INFORME), "[{").unwrap();
        assert_eq!(a.fases_informe(), Some(vec![]));
    }
}
