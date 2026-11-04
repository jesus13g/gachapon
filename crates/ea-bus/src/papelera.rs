//! La papelera de las rutinas (spec 004, RF-011): retirar una rutina mueve
//! su TOML de `rutinas_dir/<slug>.toml` a `rutinas_dir/.papelera/<slug>.toml`
//! y restaurarla lo devuelve. La base de datos no cambia: las versiones
//! registradas se conservan como historial, y es el fichero el que dice si
//! una rutina está en uso. Lo mismo que la papelera de la bóveda
//! (`<raíz>/.papelera/`), pero por slug: una rutina tiene un solo fichero.
//!
//! Es la política única de la CLI, la web y `ea-acciones`: quien lista,
//! lanza, retira o restaura pregunta aquí.

use std::fs;
use std::path::{Path, PathBuf};

use ea_store::FilaRutina;

use crate::error::{ErrorBus, Resultado};
use crate::rutina::slug_valido;

/// Subdirectorio de `rutinas_dir` donde esperan las rutinas retiradas.
pub const DIR_PAPELERA: &str = ".papelera";

/// Dónde está el TOML de una rutina registrada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ubicacion {
    /// En `rutinas_dir/<slug>.toml`: se lista y se lanza.
    Activa,
    /// Solo en la papelera: ni se lista (salvo pidiendo las retiradas) ni se
    /// lanza; se restaura.
    Retirada,
    /// En ningún sitio (borrada para siempre o perdida): ni se lista ni se
    /// lanza. Registrar una versión nueva la vuelve a poner en uso.
    SinFichero,
}

impl Ubicacion {
    pub fn as_str(self) -> &'static str {
        match self {
            Ubicacion::Activa => "activa",
            Ubicacion::Retirada => "retirada",
            Ubicacion::SinFichero => "sin_fichero",
        }
    }

    /// Por qué no se puede lanzar (ni programar) una rutina en esta
    /// ubicación; `None` si se puede. Es el mismo texto en la CLI, la web y
    /// la simulación de `ea_encolar_rutina`.
    pub fn motivo_no_lanzable(self, slug: &str) -> Option<String> {
        match self {
            Ubicacion::Activa => None,
            Ubicacion::Retirada => Some(format!(
                "la rutina '{slug}' está retirada (su TOML está en la papelera de rutinas_dir); \
                 restáurala para lanzarla"
            )),
            Ubicacion::SinFichero => Some(format!(
                "la rutina '{slug}' no tiene fichero en rutinas_dir (se borró o se perdió); \
                 registra una versión nueva para volver a usarla"
            )),
        }
    }
}

fn comprobar(slug: &str) -> Resultado<()> {
    if slug_valido(slug) { Ok(()) } else { Err(ErrorBus::Argumentos(format!("slug de rutina inválido: '{slug}'"))) }
}

/// `rutinas_dir/<slug>.toml`, con el slug validado.
pub fn ruta_activa(dir: &Path, slug: &str) -> Resultado<PathBuf> {
    comprobar(slug)?;
    Ok(dir.join(format!("{slug}.toml")))
}

/// `rutinas_dir/.papelera/<slug>.toml`, con el slug validado.
pub fn ruta_retirada(dir: &Path, slug: &str) -> Resultado<PathBuf> {
    comprobar(slug)?;
    Ok(dir.join(DIR_PAPELERA).join(format!("{slug}.toml")))
}

/// Dónde está el TOML de la rutina. Si está en los dos sitios (se registró
/// una versión nueva estando retirada y la copia vieja se quedó), manda el
/// de `rutinas_dir`: está activa.
pub fn ubicacion_rutina(dir: &Path, slug: &str) -> Resultado<Ubicacion> {
    if ruta_activa(dir, slug)?.is_file() {
        Ok(Ubicacion::Activa)
    } else if ruta_retirada(dir, slug)?.is_file() {
        Ok(Ubicacion::Retirada)
    } else {
        Ok(Ubicacion::SinFichero)
    }
}

/// Las rutinas registradas cruzadas con su fichero en `dir` (un error de
/// lectura cuenta como sin fichero): las activas o, con `retiradas`, las que
/// están en la papelera o sin fichero. Lo que listan la CLI, la TUI y la web.
pub fn rutinas_por_ubicacion(filas: Vec<FilaRutina>, dir: &Path, retiradas: bool) -> Vec<(FilaRutina, Ubicacion)> {
    filas
        .into_iter()
        .filter_map(|f| {
            let u = ubicacion_rutina(dir, &f.slug).unwrap_or(Ubicacion::SinFichero);
            ((u == Ubicacion::Activa) != retiradas).then_some((f, u))
        })
        .collect()
}

/// Error si la rutina no se puede lanzar ahora (retirada o sin fichero).
pub fn comprobar_lanzable(dir: &Path, slug: &str) -> Resultado<()> {
    match ubicacion_rutina(dir, slug)?.motivo_no_lanzable(slug) {
        None => Ok(()),
        Some(m) => Err(ErrorBus::NoLanzable(m)),
    }
}

/// Mueve el TOML de la rutina a la papelera. Exige que esté activa; si en la
/// papelera había una copia vieja, la sustituye (sus versiones registradas
/// siguen en SQLite). El renombrado es atómico dentro del mismo directorio.
pub fn retirar_rutina(dir: &Path, slug: &str) -> Resultado<()> {
    let (origen, destino) = (ruta_activa(dir, slug)?, ruta_retirada(dir, slug)?);
    if !origen.is_file() {
        return Err(ErrorBus::Argumentos(format!("la rutina '{slug}' no está activa: no hay {}", origen.display())));
    }
    fs::create_dir_all(dir.join(DIR_PAPELERA))?;
    fs::rename(&origen, &destino)?;
    Ok(())
}

/// Devuelve el TOML de la papelera a `rutinas_dir`. Exige que esté retirada
/// y no pisa un `<slug>.toml` que ya exista.
pub fn restaurar_rutina(dir: &Path, slug: &str) -> Resultado<()> {
    let (origen, destino) = (ruta_retirada(dir, slug)?, ruta_activa(dir, slug)?);
    if destino.exists() {
        return Err(ErrorBus::Argumentos(format!("la rutina '{slug}' ya está activa: existe {}", destino.display())));
    }
    if !origen.is_file() {
        return Err(ErrorBus::Argumentos(format!("la rutina '{slug}' no está en la papelera")));
    }
    // Enlace y después borrado: si entre medias alguien crea `<slug>.toml`,
    // el enlace falla en vez de pisarlo. Un sistema de ficheros sin enlaces
    // duros se conforma con el renombrado.
    match fs::hard_link(&origen, &destino) {
        Ok(()) => fs::remove_file(&origen)?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Err(e.into()),
        Err(_) => fs::rename(&origen, &destino)?,
    }
    Ok(())
}

/// Borra la copia de la papelera, si la hay (registrar una versión nueva de
/// una rutina retirada la vuelve a poner en uso). `Ok(true)` si la había.
pub fn quitar_de_papelera(dir: &Path, slug: &str) -> Resultado<bool> {
    match fs::remove_file(ruta_retirada(dir, slug)?) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
