//! Pruebas de `ea-ajustes`: siempre sobre `Fuentes::aislada` (directorios
//! temporales y entorno fijo); nunca el entorno ni los ficheros reales.

use std::path::{Path, PathBuf};

use ea_ajustes::*;
use serde_json::{Value, json};

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
    for (n, c) in &e.claves {
        assert_eq!(e.origenes[n], Origen::Defecto, "{n}");
        // el defecto de cada clave pasa su propia validación
        let ok = match &e.defectos[n] {
            Value::Array(l) => interpretar(c, &l.iter().map(|v| v.as_str().unwrap()).collect::<Vec<_>>().join(",")),
            Value::String(s) => interpretar(c, s),
            otro => interpretar(c, &otro.to_string()),
        };
        assert_eq!(ok.unwrap(), e.defectos[n], "{n}");
    }
    assert_eq!(e.ajustes.modelo_ejecutor().nombre, "qwen3-coder-30b");
    assert_eq!((e.ajustes.roles.planificador.as_str(), e.ajustes.roles.ejecutor.as_str()), ("claude-code", "qwen-code"));
    assert_eq!(e.ajustes.agentes.len(), CATALOGO.len());
    assert_eq!(validar_roles(&e.ajustes), Ok(()));
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

/// Las rutas que leen el bus, el diario y la bóveda salen solo de aquí: la
/// variable de entorno gana a `ajustes.toml` y al heredado, sin espacios en los
/// extremos y con `~` expandido, y `ea ajustes ver` dice que viene del entorno.
#[test]
fn las_rutas_del_entorno_ganan_a_los_ficheros() {
    let (d, f) = fuentes();
    escribe(&f, "ajustes.toml", "[rutas]\ndb = \"/a/ea.db\"\nestado = \"/a/estado\"\n");
    escribe(&f, "boveda.toml", "raiz = \"/a/boveda\"\n");
    let e = cargar_de(&f);
    assert_eq!((e.ajustes.rutas.db, e.ajustes.rutas.estado), (PathBuf::from("/a/ea.db"), PathBuf::from("/a/estado")));
    assert_eq!((e.ajustes.rutas.boveda, &e.origenes["rutas.boveda"]), (PathBuf::from("/a/boveda"), &Origen::Heredado { fichero: "boveda.toml" }));

    let f = Fuentes::aislada(d.path()).con_var("EA_DB_PATH", " /e/ea.db ").con_var("EA_ESTADO_DIR", "/e/estado").con_var("EA_BOVEDA_DIR", "~/boveda");
    let e = cargar_de(&f);
    assert_eq!(e.ajustes.rutas.db, PathBuf::from("/e/ea.db"));
    assert_eq!(e.ajustes.rutas.estado, PathBuf::from("/e/estado"));
    if let Some(casa) = ea_plataforma::usuario::home() {
        assert_eq!(e.ajustes.rutas.boveda, casa.join("boveda"));
    }
    for (clave, variable) in [("rutas.db", "EA_DB_PATH"), ("rutas.estado", "EA_ESTADO_DIR"), ("rutas.boveda", "EA_BOVEDA_DIR")] {
        assert_eq!(e.origenes[clave].legible(), format!("entorno: {variable}"));
    }
    // Una variable vacía no cuenta.
    let e = cargar_de(&Fuentes::aislada(d.path()).con_var("EA_ESTADO_DIR", "  "));
    assert_eq!((e.ajustes.rutas.estado, &e.origenes["rutas.estado"]), (PathBuf::from("/a/estado"), &Origen::Ajustes));
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
    assert_eq!(a.modelos["local"].nombre, "otro");
    assert_eq!(a.modelos["local"].base_url, "http://localhost:9000/v1");
    assert_eq!(a.agente.techo_topes.turnos, 5);
    assert_eq!(a.agente.techo_topes.tiempo_s, 3600);
    assert_eq!(a.agente.skills.max_por_trabajo, 1);
    assert_eq!(a.agente.skills.habilitadas, ["rust"]);
    assert_eq!((a.orquestador.intervalo_sondeo_s, a.orquestador.intervalo_heartbeat_s), (7, 40));
    assert_eq!(a.orquestador.rutinas_dir, PathBuf::from("/r"));
    assert_eq!(a.rutas.boveda, expandir_tilde("~/x"));
    assert!(a.rutas.boveda.is_absolute());
    assert_eq!(e.origenes["modelos.local.nombre"], Origen::Heredado { fichero: "orquestador.toml" });
    assert_eq!(e.origenes["rutas.boveda"].legible(), "heredado: boveda.toml");

    escribe(&f, "ajustes.toml", "[modelo]\nnombre = \"nuevo\"\n");
    let e = cargar_de(&f);
    assert_eq!(e.ajustes.modelos["local"].nombre, "nuevo");
    assert_eq!(e.origenes["modelos.local.nombre"].legible(), "heredado: ajustes.toml [modelo]");
    assert!(e.avisos.is_empty(), "{:?}", e.avisos);
    escribe(&f, "ajustes.toml", "[modelo]\nnombre = \"nuevo\"\n[modelos.local]\nnombre = \"mas-nuevo\"\n");
    assert_eq!(cargar_de(&f).ajustes.modelos["local"].nombre, "mas-nuevo");
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
    assert_eq!(e.ajustes.modelos["local"].base_url, "http://127.0.0.1:8080/v1");
    assert!(e.avisos.iter().any(|a| a.contains("modelos.local")), "{:?}", e.avisos);
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
    assert_eq!(e.ajustes.modelos["local"].nombre, "heredado");
    assert!(e.avisos.iter().any(|a| a.contains("ajustes.toml")));
    // y no se sobrescribe un fichero roto
    let r = establecer(&f, Canal::Terminal, None, &cambio("web.sondeo_ms", json!(5000)));
    assert!(matches!(r, Err(ErrorAjustes::FicheroInvalido(_))), "{r:?}");
}

