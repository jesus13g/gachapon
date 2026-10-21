//! Programar y desprogramar el timer de una rutina (spec 004, H5). Con el
//! sistema simulado, y la parte real (`Temporizadores`) con órdenes falsas
//! en un directorio temporal: nunca el `~/.config/systemd/user` real ni el
//! `systemctl` de verdad.

use ea_acciones::*;
use ea_store::{EstadoEjecucion, Store};
use serde_json::{json, Value};

fn store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_at(&dir.path().join("t.db")).unwrap();
    s.guardar_rutina(&ea_store::FilaRutina {
        slug: "diaria".into(),
        version: 2,
        definicion: "[rutina]\nslug = \"diaria\"\nversion = 2\nworktree = \"/tmp\"\ncriterio_exito = \"true\"\n[[pasos]]\ntexto = \"x\"\n".into(),
        creada_en: "2026-01-01T00:00:00Z".into(),
        creada_por: "test".into(),
    })
    .unwrap();
    (s, dir)
}

async fn simular<C: Consulta>(s: &Store, c: &C, nombre: &str, args: &Value) -> Result<Simulacion, ErrorAccion> {
    let h = observar(c, nombre, args).await?;
    simular_con(s, c, &h, nombre, args)
}

#[tokio::test]
async fn programar_rutina_simula_valida_y_rechaza_calendarios_malos() {
    let (s, _d) = store();
    let sis = SistemaSimulado::default();

    let sim = simular(&s, &sis, "ea_programar_rutina", &json!({ "slug": "diaria", "calendario": " *-*-* 08:00:00 " }))
        .await
        .unwrap();
    assert_eq!(sim.riesgo, Riesgo::Reversible);
    assert_eq!(sim.argumentos, json!({ "slug": "diaria", "calendario": "*-*-* 08:00:00" }));
    assert!(sim.texto.contains("OnCalendar=*-*-* 08:00:00"), "{}", sim.texto);
    assert!(sim.texto.contains("versión 2") && sim.texto.contains("ejecución 3"), "{}", sim.texto);
    assert_eq!(sim.precondicion, json!({ "calendario_actual": null, "fichero": false }));

    // Calendarios malos: inyección en el unit, especificadores, vacío,
    // y lo que systemd no entiende. Ninguno llega a programarse.
    for malo in ["daily\nExecStart=/bin/sh -c 'rm -rf ~'", "daily\r", "\u{7}", "", "%h", "a\\b", "cuando quieras"] {
        let args = json!({ "slug": "diaria", "calendario": malo });
        assert!(validar_argumentos("ea_programar_rutina", &args).is_err() || malo == "cuando quieras", "{malo:?}");
        let e = simular(&s, &sis, "ea_programar_rutina", &args).await.unwrap_err();
        assert!(matches!(e, ErrorAccion::Argumentos(_)), "{malo:?}: {e}");
    }
    // Solo rutinas registradas, slugs válidos y los argumentos declarados.
    for (args, no_aplica) in [
        (json!({ "slug": "no-registrada", "calendario": "daily" }), true),
        (json!({ "slug": "../x", "calendario": "daily" }), false),
        (json!({ "slug": "diaria" }), false),
        (json!({ "slug": "diaria", "calendario": "daily", "extra": 1 }), false),
    ] {
        let e = simular(&s, &sis, "ea_programar_rutina", &args).await.unwrap_err();
        assert_eq!(matches!(e, ErrorAccion::NoAplica(_)), no_aplica, "{args}: {e}");
    }
    assert!(sis.mutaciones().is_empty(), "simular no programa nada");

    // Ya programada con ese horario: no hay nada que hacer.
    let sis = SistemaSimulado::default().con_timer("diaria", "daily");
    let e = simular(&s, &sis, "ea_programar_rutina", &json!({ "slug": "diaria", "calendario": "daily" }))
        .await
        .unwrap_err();
    assert!(matches!(e, ErrorAccion::NoAplica(_)), "{e}");
}

#[tokio::test]
async fn programar_se_propone_se_revalida_y_al_aceptar_se_programa() {
    let (mut s, _d) = store();
    let sis = SistemaSimulado::default();
    let args = json!({ "slug": "diaria", "calendario": "Mon *-*-* 09:00" });
    let h = observar(&sis, "ea_programar_rutina", &args).await.unwrap();
    let (p, _) = proponer(&s, &sis, &h, "claude-code", None, "ea_programar_rutina", &args).unwrap();
    assert!(sis.mutaciones().is_empty(), "proponer no programa");

    // Alguien programa la rutina entre la simulación y la aceptación: la
    // revalidación lo ve y no se pisa.
    sis.timers.lock().unwrap().insert("diaria".into(), "daily".into());
    let p2 = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Web).await.unwrap();
    let ej = p2.ejecucion.unwrap();
    assert_eq!(ej.estado, EstadoEjecucion::Fallida);
    assert!(ej.detalle.as_deref().unwrap_or_default().contains("calendario_actual"), "{ej:?}");
    assert!(sis.mutaciones().is_empty());

    // Otra propuesta sobre el estado nuevo: se acepta y se programa.
    let h = observar(&sis, "ea_programar_rutina", &args).await.unwrap();
    let (p, _) = proponer(&s, &sis, &h, "claude-code", None, "ea_programar_rutina", &args).unwrap();
    let p3 = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Web).await.unwrap();
    assert_eq!(p3.ejecucion.unwrap().estado, EstadoEjecucion::Ejecutada);
    assert_eq!(sis.mutaciones(), ["programar:diaria=Mon *-*-* 09:00"]);

    // Y se desprograma, también de una rutina que ya no esté registrada.
    let r = ejecutar_directo(&mut s, &sis, "usuario", Canal::Web, "ea_desprogramar_rutina", &json!({ "slug": "diaria" }))
        .await
        .unwrap();
    assert_eq!(r.datos["desprogramada"], true);
    let e = simular(&s, &sis, "ea_desprogramar_rutina", &json!({ "slug": "diaria" })).await.unwrap_err();
    assert!(matches!(e, ErrorAccion::NoAplica(_)), "sin timer no hay nada que quitar: {e}");
    let sis = sis.con_timer("retirada", "daily");
    let sim = simular(&s, &sis, "ea_desprogramar_rutina", &json!({ "slug": "retirada" })).await.unwrap();
    assert!(sim.texto.contains("OnCalendar=daily") && sim.texto.contains("disable --now"), "{}", sim.texto);
}

