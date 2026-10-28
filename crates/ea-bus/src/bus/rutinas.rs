//! Las rutinas: registrar sus versiones (SQLite y fichero), leerlas y
//! lanzar una ejecución, que nunca son dos a la vez.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use ea_store::{FilaRutina, Store};

use super::{ahora_rfc3339, Bus};
use crate::error::{ErrorBus, Resultado};
use crate::fs_atomico::escribir_atomico;
use crate::rutina::{Rutina, MAX_DEFINICION};
use crate::tipos::{Estado, EstadoTrabajo};

impl Bus {
    fn store_rutinas(&self) -> Resultado<&Store> {
        self.store.as_ref().ok_or_else(|| ErrorBus::Argumentos("las rutinas necesitan la base de datos".into()))
    }

    /// Valida y registra una rutina con [`comprobar_registro`] (tamaño,
    /// formato y versión mayor que la última registrada). Guarda la versión
    /// en SQLite y copia el fichero a `dir` (`<slug>.toml`). Si la rutina
    /// estaba retirada, registrarla la vuelve a poner en uso: la copia vieja
    /// de la papelera se borra (sus versiones siguen en SQLite).
    pub fn registrar_rutina(&self, texto: &str, dir: &Path) -> Resultado<Rutina> {
        let s = self.store_rutinas()?;
        let (r, _) = comprobar_registro(s, texto)?;
        s.guardar_rutina(&FilaRutina {
            slug: r.rutina.slug.clone(),
            version: i64::from(r.rutina.version),
            definicion: texto.into(),
            creada_en: ahora_rfc3339(),
            creada_por: self.actor.clone(),
        })?;
        fs::create_dir_all(dir)?;
        escribir_atomico(&dir.join(format!("{}.toml", r.rutina.slug)), texto.as_bytes())?;
        crate::papelera::quitar_de_papelera(dir, &r.rutina.slug)?;
        Ok(r)
    }

    /// Última versión registrada de una rutina.
    pub fn rutina(&self, slug: &str) -> Resultado<Rutina> {
        let fila = self
            .store_rutinas()?
            .ultima_rutina(slug)?
            .ok_or_else(|| ErrorBus::Argumentos(format!("la rutina {slug:?} no está registrada")))?;
        Rutina::parsear(&fila.definicion)
    }

    /// Texto TOML de una versión concreta.
    pub fn definicion_rutina(&self, slug: &str, version: u32) -> Option<String> {
        self.store.as_ref()?.obtener_rutina(slug, i64::from(version)).ok().flatten().map(|f| f.definicion)
    }

    /// Última versión de cada rutina registrada.
    pub fn listar_rutinas(&self) -> Resultado<Vec<Rutina>> {
        self.store_rutinas()?.listar_rutinas()?.iter().map(|f| Rutina::parsear(&f.definicion)).collect()
    }

    /// Crea un trabajo de modo rutina con la última versión registrada. Una
    /// rutina tiene como mucho una ejecución activa (en cola o ejecutándose).
    /// Comprueba, por este orden: que está registrada, que las entradas valen
    /// ([`Rutina::a_orden`]) y, bajo un `flock` de `<raiz>/.rutinas.lock` que
    /// dura hasta crear el trabajo, que no hay otra en curso
    /// ([`ErrorBus::EnCurso`]). Es la base de [`Bus::lanzar_rutina_en`] (CLI,
    /// timer y web) y de la aplicación de `ea_encolar_rutina`.
    pub fn lanzar_rutina(&self, slug: &str, entradas: &[(String, String)]) -> Resultado<String> {
        let orden = self.rutina(slug)?.a_orden(entradas)?;
        let _bloqueo = self.bloquear_rutinas()?;
        if !self.trabajos_activos_de_rutina(slug)?.is_empty() {
            return Err(ErrorBus::EnCurso(slug.into()));
        }
        self.crear_trabajo(orden)
    }

    /// `flock` exclusivo sobre `<raiz>/.rutinas.lock`: ordena entre procesos
    /// mirar si una rutina tiene una ejecución en curso y crear la nueva. Se
    /// toma antes que el de los equipos (que `crear_trabajo` pediría con una
    /// orden de equipo) y nunca con otro en la mano. No es reentrante.
    fn bloquear_rutinas(&self) -> Resultado<fs::File> {
        let f = fs::OpenOptions::new().create(true).truncate(false).write(true).open(self.raiz.join(".rutinas.lock"))?;
        f.lock()?;
        Ok(f)
    }

