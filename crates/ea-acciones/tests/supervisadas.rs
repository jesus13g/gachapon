//! Fija, herramienta por herramienta, lo que hace cada mutante supervisada:
//! su riesgo y quién la ve o la confirma, sus campos, lo que observa y simula
//! (texto, argumentos y precondición), los errores de sus argumentos y lo que
//! deja en el diario al proponerla, aceptarla o ejecutarla directamente. Con
//! el sistema simulado en directorios temporales; lo que cambia de una
//! ejecución a otra (rutas temporales, ids, marcas de tiempo) se normaliza.
//!
//! El resultado se compara con `fijado/supervisadas.txt`. Solo si el cambio
//! es a propósito, `EA_FIJAR=1 cargo test -p ea-acciones --test supervisadas`
//! lo reescribe. Solo en Linux: la orden de la terminal de una sesión y las
//! rutas dependen del sistema.
#![cfg(target_os = "linux")]

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use ea_acciones::*;
use ea_ajustes::{Ajustes, Canal as CanalAjustes, Fuentes};
use ea_biblioteca::{Biblioteca, Contexto, Raices};
use ea_bus::InfoWorktree;
use ea_pruebas::escribe;
use ea_store::{EventFilter, Store};
use serde_json::{json, Value};

const CODEX: &str = "[mcp_servers.docs]\ncommand = \"docs-mcp\"\n\n[mcp_servers.apagado]\ncommand = \"a\"\nenabled = false\n";

fn rutina(slug: &str, version: u32, criterio: &str, extra: &str) -> String {
    format!(
        "[rutina]\nslug = \"{slug}\"\nversion = {version}\nworktree = \"/tmp\"\ncriterio_exito = \"{criterio}\"\n{extra}[[pasos]]\ntexto = \"x\"\n"
    )
}

fn nocturna(version: u32) -> String {
    rutina("nocturna", version, "true", "")
}

fn con_entradas() -> String {
    rutina("con-entradas", 1, "test -f {salida}", "[entradas]\nsalida = \"informe.md\"\n")
}

/// Un mundo nuevo: rutinas activas (`nocturna`, con timer, y `con-entradas`),
/// una retirada (`vieja`), un servicio, un worktree terminado y limpio, una
/// nota, y una biblioteca con un HOME temporal.
struct Mundo {
    d: tempfile::TempDir,
    store: Store,
    sis: SistemaSimulado,
}

impl Mundo {
    fn nuevo() -> Self {
        let d = tempfile::tempdir().unwrap();
        let store = Store::open_at(&d.path().join("t.db")).unwrap();
        let (n, c, v) = (nocturna(1), con_entradas(), rutina("vieja", 1, "true", ""));
        ea_pruebas::guardar_rutinas(&store, &[("nocturna", 1, &n), ("con-entradas", 1, &c), ("vieja", 1, &v)]);
        let home = d.path().join("home");
        escribe(home.join(".codex/config.toml"), CODEX);
        escribe(home.join(".claude/skills/propia/SKILL.md"), "---\ndescription: Propia\n---\n");
        escribe(home.join(".claude.json"), r#"{"mcpServers":{"srv":{"type":"stdio","command":"srv-mcp","args":["-v"]}}}"#);
        fs::create_dir_all(d.path().join("rt")).unwrap();
        let raices = Raices::nuevo(home, None, d.path().join("estado-biblioteca"), Some(d.path().join("rt")));
        let inventario = Biblioteca::cargar(&raices, &Ajustes::por_defecto(&Fuentes::aislada(d.path())));
        let ctx = Contexto::nuevo(raices.clone(), Fuentes::aislada(d.path()), CanalAjustes::Terminal);
        let sis = SistemaSimulado::default()
            .con_estado(&d.path().join("estado"))
            .con_servicio("ea-mcp", "active")
            .con_rutina("nocturna", &nocturna(1))
            .con_rutina("con-entradas", &con_entradas())
            .con_timer("nocturna", "daily")
            .con_retirada("vieja", "[rutina]\ndescripcion = \"x\"\n")
            .con_worktree(InfoWorktree {
                trabajo: "t-1".into(),
                ruta: "/estado/worktrees/t-1".into(),
                rama: "ea/t-1".into(),
                estado_trabajo: Some(ea_bus::EstadoTrabajo::Cerrado),
                titulo: Some("t".into()),
                limpio: Some(true),
                ocupado: false,
            })
            .con_nota("claude-code/tema/nota.md", "Una nota con contenido.")
            .con_biblioteca(ctx)
            .con_sesiones(inventario, raices);
        Mundo { d, store, sis }
    }

    /// El diario, del más antiguo al más reciente.
    fn diario(&self) -> String {
        let mut eventos = self.store.list_events(&EventFilter { limit: Some(500), ..Default::default() }).unwrap();
        eventos.reverse();
        eventos
            .iter()
            .map(|e| {
                format!(
                    "  {} {} trabajo={:?} {:?} {}\n",
                    e.actor,
                    e.action,
                    e.trabajo,
                    e.outcome,
                    e.detail.as_deref().unwrap_or_default()
                )
            })
            .collect()
    }

    /// Quita lo que cambia de una ejecución a otra: los valores dados y la
    /// carpeta temporal del mundo.
    fn normalizar(&self, texto: &str, volatiles: &[(String, &str)]) -> String {
        let mut t = texto.to_string();
        for (valor, marca) in volatiles.iter().filter(|(v, _)| !v.is_empty()) {
            t = t.replace(valor.as_str(), marca);
        }
        t.replace(&self.d.path().display().to_string(), "<TMP>")
    }
}

fn si(b: bool) -> &'static str {
    if b { "sí" } else { "no" }
}

