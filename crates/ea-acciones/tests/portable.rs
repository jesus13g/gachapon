//! Los backends de launchd y del Programador de tareas (spec 020), con un
//! `launchctl`/`schtasks` falso (un guion de `sh` que apunta sus argumentos)
//! en un directorio temporal. Nunca toca el sistema real.
#![cfg(unix)]

use std::path::{Path, PathBuf};

use ea_acciones::servicios::Backend;
use ea_acciones::{GestorServicios, Temporizadores};
use ea_plataforma::Sistema;
use time::macros::datetime;

/// Un gestor falso: apunta cada llamada (un argumento por línea, `--` al
/// final), `print`/`/Query` responden según exista el fichero `cargado`,
/// `bootstrap`/`/Create` lo crean y `bootout`/`/Delete` lo quitan.
fn falso(dir: &Path) -> (Vec<String>, PathBuf) {
    let log = dir.join("ordenes.log");
    let guion = dir.join("gestor.sh");
    std::fs::write(
        &guion,
        format!(
            "printf '%s\\n' \"$@\" -- >> '{log}'\n\
             c='{dir}/cargado'\n\
             case \"$1\" in\n\
               print|/Query) if [ -f \"$c\" ]; then echo 'state = running'; exit 0; else exit 1; fi;;\n\
               bootstrap|/Create) if [ -f '{dir}/fallo' ]; then echo 'no se pudo' >&2; exit 5; fi; touch \"$c\";;\n\
               bootout|/Delete) rm -f \"$c\";;\n\
               kickstart|/Run|/End) ;;\n\
             esac\n",
            log = log.display(),
            dir = dir.display()
        ),
    )
    .unwrap();
    (vec!["sh".into(), guion.display().to_string()], log)
}

fn temporizadores(dir: &Path, backend: Backend) -> (Temporizadores, PathBuf) {
    let (gestor, log) = falso(dir);
    let mut t = Temporizadores::portable(
        backend,
        dir.join("LaunchAgents"),
        gestor,
        dir.join("programadas"),
        PathBuf::from("/Apps/ea con espacio/ea"),
        Some(501),
    );
    // Jueves 22 de octubre de 2026, 07:00 en un sitio con UTC+2.
    t.reloj = Some((datetime!(2026-10-22 07:00:00), 7200));
    (t, log)
}

#[tokio::test]
async fn validar_calendario_fuera_de_linux_usa_el_subconjunto() {
    let d = tempfile::tempdir().unwrap();
    let (t, _) = temporizadores(d.path(), Backend::Launchd);
    let c = t.validar_calendario("Mon..Fri 09:00").await.unwrap().unwrap();
    assert_eq!(c.normalizado, "Mon..Fri *-*-* 09:00:00");
    assert_eq!(c.proximas, ["Thu 2026-10-22 09:00:00", "Fri 2026-10-23 09:00:00", "Mon 2026-10-26 09:00:00"]);
    for malo in ["*:0/15", "Mon,Tue,Wed 08:00,09:00", "2026-10-22 08:00", "daily\nx"] {
        assert!(t.validar_calendario(malo).await.unwrap().is_err(), "{malo:?}");
    }
    let e = t.validar_calendario("quarterly").await.unwrap().unwrap_err();
    assert!(e.contains("fuera de Linux solo se admite"), "{e}");
}