#[test]
fn desde_la_web_no_se_tocan_las_claves_de_terminal() {
    let (_d, f) = fuentes();
    for (k, v) in
        [("agente.sandbox", json!(false)), ("modelo.qwen_bin", json!("/tmp/x")), ("agentes.codex.bin", json!("/tmp/x")), ("rutas.db", json!("/tmp/x.db"))]
    {
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
    let lote = vec![("web.diario_limite".to_string(), Some(json!(50))), ("web.sondeo_ms".to_string(), Some(json!(1)))];
    assert!(establecer(&f, Canal::Terminal, None, &lote).is_err());
    assert_eq!(std::fs::read(f.fichero()).unwrap(), antes);
    let r = establecer(&f, Canal::Terminal, None, &cambio("red.puerto_mcp", json!(8092)));
    assert!(matches!(r, Err(ErrorAjustes::ValorInvalido { .. })), "{r:?}");
    assert!(matches!(establecer(&f, Canal::Terminal, None, &[]), Err(ErrorAjustes::SinCambios)));
}

fn temporales(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".tmp")).collect()
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
    let casa = ea_plataforma::usuario::home().unwrap();
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
    let e = cargar_de(&f);
    assert_eq!(total, e.claves.len());
    // 2 roles, 2 claves por agente del catálogo y 3 del perfil local
    let por = |id: &str| r.secciones.iter().find(|s| s.id == id).unwrap().claves.len();
    assert_eq!((por("roles"), por("agentes"), por("modelos")), (2, 2 * CATALOGO.len(), 3));
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["cliente"]["sondeo_ms"], 3000);
    assert_eq!(v["heredados"].as_array().unwrap().len(), 3);
    let agente = v["secciones"].as_array().unwrap().iter().find(|s| s["id"] == "agente").unwrap();
    let turnos = agente["claves"].as_array().unwrap().iter().find(|c| c["clave"] == "agente.techo_topes.turnos").unwrap();
    assert_eq!(turnos["origen"]["tipo"], "defecto");
    assert_eq!((turnos["min"].clone(), turnos["max"].clone()), (json!(1), json!(100)));
    assert_eq!(turnos["endurecible"], true);
    let agentes = v["secciones"].as_array().unwrap().iter().find(|s| s["id"] == "agentes").unwrap();
    let bin = agentes["claves"].as_array().unwrap().iter().find(|c| c["clave"] == "agentes.codex.bin").unwrap();
    assert_eq!((bin["titulo"].clone(), bin["edicion"].clone()), (json!("Codex · binario"), json!("terminal")));
    let roles = v["secciones"].as_array().unwrap().iter().find(|s| s["id"] == "roles").unwrap();
    assert_eq!(roles["claves"][1]["permitidos"], json!(IDS_AGENTES));
}

// ------------------------------------------------- 017: roles, agentes y modelos

