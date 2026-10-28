//! Pruebas del flujo de mutaciones supervisadas con el sistema simulado:
//! nunca se toca `systemd` ni el disco.

use ea_acciones::*;
use ea_store::{EstadoEjecucion, EstadoPropuesta, EventFilter, NuevaPropuesta, Store, StoreError};
use serde_json::{Value, json};

/// Un directorio absoluto que existe en cada sistema (`/tmp` no es absoluta en Windows).
#[cfg(windows)]
macro_rules! tmp {
    () => {
        "C:/Windows"
    };
}
#[cfg(not(windows))]
macro_rules! tmp {
    () => {
        "/tmp"
    };
}

/// Una sola raíz de estado del bus para todo el proceso de pruebas.
fn estado() -> &'static std::path::Path {
    static ESTADO: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    ESTADO.get_or_init(|| tempfile::tempdir().unwrap()).path()
}

fn store() -> (Store, tempfile::TempDir) {
    let def = |slug: &str| {
        format!(
            concat!("[rutina]\nslug = \"{slug}\"\nversion = 1\nworktree = \"", tmp!(), "\"\ncriterio_exito = \"true\"\n[[pasos]]\ntexto = \"x\"\n"),
            slug = slug
        )
        .replace(tmp!(), ea_pruebas::repo_comun())
    };
    let (revision, vieja, nocturna) = (def("revision-diaria"), def("vieja"), def("nocturna"));
    ea_pruebas::store(&[
        ("revision-diaria", 1, &revision),
        ("vieja", 1, &vieja),
        ("nocturna", 1, &nocturna),
        (
            "con-entradas",
            1,
            &concat!(
                "[rutina]\nslug = \"con-entradas\"\nversion = 1\nworktree = \"",
                tmp!(),
                "\"\ncriterio_exito = \"test -f {salida}\"\n[entradas]\nsalida = \"informe.md\"\n[[pasos]]\ntexto = \"escribe {salida}\"\n"
            )
            .replace(tmp!(), ea_pruebas::repo_comun()),
        ),
    ])
}

fn sistema() -> SistemaSimulado {
    SistemaSimulado::default()
        .con_estado(estado())
        .con_servicio("ea-mcp", "active")
        // «vieja» está retirada: en la papelera, lo único que se borra para siempre.
        .con_retirada("vieja", "[rutina]\ndescripcion = \"x\"\n")
        .con_rutina("revision-diaria", "[rutina]\ndescripcion = \"y\"\n")
        .con_rutina("nocturna", "[rutina]\ndescripcion = \"z\"\n")
        .con_rutina("con-entradas", "[rutina]\ndescripcion = \"e\"\n")
}

fn acciones(s: &Store) -> Vec<String> {
    let mut v: Vec<_> = s.list_events(&EventFilter { limit: Some(500), ..Default::default() }).unwrap().into_iter().map(|e| e.action).collect();
    v.reverse();
    v
}

async fn simular(s: &Store, sis: &SistemaSimulado, nombre: &str, args: &Value) -> Result<Simulacion, ErrorAccion> {
    let h = observar(sis, nombre, args).await?;
    simular_con(s, sis, &h, nombre, args)
}

async fn proponer_(s: &Store, sis: &SistemaSimulado, prop: &str, nombre: &str, args: &Value) -> Result<(ea_store::Propuesta, bool), ErrorAccion> {
    let h = observar(sis, nombre, args).await?;
    proponer(s, sis, &h, prop, None, nombre, args)
}

fn todas_las_llamadas() -> Vec<(&'static str, Value)> {
    vec![
        ("ea_encolar_rutina", json!({ "slug": "revision-diaria" })),
        ("ea_reiniciar_servicio", json!({ "servicio": "ea-mcp" })),
        ("ea_eliminar_rutina", json!({ "slug": "vieja" })),
        ("ea_retirar_rutina", json!({ "slug": "nocturna" })),
        ("ea_restaurar_rutina", json!({ "slug": "vieja" })),
    ]
}