#[tokio::test]
async fn launchd_programa_consulta_y_desprograma() {
    let d = tempfile::tempdir().unwrap();
    let (t, log) = temporizadores(d.path(), Backend::Launchd);
    assert!(!t.desprogramar("diaria").await.unwrap(), "sin nada programado");

    t.programar("diaria", "Mon,Wed 09:30").await.unwrap();
    let plist = d.path().join("LaunchAgents/es.gachapon.ea-rutina.diaria.plist");
    let texto = std::fs::read_to_string(&plist).unwrap();
    assert!(texto.contains("<string>es.gachapon.ea-rutina.diaria</string>"), "{texto}");
    assert!(texto.contains("<string>/Apps/ea con espacio/ea</string>"));
    assert!(texto.contains("<string>rutina</string>\n    <string>lanzar</string>\n    <string>diaria</string>"));
    assert!(texto.contains("<key>Weekday</key>\n      <integer>1</integer>"));
    assert!(texto.contains("<key>Weekday</key>\n      <integer>3</integer>"));
    assert!(texto.contains("<key>Hour</key>\n      <integer>9</integer>\n      <key>Minute</key>\n      <integer>30</integer>"));
    assert_eq!(t.calendario_actual("diaria").unwrap().as_deref(), Some("Mon,Wed 09:30"));

    let e = t.estado("diaria").await.unwrap();
    assert!(e.fichero && e.activo.as_deref() == Some("active") && e.calendario.as_deref() == Some("Mon,Wed 09:30"), "{e:?}");
    // Hoy es jueves 22: el próximo disparo es el lunes 26 a las 09:30 locales (07:30 UTC).
    let (proxima, activo) = t.proxima("diaria").await.unwrap();
    assert_eq!(activo, "active");
    assert_eq!(proxima, Some(datetime!(2026-10-26 07:30:00 UTC).unix_timestamp()));
    assert_eq!(t.proxima_legible("diaria").await.as_deref(), Some("Mon 2026-10-26 09:30:00"));

    assert!(t.desprogramar("diaria").await.unwrap());
    assert!(!plist.exists() && t.calendario_actual("diaria").unwrap().is_none());
    assert!(t.proxima("diaria").await.is_none());

    let ordenes = std::fs::read_to_string(&log).unwrap();
    let uid_label = "gui/501/es.gachapon.ea-rutina.diaria";
    for esperado in [
        format!("bootout\n{uid_label}\n--\n"),
        format!("bootstrap\ngui/501\n{}\n--\n", plist.display()),
    ] {
        assert!(ordenes.contains(&esperado), "falta {esperado:?} en:\n{ordenes}");
    }
}

#[tokio::test]
async fn launchd_deja_todo_como_estaba_si_bootstrap_falla_y_rechaza_lo_no_portable() {
    let d = tempfile::tempdir().unwrap();
    let (t, _) = temporizadores(d.path(), Backend::Launchd);
    std::fs::write(d.path().join("fallo"), "").unwrap();
    let e = t.programar("diaria", "daily").await.unwrap_err();
    assert!(e.contains("el plist se dejó como estaba"), "{e}");
    assert!(!d.path().join("LaunchAgents/es.gachapon.ea-rutina.diaria.plist").exists());
    assert!(t.programar("diaria", "*:0/15").await.unwrap_err().contains("fuera de Linux solo se admite"));
    assert!(t.programar("../x", "daily").await.is_err());
}

#[tokio::test]
async fn schtasks_programa_consulta_y_desprograma() {
    let d = tempfile::tempdir().unwrap();
    let (t, log) = temporizadores(d.path(), Backend::Schtasks);
    t.programar("diaria", "Mon..Fri 09:00").await.unwrap();
    assert_eq!(t.calendario_actual("diaria").unwrap().as_deref(), Some("Mon..Fri 09:00"));
    let e = t.estado("diaria").await.unwrap();
    assert!(e.fichero && e.activo.as_deref() == Some("active"), "{e:?}");
    assert_eq!(t.proxima_legible("diaria").await.as_deref(), Some("Thu 2026-10-22 09:00:00"));

    assert!(t.desprogramar("diaria").await.unwrap());
    assert!(t.calendario_actual("diaria").unwrap().is_none());
    assert!(!t.desprogramar("diaria").await.unwrap());

    let ordenes = std::fs::read_to_string(&log).unwrap();
    let crear = "/Create\n/F\n/SC\nWEEKLY\n/D\nMON,TUE,WED,THU,FRI\n/ST\n09:00\n/RL\nLIMITED\n/TN\necosistema-asistente\\rutina-diaria\n/TR\n\"/Apps/ea con espacio/ea\" rutina lanzar diaria\n--\n";
    assert!(ordenes.contains(crear), "{ordenes}");
    assert!(ordenes.contains("/Delete\n/TN\necosistema-asistente\\rutina-diaria\n/F\n--\n"), "{ordenes}");
}