/// Los ids del catálogo y el binario y el perfil por defecto de cada agente,
/// tal como están en disco y en la API.
#[test]
fn los_agentes_conservan_sus_nombres_y_sus_valores_por_defecto() {
    assert_eq!(IDS_AGENTES, ["claude-code", "qwen-code", "codex", "opencode", "antigravity"]);
    let (_d, f) = fuentes();
    let a = Ajustes::por_defecto(&f);
    let defecto: Vec<_> = a.agentes.iter().map(|(id, c)| (id.as_str(), c.bin.as_str(), c.modelo.as_str())).collect();
    assert_eq!(
        defecto,
        [
            ("antigravity", "agy", ""),
            ("claude-code", "claude", ""),
            ("codex", "codex", "local"),
            ("opencode", "opencode", "local"),
            ("qwen-code", "qwen", "local"),
        ]
    );
}

#[test]
fn el_catalogo_y_los_roles() {
    assert_eq!(CATALOGO.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), IDS_AGENTES);
    let admite = |id, r| ficha(id).unwrap().admite(r);
    for id in ["qwen-code", "codex", "opencode"] {
        assert!(admite(id, Rol::Ejecutor) && admite(id, Rol::Planificador), "{id}");
    }
    for id in ["claude-code", "antigravity"] {
        assert!(!admite(id, Rol::Ejecutor) && admite(id, Rol::Planificador), "{id}");
    }
    assert!(ficha("gpt").is_none());
    // en disco, los nombres de siempre; se aceptan los dos
    assert_eq!(serde_json::to_value(Rol::Planificador).unwrap(), json!("claude-code"));
    assert_eq!(serde_json::to_value(Rol::Ejecutor).unwrap(), json!("modelo-local"));
    assert_eq!(serde_json::from_value::<Rol>(json!("ejecutor")).unwrap(), Rol::Ejecutor);
    assert_eq!(serde_json::from_value::<Rol>(json!("claude-code")).unwrap(), Rol::Planificador);
    assert_eq!((Rol::desde("planificador"), Rol::desde("modelo-local"), Rol::desde("otro")), (Some(Rol::Planificador), Some(Rol::Ejecutor), None));
    // claves con patrón
    assert!(clave("modelos.gpt.nombre").is_some());
    assert!(clave("modelos.GPT.nombre").is_none());
    assert!(clave("modelos.gpt.otro").is_none());
    assert!(clave("agentes.codex.modelo").is_some());
    assert!(clave("agentes.nadie.bin").is_none());
    assert!(clave("modelos.*.nombre").is_none());
    assert_eq!(sin_alias("modelo.qwen_bin"), "agentes.qwen-code.bin");
}

#[test]
fn cambiar_de_ejecutor_desde_la_web_aplica_en_caliente() {
    let (_d, f) = fuentes();
    let a = establecer(&f, Canal::Web, Some(""), &cambio("roles.ejecutor", json!("codex"))).unwrap();
    assert_eq!((a[0].clave.as_str(), a[0].antes.clone(), a[0].despues.clone()), ("roles.ejecutor", json!("qwen-code"), json!("codex")));
    assert_eq!(a[0].aplica, Aplica::EnCaliente);
    let e = cargar_de(&f);
    assert_eq!(e.ajustes.agente_de(Rol::Ejecutor).0, "codex");
    assert_eq!(e.ajustes.modelo_ejecutor().base_url, "http://127.0.0.1:8080/v1");
    // y el planificador también, a un agente distinto
    establecer(&f, Canal::Web, Some(&version(&f)), &cambio("roles.planificador", json!("opencode"))).unwrap();
    assert_eq!(cargar_de(&f).ajustes.roles.planificador, "opencode");
    let r = establecer(&f, Canal::Web, Some(&version(&f)), &cambio("roles.ejecutor", json!("nadie")));
    assert!(matches!(r, Err(ErrorAjustes::ValorInvalido { .. })), "{r:?}");
}

