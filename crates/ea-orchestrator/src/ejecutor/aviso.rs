//! Los avisos de escritorio del ejecutor al entregar un trabajo: con la
//! acción «abrir» del panel en Linux, simples en el resto, y callados con el
//! ajuste apagado o con `ea-escritorio` vivo (los da él).

use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};

use crate::config::ConfigOrquestador;

/// El lanzador del panel web del checkout con que se compiló el ejecutor:
/// abre `ea-web` en un enlace profundo (`ea-web-open '#trabajos/<id>'`). Solo
/// se usa si no hay uno junto a los binarios ([`lanzador_web`]).
#[cfg(target_os = "linux")]
const EA_WEB_OPEN_COMPILADO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../ea-web/bin/ea-web-open");

/// `ea-web-open`: el que esté junto a los binarios (`hermano`) y, si no hay,
/// el del checkout con que se compiló.
#[cfg(any(target_os = "linux", test))]
fn lanzador_web(hermano: Option<std::path::PathBuf>, compilado: &Path) -> std::path::PathBuf {
    hermano.unwrap_or_else(|| compilado.to_path_buf())
}

/// La orden del aviso de escritorio con la acción «abrir» (solo Linux): lanza
/// `abrir '#trabajos/<id>'`. `notify-send --action` espera a que se pulse o se
/// descarte el aviso; si no admite acciones (o falla), se avisa sin ella. Los
/// textos van como argumentos, nunca dentro del guion. Sale con error solo si
/// `notify-send` falta o falla (el aviso no se entregó).
#[cfg(target_os = "linux")]
fn orden_aviso(titulo: &str, resultado: &str, id: &str, abrir: &Path) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c")
        .arg(r#"r=$(notify-send --action=abrir=abrir "$1" "$2") || exec notify-send "$1" "$2"; if [ "$r" = abrir ]; then "$3" "$4"; fi; exit 0"#)
        .arg("ea-aviso")
        .arg(format!("ea: {titulo}"))
        .arg(resultado)
        .arg(abrir)
        .arg(format!("#trabajos/{id}"));
    c
}

/// ¿Hay un `ea-escritorio` vivo? Lo dice su fichero de presencia en el
/// directorio de ejecución (`<ejecucion>/ea-escritorio/presencia`, con su pid
/// en la primera palabra): si el pid ya no existe (el shell murió sin limpiar),
/// no cuenta.
/// Además el proceso con ese pid tiene que ser de verdad `ea-escritorio`: un
/// pid reutilizado por otro programa tras un cierre sucio no cuenta.
fn escritorio_vivo(ejecucion: Option<&Path>) -> bool {
    escritorio_vivo_con(ejecucion, ea_plataforma::proceso::vive, ea_plataforma::proceso::nombre)
}

/// ¿Es `nombre` el ejecutable del shell? Sin nombre legible se da por bueno
/// (el pid vive y no se puede comprobar más).
fn es_ea_escritorio(nombre: Option<&str>) -> bool {
    nombre.is_none_or(|n| n.to_ascii_lowercase().starts_with("ea-escritorio"))
}

fn escritorio_vivo_con(ejecucion: Option<&Path>, vive: impl Fn(u32) -> bool, nombre: impl Fn(u32) -> Option<String>) -> bool {
    let Some(ejecucion) = ejecucion else { return false };
    let Ok(texto) = std::fs::read_to_string(ea_ajustes::fichero_presencia_escritorio(ejecucion)) else { return false };
    let Some(pid) = texto.split_whitespace().next().and_then(|p| p.parse::<u32>().ok()) else { return false };
    vive(pid) && es_ea_escritorio(nombre(pid).as_deref())
}

/// ¿Avisa el orquestador? No con `avisos_escritorio = false` (no suena nadie) ni
/// con el shell de escritorio vivo (los avisos los da él).
fn debe_avisar(config: &ConfigOrquestador, ejecucion: Option<&Path>) -> bool {
    config.avisos_escritorio && !escritorio_vivo(ejecucion)
}

/// El lanzador de la acción «abrir»: `ea-escritorio` si está instalado y, si no,
/// `ea-web-open` (si existe). Ambos se invocan igual: `<lanzador> '#trabajos/<id>'`.
#[cfg(any(target_os = "linux", test))]
fn elegir_lanzador(escritorio: Option<std::path::PathBuf>, web_open: &Path) -> Option<std::path::PathBuf> {
    escritorio.or_else(|| web_open.is_file().then(|| web_open.to_path_buf()))
}