#[tokio::test]
async fn simular_describe_sin_ningun_efecto() {
    let (s, _d) = store();
    let sis = sistema();
    let eventos_antes = acciones(&s).len();
    for (nombre, args) in todas_las_llamadas() {
        let sim = simular(&s, &sis, nombre, &args).await.unwrap();
        assert!(!sim.texto.is_empty(), "{nombre}");
        assert_eq!(sim.riesgo, buscar(nombre).unwrap().riesgo);
    }
    assert_eq!(acciones(&s).len(), eventos_antes, "simular no escribe en el diario ni en ningún sitio");
    assert!(sis.mutaciones().is_empty());
    assert!(sis.rutina_retirada("vieja").unwrap().is_some());
    assert!(sis.rutina_contenido("nocturna").unwrap().is_some());
}

#[tokio::test]
async fn argumentos_invalidos_y_fuera_de_lista_se_rechazan() {
    let (s, _d) = store();
    let sis = sistema();
    for (nombre, args) in [
        ("ea_reiniciar_servicio", json!({ "servicio": "sshd" })),
        ("ea_reiniciar_servicio", json!({ "servicio": "ea-mcp; rm -rf /" })),
        ("ea_eliminar_rutina", json!({ "slug": "../../etc/passwd" })),
        ("ea_eliminar_rutina", json!({ "slug": "no-existe" })),
        ("ea_eliminar_rutina", json!({ "slug": "vieja", "extra": 1 })),
        ("ea_listar_trabajos", json!({})), // no es supervisada
        ("ea_inexistente", json!({})),
    ] {
        assert!(simular(&s, &sis, nombre, &args).await.is_err(), "{nombre} {args}");
    }
}

#[tokio::test]
async fn proponer_encola_pero_no_ejecuta_y_no_duplica() {
    let (s, _d) = store();
    let sis = sistema();
    let args = json!({ "servicio": "ea-mcp" });

    let (p, nueva) = proponer_(&s, &sis, "claude-code", "ea_reiniciar_servicio", &args).await.unwrap();
    assert!(nueva);
    assert_eq!(p.estado, EstadoPropuesta::Pendiente);
    assert_eq!(p.proponente.as_deref(), Some("claude-code"));
    let a = p.accion.as_ref().unwrap();
    assert_eq!(a.riesgo, "reversible");
    assert!(a.simulacion.contains("ea-mcp"));
    assert!(p.ejecucion.is_none());
    assert!(sis.mutaciones().is_empty(), "proponer no ejecuta");

    let (p2, nueva2) = proponer_(&s, &sis, "modelo-local", "ea_reiniciar_servicio", &args).await.unwrap();
    assert!(!nueva2);
    assert_eq!(p2.id, p.id);
    assert!(acciones(&s).contains(&"propuesta_encolada".to_string()));
}

#[tokio::test]
async fn la_cola_de_propuestas_con_accion_tiene_tope_por_proponente() {
    let (s, _d) = store();
    let mut sis = sistema();
    for i in 0..=MAX_PEND {
        sis = sis.con_retirada(&format!("r{i}"), "x");
    }
    for i in 0..MAX_PEND {
        proponer_(&s, &sis, "claude-code", "ea_eliminar_rutina", &json!({ "slug": format!("r{i}") })).await.unwrap();
    }
    let e = proponer_(&s, &sis, "claude-code", "ea_eliminar_rutina", &json!({ "slug": format!("r{MAX_PEND}") })).await.unwrap_err();
    assert!(e.to_string().contains("pendientes") && e.to_string().contains("claude-code"), "{e}");
    // El tope es por proponente (U-08): uno lleno no bloquea a otro, y una
    // igual a una pendiente ajena se sigue devolviendo sin duplicar.
    let (p, nueva) = proponer_(&s, &sis, "modelo-local", "ea_eliminar_rutina", &json!({ "slug": format!("r{MAX_PEND}") })).await.unwrap();
    assert!(nueva && p.proponente.as_deref() == Some("modelo-local"));
    let (igual, nueva) = proponer_(&s, &sis, "modelo-local", "ea_eliminar_rutina", &json!({ "slug": "r0" })).await.unwrap();
    assert!(!nueva && igual.proponente.as_deref() == Some("claude-code"));
}
const MAX_PEND: usize = ea_acciones::flujo::MAX_PENDIENTES_CON_ACCION;

