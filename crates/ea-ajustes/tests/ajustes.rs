//! Pruebas de `ea-ajustes`: siempre sobre `Fuentes::aislada` (directorios
//! temporales y entorno fijo); nunca el entorno ni los ficheros reales.

use std::path::{Path, PathBuf};

use ea_ajustes::*;
use serde_json::{json, Value};

fn fuentes() -> (tempfile::TempDir, Fuentes) {
    let d = tempfile::tempdir().unwrap();
    let f = Fuentes::aislada(d.path());
    (d, f)
}

fn escribe(f: &Fuentes, nombre: &str, texto: &str) {
    std::fs::create_dir_all(&f.dir_config).unwrap();
    std::fs::write(f.dir_config.join(nombre), texto).unwrap();
}

fn cambio(clave: &str, v: Value) -> Vec<(String, Option<Value>)> {
    vec![(clave.to_string(), Some(v))]
}

fn version(f: &Fuentes) -> String {
    cargar_de(f).version
}

#[test]
fn sin_ficheros_todo_es_por_defecto_y_cada_defecto_es_valido() {
    let (_d, f) = fuentes();
    let e = cargar_de(&f);
    assert!(!e.existe);
    assert_eq!(e.version, "");
    assert!(e.avisos.is_empty(), "{:?}", e.avisos);
    assert_eq!(e.ajustes, Ajustes::por_defecto(&f));
    for c in CLAVES {
        assert_eq!(e.origenes[c.nombre], Origen::Defecto, "{}", c.nombre);
        // el defecto de cada clave pasa su propia validación
        let ok = match &e.defectos[c.nombre] {
            Value::Array(l) => interpretar(c, &l.iter().map(|v| v.as_str().unwrap()).collect::<Vec<_>>().join(",")),
            Value::String(s) => interpretar(c, s),
            otro => interpretar(c, &otro.to_string()),
        };
        assert_eq!(ok.unwrap(), e.defectos[c.nombre], "{}", c.nombre);
    }
    assert_eq!(e.ajustes.modelo.nombre, "qwen3-coder-30b");
    assert_eq!(e.ajustes.agente.techo_topes, TECHO_DURO);
    assert_eq!(e.ajustes.rutas.db, f.dir_datos.join("ecosistema.db"));
    assert_eq!(e.ajustes.rutas.boveda, f.dir_datos.join("boveda"));
    assert_eq!(e.ajustes.rutas.estado, f.dir_estado);
    assert_eq!(e.ajustes.orquestador.rutinas_dir, f.dir_config.join("rutinas"));
}

#[test]
fn precedencia_defecto_ajustes_entorno() {
    let (d, f) = fuentes();
    assert_eq!(cargar_de(&f).ajustes.red.puerto_web, 8092);
    escribe(&f, "ajustes.toml", "[red]\npuerto_web = 9100\n");
    let e = cargar_de(&f);
    assert_eq!((e.ajustes.red.puerto_web, &e.origenes["red.puerto_web"]), (9100, &Origen::Ajustes));
    let f = Fuentes::aislada(d.path()).con_var("EA_WEB_PORT", "9200");
    let e = cargar_de(&f);
    assert_eq!(e.ajustes.red.puerto_web, 9200);
    assert_eq!(e.origenes["red.puerto_web"], Origen::Entorno { variable: "EA_WEB_PORT" });
    assert_eq!(e.origenes["red.puerto_web"].legible(), "entorno: EA_WEB_PORT");
}