/// Aviso de escritorio sobre un trabajo, mejor esfuerzo. Desacoplado: el
/// ejecutor no lo espera (un hilo recoge el proceso cuando acaba, para no
/// dejar zombis). En Linux con un lanzador (`ea-escritorio` o el del panel), el
/// aviso lleva la acción «abrir»; en cualquier otro caso es un aviso simple
/// (`ea-plataforma`). Calla con el ajuste en `false` o con `ea-escritorio` vivo.
pub(super) fn avisar(config: &ConfigOrquestador, titulo: &str, resultado: &str, id: &str) {
    if !debe_avisar(config, ea_ajustes::dir_ejecucion(&ea_ajustes::Entorno::Proceso).as_deref()) {
        return;
    }
    #[cfg(target_os = "linux")]
    {
        // `ea-escritorio` junto a los demás binarios, si existe.
        let web_open = lanzador_web(ea_plataforma::rutas::binario_hermano("ea-web-open"), Path::new(EA_WEB_OPEN_COMPILADO));
        if let Some(abrir) = elegir_lanzador(ea_plataforma::rutas::binario_hermano("ea-escritorio"), &web_open) {
            let mut c = orden_aviso(titulo, resultado, id, &abrir);
            c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
            let (t, r) = (format!("ea: {titulo}"), resultado.to_string());
            match c.spawn() {
                Ok(mut hijo) => {
                    // Si `notify-send` falta o falla, el aviso no se entregó:
                    // se cae al mecanismo simple de la plataforma.
                    std::thread::spawn(move || {
                        if !hijo.wait().is_ok_and(|st| st.success()) {
                            avisar_simple(&t, &r);
                        }
                    });
                }
                Err(e) => {
                    tracing::debug!(error = %e, "no se pudo lanzar el aviso de escritorio");
                    std::thread::spawn(move || avisar_simple(&t, &r));
                }
            }
            return;
        }
    }
    let _ = id;
    let (titulo, resultado) = (format!("ea: {titulo}"), resultado.to_string());
    std::thread::spawn(move || avisar_simple(&titulo, &resultado));
}