#[tokio::test]
async fn aceptar_ejecuta_y_registra_en_propuesta_y_diario() {
    let (mut s, _d) = store();
    let sis = sistema();
    let (p, _) = proponer_(&s, &sis, "claude-code", "ea_reiniciar_servicio", &json!({ "servicio": "ea-mcp" })).await.unwrap();

    let p = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Web).await.unwrap();
    assert_eq!(p.estado, EstadoPropuesta::Aceptada);
    let ej = p.ejecucion.unwrap();
    assert_eq!(ej.estado, EstadoEjecucion::Ejecutada);
    assert!(ej.detalle.unwrap().contains("reiniciado"));
    assert_eq!(sis.mutaciones(), vec!["reiniciar:ea-mcp"]);

    let ev = s
        .list_events(&EventFilter { limit: Some(50), ..Default::default() })
        .unwrap()
        .into_iter()
        .find(|e| e.action == "accion_ejecutada")
        .expect("evento accion_ejecutada");
    assert_eq!(ev.actor, "usuario");
    let d: Value = serde_json::from_str(&ev.detail.unwrap()).unwrap();
    assert_eq!(d["herramienta"], "ea_reiniciar_servicio");
    assert_eq!(d["argumentos"]["servicio"], "ea-mcp");
    assert_eq!(d["proponente"], "claude-code");
    assert_eq!(d["canal"], "web");
    assert_eq!(d["propuesta"], p.id);
}

#[tokio::test]
async fn aceptar_dos_veces_no_ejecuta_dos_veces() {
    let (mut s, _d) = store();
    let sis = sistema();
    let (p, _) = proponer_(&s, &sis, "claude-code", "ea_reiniciar_servicio", &json!({ "servicio": "ea-mcp" })).await.unwrap();
    aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Terminal).await.unwrap();
    let e = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Terminal).await.unwrap_err();
    assert!(matches!(e, ErrorAccion::Store(StoreError::PropuestaYaDecidida(_))));
    assert_eq!(sis.mutaciones(), vec!["reiniciar:ea-mcp"]);
    // y rechazarla después tampoco procede
    assert!(rechazar_propuesta(&s, p.id, "usuario", Canal::Web).is_err());
}

#[tokio::test]
async fn el_almacen_impide_aceptar_sin_ejecutar_una_propuesta_con_accion() {
    let (s, _d) = store();
    let sis = sistema();
    let (p, _) = proponer_(&s, &sis, "claude-code", "ea_reiniciar_servicio", &json!({ "servicio": "ea-mcp" })).await.unwrap();
    let e = s.decidir_propuesta(p.id, EstadoPropuesta::Aceptada).unwrap_err();
    assert!(matches!(e, StoreError::PropuestaConAccion(_)));
    assert_eq!(s.get_propuesta(p.id).unwrap().estado, EstadoPropuesta::Pendiente);
}

#[tokio::test]
async fn revalida_antes_de_ejecutar_si_el_estado_cambio() {
    let (mut s, _d) = store();
    let sis = sistema();

    // Rutina que cambia de contenido entre la simulación y la ejecución.
    let (p, _) = proponer_(&s, &sis, "claude-code", "ea_eliminar_rutina", &json!({ "slug": "vieja" })).await.unwrap();
    sis.retiradas.lock().unwrap().insert("vieja".into(), "otro contenido".into());
    let p = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Terminal).await.unwrap();
    assert_eq!(p.ejecucion.unwrap().estado, EstadoEjecucion::Fallida);
    assert!(sis.rutina_retirada("vieja").unwrap().is_some());
    assert!(sis.mutaciones().is_empty());

    // Servicio desinstalado.
    let (p, _) = proponer_(&s, &sis, "claude-code", "ea_reiniciar_servicio", &json!({ "servicio": "ea-mcp" })).await.unwrap();
    sis.servicios.lock().unwrap().clear();
    let p = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Terminal).await.unwrap();
    assert_eq!(p.ejecucion.unwrap().estado, EstadoEjecucion::Fallida);
}