#[test]
fn los_toml_heredados_siguen_valiendo_y_ajustes_les_gana() {
    let (_d, f) = fuentes();
    escribe(
        &f,
        "orquestador.toml",
        "modelo = \"otro\"\nbase_url = \"http://localhost:9000/v1\"\n[topes]\nturnos = 5\n[skills]\nmax_por_trabajo = 1\nhabilitadas = [\"rust\"]\n",
    );
    escribe(&f, "orchestrator.toml", "intervalo_sondeo_s = 7\nintervalo_heartbeat_s = 40\nrutinas_dir = \"/r\"\n");
    escribe(&f, "boveda.toml", "raiz = \"~/x\"\n");
    let e = cargar_de(&f);
    let a = &e.ajustes;
    assert_eq!(a.modelo.nombre, "otro");
    assert_eq!(a.modelo.base_url, "http://localhost:9000/v1");
    assert_eq!(a.agente.techo_topes.turnos, 5);
    assert_eq!(a.agente.techo_topes.tiempo_s, 3600);
    assert_eq!(a.agente.skills.max_por_trabajo, 1);
    assert_eq!(a.agente.skills.habilitadas, ["rust"]);
    assert_eq!((a.orquestador.intervalo_sondeo_s, a.orquestador.intervalo_heartbeat_s), (7, 40));
    assert_eq!(a.orquestador.rutinas_dir, PathBuf::from("/r"));
    assert_eq!(a.rutas.boveda, expandir_tilde("~/x"));
    assert!(a.rutas.boveda.is_absolute());
    assert_eq!(e.origenes["modelo.nombre"], Origen::Heredado { fichero: "orquestador.toml" });
    assert_eq!(e.origenes["rutas.boveda"].legible(), "heredado: boveda.toml");

    escribe(&f, "ajustes.toml", "[modelo]\nnombre = \"nuevo\"\n");
    let e = cargar_de(&f);
    assert_eq!(e.ajustes.modelo.nombre, "nuevo");
    assert_eq!(e.ajustes.agente.techo_topes.turnos, 5);
}

#[test]
fn base_url_solo_loopback() {
    for mala in [
        "http://192.168.1.2:8080/v1",
        "http://127.0.0.1.evil.com/",
        "http://a@127.0.0.1/",
        "ftp://127.0.0.1",
        "http://0.0.0.0:8080",
        "http://127.0.0.1:99999/",
        "http://[::2]:8080",
    ] {
        assert!(validar_base_url(mala).is_err(), "{mala}");
    }
    for buena in ["http://127.0.0.1:8080/v1", "https://localhost/v1", "http://[::1]:8080", "http://LOCALHOST:1"] {
        assert!(validar_base_url(buena).is_ok(), "{buena}");
    }
    let (_d, f) = fuentes();
    escribe(&f, "ajustes.toml", "[modelo]\nbase_url = \"http://10.0.0.1:8080/v1\"\n");
    let e = cargar_de(&f);
    assert_eq!(e.ajustes.modelo.base_url, "http://127.0.0.1:8080/v1");
    assert!(e.avisos.iter().any(|a| a.contains("modelo.base_url")), "{:?}", e.avisos);
}

#[test]
fn fuera_de_rango_y_cruces_se_ignoran_con_aviso() {
    let (_d, f) = fuentes();
    escribe(
        &f,
        "ajustes.toml",
        "[agente.techo_topes]\nturnos = 1000\n[agente.skills]\npueden_elegir = [\"modelo-local\"]\n[red]\npuerto_web = 8765\n[otra]\ncosa = 1\n",
    );
    let e = cargar_de(&f);
    assert_eq!(e.ajustes.agente.techo_topes.turnos, 100);
    assert_eq!(e.ajustes.agente.skills.pueden_elegir, ["claude-code"]);
    assert_ne!(e.ajustes.red.puerto_web, e.ajustes.red.puerto_mcp);
    for trozo in ["turnos", "pueden_elegir", "coinciden", "otra.cosa"] {
        assert!(e.avisos.iter().any(|a| a.contains(trozo)), "{trozo}: {:?}", e.avisos);
    }
}

#[test]
fn un_ajustes_toml_roto_se_ignora_sin_panico() {
    let (_d, f) = fuentes();
    escribe(&f, "ajustes.toml", "esto [no es toml =\n");
    escribe(&f, "orquestador.toml", "modelo = \"heredado\"\n");
    let e = cargar_de(&f);
    assert!(e.existe);
    assert_eq!(e.ajustes.modelo.nombre, "heredado");
    assert!(e.avisos.iter().any(|a| a.contains("ajustes.toml")));
    // y no se sobrescribe un fichero roto
    let r = establecer(&f, Canal::Terminal, None, &cambio("web.sondeo_ms", json!(5000)));
    assert!(matches!(r, Err(ErrorAjustes::FicheroInvalido(_))), "{r:?}");
}