#[test]
fn ningun_agente_ocupa_los_dos_roles() {
    let (_d, f) = fuentes();
    establecer(&f, Canal::Terminal, None, &cambio("roles.ejecutor", json!("codex"))).unwrap();
    let r = establecer(&f, Canal::Terminal, None, &cambio("roles.planificador", json!("codex")));
    match r {
        Err(ErrorAjustes::ValorInvalido { clave, motivo }) => {
            assert_eq!(clave, "roles.planificador");
            assert!(motivo.contains("a la vez"), "{motivo}");
        }
        otro => panic!("{otro:?}"),
    }
    // escrito a mano: al cargar, aviso y el planificador vuelve al de por defecto
    escribe(&f, "ajustes.toml", "[roles]\nplanificador = \"codex\"\nejecutor = \"codex\"\n");
    let e = cargar_de(&f);
    assert_eq!((e.ajustes.roles.planificador.as_str(), e.ajustes.roles.ejecutor.as_str()), ("claude-code", "codex"));
    assert!(e.avisos.iter().any(|a| a.contains("roles.planificador")), "{:?}", e.avisos);
}

#[test]
fn cada_agente_solo_ocupa_los_roles_que_admite() {
    let (_d, f) = fuentes();
    for id in ["antigravity", "claude-code"] {
        let r = establecer(&f, Canal::Terminal, None, &cambio("roles.ejecutor", json!(id)));
        assert!(matches!(r, Err(ErrorAjustes::ValorInvalido { .. })), "{id}: {r:?}");
    }
    assert!(!f.fichero().exists());
    escribe(&f, "ajustes.toml", "[roles]\nejecutor = \"antigravity\"\n");
    let e = cargar_de(&f);
    assert_eq!(e.ajustes.roles.ejecutor, "qwen-code");
    assert!(e.avisos.iter().any(|a| a.contains("no puede ser ejecutor")), "{:?}", e.avisos);
    // Antigravity sí como planificador
    std::fs::remove_file(f.fichero()).unwrap();
    establecer(&f, Canal::Terminal, None, &cambio("roles.planificador", json!("antigravity"))).unwrap();
}

#[test]
fn el_modelo_del_ejecutor_es_local() {
    let (_d, f) = fuentes();
    // un perfil remoto: solo para el planificador
    let lote = vec![("modelos.gpt.nombre".to_string(), Some(json!("gpt-5-codex"))), ("modelos.gpt.remoto".to_string(), Some(json!(true)))];
    establecer(&f, Canal::Web, Some(""), &lote).unwrap();
    let e = cargar_de(&f);
    assert_eq!(e.ajustes.modelos["gpt"], PerfilModelo { nombre: "gpt-5-codex".into(), base_url: "".into(), remoto: true });
    establecer(&f, Canal::Terminal, None, &cambio("agentes.codex.modelo", json!("gpt"))).unwrap();
    establecer(&f, Canal::Terminal, None, &cambio("roles.planificador", json!("codex"))).unwrap();
    assert_eq!(cargar_de(&f).ajustes.modelo_de("codex").unwrap().nombre, "gpt-5-codex");
    // codex como ejecutor con un modelo remoto: no
    let mut lote = vec![("roles.planificador".to_string(), Some(json!("claude-code")))];
    lote.push(("roles.ejecutor".to_string(), Some(json!("codex"))));
    let r = establecer(&f, Canal::Terminal, None, &lote);
    match r {
        Err(ErrorAjustes::ValorInvalido { motivo, .. }) => assert!(motivo.contains("remoto"), "{motivo}"),
        otro => panic!("{otro:?}"),
    }
    // ni volver remoto el perfil que usa el ejecutor
    let r = establecer(&f, Canal::Web, Some(&version(&f)), &cambio("modelos.local.remoto", json!(true)));
    assert!(matches!(r, Err(ErrorAjustes::ValorInvalido { .. })), "{r:?}");
    // ni quitarle el modelo al ejecutor
    let r = establecer(&f, Canal::Web, Some(&version(&f)), &cambio("agentes.qwen-code.modelo", json!("")));
    assert!(matches!(r, Err(ErrorAjustes::ValorInvalido { .. })), "{r:?}");
    // ni apuntar a un perfil que no existe
    let r = establecer(&f, Canal::Web, Some(&version(&f)), &cambio("agentes.opencode.modelo", json!("nada")));
    assert!(matches!(r, Err(ErrorAjustes::ValorInvalido { .. })), "{r:?}");
    // ni borrar un perfil que alguien usa
    let quitar = vec![("modelos.gpt.nombre".to_string(), None), ("modelos.gpt.remoto".to_string(), None)];
    let r = establecer(&f, Canal::Web, Some(&version(&f)), &quitar);
    assert!(matches!(r, Err(ErrorAjustes::ValorInvalido { .. })), "{r:?}");

    // escrito a mano: el ejecutor pierde el perfil remoto y vuelve al suyo
    escribe(&f, "ajustes.toml", "[modelos.gpt]\nnombre = \"g\"\nremoto = true\n[agentes.qwen-code]\nmodelo = \"gpt\"\n");
    let e = cargar_de(&f);
    assert_eq!(e.ajustes.agentes["qwen-code"].modelo, "local");
    assert!(e.avisos.iter().any(|a| a.contains("agentes.qwen-code.modelo") && a.contains("remoto")), "{:?}", e.avisos);
}