#[tokio::test]
async fn un_fallo_del_sistema_queda_como_ejecucion_fallida() {
    let (mut s, _d) = store();
    let sis = sistema();
    *sis.fallo_reinicio.lock().unwrap() = Some("Unit not found".into());
    let (p, _) = proponer_(&s, &sis, "claude-code", "ea_reiniciar_servicio", &json!({ "servicio": "ea-mcp" })).await.unwrap();
    let p = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Web).await.unwrap();
    let ej = p.ejecucion.unwrap();
    assert_eq!(ej.estado, EstadoEjecucion::Fallida);
    assert!(ej.detalle.unwrap().contains("Unit not found"));
}

#[tokio::test]
async fn un_irreversible_no_se_confirma_desde_la_web_y_la_propuesta_sigue_pendiente() {
    let (mut s, _d) = store();
    let sis = sistema();
    let (p, _) = proponer_(&s, &sis, "claude-code", "ea_eliminar_rutina", &json!({ "slug": "vieja" })).await.unwrap();
    assert_eq!(p.accion.as_ref().unwrap().riesgo, "irreversible");

    let e = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Web).await.unwrap_err();
    assert!(matches!(e, ErrorAccion::Prohibida(_)), "{e}");
    assert_eq!(s.get_propuesta(p.id).unwrap().estado, EstadoPropuesta::Pendiente);
    assert!(sis.rutina_retirada("vieja").unwrap().is_some());
    assert!(acciones(&s).contains(&"accion_fallida".to_string()), "el intento por la web queda en el diario");

    let e = ejecutar_directo(&mut s, &sis, "usuario", Canal::Web, "ea_eliminar_rutina", &json!({ "slug": "vieja" })).await.unwrap_err();
    assert!(matches!(e, ErrorAccion::Prohibida(_)));
    assert!(acciones(&s).contains(&"accion_fallida".to_string()), "el intento queda en el diario");
    assert!(sis.mutaciones().is_empty());

    // Desde la terminal sí.
    let p = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Terminal).await.unwrap();
    assert_eq!(p.ejecucion.unwrap().estado, EstadoEjecucion::Ejecutada);
    assert!(sis.rutina_retirada("vieja").unwrap().is_none());
    assert_eq!(sis.mutaciones(), vec!["eliminar_rutina:vieja"]);
}

#[tokio::test]
async fn las_propuestas_de_texto_libre_siguen_siendo_constancia() {
    let (mut s, _d) = store();
    let sis = sistema();
    let p = s.crear_propuesta(NuevaPropuesta { proponente: "claude-code".into(), descripcion: "Revisar a mano".into(), accion: None }).unwrap();
    assert!(p.accion.is_none());
    let p = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Web).await.unwrap();
    assert_eq!(p.estado, EstadoPropuesta::Aceptada);
    assert!(p.ejecucion.is_none());
    assert!(sis.mutaciones().is_empty());
    assert!(acciones(&s).contains(&"propuesta_decidida".to_string()));
}