#[test]
fn desde_la_web_no_se_tocan_las_claves_de_terminal() {
    let (_d, f) = fuentes();
    for (k, v) in [("agente.sandbox", json!(false)), ("modelo.qwen_bin", json!("/tmp/x")), ("rutas.db", json!("/tmp/x.db"))] {
        let r = establecer(&f, Canal::Web, Some(""), &cambio(k, v));
        assert!(matches!(r, Err(ErrorAjustes::NoEditable { .. })), "{k}: {r:?}");
    }
    assert!(!f.fichero().exists());
    // desde la terminal sí
    establecer(&f, Canal::Terminal, None, &cambio("agente.sandbox", json!(false))).unwrap();
    assert!(!cargar_de(&f).ajustes.agente.sandbox);
}

#[test]
fn lo_endurecible_solo_baja_desde_la_web() {
    let (_d, f) = fuentes();
    let k = "agente.techo_topes.turnos";
    let a = establecer(&f, Canal::Web, Some(""), &cambio(k, json!(40))).unwrap();
    assert_eq!((a[0].antes.clone(), a[0].despues.clone()), (json!(100), json!(40)));
    let r = establecer(&f, Canal::Web, Some(&version(&f)), &cambio(k, json!(50)));
    assert!(matches!(r, Err(ErrorAjustes::SoloEndurecer { .. })), "{r:?}");
    // quitar la clave también afloja (el defecto es 100)
    let r = establecer(&f, Canal::Web, Some(&version(&f)), &[(k.to_string(), None)]);
    assert!(matches!(r, Err(ErrorAjustes::SoloEndurecer { .. })), "{r:?}");
    // la terminal sube hasta el techo duro, no más
    establecer(&f, Canal::Terminal, None, &cambio(k, json!(100))).unwrap();
    let r = establecer(&f, Canal::Terminal, None, &cambio(k, json!(101)));
    assert!(matches!(r, Err(ErrorAjustes::ValorInvalido { .. })), "{r:?}");
    assert_eq!(cargar_de(&f).ajustes.agente.techo_topes.turnos, 100);
}

#[test]
fn version_entorno_y_lotes_todo_o_nada() {
    let (d, f) = fuentes();
    establecer(&f, Canal::Web, Some(""), &cambio("web.sondeo_ms", json!(5000))).unwrap();
    let r = establecer(&f, Canal::Web, Some(""), &cambio("web.sondeo_ms", json!(6000)));
    assert!(matches!(r, Err(ErrorAjustes::Version)), "{r:?}");
    let fe = Fuentes::aislada(d.path()).con_var("EA_WEB_PORT", "9000");
    let r = establecer(&fe, Canal::Terminal, None, &cambio("red.puerto_web", json!(9100)));
    assert!(matches!(r, Err(ErrorAjustes::FijadaPorEntorno { .. })), "{r:?}");
    let r = establecer(&f, Canal::Terminal, None, &cambio("no.existe", json!(1)));
    assert!(matches!(r, Err(ErrorAjustes::ClaveDesconocida(_))), "{r:?}");
    let antes = std::fs::read(f.fichero()).unwrap();
    let lote = vec![
        ("web.diario_limite".to_string(), Some(json!(50))),
        ("web.sondeo_ms".to_string(), Some(json!(1))),
    ];
    assert!(establecer(&f, Canal::Terminal, None, &lote).is_err());
    assert_eq!(std::fs::read(f.fichero()).unwrap(), antes);
    let r = establecer(&f, Canal::Terminal, None, &cambio("red.puerto_mcp", json!(8092)));
    assert!(matches!(r, Err(ErrorAjustes::ValorInvalido { .. })), "{r:?}");
    assert!(matches!(establecer(&f, Canal::Terminal, None, &[]), Err(ErrorAjustes::SinCambios)));
}

fn temporales(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect()
}

