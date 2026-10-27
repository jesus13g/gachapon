//! La promoción de una nota a la región compartida apoyada en el mismo flujo
//! que el resto de mutaciones: un agente solo propone; aceptar (el usuario)
//! la ejecuta; el diario lo cuenta. Con bóveda y base temporales.

use ea_acciones::*;
use ea_boveda::{Boveda, Limites, NuevaNota};
use ea_store::{EstadoEjecucion, EstadoPropuesta, EventFilter, Store};
use serde_json::{json, Value};

struct Entorno {
    store: Store,
    boveda: Boveda,
    dir: tempfile::TempDir,
}

fn entorno() -> Entorno {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_at(&dir.path().join("t.db")).unwrap();
    let boveda = Boveda::en(dir.path().join("boveda"));
    Entorno { store, boveda, dir }
}

impl Entorno {
    fn real(&self) -> SistemaReal {
        SistemaReal { rutinas_dir: self.dir.path().join("rutinas"), boveda: self.boveda.clone(), propio: None, temporizadores: ea_acciones::Temporizadores::systemd(self.dir.path().join("unidades"), vec!["true".into()], vec!["true".into()]) }
    }
    fn consulta(&self) -> SoloConsulta {
        SoloConsulta { rutinas_dir: self.dir.path().join("rutinas"), boveda: self.boveda.clone() }
    }
    fn nota(&mut self, autor: &str, titulo: &str, cuerpo: &str) -> String {
        self.boveda
            .escribir_nota(
                &mut self.store,
                autor,
                &NuevaNota { titulo, cuerpo, ruta: None, tags: &[], origen: "manual", sobrescribir: false },
                &Limites::AMPLIOS,
            )
            .unwrap()
            .ruta
    }
    fn compartida(&self) -> std::path::PathBuf {
        self.boveda.raiz().join("compartida")
    }
    fn acciones(&self) -> Vec<String> {
        let mut v: Vec<_> = self
            .store
            .list_events(&EventFilter { limit: Some(500), ..Default::default() })
            .unwrap()
            .into_iter()
            .map(|e| e.action)
            .collect();
        v.reverse();
        v
    }
}

async fn proponer_(e: &Entorno, quien: &str, args: &Value) -> Result<ea_store::Propuesta, ErrorAccion> {
    let c = e.consulta();
    let h = observar(&c, "ea_promover_nota", args).await?;
    Ok(proponer(&e.store, &c, &h, quien, None, "ea_promover_nota", args)?.0)
}

#[tokio::test]
async fn proponer_no_copia_nada_y_aceptar_la_ejecuta_con_su_rastro_en_el_diario() {
    let mut e = entorno();
    let ruta = e.nota("modelo-local", "Hallazgo", "El puerto es 8765.");
    let args = json!({ "ruta": ruta });

    // Un agente propone: simulación y propuesta pendiente, la compartida sigue vacía.
    let p = proponer_(&e, "modelo-local", &args).await.unwrap();
    assert_eq!(p.estado, EstadoPropuesta::Pendiente);
    let a = p.accion.as_ref().unwrap();
    assert_eq!((a.herramienta.as_str(), a.riesgo.as_str()), ("ea_promover_nota", "reversible"));
    assert!(a.simulacion.contains("compartida/hallazgo.md"));
    assert!(!e.compartida().exists());
    // Una propuesta idéntica no se duplica.
    let otra = proponer_(&e, "modelo-local", &args).await.unwrap();
    assert_eq!(otra.id, p.id);

    // El usuario acepta (en la web, p. ej.): se copia, queda ejecutada y en el diario.
    let sis = e.real();
    let p = aceptar_propuesta(&mut e.store, &sis, p.id, "usuario", Canal::Web).await.unwrap();
    assert_eq!(p.ejecucion.as_ref().unwrap().estado, EstadoEjecucion::Ejecutada, "{:?}", p.ejecucion);
    let copia = e.boveda.leer("compartida/hallazgo.md", 0, &Limites::AMPLIOS).unwrap();
    assert_eq!(copia.contenido, "El puerto es 8765.\n");
    let c = copia.cabecera.unwrap();
    assert_eq!((c.promocion.as_str(), c.promovida_por.as_deref()), ("promovida", Some("usuario")));
    // El original se conserva.
    assert!(e.boveda.raiz().join(&ruta).exists());

    let acc = e.acciones();
    for esperada in ["propuesta_encolada", "propuesta_decidida", "boveda_promocion", "accion_ejecutada"] {
        assert!(acc.contains(&esperada.to_string()), "falta {esperada}: {acc:?}");
    }
    let ev = e.store.list_events(&EventFilter { limit: Some(50), ..Default::default() }).unwrap();
    let promo = ev.iter().find(|x| x.action == "boveda_promocion").unwrap();
    assert_eq!(promo.actor, "usuario");
    assert!(promo.detail.as_ref().unwrap().contains("\"canal\":\"web\""));
}