#[tokio::test]
async fn encolar_rutina_con_entradas_muestra_el_criterio_y_las_valida() {
    let (mut s, _d) = store();
    let sis = sistema();
    let args = json!({ "slug": "con-entradas", "entradas": { "salida": "otro.md" } });
    validar_argumentos("ea_encolar_rutina", &args).unwrap();
    let sim = simular(&s, &sis, "ea_encolar_rutina", &args).await.unwrap();
    assert!(sim.texto.contains("`test -f otro.md`"), "{}", sim.texto);
    assert!(sim.texto.contains("salida = \"otro.md\""), "{}", sim.texto);
    assert_eq!(sim.argumentos, args);
    // Sin entradas, los argumentos normalizados no cambian y el criterio es el de por defecto.
    let sim = simular(&s, &sis, "ea_encolar_rutina", &json!({ "slug": "con-entradas" })).await.unwrap();
    assert_eq!(sim.argumentos, json!({ "slug": "con-entradas" }));
    assert!(sim.texto.contains("`test -f informe.md`"));

    let noexiste = json!({ "slug": "con-entradas", "entradas": { "noexiste": "1" } });
    assert!(matches!(simular(&s, &sis, "ea_encolar_rutina", &noexiste).await, Err(ErrorAccion::Argumentos(_))));
    let salto = json!({ "slug": "con-entradas", "entradas": { "salida": "a\nrm x" } });
    assert!(matches!(validar_argumentos("ea_encolar_rutina", &salto), Err(ErrorAccion::Argumentos(_))));
    assert!(matches!(proponer_(&s, &sis, "claude-code", "ea_encolar_rutina", &salto).await, Err(ErrorAccion::Argumentos(_))));

    let r = ejecutar_directo(&mut s, &sis, "usuario", Canal::Web, "ea_encolar_rutina", &args).await.unwrap();
    let orden = ea_bus::Bus::abrir_en(estado()).unwrap().leer_orden(r.trabajo.as_deref().unwrap()).unwrap();
    assert_eq!(orden.criterio_exito.as_deref(), Some("test -f otro.md"));
}

#[tokio::test]
async fn registrar_rutina_se_simula_con_el_diff_se_propone_y_al_aceptar_se_registra() {
    let (mut s, _d) = store();
    let sis = sistema();
    let v1 = s.ultima_rutina("con-entradas").unwrap().unwrap().definicion;
    let v2 = v1.replace("version = 1", "version = 2").replace("test -f {salida}", "test -s {salida}");
    let args = json!({ "toml": v2 });
    validar_argumentos("ea_registrar_rutina", &args).unwrap();

    // Simular: el diff y el criterio resultante, sin escribir nada.
    let eventos = acciones(&s).len();
    let sim = simular(&s, &sis, "ea_registrar_rutina", &args).await.unwrap();
    assert_eq!(sim.riesgo, Riesgo::Reversible);
    assert!(sim.texto.contains("-criterio_exito = \"test -f {salida}\""), "{}", sim.texto);
    assert!(sim.texto.contains("+criterio_exito = \"test -s {salida}\""), "{}", sim.texto);
    assert!(sim.texto.contains("`test -s informe.md`"), "{}", sim.texto);
    assert_eq!(sim.precondicion, json!({ "slug": "con-entradas", "version_registrada": 1 }));
    assert_eq!(acciones(&s).len(), eventos);
    assert_eq!(s.ultima_rutina("con-entradas").unwrap().unwrap().version, 1);

    // Proponer (lo que hace Claude Code por MCP): encola, no registra.
    let (p, nueva) = proponer_(&s, &sis, "claude-code", "ea_registrar_rutina", &args).await.unwrap();
    assert!(nueva);
    assert!(p.accion.as_ref().unwrap().simulacion.contains("test -s informe.md"));
    assert!(sis.mutaciones().is_empty());
    assert_eq!(s.ultima_rutina("con-entradas").unwrap().unwrap().version, 1);

    // Aceptar (el usuario): registra la v2 con el actor del usuario.
    let p = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Web).await.unwrap();
    assert_eq!(p.ejecucion.as_ref().unwrap().estado, EstadoEjecucion::Ejecutada, "{:?}", p.ejecucion);
    assert_eq!(sis.mutaciones(), vec!["registrar_rutina:con-entradas@2"]);
    let fila = s.ultima_rutina("con-entradas").unwrap().unwrap();
    assert_eq!((fila.version, fila.creada_por.as_str()), (2, "usuario"));
    assert_eq!(fila.definicion, v2);
    assert!(acciones(&s).contains(&"accion_ejecutada".to_string()));

    // Ya registrada: la misma versión no aplica, y un TOML roto no es ni argumento válido.
    assert!(matches!(simular(&s, &sis, "ea_registrar_rutina", &args).await, Err(ErrorAccion::NoAplica(_))));
    let roto = json!({ "toml": "[rutina]\nslug = \"x\"\n" });
    assert!(matches!(validar_argumentos("ea_registrar_rutina", &roto), Err(ErrorAccion::Argumentos(_))));
    assert!(matches!(proponer_(&s, &sis, "claude-code", "ea_registrar_rutina", &roto).await, Err(ErrorAccion::Argumentos(_))));
}