#[test]
fn perfiles_de_modelo() {
    let (_d, f) = fuentes();
    // un perfil local nuevo, con su URL de loopback, y Codex como ejecutor con él
    let lote = vec![
        ("modelos.rapido.nombre".to_string(), Some(json!("qwen3-8b"))),
        ("modelos.rapido.base_url".to_string(), Some(json!("http://127.0.0.1:8081/v1"))),
        ("agentes.codex.modelo".to_string(), Some(json!("rapido"))),
        ("roles.ejecutor".to_string(), Some(json!("codex"))),
    ];
    establecer(&f, Canal::Web, Some(""), &lote).unwrap();
    let e = cargar_de(&f);
    assert_eq!(e.ajustes.modelo_ejecutor().nombre, "qwen3-8b");
    assert_eq!(e.ajustes.modelos.len(), 2);
    assert!(e.claves.iter().any(|(n, _)| n == "modelos.rapido.remoto"));
    // un perfil a medias (sin URL y sin ser remoto) no se escribe
    let r = establecer(&f, Canal::Web, Some(&version(&f)), &cambio("modelos.otro.nombre", json!("x")));
    assert!(matches!(r, Err(ErrorAjustes::ValorInvalido { .. })), "{r:?}");
    // ni un perfil local con una URL de fuera
    let r = establecer(&f, Canal::Web, Some(&version(&f)), &cambio("modelos.rapido.base_url", json!("https://api.ejemplo.com/v1")));
    assert!(matches!(r, Err(ErrorAjustes::ValorInvalido { .. })), "{r:?}");
    // id de perfil inválido
    let r = establecer(&f, Canal::Web, Some(&version(&f)), &cambio("modelos.Mal.nombre", json!("x")));
    assert!(matches!(r, Err(ErrorAjustes::ClaveDesconocida(_))), "{r:?}");
    // escrito a mano a medias: se descarta el perfil, con aviso
    escribe(&f, "ajustes.toml", "[modelos.medio]\nnombre = \"x\"\n");
    let e = cargar_de(&f);
    assert!(!e.ajustes.modelos.contains_key("medio"));
    assert!(!e.valores.contains_key("modelos.medio.nombre"));
    assert!(e.avisos.iter().any(|a| a.contains("modelos.medio") && a.contains("descarta")), "{:?}", e.avisos);
}

#[test]
fn las_claves_antiguas_del_modelo_pasan_a_las_nuevas_al_escribir() {
    let (_d, f) = fuentes();
    escribe(&f, "ajustes.toml", "[modelo]\nnombre = \"viejo\"\nqwen_bin = \"/opt/qwen\"\n\n[web]\nsondeo_ms = 4000\n");
    let e = cargar_de(&f);
    assert_eq!(e.ajustes.agentes["qwen-code"].bin, "/opt/qwen");
    assert_eq!(e.origenes["agentes.qwen-code.bin"].legible(), "heredado: ajustes.toml [modelo]");
    // el nombre antiguo sigue valiendo para escribir, y se escribe el nuevo
    let a = establecer(&f, Canal::Terminal, None, &cambio("modelo.nombre", json!("nuevo"))).unwrap();
    assert_eq!(a[0].clave, "modelos.local.nombre");
    let t = std::fs::read_to_string(f.fichero()).unwrap();
    assert!(t.contains("[modelos.local]") && t.contains("nombre = \"nuevo\""), "{t}");
    assert!(!t.contains("\"viejo\""), "{t}");
    assert!(t.contains("qwen_bin = \"/opt/qwen\""), "{t}");
    establecer(&f, Canal::Terminal, None, &cambio("agentes.qwen-code.bin", json!("/opt/qwen2"))).unwrap();
    let t = std::fs::read_to_string(f.fichero()).unwrap();
    assert!(!t.contains("[modelo]"), "{t}");
    assert!(t.contains("sondeo_ms = 4000"), "{t}");
    let e = cargar_de(&f);
    assert_eq!((e.ajustes.modelos["local"].nombre.as_str(), e.ajustes.agentes["qwen-code"].bin.as_str()), ("nuevo", "/opt/qwen2"));
    assert!(e.avisos.is_empty(), "{:?}", e.avisos);
}