#[tokio::test]
async fn los_gestores_de_servicios_hablan_con_su_sistema() {
    let d = tempfile::tempdir().unwrap();
    let (orden, log) = falso(d.path());
    let mut l = GestorServicios::para(Sistema::MacOs);
    l.orden = orden.clone();
    l.uid = Some(501);
    assert!(!l.estado("ea-web").await.unwrap().cargado);
    std::fs::write(d.path().join("cargado"), "").unwrap();
    let e = l.estado("ea-web").await.unwrap();
    assert!(e.cargado && e.activo == "active");
    l.reiniciar("ea-web").await.unwrap();
    let panel = l.panel(&["ea-web", "ea-mcp"]).await.unwrap();
    assert_eq!(panel.len(), 2);
    assert!(panel.iter().all(|p| p.activo == "active" && p.activo_s.is_none()));

    let mut w = GestorServicios::para(Sistema::Windows);
    w.orden = orden;
    assert!(w.estado("ea-web").await.unwrap().cargado);
    w.reiniciar("ea-web").await.unwrap();

    let ordenes = std::fs::read_to_string(&log).unwrap();
    assert!(ordenes.contains("print\ngui/501/es.gachapon.ea-web\n--\n"), "{ordenes}");
    assert!(ordenes.contains("kickstart\n-k\ngui/501/es.gachapon.ea-web\n--\n"), "{ordenes}");
    assert!(ordenes.contains("/End\n/TN\necosistema-asistente\\ea-web\n--\n/Run\n/TN\necosistema-asistente\\ea-web\n--\n"), "{ordenes}");
    assert!(ordenes.contains("/Query\n/TN\necosistema-asistente\\ea-web\n/FO\nCSV\n/NH\n/V\n--\n"), "{ordenes}");
}

#[tokio::test]
async fn el_gestor_de_systemd_sigue_igual() {
    let d = tempfile::tempdir().unwrap();
    let log = d.path().join("ordenes.log");
    let guion = d.path().join("systemctl.sh");
    std::fs::write(
        &guion,
        format!(
            "printf '%s\\n' \"$@\" -- >> '{}'\n\
             case \"$2\" in\n\
               show) printf 'LoadState=loaded\\nActiveState=active\\n';;\n\
             esac\n",
            log.display()
        ),
    )
    .unwrap();
    let mut g = GestorServicios::para(Sistema::Linux);
    g.orden = vec!["sh".into(), guion.display().to_string()];
    let e = g.estado("ea-mcp").await.unwrap();
    assert!(e.cargado && e.activo == "active");
    g.reiniciar("ea-mcp").await.unwrap();
    let ordenes = std::fs::read_to_string(&log).unwrap();
    assert!(ordenes.contains("--user\nshow\n-p\nLoadState\n-p\nActiveState\nea-mcp.service\n--\n"), "{ordenes}");
    assert!(ordenes.contains("--user\nrestart\nea-mcp.service\n--\n"), "{ordenes}");
}

#[tokio::test]
async fn linux_escribe_el_servicio_de_la_rutina_con_la_ruta_real_si_falta() {
    let d = tempfile::tempdir().unwrap();
    let (gestor, _) = falso(d.path());
    let mut t = Temporizadores::systemd(d.path().join("unidades"), gestor.clone(), gestor);
    t.ea = Some(PathBuf::from("/opt/ea/ea"));
    t.programar("diaria", "daily").await.unwrap();
    let servicio = std::fs::read_to_string(d.path().join("unidades/ea-rutina@.service")).unwrap();
    assert!(servicio.contains("ExecStart=\"/opt/ea/ea\" rutina lanzar %i\n"), "{servicio}");
    assert!(servicio.contains("Type=oneshot") && !servicio.contains("%h"));
    // Si ya existe (p. ej. enlazado a la plantilla del repo), no se toca.
    std::fs::write(d.path().join("unidades/ea-rutina@.service"), "mio\n").unwrap();
    t.programar("diaria", "hourly").await.unwrap();
    assert_eq!(std::fs::read_to_string(d.path().join("unidades/ea-rutina@.service")).unwrap(), "mio\n");
}