#[test]
fn escribir_conserva_comentarios_y_claves_ajenas() {
    let (_d, f) = fuentes();
    escribe(&f, "ajustes.toml", "# mis ajustes\n[web]\n# refresco\nsondeo_ms = 3000 # lento\n\n[modelo]\nnombre = \"m\"\n");
    establecer(&f, Canal::Terminal, None, &cambio("web.sondeo_ms", json!(4000))).unwrap();
    establecer(&f, Canal::Terminal, None, &cambio("agente.techo_topes.iteraciones", json!(3))).unwrap();
    let t = std::fs::read_to_string(f.fichero()).unwrap();
    for trozo in ["# mis ajustes", "# refresco", "sondeo_ms = 4000", "# lento", "nombre = \"m\"", "[agente.techo_topes]", "iteraciones = 3"] {
        assert!(t.contains(trozo), "{trozo}: {t}");
    }
    assert!(!t.contains("[agente]\n"), "{t}");
    assert!(temporales(&f.dir_config).is_empty());
    establecer(&f, Canal::Terminal, None, &[("web.sondeo_ms".into(), None)]).unwrap();
    let e = cargar_de(&f);
    assert_eq!((e.ajustes.web.sondeo_ms, &e.origenes["web.sondeo_ms"]), (3000, &Origen::Defecto));
    assert!(std::fs::read_to_string(f.fichero()).unwrap().contains("# mis ajustes"));
}

#[test]
fn utilidades() {
    let casa = directories::BaseDirs::new().unwrap().home_dir().to_path_buf();
    assert_eq!(expandir_tilde("~"), casa);
    assert_eq!(expandir_tilde("~/x/y"), casa.join("x/y"));
    assert_eq!(expandir_tilde("/abs"), PathBuf::from("/abs"));
    assert_eq!(expandir_tilde("rel/a"), PathBuf::from("rel/a"));
    let (_d, f) = fuentes();
    assert_eq!(Ajustes::por_defecto(&f).umbral_latido_s(), 60);
    let c = |n| clave(n).unwrap();
    assert_eq!(interpretar(c("agente.sandbox"), "no").unwrap(), json!(false));
    assert_eq!(interpretar(c("agente.sandbox"), "true").unwrap(), json!(true));
    assert!(interpretar(c("agente.sandbox"), "quizá").is_err());
    assert_eq!(interpretar(c("web.sondeo_ms"), " 4000 ").unwrap(), json!(4000));
    assert!(interpretar(c("web.sondeo_ms"), "4k").is_err());
    assert_eq!(interpretar(c("agente.skills.habilitadas"), "rust, tests,,rust").unwrap(), json!(["rust", "tests"]));
    assert_eq!(interpretar(c("agente.skills.pueden_elegir"), "").unwrap(), json!([]));
    assert_eq!(huella(b""), "");
    assert_eq!(huella(b"a").len(), 16);
}

#[test]
fn los_defectos_del_sistema_son_las_rutas_de_siempre() {
    let f = Fuentes::sistema();
    let a = Ajustes::por_defecto(&f);
    let d = directories::ProjectDirs::from("", "", "ecosistema-asistente").unwrap();
    if std::env::var(ENV_CONFIG).map_or(true, |v| v.trim().is_empty()) {
        assert_eq!(a.orquestador.rutinas_dir, d.config_dir().join("rutinas"));
        assert_eq!(PathBuf::from(&a.agente.skills.dir), d.config_dir().join("qwen-skills"));
    }
    assert_eq!(a.rutas.db, d.data_dir().join("ecosistema.db"));
    assert_eq!(a.rutas.boveda, d.data_dir().join("boveda"));
    assert_eq!(a.rutas.estado, d.state_dir().unwrap_or_else(|| d.data_local_dir()));
}

#[test]
fn el_resumen_lleva_todas_las_claves_por_seccion() {
    let (_d, f) = fuentes();
    let r = cargar_de(&f).resumen(&f);
    let total: usize = r.secciones.iter().map(|s| s.claves.len()).sum();
    assert_eq!(total, CLAVES.len());
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["cliente"]["sondeo_ms"], 3000);
    assert_eq!(v["heredados"].as_array().unwrap().len(), 3);
    let agente = v["secciones"].as_array().unwrap().iter().find(|s| s["id"] == "agente").unwrap();
    let turnos = agente["claves"].as_array().unwrap().iter().find(|c| c["clave"] == "agente.techo_topes.turnos").unwrap();
    assert_eq!(turnos["origen"]["tipo"], "defecto");
    assert_eq!((turnos["min"].clone(), turnos["max"].clone()), (json!(1), json!(100)));
    assert_eq!(turnos["endurecible"], true);
}
