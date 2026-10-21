//! Retirar y restaurar una rutina (spec 004, RF-011) con el sistema
//! simulado y con el real sobre un directorio temporal.

use ea_acciones::*;
use ea_store::Store;
use serde_json::{json, Value};

fn store() -> (Store, tempfile::TempDir) {
    static ESTADO: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let estado = ESTADO.get_or_init(|| tempfile::tempdir().unwrap());
    unsafe { std::env::set_var("EA_ESTADO_DIR", estado.path()) };
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_at(&dir.path().join("t.db")).unwrap();
    s.guardar_rutina(&ea_store::FilaRutina {
        slug: "nocturna".into(),
        version: 1,
        definicion: RUTINA.into(),
        creada_en: "2026-01-01T00:00:00Z".into(),
        creada_por: "test".into(),
    })
    .unwrap();
    (s, dir)
}

const RUTINA: &str = "[rutina]\nslug = \"nocturna\"\nversion = 1\nworktree = \"/tmp\"\ncriterio_exito = \"true\"\n[[pasos]]\ntexto = \"x\"\n";

async fn simular<C: Consulta>(s: &Store, c: &C, nombre: &str, args: &Value) -> Result<Simulacion, ErrorAccion> {
    let h = observar(c, nombre, args).await?;
    simular_con(s, c, &h, nombre, args)
}

/// Retirar desprograma el timer y lo dice; con la rutina retirada, encolarla
/// y programarla se rechazan; restaurar la devuelve sin reprogramar el
/// timer; y borrarla para siempre exige que esté retirada.
#[tokio::test]
async fn retirar_desprograma_el_timer_y_restaurar_no_lo_reprograma() {
    let (mut s, _d) = store();
    let sis = SistemaSimulado::default().con_rutina("nocturna", RUTINA).con_timer("nocturna", "daily");

    // Activa: no se borra para siempre ni se restaura.
    let e = simular(&s, &sis, "ea_eliminar_rutina", &json!({ "slug": "nocturna" })).await.unwrap_err();
    assert!(e.to_string().contains("retírala antes"), "{e}");
    assert!(simular(&s, &sis, "ea_restaurar_rutina", &json!({ "slug": "nocturna" })).await.is_err());

    let sim = simular(&s, &sis, "ea_retirar_rutina", &json!({ "slug": "nocturna" })).await.unwrap();
    assert_eq!(sim.riesgo, Riesgo::Reversible);
    assert!(sim.texto.contains("desprograma su timer (OnCalendar=daily)"), "{}", sim.texto);
    assert!(sim.texto.contains("ea_restaurar_rutina"), "{}", sim.texto);
    let r = ejecutar(&mut s, &sis, "usuario", Canal::Web, "ea_retirar_rutina", &sim.argumentos, &sim.precondicion, None)
        .await
        .unwrap();
    assert!(r.texto.contains("timer desprogramado"), "{}", r.texto);
    assert_eq!(sis.mutaciones(), vec!["desprogramar:nocturna", "retirar:nocturna"]);
    assert!(sis.rutina_contenido("nocturna").unwrap().is_none());
    assert!(sis.rutina_retirada("nocturna").unwrap().is_some());

    // Retirada: no se lanza ni se programa, con el texto único del bus.
    let motivo = ea_bus::Ubicacion::Retirada.motivo_no_lanzable("nocturna").unwrap();
    let e = simular(&s, &sis, "ea_encolar_rutina", &json!({ "slug": "nocturna" })).await.unwrap_err();
    assert_eq!(e.to_string(), ErrorAccion::NoAplica(motivo.clone()).to_string());
    let e = simular(&s, &sis, "ea_programar_rutina", &json!({ "slug": "nocturna", "calendario": "daily" }))
        .await
        .unwrap_err();
    assert!(e.to_string().contains(&motivo), "{e}");
    assert!(simular(&s, &sis, "ea_retirar_rutina", &json!({ "slug": "nocturna" })).await.is_err());

    let sim = simular(&s, &sis, "ea_restaurar_rutina", &json!({ "slug": "nocturna" })).await.unwrap();
    assert!(sim.texto.contains("No vuelve a programar su timer"), "{}", sim.texto);
    ejecutar(&mut s, &sis, "usuario", Canal::Web, "ea_restaurar_rutina", &sim.argumentos, &sim.precondicion, None)
        .await
        .unwrap();
    assert!(sis.rutina_contenido("nocturna").unwrap().is_some());
    assert!(!sis.timers.lock().unwrap().contains_key("nocturna"), "restaurar no reprograma");
    assert!(simular(&s, &sis, "ea_encolar_rutina", &json!({ "slug": "nocturna" })).await.is_ok());

    // Sin timer, retirar lo dice y no lo toca.
    let sim = simular(&s, &sis, "ea_retirar_rutina", &json!({ "slug": "nocturna" })).await.unwrap();
    assert!(sim.texto.contains("No tiene timer"), "{}", sim.texto);
}

/// Con el sistema real sobre `rutinas_dir` temporal: el TOML va a
/// `.papelera/` y vuelve, y la revalidación para un retiro si el fichero
/// cambió desde la simulación.
#[tokio::test]
async fn la_papelera_real_mueve_el_fichero_y_revalida() {
    let (mut s, d) = store();
    let rutinas = d.path().join("rutinas");
    std::fs::create_dir_all(&rutinas).unwrap();
    std::fs::write(rutinas.join("nocturna.toml"), RUTINA).unwrap();
    let unidades = d.path().join("unidades");
    std::fs::create_dir_all(&unidades).unwrap();
    // `true` como systemctl: no hay timer y nada sale del directorio temporal.
    let temporizadores =
        Temporizadores { unidades_dir: unidades, systemctl: vec!["true".into()], systemd_analyze: vec!["true".into()] };
    let sis = SistemaReal {
        rutinas_dir: rutinas.clone(),
        boveda: Boveda::en(d.path().join("boveda")),
        propio: None,
        temporizadores,
    };

    let sim = simular(&s, &sis, "ea_retirar_rutina", &json!({ "slug": "nocturna" })).await.unwrap();
    std::fs::write(rutinas.join("nocturna.toml"), format!("{RUTINA}# cambio\n")).unwrap();
    let e = ejecutar(&mut s, &sis, "usuario", Canal::Web, "ea_retirar_rutina", &sim.argumentos, &sim.precondicion, None)
        .await
        .unwrap_err();
    assert!(e.to_string().contains("cambió"), "{e}");
    assert!(rutinas.join("nocturna.toml").exists());

    let sim = simular(&s, &sis, "ea_retirar_rutina", &json!({ "slug": "nocturna" })).await.unwrap();
    ejecutar(&mut s, &sis, "usuario", Canal::Web, "ea_retirar_rutina", &sim.argumentos, &sim.precondicion, None)
        .await
        .unwrap();
    assert!(!rutinas.join("nocturna.toml").exists());
    assert!(rutinas.join(ea_bus::DIR_PAPELERA).join("nocturna.toml").exists());
    assert_eq!(ea_bus::ubicacion_rutina(&rutinas, "nocturna").unwrap(), ea_bus::Ubicacion::Retirada);

    let sim = simular(&s, &sis, "ea_restaurar_rutina", &json!({ "slug": "nocturna" })).await.unwrap();
    ejecutar(&mut s, &sis, "usuario", Canal::Web, "ea_restaurar_rutina", &sim.argumentos, &sim.precondicion, None)
        .await
        .unwrap();
    assert!(rutinas.join("nocturna.toml").exists());
    assert!(!rutinas.join(ea_bus::DIR_PAPELERA).join("nocturna.toml").exists());
}
