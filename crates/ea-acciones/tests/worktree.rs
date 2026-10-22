//! `ea_limpiar_worktree` (spec 019, anexo A): simula sin cambiar nada, solo
//! trabajos terminados con el worktree limpio, nunca con `--force`.

use ea_acciones::*;
use ea_bus::InfoWorktree;
use ea_store::Store;
use serde_json::{json, Value};

fn store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    (Store::open_at(&dir.path().join("t.db")).unwrap(), dir)
}

fn info(trabajo: &str, estado: &'static str, limpio: Option<bool>, ocupado: bool) -> InfoWorktree {
    InfoWorktree {
        trabajo: trabajo.into(),
        ruta: format!("/estado/worktrees/{trabajo}"),
        rama: format!("ea/{trabajo}"),
        estado_trabajo: Some(estado),
        titulo: Some("t".into()),
        limpio,
        ocupado,
    }
}

async fn simular(s: &Store, c: &SistemaSimulado, args: &Value) -> Result<Simulacion, ErrorAccion> {
    let h = observar(c, "ea_limpiar_worktree", args).await?;
    simular_con(s, c, &h, "ea_limpiar_worktree", args)
}

#[tokio::test]
async fn simular_no_cambia_nada_y_ejecutar_quita_el_worktree() {
    let (mut s, _d) = store();
    let sis = SistemaSimulado::default().con_worktree(info("t-1", "cerrado", Some(true), false));
    let args = json!({ "trabajo": "t-1" });
    let sim = simular(&s, &sis, &args).await.unwrap();
    assert_eq!(sim.riesgo, Riesgo::Reversible);
    assert!(sim.texto.contains("sin --force") && sim.texto.contains("ea/t-1"), "{}", sim.texto);
    assert!(sis.mutaciones().is_empty() && sis.worktree_info("t-1").unwrap().is_some());
    let r = ejecutar(&mut s, &sis, "usuario", Canal::Web, "ea_limpiar_worktree", &sim.argumentos, &sim.precondicion, None)
        .await
        .unwrap();
    assert!(r.texto.contains("ea/t-1"), "{}", r.texto);
    assert_eq!(sis.mutaciones(), vec!["limpiar_worktree:t-1"]);
    assert!(sis.worktree_info("t-1").unwrap().is_none());
    // ya no hay nada que limpiar
    assert!(matches!(simular(&s, &sis, &args).await.unwrap_err(), ErrorAccion::NoAplica(_)));
}

#[tokio::test]
async fn se_rechaza_si_no_ha_terminado_esta_ocupado_o_esta_sucio() {
    let (s, _d) = store();
    for (w, frase) in [
        (info("t-1", "ejecutando", Some(true), false), "no ha terminado"),
        (info("t-1", "en_revision", Some(true), false), "no ha terminado"),
        (info("t-1", "cerrado", Some(true), true), "semáforo"),
        (info("t-1", "cancelado", Some(false), false), "nunca se fuerza"),
        (info("t-1", "cerrado", None, false), "no pudo decir"),
    ] {
        let sis = SistemaSimulado::default().con_worktree(w);
        let e = simular(&s, &sis, &json!({ "trabajo": "t-1" })).await.unwrap_err();
        assert!(matches!(e, ErrorAccion::NoAplica(_)) && e.to_string().contains(frase), "{frase}: {e}");
    }
    for malo in [json!({ "trabajo": "../x" }), json!({ "trabajo": "" }), json!({ "trabajo": "t-1", "extra": 1 })] {
        assert!(simular(&s, &SistemaSimulado::default(), &malo).await.is_err(), "{malo}");
    }
}

#[tokio::test]
async fn al_ejecutar_se_revalida_con_el_estado_de_ahora() {
    let (mut s, _d) = store();
    let sis = SistemaSimulado::default().con_worktree(info("t-1", "cerrado", Some(true), false));
    let sim = simular(&s, &sis, &json!({ "trabajo": "t-1" })).await.unwrap();
    // mientras el usuario decidía, alguien ensució el worktree
    sis.worktrees.lock().unwrap().get_mut("t-1").unwrap().limpio = Some(false);
    let e = ejecutar(&mut s, &sis, "usuario", Canal::Web, "ea_limpiar_worktree", &sim.argumentos, &sim.precondicion, None)
        .await
        .unwrap_err();
    assert!(e.to_string().contains("el estado cambió"), "{e}");
    assert!(sis.mutaciones().is_empty());
}
