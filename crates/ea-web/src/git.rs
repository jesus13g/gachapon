//! `git` sobre el worktree de un trabajo: el de `ea_bus::git` (endurecido),
//! fuera del runtime y con tiempo límite.

use std::path::PathBuf;
use std::time::Duration;

use ea_bus::ErrorBus;

use crate::servicio::{ErrorWeb, Resultado};

const LIMITE: Duration = Duration::from_secs(10);

/// Corre `f` (git, que bloquea) en un hilo aparte. Si git vence `LIMITE`,
/// `ea_bus::git` lo mata y aquí es un `Tiempo`.
async fn bloqueante<T: Send + 'static>(f: impl FnOnce() -> ea_bus::Resultado<T> + Send + 'static) -> Resultado<T> {
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(ErrorBus::GitSinRespuesta(_))) => Err(ErrorWeb::Tiempo("git no respondió".into())),
        Ok(Err(e)) => Err(e.into()),
        Err(e) => Err(ErrorWeb::Interno(e.to_string())),
    }
}

/// Lo cambiado desde `inicio` más los ficheros nuevos sin seguimiento,
/// recortado a `max` bytes. Devuelve el texto y si se recortó.
pub async fn diff_desde(wt: &str, inicio: &str, max: usize) -> Resultado<(String, bool)> {
    let (wt, inicio) = (PathBuf::from(wt), inicio.to_string());
    let texto = bloqueante(move || ea_bus::git::diff_trabajo(&wt, &inicio, LIMITE)).await?;
    Ok(recortar(texto, max))
}

/// `git diff base fin` entre dos árboles (las instantáneas de una fase,
/// `ea_bus::rango_diff_fase`), recortado a `max` bytes.
pub async fn diff_entre(wt: &str, base: &str, fin: &str, max: usize) -> Resultado<(String, bool)> {
    let (wt, base, fin) = (PathBuf::from(wt), base.to_string(), fin.to_string());
    let texto = bloqueante(move || ea_bus::git::diff_entre(&wt, &base, &fin, false, LIMITE)).await?;
    Ok(recortar(texto, max))
}

fn recortar(mut texto: String, max: usize) -> (String, bool) {
    let truncado = texto.len() > max;
    if truncado {
        let mut fin = max;
        while !texto.is_char_boundary(fin) {
            fin -= 1;
        }
        texto.truncate(fin);
    }
    (texto, truncado)
}

/// El worktree de una orden nueva o de un equipo, como lo valida `ea trabajo
/// crear` ([`ea_bus::validar_worktree`]: ruta absoluta, existente, directorio
/// y repo git con commits), fuera del runtime y con el tope del panel.
/// Devuelve la forma canónica.
pub async fn validar_worktree(wt: &str) -> Resultado<String> {
    let wt = wt.to_string();
    bloqueante(move || ea_bus::validar_worktree(&wt, LIMITE)).await
}