/// Aviso simple de la plataforma (síncrono; llamar desde un hilo).
fn avisar_simple(titulo: &str, resultado: &str) {
    if let Err(e) = ea_plataforma::escritorio::avisar(titulo, resultado) {
        tracing::debug!(error = %e, "no se pudo entregar el aviso de escritorio");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_aviso_calla_con_el_ajuste_apagado_y_con_el_shell_vivo() {
        let d = tempfile::tempdir().unwrap();
        let con = ConfigOrquestador { avisos_escritorio: true, ..Default::default() };
        let sin = ConfigOrquestador { avisos_escritorio: false, ..Default::default() };
        // Sin presencia: avisa solo si el ajuste lo permite.
        assert!(debe_avisar(&con, Some(d.path())));
        assert!(debe_avisar(&con, None));
        assert!(!debe_avisar(&sin, Some(d.path())));
        // Presencia viva (este mismo proceso): calla; con el ajuste apagado, también.
        let fichero = ea_ajustes::fichero_presencia_escritorio(d.path());
        std::fs::create_dir_all(fichero.parent().unwrap()).unwrap();
        std::fs::write(&fichero, format!("{}\n", std::process::id())).unwrap();
        // (el nombre del proceso lo decide `es_ea_escritorio`; ver su prueba)
        let nombre_ok = |_| Some("ea-escritorio".to_string());
        assert!(escritorio_vivo_con(Some(d.path()), ea_plataforma::proceso::vive, nombre_ok));
        assert!(!escritorio_vivo_con(Some(d.path()), ea_plataforma::proceso::vive, |_| Some("firefox".into())));
        assert!(!escritorio_vivo_con(Some(d.path()), |_| false, nombre_ok));
        // Este proceso es el binario de pruebas, no ea-escritorio: pid reutilizado.
        assert!(!escritorio_vivo(Some(d.path())));
        assert!(debe_avisar(&con, Some(d.path())));
        assert!(!debe_avisar(&sin, Some(d.path())));
        // Presencia de un pid muerto o ilegible: no cuenta y avisa el orquestador.
        std::fs::write(&fichero, "4294967294").unwrap();
        assert!(!escritorio_vivo(Some(d.path())));
        assert!(debe_avisar(&con, Some(d.path())));
        std::fs::write(&fichero, "basura").unwrap();
        assert!(debe_avisar(&con, Some(d.path())));
    }

    #[test]
    fn solo_el_ejecutable_ea_escritorio_cuenta_como_shell() {
        assert!(es_ea_escritorio(Some("ea-escritorio")));
        assert!(es_ea_escritorio(Some("ea-escritorio.exe")));
        assert!(es_ea_escritorio(None));
        assert!(!es_ea_escritorio(Some("firefox")));
        assert!(!es_ea_escritorio(Some("")));
    }

    #[test]
    fn la_accion_abrir_prefiere_ea_escritorio() {
        let d = tempfile::tempdir().unwrap();
        let web_open = d.path().join("ea-web-open");
        let escritorio = d.path().join("ea-escritorio");
        // Ninguno: sin acción.
        assert_eq!(elegir_lanzador(None, &web_open), None);
        // Solo el del panel.
        std::fs::write(&web_open, "").unwrap();
        assert_eq!(elegir_lanzador(None, &web_open), Some(web_open.clone()));
        // Con ea-escritorio, manda este.
        assert_eq!(elegir_lanzador(Some(escritorio.clone()), &web_open), Some(escritorio.clone()));
        assert_eq!(elegir_lanzador(Some(escritorio.clone()), &d.path().join("no-existe")), Some(escritorio));
    }

    #[test]
    fn el_lanzador_del_panel_junto_a_los_binarios_manda_sobre_el_compilado() {
        let compilado = Path::new("/clon/crates/ea-web/bin/ea-web-open");
        let junto = std::path::PathBuf::from("/opt/ea/bin/ea-web-open");
        assert_eq!(lanzador_web(Some(junto.clone()), compilado), junto);
        assert_eq!(lanzador_web(None, compilado), compilado);
    }

    #[cfg(target_os = "linux")]
    fn argumentos(c: &Command) -> Vec<String> {
        c.get_args().map(|a| a.to_string_lossy().into_owned()).collect()
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn el_aviso_lleva_la_accion_abrir_con_el_enlace_al_trabajo() {
        // El lanzador existe en el checkout (si se mueve, que falle aquí).
        assert!(Path::new(EA_WEB_OPEN_COMPILADO).is_file(), "{EA_WEB_OPEN_COMPILADO}");
        let c = orden_aviso("misión $(rm -rf ~)", "terminado", "20261020-101500-ab12", Path::new("/x/ea-web-open"));
        assert_eq!(c.get_program(), "sh");
        let a = argumentos(&c);
        assert!(a[1].contains("--action=abrir=abrir"));
        // Los textos van como argumentos posicionales, no dentro del guion.
        assert!(!a[1].contains("rm -rf"));
        assert_eq!(&a[2..], ["ea-aviso", "ea: misión $(rm -rf ~)", "terminado", "/x/ea-web-open", "#trabajos/20261020-101500-ab12"]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn el_guion_del_aviso_lanza_el_enlace_solo_si_se_pulsa_abrir() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let abierto = d.path().join("abierto");
        let escribe = |nombre: &str, cuerpo: String| {
            let r = bin.join(nombre);
            std::fs::write(&r, format!("#!/bin/sh\n{cuerpo}\n")).unwrap();
            std::fs::set_permissions(&r, std::fs::Permissions::from_mode(0o755)).unwrap();
            r
        };
        let abrir = escribe("abrir", format!("printf %s \"$1\" > '{}'", abierto.display()));
        for (respuesta, esperado) in [("abrir", "#trabajos/x"), ("", "")] {
            let _ = std::fs::remove_file(&abierto);
            escribe("notify-send", format!("echo '{respuesta}'"));
            let st = orden_aviso("t", "r", "x", &abrir)
                .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
                .status()
                .unwrap();
            assert!(st.success());
            assert_eq!(std::fs::read_to_string(&abierto).unwrap_or_default(), esperado, "respuesta {respuesta:?}");
        }
    }
}