#[tokio::test]
async fn registrar_rutina_revalida_la_version_al_aceptar() {
    let (mut s, _d) = store();
    let sis = sistema();
    let v1 = s.ultima_rutina("nocturna").unwrap().unwrap().definicion;
    let args = json!({ "toml": v1.replace("version = 1", "version = 2") });
    let (p, _) = proponer_(&s, &sis, "claude-code", "ea_registrar_rutina", &args).await.unwrap();
    // Mientras tanto, alguien registra la v3 por otro camino.
    sis.rutina_registrar(&s, &v1.replace("version = 1", "version = 3"), "ea-cli").unwrap();
    let p = aceptar_propuesta(&mut s, &sis, p.id, "usuario", Canal::Web).await.unwrap();
    let ej = p.ejecucion.unwrap();
    assert_eq!(ej.estado, EstadoEjecucion::Fallida);
    assert!(ej.detalle.unwrap().contains("el estado cambió"));
    assert_eq!(s.ultima_rutina("nocturna").unwrap().unwrap().version, 3);
}

#[tokio::test]
async fn ejecutar_directo_registra_en_el_diario_como_la_via_de_propuestas() {
    let (mut s, _d) = store();
    let sis = sistema();
    ejecutar_directo(&mut s, &sis, "usuario", Canal::Web, "ea_reiniciar_servicio", &json!({ "servicio": "ea-mcp" })).await.unwrap();
    assert_eq!(sis.mutaciones(), vec!["reiniciar:ea-mcp"]);
    assert!(acciones(&s).contains(&"accion_ejecutada".to_string()));

    let r = ejecutar_directo(&mut s, &sis, "usuario", Canal::Web, "ea_encolar_rutina", &json!({ "slug": "nocturna" })).await.unwrap();
    assert_eq!(r.trabajo.as_deref(), r.datos["trabajo"].as_str());
    let trabajo = r.datos["trabajo"].as_str().unwrap().to_string();
    let bus = ea_bus::Bus::abrir_en(estado()).unwrap();
    assert_eq!(bus.leer_orden(&trabajo).unwrap().rutina.unwrap().slug, "nocturna");
    assert_eq!(bus.listar_cola(ea_bus::Rol::Ejecutor).unwrap().iter().filter(|e| e.trabajo == trabajo).count(), 1);
    // No se lanza dos veces la misma rutina mientras la anterior no termina.
    assert!(ejecutar_directo(&mut s, &sis, "usuario", Canal::Web, "ea_encolar_rutina", &json!({ "slug": "nocturna" })).await.is_err());
}

#[test]
fn invariantes_del_catalogo() {
    // Solo ea_escribir_nota (la región propia de cada agente) es mutante de
    // ejecución directa (excepción razonada).
    let directas_mutantes: Vec<_> = CATALOGO.iter().filter(|h| h.riesgo.es_mutante() && h.ejecucion == Ejecucion::Directa).map(|h| h.nombre).collect();
    assert_eq!(directas_mutantes, ["ea_escribir_nota"]);
    // Los irreversibles son siempre supervisados y nunca van al modelo local.
    for h in CATALOGO.iter().filter(|h| h.riesgo == Riesgo::Irreversible) {
        assert_eq!(h.ejecucion, Ejecucion::Supervisada);
        assert!(!h.local, "{}", h.nombre);
    }
    // Todas las lecturas están en la proyección local.
    assert!(CATALOGO.iter().filter(|h| h.riesgo == Riesgo::Lectura).all(|h| h.local));
    let mut nombres: Vec<_> = CATALOGO.iter().map(|h| h.nombre).collect();
    nombres.sort();
    nombres.dedup();
    assert_eq!(nombres.len(), CATALOGO.len());
}