fn res<T>(r: Result<T, ErrorAccion>) -> String {
    match r {
        Ok(_) => "ok".into(),
        Err(e) => format!("error {e:?}"),
    }
}

/// Lo que cambia entre ejecuciones dentro de un resultado o una simulación.
fn volatiles_de(datos: &Value, precondicion: Option<&Value>, v: &mut Vec<(String, &'static str)>) {
    for (clave, marca) in [("copia", "<COPIA>"), ("trabajo", "<TRABAJO>"), ("sesion", "<SESION>")] {
        if let Some(s) = datos[clave].as_str().filter(|s| !s.starts_with("t-")) {
            v.push((s.to_string(), marca));
        }
    }
    if let Some(p) = precondicion.and_then(|p| p["plan"].as_str()) {
        v.push((p.to_string(), "<HUELLA_PLAN>"));
    }
}

async fn fijar(nombre: &str, args: Value, errores: &[Value]) -> String {
    let mut s = String::new();
    let h = buscar(nombre).unwrap();
    writeln!(s, "== {nombre}").unwrap();
    writeln!(s, "catálogo: riesgo={} ejecución={:?} local={}", h.riesgo.as_str(), h.ejecucion, h.local).unwrap();
    let actores: Vec<String> = ["claude-code", "modelo-local", "orquestador", "sin-rol", "desconocido", "usuario"]
        .iter()
        .map(|a| format!("{a}={}", si(autorizar_mcp(a, h).is_ok())))
        .collect();
    writeln!(s, "mcp: {}", actores.join(" ")).unwrap();
    writeln!(
        s,
        "confirma: web={} terminal={}",
        si(puede_confirmar(Canal::Web, h.riesgo).is_ok()),
        si(puede_confirmar(Canal::Terminal, h.riesgo).is_ok())
    )
    .unwrap();
    for c in campos(nombre) {
        writeln!(
            s,
            "campo: {} obligatorio={} entero={} largo={} opciones={:?} ayuda={:?}",
            c.nombre, c.obligatorio, c.entero, c.largo, c.opciones, c.ayuda
        )
        .unwrap();
    }
    writeln!(s, "argumentos: {args}").unwrap();
    writeln!(s, "validar: {}", res(validar_argumentos(nombre, &args))).unwrap();

    // Proponer (el planificador) y aceptar (el usuario).
    let mut m = Mundo::nuevo();
    let mut volatiles = Vec::new();
    let mut t = String::new();
    for malo in errores {
        let simulado = match observar(&m.sis, nombre, malo).await {
            Ok(hechos) => res(simular_con(&m.store, &m.sis, &hechos, nombre, malo)),
            Err(e) => format!("observar: error {e:?}"),
        };
        writeln!(t, "error: {malo}\n  validar: {}\n  simular: {simulado}", res(validar_argumentos(nombre, malo))).unwrap();
    }
    let hechos = observar(&m.sis, nombre, &args).await.unwrap();
    writeln!(t, "hechos: {hechos:?}").unwrap();
    let sim = simular_con(&m.store, &m.sis, &hechos, nombre, &args).unwrap();
    volatiles_de(&Value::Null, Some(&sim.precondicion), &mut volatiles);
    writeln!(t, "simulación: herramienta={} riesgo={}", sim.herramienta, sim.riesgo.as_str()).unwrap();
    writeln!(t, "  argumentos: {}", sim.argumentos).unwrap();
    writeln!(t, "  precondición: {}", sim.precondicion).unwrap();
    writeln!(t, "  texto: {}", sim.texto).unwrap();
    writeln!(t, "mutaciones tras simular: {:?}", m.sis.mutaciones()).unwrap();
    let (p, nueva) = proponer(&m.store, &m.sis, &hechos, "claude-code", None, nombre, &args).unwrap();
    let (otra, otra_nueva) = proponer(&m.store, &m.sis, &hechos, "modelo-local", None, nombre, &args).unwrap();
    writeln!(t, "proponer: #{} nueva={nueva}; otra vez: #{} nueva={otra_nueva}", p.id, otra.id).unwrap();
    let canal = if puede_confirmar(Canal::Web, h.riesgo).is_ok() {
        Canal::Web
    } else {
        let e = aceptar_propuesta(&mut m.store, &m.sis, p.id, "usuario", Canal::Web).await.unwrap_err();
        let sigue = m.store.get_propuesta(p.id).unwrap().estado;
        writeln!(t, "aceptar desde la web: error {e:?}; sigue {}", sigue.as_str()).unwrap();
        Canal::Terminal
    };
    let (p, r) = aceptar_propuesta_con_resultado(&mut m.store, &m.sis, p.id, "usuario", canal).await.unwrap();
    let ej = p.ejecucion.as_ref().unwrap();
    writeln!(t, "aceptar ({}): {} / {:?}: {}", canal.nombre(), p.estado.as_str(), ej.estado, ej.detalle.as_deref().unwrap_or_default())
        .unwrap();
    let r = r.unwrap();
    volatiles_de(&r.datos, None, &mut volatiles);
    writeln!(t, "resultado: trabajo={:?} evento={:?}\n  datos: {}\n  texto: {}", r.trabajo, r.evento, r.datos, r.texto).unwrap();
    writeln!(t, "mutaciones: {:?}", m.sis.mutaciones()).unwrap();
    writeln!(t, "diario:\n{}", m.diario()).unwrap();
    s.push_str(&m.normalizar(&t, &volatiles));

    // Ejecución directa (un botón o una orden del usuario), en otro mundo.
    let mut m = Mundo::nuevo();
    let mut volatiles = Vec::new();
    let mut t = String::new();
    if canal == Canal::Terminal {
        let e = ejecutar_directo(&mut m.store, &m.sis, "usuario", Canal::Web, nombre, &args).await.unwrap_err();
        writeln!(t, "directa desde la web: error {e:?}").unwrap();
    }
    let r = ejecutar_directo(&mut m.store, &m.sis, "usuario", canal, nombre, &args).await.unwrap();
    volatiles_de(&r.datos, None, &mut volatiles);
    writeln!(t, "directa ({}): trabajo={:?} evento={:?}\n  datos: {}\n  texto: {}", canal.nombre(), r.trabajo, r.evento, r.datos, r.texto)
        .unwrap();
    writeln!(t, "mutaciones: {:?}", m.sis.mutaciones()).unwrap();
    writeln!(t, "diario:\n{}", m.diario()).unwrap();
    s.push_str(&m.normalizar(&t, &volatiles));
    s
}

const SKILL: &str = "claude-code/skill/usuario/propia";
const MCP: &str = "claude-code/mcp/usuario/srv";

#[tokio::test]
async fn cada_herramienta_supervisada_hace_lo_mismo_que_siempre() {
    let casos: Vec<(&str, Value, Vec<Value>)> = vec![
        (
            "ea_encolar_rutina",
            json!({ "slug": "con-entradas", "entradas": { "salida": "otro.md" } }),
            vec![
                json!({ "slug": "../x" }),
                json!({ "slug": "nocturna", "extra": 1 }),
                json!({ "slug": "no-registrada" }),
                json!({ "slug": "con-entradas", "entradas": { "noexiste": "1" } }),
                json!({ "slug": "con-entradas", "entradas": { "salida": "a\nb" } }),
                json!({ "slug": "vieja" }),
            ],
        ),
        (
            "ea_registrar_rutina",
            json!({ "toml": nocturna(2).replace("criterio_exito = \"true\"", "criterio_exito = \"test -d .\"") }),
            vec![json!({ "toml": "[rutina]\nslug = \"x\"\n" }), json!({ "toml": nocturna(1) }), json!({ "toml": 1 })],
        ),
        (
            "ea_reiniciar_servicio",
            json!({ "servicio": "ea-mcp" }),
            vec![json!({ "servicio": "sshd" }), json!({ "servicio": "ea-web" }), json!({})],
        ),
        (
            "ea_programar_rutina",
            json!({ "slug": "nocturna", "calendario": " *-*-* 08:00:00 " }),
            vec![
                json!({ "slug": "nocturna", "calendario": "a%b" }),
                json!({ "slug": "nocturna", "calendario": "cuando quieras" }),
                json!({ "slug": "nocturna", "calendario": "daily" }),
                json!({ "slug": "vieja", "calendario": "daily" }),
                json!({ "slug": "no-registrada", "calendario": "daily" }),
            ],
        ),
        ("ea_desprogramar_rutina", json!({ "slug": "nocturna" }), vec![json!({ "slug": "con-entradas" }), json!({ "slug": "a b" })]),
        ("ea_retirar_rutina", json!({ "slug": "nocturna" }), vec![json!({ "slug": "vieja" }), json!({ "slug": "inexistente" })]),
        ("ea_restaurar_rutina", json!({ "slug": "vieja" }), vec![json!({ "slug": "nocturna" }), json!({ "slug": "inexistente" })]),
        ("ea_eliminar_rutina", json!({ "slug": "vieja" }), vec![json!({ "slug": "nocturna" }), json!({ "slug": "inexistente" })]),
        (
            "ea_limpiar_worktree",
            json!({ "trabajo": "t-1" }),
            vec![json!({ "trabajo": "../x" }), json!({ "trabajo": "t-2" }), json!({ "trabajo": "t-1", "extra": 1 })],
        ),
        (
            "ea_promover_nota",
            json!({ "ruta": "claude-code/tema/nota.md", "destino": " " }),
            vec![
                json!({ "ruta": "compartida/x.md" }),
                json!({ "ruta": "claude-code/no.md" }),
                json!({ "ruta": "claude-code/tema/nota.md", "destino": "../x" }),
            ],
        ),
        (
            "ea_lanzar_sesion",
            json!({ "agente": "claude-code", "herramientas": format!("{SKILL}\n{MCP}, {SKILL}") }),
            vec![
                json!({ "agente": "emacs", "herramientas": [SKILL] }),
                json!({ "agente": "claude-code", "herramientas": [] }),
                json!({ "agente": "codex", "herramientas": [SKILL] }),
                json!({ "agente": "claude-code", "herramientas": ["claude-code/mcp/usuario/fantasma"] }),
            ],
        ),
        (
            "ea_activar_herramienta",
            json!({ "elemento": "codex/mcp/usuario/apagado" }),
            vec![json!({ "elemento": "codex/mcp/usuario/docs" }), json!({ "elemento": "../../etc/passwd" })],
        ),
        (
            "ea_desactivar_herramienta",
            json!({ "elemento": "codex/mcp/usuario/docs" }),
            vec![json!({ "elemento": "codex/mcp/usuario/no-existe" }), json!({ "elemento": "codex/mcp/usuario/docs", "extra": 1 })],
        ),
    ];
    let mut actual = String::new();
    for (nombre, args, errores) in casos {
        actual.push_str(&fijar(nombre, args, &errores).await);
        actual.push('\n');
    }
    // Lo que no es una herramienta supervisada.
    let m = Mundo::nuevo();
    for nombre in ["ea_status", "ea_escribir_nota", "ea_inexistente"] {
        let hechos = observar(&m.sis, nombre, &json!({})).await;
        let simulado = hechos.and_then(|h| simular_con(&m.store, &m.sis, &h, nombre, &json!({})));
        writeln!(
            actual,
            "== {nombre}\ncampos: {}\nvalidar: {}\nsimular: {}\n",
            campos(nombre).len(),
            res(validar_argumentos(nombre, &json!({}))),
            res(simulado)
        )
        .unwrap();
    }

    let fichero = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fijado/supervisadas.txt");
    if std::env::var_os("EA_FIJAR").is_some() {
        fs::create_dir_all(fichero.parent().unwrap()).unwrap();
        fs::write(&fichero, &actual).unwrap();
    }
    let esperado = fs::read_to_string(&fichero).unwrap_or_default();
    if let Some((n, (a, e))) = actual.lines().zip(esperado.lines()).enumerate().find(|(_, (a, e))| a != e) {
        panic!("la línea {} cambió:\n  antes: {e}\n  ahora: {a}", n + 1);
    }
    assert_eq!(actual.lines().count(), esperado.lines().count(), "cambió el número de líneas");
}