#[tokio::test]
async fn rechazar_la_propuesta_no_copia_nada() {
    let mut e = entorno();
    let ruta = e.nota("claude-code", "No", "no quiero esto");
    let p = proponer_(&e, "claude-code", &json!({ "ruta": ruta })).await.unwrap();
    rechazar_propuesta(&e.store, p.id, "usuario", Canal::Terminal).unwrap();
    assert!(!e.compartida().exists());
    assert!(!e.acciones().contains(&"boveda_promocion".to_string()));
}

#[tokio::test]
async fn si_la_nota_cambia_tras_proponer_aceptar_falla_y_no_copia() {
    let mut e = entorno();
    let ruta = e.nota("modelo-local", "Cambiante", "versión uno");
    let p = proponer_(&e, "modelo-local", &json!({ "ruta": ruta })).await.unwrap();
    // El agente edita la nota después de proponerla: el usuario vio otra cosa.
    e.boveda
        .escribir_nota(
            &mut e.store,
            "modelo-local",
            &NuevaNota { titulo: "Cambiante", cuerpo: "versión dos", ruta: Some("cambiante.md"), tags: &[], origen: "manual", sobrescribir: true },
            &Limites::AMPLIOS,
        )
        .unwrap();
    let sis = e.real();
    let p = aceptar_propuesta(&mut e.store, &sis, p.id, "usuario", Canal::Terminal).await.unwrap();
    let ej = p.ejecucion.unwrap();
    assert_eq!(ej.estado, EstadoEjecucion::Fallida);
    assert!(ej.detalle.unwrap().contains("cambió"));
    assert!(!e.compartida().exists());
    assert!(e.acciones().contains(&"accion_fallida".to_string()));
}

#[tokio::test]
async fn la_capa_de_consulta_no_puede_promover_y_las_rutas_hostiles_no_llegan_a_proponerse() {
    let mut e = entorno();
    let ruta = e.nota("modelo-local", "Algo", "x");
    // Solo con SoloConsulta (lo que tienen ea-mcp y el orquestador) no hay forma
    // de ejecutar: `ejecutar` exige un `Sistema`, y SoloConsulta no lo implementa.
    // (Se comprueba en compilación; aquí, que simular tampoco escribe.)
    let c = e.consulta();
    let h = observar(&c, "ea_promover_nota", &json!({ "ruta": ruta })).await.unwrap();
    simular_con(&e.store, &c, &h, "ea_promover_nota", &json!({ "ruta": ruta })).unwrap();
    assert!(!e.compartida().exists());

    for mala in ["../x.md", "/etc/passwd", "compartida/x.md", "modelo-local/no-existe.md", "otra/x.md"] {
        let r = proponer_(&e, "modelo-local", &json!({ "ruta": mala })).await;
        assert!(r.is_err(), "{mala}");
    }
    let r = proponer_(&e, "modelo-local", &json!({ "ruta": ruta, "destino": "../../fuera.md" })).await;
    assert!(r.is_err());
    let r = proponer_(&e, "modelo-local", &json!({ "ruta": ruta, "extra": 1 })).await;
    assert!(matches!(r, Err(ErrorAccion::Argumentos(_))));

    // Un agente tampoco llega por el atajo de la API de la bóveda.
    let plan = e.boveda.planear_promocion(&ruta, None).unwrap();
    for quien in ["modelo-local", "claude-code", "orquestador"] {
        assert!(e.boveda.promover(&mut e.store, &ruta, None, &plan.huella, quien, "x").is_err());
    }
    assert!(!e.compartida().exists());
}

#[tokio::test]
async fn con_el_sistema_simulado_la_promocion_pasa_por_el_mismo_flujo() {
    let (mut s, _d) = {
        let d = tempfile::tempdir().unwrap();
        (Store::open_at(&d.path().join("t.db")).unwrap(), d)
    };
    let sis = SistemaSimulado::default().con_nota("claude-code/a.md", "contenido");
    let args = json!({ "ruta": "claude-code/a.md", "destino": "x/b.md" });
    let h = observar(&sis, "ea_promover_nota", &args).await.unwrap();
    let (p, _) = proponer(&s, &sis, &h, "claude-code", None, "ea_promover_nota", &args).unwrap();
    assert!(sis.mutaciones().is_empty());
    let p = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Terminal).await.unwrap();
    assert_eq!(p.ejecucion.unwrap().estado, EstadoEjecucion::Ejecutada);
    assert_eq!(sis.mutaciones(), vec!["promover:claude-code/a.md->compartida/x/b.md".to_string()]);
}

#[test]
fn el_catalogo_dice_quien_ve_que() {
    let escribir = buscar("ea_escribir_nota").unwrap();
    assert!(autorizar_mcp("orquestador", escribir).is_err(), "lo desatendido no escribe");
    assert!(autorizar_mcp("modelo-local", escribir).is_ok());
    assert!(autorizar_mcp("claude-code", escribir).is_ok());
    let promover = buscar("ea_promover_nota").unwrap();
    assert_eq!(promover.ejecucion, Ejecucion::Supervisada);
    assert!(autorizar_mcp("orquestador", promover).is_err());
    for n in ["ea_buscar_notas", "ea_leer_nota"] {
        let h = buscar(n).unwrap();
        assert_eq!(h.riesgo, Riesgo::Lectura);
        assert!(autorizar_mcp("orquestador", h).is_ok());
    }
}