/// Las supervisadas del catálogo (la tabla de riesgo) son exactamente los
/// casos de `Supervisada`, en el mismo orden, y cada nombre vuelve a su caso.
#[test]
fn las_supervisadas_del_catalogo_son_las_del_enum() {
    let catalogo: Vec<_> = CATALOGO.iter().filter(|h| h.ejecucion == Ejecucion::Supervisada).map(|h| h.nombre).collect();
    let todas: Vec<_> = Supervisada::TODAS.iter().map(|s| s.nombre()).collect();
    assert_eq!(catalogo, todas);
    for s in Supervisada::TODAS {
        assert_eq!(Supervisada::desde(s.nombre()), Some(s));
        assert_eq!(s.ficha().nombre, s.nombre());
    }
    assert_eq!(Supervisada::desde("ea_status"), None);
}

#[test]
fn autorizacion_por_actor() {
    let h = |n| buscar(n).unwrap();
    // Lo desatendido es de solo lectura, sin excepciones.
    for herr in CATALOGO {
        assert_eq!(autorizar_mcp("orquestador", herr).is_ok(), herr.riesgo == Riesgo::Lectura, "{}", herr.nombre);
    }
    // El modelo local: lecturas y como mucho proponer promover notas.
    assert!(autorizar_mcp("modelo-local", h("ea_status")).is_ok());
    assert!(autorizar_mcp("modelo-local", h("ea_ver_trabajo")).is_ok());
    assert!(autorizar_mcp("modelo-local", h("ea_promover_nota")).is_ok());
    for n in ["ea_eliminar_rutina", "ea_reiniciar_servicio", "ea_encolar_rutina", "ea_registrar_rutina"] {
        assert!(autorizar_mcp("modelo-local", h(n)).is_err(), "{n}");
    }
    assert!(autorizar_mcp("claude-code", h("ea_eliminar_rutina")).is_ok());
    // Un cliente desconocido o un agente sin rol solo lee (spec 017, RF-013).
    for actor in ["desconocido", "sin-rol", "codex", ""] {
        assert!(autorizar_mcp(actor, h("ea_status")).is_ok(), "{actor}");
        for n in ["ea_escribir_nota", "ea_promover_nota", "ea_encolar_rutina", "ea_eliminar_rutina"] {
            assert!(autorizar_mcp(actor, h(n)).is_err(), "{actor} {n}");
        }
    }
}

#[test]
fn el_canal_web_nunca_confirma_irreversibles() {
    assert!(puede_confirmar(Canal::Web, Riesgo::Reversible).is_ok());
    assert!(puede_confirmar(Canal::Web, Riesgo::Irreversible).is_err());
    assert!(puede_confirmar(Canal::Terminal, Riesgo::Irreversible).is_ok());
}

#[test]
fn los_campos_declarados_casan_con_los_argumentos_que_se_validan() {
    for h in CATALOGO.iter().filter(|h| h.ejecucion == Ejecucion::Supervisada) {
        let campos = campos(h.nombre);
        assert!(!campos.is_empty(), "{} no declara campos", h.nombre);

        let valor = |c: &Campo| match (c.entero, c.opciones.first(), c.nombre) {
            (true, _, _) => json!(1),
            (_, Some(o), _) => json!(o),
            (_, _, "elemento") => json!("codex/mcp/usuario/docs"),
            (_, _, "ruta") => json!("claude-code/tema/nota.md"),
            (_, _, "destino") => json!("tema/nota.md"),
            (_, _, "toml") => {
                json!(concat!("[rutina]\nslug = \"x\"\nversion = 1\nworktree = \"", tmp!(), "\"\ncriterio_exito = \"true\"\n[[pasos]]\ntexto = \"x\"\n"))
            }
            _ => json!("revision-diaria"),
        };
        // Solo los obligatorios, y luego todos: ambas formas deben validar.
        for solo_obligatorios in [true, false] {
            let args: serde_json::Map<String, Value> =
                campos.iter().filter(|c| c.obligatorio || !solo_obligatorios).map(|c| (c.nombre.to_string(), valor(c))).collect();
            validar_argumentos(h.nombre, &Value::Object(args)).unwrap_or_else(|e| panic!("{}: {e}", h.nombre));
        }
    }
}
