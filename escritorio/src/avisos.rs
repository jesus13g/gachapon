//! Avisos nativos de fin de trabajo (spec 021, decisión 9, RF-009/RF-010) y
//! fichero de presencia. Con el shell vivo solo él avisa; el orquestador
//! calla al ver la presencia. `agente.avisos_escritorio` se relee en cada
//! aviso, sin reiniciar.

use std::path::PathBuf;

use ea_ajustes::Entorno;

use crate::vigia::AvisoTrabajo;

/// Texto del aviso: título y cuerpo.
pub fn texto(a: &AvisoTrabajo) -> (String, String) {
    let titulo = match a.estado.as_str() {
        "en_espera" => "Trabajo en espera",
        "cerrado" => "Trabajo cerrado",
        _ => "Trabajo listo para revisar",
    };
    (titulo.to_string(), a.titulo.clone())
}

/// ¿Está activado el ajuste? Se lee del disco en cada llamada.
pub fn activados() -> bool {
    ea_ajustes::cargar().agente.avisos_escritorio
}

/// Ruta del fichero de presencia, si hay directorio de ejecución.
pub fn fichero_presencia() -> Option<PathBuf> {
    ea_ajustes::dir_ejecucion(&Entorno::Proceso).map(|d| ea_ajustes::fichero_presencia_escritorio(&d))
}

/// Contenido del fichero de presencia: el pid.
pub fn contenido_presencia(pid: u32) -> String {
    format!("{pid}\n")
}

/// Crea la presencia (su pid, 0600). Falla en silencio: sin ella el
/// orquestador sigue avisando y habría dos avisos, no ninguno.
pub fn crear_presencia() {
    let Some(f) = fichero_presencia() else { return };
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
        privado(dir, 0o700);
    }
    if std::fs::write(&f, contenido_presencia(std::process::id())).is_ok() {
        privado(&f, 0o600);
    }
}

/// Borra la presencia al salir (solo si es la nuestra).
pub fn borrar_presencia() {
    let Some(f) = fichero_presencia() else { return };
    let es_nuestra = std::fs::read_to_string(&f).is_ok_and(|t| t.trim() == std::process::id().to_string());
    if es_nuestra {
        let _ = std::fs::remove_file(&f);
    }
}

#[cfg(unix)]
fn privado(ruta: &std::path::Path, modo: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(ruta, std::fs::Permissions::from_mode(modo));
}

#[cfg(not(unix))]
fn privado(_ruta: &std::path::Path, _modo: u32) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_texto_depende_del_estado() {
        let a = |e: &str| AvisoTrabajo { id: "x".into(), titulo: "Arreglar".into(), estado: e.into() };
        assert_eq!(texto(&a("en_revision")), ("Trabajo listo para revisar".into(), "Arreglar".into()));
        assert_eq!(texto(&a("cerrado")).0, "Trabajo cerrado");
        assert_eq!(texto(&a("en_espera")).0, "Trabajo en espera");
    }

    #[test]
    fn la_presencia_es_el_pid() {
        assert_eq!(contenido_presencia(4242), "4242\n");
    }
}