#[test]
fn un_ajustes_toml_imposible_cae_a_los_roles_de_fabrica() {
    let (_d, f) = fuentes();
    escribe(
        &f,
        "ajustes.toml",
        "[modelos.local]\nremoto = true\n[roles]\nplanificador = \"qwen-code\"\nejecutor = \"opencode\"\n[agentes.opencode]\nmodelo = \"\"\n",
    );
    let e = cargar_de(&f);
    assert_eq!(validar_roles(&e.ajustes), Ok(()));
    assert_eq!(e.ajustes.roles.ejecutor, "qwen-code");
    assert_ne!(e.ajustes.roles.planificador, e.ajustes.roles.ejecutor);
    assert!(!e.ajustes.modelo_ejecutor().remoto);
    assert!(!e.conflictos.is_empty());
}

#[test]
fn categorias_de_la_biblioteca_se_escriben_desde_la_web_y_se_leen() {
    let (_d, f) = fuentes();
    assert!(cargar_de(&f).ajustes.biblioteca.categorias.is_empty());
    let id = "claude-code/plugin/usuario/foo@mkt.v2";
    establecer(&f, Canal::Web, None, &cambio("biblioteca.categorias", json!({ id: " Trabajo ", "codex/mcp/usuario/x": "Pruebas" }))).unwrap();
    let e = cargar_de(&f);
    assert!(e.avisos.is_empty(), "{:?}", e.avisos);
    assert_eq!(e.ajustes.biblioteca.categorias[id], "Trabajo");
    assert_eq!(e.ajustes.biblioteca.categorias.len(), 2);
    // una categoría vacía o un valor que no es texto se rechazan
    assert!(establecer(&f, Canal::Web, None, &cambio("biblioteca.categorias", json!({ id: "" }))).is_err());
    assert!(establecer(&f, Canal::Web, None, &cambio("biblioteca.categorias", json!({ id: 3 }))).is_err());
    assert!(interpretar(clave("biblioteca.categorias").unwrap(), r#"{"a/b/c/d":"X"}"#).is_ok());
}

// ------------------------------------------------- 019: equipos

#[test]
fn el_paralelo_vale_uno_por_defecto_va_de_1_a_8_y_solo_se_cambia_desde_la_terminal() {
    let (_d, f) = fuentes();
    assert_eq!(cargar_de(&f).ajustes.orquestador.paralelo, 1);
    let c = clave("orquestador.paralelo").unwrap();
    assert_eq!(interpretar(c, "4").unwrap(), json!(4));
    assert!(interpretar(c, "0").is_err());
    assert!(interpretar(c, "9").is_err());
    assert!(establecer(&f, Canal::Web, Some(""), &cambio("orquestador.paralelo", json!(2))).is_err());
    establecer(&f, Canal::Terminal, None, &cambio("orquestador.paralelo", json!(3))).unwrap();
    assert_eq!(cargar_de(&f).ajustes.orquestador.paralelo, 3);
}

// ------------------------------------------------- 023: instalación

#[test]
fn la_distribucion_de_wsl_vale_vacia_por_defecto_y_solo_se_cambia_desde_la_terminal() {
    let (_d, f) = fuentes();
    assert_eq!(cargar_de(&f).ajustes.escritorio.wsl, "");
    let c = clave("escritorio.wsl").unwrap();
    assert_eq!(interpretar(c, "Ubuntu-24.04").unwrap(), json!("Ubuntu-24.04"));
    assert_eq!(interpretar(c, "").unwrap(), json!(""));
    let largo = "a".repeat(65);
    for malo in ["con espacio", "a;b", "x/y", largo.as_str()] {
        assert!(interpretar(c, malo).is_err(), "{malo}");
    }
    assert!(establecer(&f, Canal::Web, Some(""), &cambio("escritorio.wsl", json!("Ubuntu"))).is_err());
    establecer(&f, Canal::Terminal, None, &cambio("escritorio.wsl", json!("Ubuntu"))).unwrap();
    assert_eq!(cargar_de(&f).ajustes.escritorio.wsl, "Ubuntu");
    establecer(&f, Canal::Terminal, None, &cambio("escritorio.wsl", json!(""))).unwrap();
    assert_eq!(cargar_de(&f).ajustes.escritorio.wsl, "");
}

// ------------------------------------------------- U-01: el criterio en el sandbox

/// `agente.criterio.escribibles`: vacía por defecto, solo desde la terminal,
/// rutas absolutas (o con `~`) que existen, guardadas como se escribieron y
/// expandidas en los ajustes; nunca el directorio personal ni un ancestro
/// suyo, ni un directorio del ecosistema (ni dentro, ni por encima).
#[test]
fn las_rutas_escribibles_del_criterio_se_validan_y_solo_se_cambian_desde_la_terminal() {
    let (d, f) = fuentes();
    let k = "agente.criterio.escribibles";
    assert!(cargar_de(&f).ajustes.agente.criterio.escribibles.is_empty());
    let c = clave(k).unwrap();
    assert_eq!((c.edicion, c.tipo.nombre()), (Edicion::Terminal, "lista"));
    assert!(interpretar(c, "relativa/x").is_err());
    assert!(interpretar(c, "~otro/x").is_err());
    let a = d.path().join("a");
    let a = a.to_str().unwrap();
    assert_eq!(interpretar(c, &format!(" {a} , ~/b,{a} ")).unwrap(), json!([a, "~/b"]));

    let cache = d.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let r = establecer(&f, Canal::Web, Some(""), &cambio(k, json!([cache.to_str().unwrap()])));
    assert!(matches!(r, Err(ErrorAjustes::NoEditable { .. })), "{r:?}");
    establecer(&f, Canal::Terminal, None, &cambio(k, json!([cache.to_str().unwrap()]))).unwrap();
    let e = cargar_de(&f);
    assert_eq!(e.ajustes.agente.criterio.escribibles, std::slice::from_ref(&cache));
    assert_eq!(e.valores[k], json!([cache.to_str().unwrap()]));
    assert_eq!(validar_escribibles(&e.ajustes.agente.criterio.escribibles, &f, &e.ajustes).unwrap(), [ea_plataforma::rutas::canonica(&cache).unwrap()]);

    // Lo que no vale no se escribe y deja la lista como estaba.
    for d in [&f.dir_config, &f.dir_datos, &f.dir_estado] {
        std::fs::create_dir_all(d.join("sub")).unwrap();
    }
    let casa = ea_plataforma::usuario::home().unwrap();
    let malas: Vec<(PathBuf, &str)> = vec![
        (d.path().join("no-existe"), "no existe"),
        (casa.clone(), "directorio personal"),
        (casa.parent().unwrap().to_path_buf(), "directorio personal"),
        (d.path().ancestors().last().unwrap().to_path_buf(), "directorio personal"),
        (f.dir_config.clone(), "del ecosistema"),
        (f.dir_datos.join("sub"), "del ecosistema"),
        (f.dir_estado.join("sub"), "del ecosistema"),
        (d.path().to_path_buf(), "del ecosistema"),
    ];
    for (mala, motivo) in &malas {
        let r = establecer(&f, Canal::Terminal, None, &cambio(k, json!([cache.to_str().unwrap(), mala.to_str().unwrap()])));
        assert!(matches!(&r, Err(ErrorAjustes::ValorInvalido { clave, motivo: m }) if clave == k && m.contains(motivo)), "{mala:?}: {r:?}");
        assert_eq!(cargar_de(&f).ajustes.agente.criterio.escribibles, std::slice::from_ref(&cache), "{mala:?}");
    }
    let r = establecer(&f, Canal::Terminal, None, &cambio(k, json!(["~"])));
    assert!(matches!(&r, Err(ErrorAjustes::ValorInvalido { motivo, .. }) if motivo.contains("directorio personal")), "{r:?}");

    // El directorio de ejecución (la clave del panel), tampoco.
    // (`XDG_RUNTIME_DIR` en Unix; en Windows cuelga de `LOCALAPPDATA`.)
    let rt = d.path().join("rt");
    let f2 = Fuentes::aislada(d.path()).con_var("XDG_RUNTIME_DIR", rt.to_str().unwrap()).con_var("LOCALAPPDATA", rt.to_str().unwrap());
    let web = ea_ajustes::dir_ejecucion(&f2.entorno).unwrap().join("ea-web");
    std::fs::create_dir_all(&web).unwrap();
    let r = establecer(&f2, Canal::Terminal, None, &cambio(k, json!([web.to_str().unwrap()])));
    assert!(matches!(&r, Err(ErrorAjustes::ValorInvalido { motivo, .. }) if motivo.contains("del ecosistema")), "{r:?}");

    // Una ruta que deja de valer (aquí, borrada) se ignora al cargar, con aviso.
    std::fs::remove_dir(&cache).unwrap();
    let e = cargar_de(&f);
    assert!(e.ajustes.agente.criterio.escribibles.is_empty());
    assert!(e.avisos.iter().any(|a| a.contains(k) && a.contains("no existe")), "{:?}", e.avisos);
}

/// Spec 024: `agente.tapar`, vacía por defecto, solo desde la terminal,
/// rutas absolutas (o con `~`) sin `..` ni la raíz, guardadas como se
/// escribieron y expandidas en los ajustes. No hace falta que existan.
#[test]
fn agente_tapar_valida_rutas_y_solo_se_cambia_desde_la_terminal() {
    let (d, f) = fuentes();
    let k = "agente.tapar";
    assert!(cargar_de(&f).ajustes.agente.tapar.is_empty(), "por defecto no tapa nada");
    let c = clave(k).unwrap();
    assert_eq!((c.edicion, c.tipo.nombre(), c.endurecible), (Edicion::Terminal, "lista", false));
    // Rutas absolutas en cada sistema (en Windows, con unidad).
    let secreto = d.path().join("secreto");
    let secreto = secreto.to_str().unwrap();
    assert_eq!(interpretar(c, &format!("~/.config/x, {secreto},{secreto}")).unwrap(), json!(["~/.config/x", secreto]));
    assert_eq!(interpretar(c, "").unwrap(), json!([]));
    let con_puntos = d.path().join("a").join("..").join("b");
    let raiz = d.path().ancestors().last().unwrap();
    for malo in ["relativa/ruta", con_puntos.to_str().unwrap(), raiz.to_str().unwrap()] {
        assert!(interpretar(c, malo).is_err(), "{malo}");
    }
    for malo in [json!(secreto), json!([1]), json!([""]), json!([format!("{secreto}\nb")])] {
        let r = establecer(&f, Canal::Terminal, None, &cambio(k, malo.clone()));
        assert!(matches!(r, Err(ErrorAjustes::ValorInvalido { .. })), "{malo}: {r:?}");
    }
    let r = establecer(&f, Canal::Web, Some(""), &cambio(k, json!([])));
    assert!(matches!(r, Err(ErrorAjustes::NoEditable { .. })), "{r:?}");
    establecer(&f, Canal::Terminal, None, &cambio(k, json!(["~/.secreto", secreto]))).unwrap();
    let e = cargar_de(&f);
    assert_eq!(e.ajustes.agente.tapar, [expandir_tilde("~/.secreto"), PathBuf::from(secreto)]);
    assert_eq!(e.valores[k], json!(["~/.secreto", secreto]));
}

/// U-09: el techo del tope del criterio es uno más de `agente.techo_topes`:
/// 3600 s de fábrica, de 10 a 3600, solo baja desde la web y se hereda del
/// `[topes]` de `orquestador.toml`.
#[test]
fn el_techo_del_criterio_es_un_techo_de_topes_mas() {
    let (_d, f) = fuentes();
    let k = "agente.techo_topes.tiempo_criterio_s";
    assert_eq!(cargar_de(&f).ajustes.agente.techo_topes.tiempo_criterio_s, 3600);
    let c = clave(k).unwrap();
    assert!(c.endurecible && c.edicion == Edicion::Web);
    assert!(interpretar(c, "9").is_err() && interpretar(c, "3601").is_err());
    escribe(&f, "orquestador.toml", "[topes]\ntiempo_criterio_s = 300\n");
    assert_eq!(cargar_de(&f).ajustes.agente.techo_topes.tiempo_criterio_s, 300);
    let v = version(&f);
    establecer(&f, Canal::Web, Some(&v), &cambio(k, json!(200))).unwrap();
    let v = version(&f);
    let r = establecer(&f, Canal::Web, Some(&v), &cambio(k, json!(250)));
    assert!(matches!(r, Err(ErrorAjustes::SoloEndurecer { .. })), "{r:?}");
    establecer(&f, Canal::Terminal, None, &cambio(k, json!(900))).unwrap();
    assert_eq!(cargar_de(&f).ajustes.agente.techo_topes.tiempo_criterio_s, 900);
}