/// Un `systemctl` y un `systemd-analyze` falsos: apuntan sus argumentos (uno
/// por línea) y responden como los de verdad lo justo para las pruebas.
fn temporizadores(dir: &std::path::Path) -> (Temporizadores, std::path::PathBuf) {
    let log = dir.join("ordenes.log");
    let sc = dir.join("systemctl.sh");
    std::fs::write(
        &sc,
        format!(
            "printf '%s\\n' systemctl \"$@\" -- >> '{log}'\n\
             if [ \"$2\" = is-active ]; then\n\
               if [ -f '{dir}/unidades/'\"$3\" ]; then echo active; else echo inactive; exit 3; fi\n\
             fi\n",
            log = log.display(),
            dir = dir.display()
        ),
    )
    .unwrap();
    let sa = dir.join("analyze.sh");
    std::fs::write(
        &sa,
        format!(
            "printf '%s\\n' analyze \"$@\" -- >> '{log}'\n\
             case \"$4\" in *malo*) echo \"Failed to parse calendar specification '$4'\" >&2; exit 1;; esac\n\
             echo \"Normalized form: $4\"\n\
             echo '    Next elapse: Thu 2026-10-22 08:00:00 CEST'\n\
             echo '   Iteration #2: Fri 2026-10-23 08:00:00 CEST'\n\
             echo '   Iteration #3: Sat 2026-10-24 08:00:00 CEST'\n",
            log = log.display()
        ),
    )
    .unwrap();
    let sh = |p: &std::path::Path| vec!["sh".to_string(), p.display().to_string()];
    (Temporizadores { unidades_dir: dir.join("unidades"), systemctl: sh(&sc), systemd_analyze: sh(&sa) }, log)
}

#[tokio::test]
async fn el_timer_real_se_escribe_con_argumentos_y_se_quita() {
    let d = tempfile::tempdir().unwrap();
    let (t, log) = temporizadores(d.path());

    let c = t.validar_calendario("*-*-* 08:00:00").await.unwrap().unwrap();
    assert_eq!(c.proximas.len(), 3);
    assert!(t.validar_calendario("muy malo").await.unwrap().is_err());
    assert!(t.validar_calendario("daily\nExecStart=x").await.unwrap().is_err());

    // El calendario con espacios y `;` llega como un solo argumento, sin shell.
    t.programar("diaria", "Mon *-*-* 09:00").await.unwrap();
    let fichero = d.path().join("unidades/ea-rutina@diaria.timer");
    let texto = std::fs::read_to_string(&fichero).unwrap();
    assert!(texto.contains("\nOnCalendar=Mon *-*-* 09:00\n"), "{texto}");
    assert_eq!(t.calendario_actual("diaria").unwrap().as_deref(), Some("Mon *-*-* 09:00"));
    let e = t.estado("diaria").await.unwrap();
    assert!(e.fichero && e.activo.as_deref() == Some("active"), "{e:?}");

    assert!(t.desprogramar("diaria").await.unwrap());
    assert!(!fichero.exists());
    assert!(!t.desprogramar("diaria").await.unwrap(), "la segunda vez no hay nada");

    let ordenes = std::fs::read_to_string(&log).unwrap();
    for esperado in [
        "analyze\ncalendar\n--iterations=3\n--\n*-*-* 08:00:00\n",
        "systemctl\n--user\ndaemon-reload\n",
        "systemctl\n--user\nenable\nea-rutina@diaria.timer\n",
        "systemctl\n--user\nrestart\nea-rutina@diaria.timer\n",
        "systemctl\n--user\ndisable\n--now\nea-rutina@diaria.timer\n",
    ] {
        assert!(ordenes.contains(esperado), "falta {esperado:?} en:\n{ordenes}");
    }
    // El calendario malo con salto de línea ni siquiera llegó a systemd.
    assert!(!ordenes.contains("ExecStart"));
}

#[tokio::test]
async fn si_systemctl_falla_el_fichero_del_timer_queda_como_estaba() {
    let d = tempfile::tempdir().unwrap();
    let (mut t, _) = temporizadores(d.path());
    t.systemctl = vec!["false".into()];
    assert!(t.programar("diaria", "daily").await.is_err());
    assert!(!d.path().join("unidades/ea-rutina@diaria.timer").exists());
}