    /// Como [`Bus::lanzar_rutina`], pero antes mira el fichero en
    /// `rutinas_dir` ([`crate::comprobar_lanzable`]): una rutina retirada o
    /// sin fichero no se lanza. Es lo que usan la CLI (y con ella el timer)
    /// y la web; `ea_encolar_rutina` lo comprueba al simular.
    pub fn lanzar_rutina_en(&self, dir: &Path, slug: &str, entradas: &[(String, String)]) -> Resultado<String> {
        crate::papelera::comprobar_lanzable(dir, slug)?;
        self.lanzar_rutina(slug, entradas)
    }

    /// Trabajos de una rutina que aún no han terminado.
    pub fn trabajos_activos_de_rutina(&self, slug: &str) -> Resultado<Vec<String>> {
        let mut v = Vec::new();
        for id in self.listar_trabajos()? {
            let (Ok(o), Ok(e)) = (self.leer_orden(&id), self.leer_estado(&id)) else { continue };
            if o.rutina.is_some_and(|r| r.slug == slug) && matches!(e.estado, EstadoTrabajo::EnCola | EstadoTrabajo::Ejecutando) {
                v.push(id);
            }
        }
        Ok(v)
    }

    /// Trabajos de una rutina, del más reciente al más antiguo, con su estado.
    pub fn trabajos_de_rutina(&self, slug: &str) -> Resultado<Vec<(String, Estado)>> {
        let mut v = Vec::new();
        for id in self.listar_trabajos()?.into_iter().rev() {
            let (Ok(o), Ok(e)) = (self.leer_orden(&id), self.leer_estado(&id)) else { continue };
            if o.rutina.is_some_and(|r| r.slug == slug) {
                v.push((id, e));
            }
        }
        Ok(v)
    }

    /// La última ejecución (el trabajo más reciente, con su estado) de cada
    /// una de `slugs`, en una sola pasada del más nuevo al más viejo (los ids
    /// empiezan por la fecha) que acaba en cuanto las ha visto todas. Un
    /// trabajo con la orden o el estado ilegibles no cuenta, como en
    /// [`Bus::trabajos_de_rutina`].
    pub fn ultimas_ejecuciones(&self, slugs: &HashSet<String>) -> Resultado<HashMap<String, (String, Estado)>> {
        let mut out = HashMap::new();
        for id in self.listar_trabajos()?.into_iter().rev() {
            if out.len() == slugs.len() {
                break;
            }
            let Some(slug) = self.leer_orden(&id).ok().and_then(|o| o.rutina).map(|r| r.slug) else { continue };
            if slugs.contains(&slug) && !out.contains_key(&slug)
                && let Ok(e) = self.leer_estado(&id) {
                out.insert(slug, (id, e));
            }
        }
        Ok(out)
    }
}

/// Lo que comprueba registrar una rutina, sin escribir nada: la política
/// única de la CLI, la web y `ea_registrar_rutina`. La definición no pasa de
/// [`MAX_DEFINICION`] bytes, es una rutina válida ([`Rutina::parsear`]) y su
/// versión es mayor que la última registrada. Devuelve la rutina y la última
/// versión registrada, si la hay.
pub fn comprobar_registro(store: &Store, texto: &str) -> Resultado<(Rutina, Option<FilaRutina>)> {
    if texto.len() > MAX_DEFINICION {
        return Err(ErrorBus::Argumentos(format!(
            "la definición es demasiado larga ({} bytes; como mucho {MAX_DEFINICION})",
            texto.len()
        )));
    }
    let r = Rutina::parsear(texto)?;
    let anterior = store.ultima_rutina(&r.rutina.slug)?;
    if let Some(ult) = &anterior && i64::from(r.rutina.version) <= ult.version {
        return Err(ErrorBus::Argumentos(format!(
            "la rutina {} ya está en la versión {}; la nueva debe ser mayor (es la {})",
            r.rutina.slug, ult.version, r.rutina.version
        )));
    }
    Ok((r, anterior))
}
