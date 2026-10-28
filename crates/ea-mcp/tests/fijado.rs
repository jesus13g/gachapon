//! Fija lo que el servidor MCP dice, sobre HTTP de verdad: el `initialize`,
//! la lista de herramientas con sus descripciones, anotaciones y esquemas de
//! argumentos, la respuesta (o el error) de cada herramienta para varios
//! clientes, y lo que queda en el diario. Con bus, base, bóveda, rutinas y
//! HOME de `tempfile`; las fechas, las duraciones y la carpeta temporal se
//! normalizan. Solo se llama a lo que no depende del sistema (ni `systemctl`
//! ni el HOME real).
//!
//! El resultado se compara con `fijado/mcp.txt`. Solo si el cambio es a
//! propósito, `EA_FIJAR=1 cargo test -p ea-mcp --test fijado` lo reescribe.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use ea_biblioteca::Raices;
use ea_boveda::Boveda;
use ea_bus::{Bus, Evento, Fase, Orden};
use ea_mcp::EaTools;
use ea_store::{EventFilter, Store};
use ea_pruebas::escribe;
use ea_pruebas::mcp::{conectar, Cliente};
use rmcp::model::CallToolRequestParams;
use serde_json::{json, Value};
use tokio::sync::Mutex;

const RUTINA: &str = "[rutina]\nslug = \"nocturna\"\nversion = 1\nworktree = \"/tmp\"\ncriterio_exito = \"true\"\n[[pasos]]\ntexto = \"x\"\n";

/// Dos trabajos con ids fijos: una misión cerrada y una obra en cola.
fn sembrar(bus: &Bus, wt: &str) -> [String; 2] {
    let ids = ["20260101T000001-00000001", "20260101T000002-00000002"].map(String::from);
    let fase = |n: u32| Fase { numero: n, titulo: format!("fase {n}"), instrucciones: "i".into(), entregable: "e".into(), criterio: "true".into(), skills: vec![] };
    let ordenes = [Orden::mision("una misión", wt, "o", "true", "prueba"), Orden::obra("una obra", wt, "o", vec![fase(1), fase(2)], None, "prueba")];
    for (id, base) in ids.iter().zip(ordenes) {
        bus.crear_trabajo(Orden { id: id.clone(), creada_en: "2026-01-01T00:00:00Z".into(), ..base }).unwrap();
    }
    bus.retirar_de_colas(&ids[0]).unwrap();
    bus.aplicar(&ids[0], Evento::Tomar).unwrap();
    let informe = serde_json::from_value(json!({ "resultado": "terminado", "duracion_s": 12 })).unwrap();
    bus.aplicar(&ids[0], Evento::Entregar(informe)).unwrap();
    bus.aplicar(&ids[0], Evento::Cerrar).unwrap();
    ids
}

/// `AAAA-MM-DDThh:mm:ss`, con fracción y zona opcionales: los bytes que ocupa.
fn fecha(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let d = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
    let c = |i: usize, ch: u8| b.get(i) == Some(&ch);
    let forma = (0..4).all(d) && c(4, b'-') && d(5) && d(6) && c(7, b'-') && d(8) && d(9) && c(10, b'T');
    if !(forma && d(11) && d(12) && c(13, b':') && d(14) && d(15) && c(16, b':') && d(17) && d(18)) {
        return None;
    }
    let mut n = 19;
    if c(n, b'.') {
        n += 1;
        while d(n) {
            n += 1;
        }
    }
    if c(n, b'Z') {
        n += 1;
    } else if (c(n, b'+') || c(n, b'-')) && d(n + 1) && d(n + 2) && c(n + 3, b':') && d(n + 4) && d(n + 5) {
        n += 6;
    }
    Some(n)
}

/// Sin fechas, sin duraciones y sin la carpeta temporal.
fn normalizar(texto: &str, tmp: &str) -> String {
    let texto = texto.replace(tmp, "<TMP>");
    let mut out = String::new();
    let mut resto = texto.as_str();
    while let Some(ch) = resto.chars().next() {
        if let Some(n) = fecha(resto) {
            out.push_str("<FECHA>");
            resto = &resto[n..];
            continue;
        }
        if let Some(r) = resto.strip_prefix("\"duracion_ms\": ") {
            let n = r.bytes().take_while(u8::is_ascii_digit).count();
            if n > 0 {
                out.push_str("\"duracion_ms\": <MS>");
                resto = &r[n..];
                continue;
            }
        }
        // La de una nota escrita: su cabecera lleva la fecha.
        if let Some(r) = resto.strip_prefix("\"huella\":\"") {
            out.push_str("\"huella\":\"<HUELLA>");
            resto = &r[r.bytes().take_while(u8::is_ascii_hexdigit).count()..];
            continue;
        }
        out.push(ch);
        resto = &resto[ch.len_utf8()..];
    }
    out
}

struct Registro {
    texto: String,
    tmp: String,
}

impl Registro {
    async fn llamar(&mut self, c: &Cliente, cliente: &str, herramienta: &str, args: Value) {
        let r = c
            .call_tool(CallToolRequestParams::new(herramienta.to_string()).with_arguments(args.as_object().cloned().unwrap_or_default()))
            .await;
        let mut t = format!(">> {cliente} {herramienta} {args}\n");
        match r {
            Ok(r) => {
                let mut v = serde_json::to_value(&r).unwrap();
                let contenido = v["content"][0]["text"].take();
                writeln!(t, "<< {v}\n{}", contenido.as_str().unwrap_or_default()).unwrap();
            }
            Err(e) => writeln!(t, "<< error {e:?}").unwrap(),
        }
        self.texto.push_str(&normalizar(&t, &self.tmp));
    }
}

#[tokio::test]
async fn el_servidor_mcp_dice_lo_mismo_que_siempre() {
    let dir = tempfile::tempdir().unwrap();
    let tmp = dir.path().display().to_string();
    let estado = dir.path().join("estado");
    let bus = Bus::abrir_en(&estado).unwrap();
    let ids = sembrar(&bus, &tmp);
    let rutinas = dir.path().join("rutinas");
    escribe(rutinas.join("nocturna.toml"), RUTINA);
    escribe(rutinas.join(".papelera/retirada.toml"), "[rutina]\n");
    let home = dir.path().join("home");
    escribe(home.join(".codex/config.toml"), "[mcp_servers.docs]\ncommand = \"docs-mcp\"\n");
    let raices = Raices::nuevo(home, None, dir.path().join("estado-biblioteca"), None);
    let db = dir.path().join("t.db");
    let store = Store::open_at(&db).unwrap();
    // La versión registrada, con un repo de verdad (U-05: el worktree de una
    // rutina es la raíz de un repo); el fichero de `rutinas_dir` sigue igual.
    let repo = ea_pruebas::repo_git(&dir.path().join("repo"), true);
    let rutina = RUTINA.replace("\"/tmp\"", &format!("\"{}\"", repo.display()));
    ea_pruebas::guardar_rutinas(&store, &[("nocturna", 1, &rutina)]);
    let store = Arc::new(Mutex::new(store));
    let boveda = dir.path().join("boveda");
    let ajustes = ea_ajustes::Fuentes::aislada(&dir.path().join("ajustes"));
    let (r, e) = (rutinas.clone(), estado.clone());
    let router = ea_mcp::router(
        move || EaTools::con_entorno(store.clone(), r.clone(), Boveda::en(boveda.clone()), e.clone()).con_ajustes(ajustes.clone()).con_credencial(credencial()).con_raices(raices.clone()),
        Default::default(),
    );
    let url = ea_pruebas::mcp::servidor(router).await;

    let mut reg = Registro { texto: String::new(), tmp };
    let planificador = conectar(&url, "claude-code").await;
    let ejecutor = conectar(&url, "qwen-code-mcp-client").await;
    let orquestador = conectar(&url, "ea-orchestrator").await;
    let desconocido = conectar(&url, "lo-que-sea").await;

    let mut info = serde_json::to_value(planificador.peer_info().unwrap()).unwrap();
    info["serverInfo"]["version"] = json!("<VERSION>");
    writeln!(reg.texto, "== initialize\n{}", serde_json::to_string_pretty(&info).unwrap()).unwrap();
    let lista = planificador.list_tools(Default::default()).await.unwrap();
    writeln!(reg.texto, "== tools/list\n{}", serde_json::to_string_pretty(&lista).unwrap()).unwrap();
    writeln!(reg.texto, "== llamadas").unwrap();

    let (p, x, o, d) = (&planificador, &ejecutor, &orquestador, &desconocido);
    let (pc, xc, oc, dc) = ("claude-code", "qwen-code-mcp-client", "ea-orchestrator", "lo-que-sea");
    // Consultas.
    reg.llamar(p, pc, "ea_status", json!({})).await;
    reg.llamar(p, pc, "ea_listar_trabajos", json!({})).await;
    reg.llamar(p, pc, "ea_listar_trabajos", json!({ "estado": "cerrado" })).await;
    reg.llamar(p, pc, "ea_listar_trabajos", json!({ "modo": "obra", "limite": 1 })).await;
    reg.llamar(x, xc, "ea_ver_trabajo", json!({ "id": ids[0] })).await;
    reg.llamar(p, pc, "ea_ver_trabajo", json!({ "id": "no-existe" })).await;
    reg.llamar(o, oc, "ea_listar_equipos", json!({})).await;
    reg.llamar(p, pc, "ea_ver_equipo", json!({ "id": "no-existe", "trabajos": 3 })).await;
    reg.llamar(p, pc, "ea_listar_diario", json!({ "limite": 3 })).await;
    reg.llamar(p, pc, "ea_listar_diario", json!({ "actor": "qwen-code", "trabajo": ids[0] })).await;
    reg.llamar(p, pc, "ea_listar_diario", json!({ "desde": "2026-13-01" })).await;
    reg.llamar(p, pc, "ea_biblioteca", json!({ "agente": "codex" })).await;
    reg.llamar(x, xc, "ea_biblioteca", json!({ "id": "codex/mcp/usuario/docs" })).await;
    reg.llamar(p, pc, "ea_biblioteca", json!({ "tipo": "nada" })).await;
    reg.llamar(o, oc, "ea_biblioteca", json!({ "agente": "codex", "activado": false })).await;
    // Bóveda.
    reg.llamar(p, pc, "ea_escribir_nota", json!({ "titulo": "Puerto del MCP", "contenido": "El puerto es 8765.", "simular": true })).await;
    reg.llamar(p, pc, "ea_escribir_nota", json!({ "titulo": "Puerto del MCP", "contenido": "El puerto es 8765.", "etiquetas": ["mcp"], "trabajo": ids[0] })).await;
    reg.llamar(p, pc, "ea_escribir_nota", json!({ "titulo": "Puerto del MCP", "contenido": "otra vez" })).await;
    reg.llamar(x, xc, "ea_escribir_nota", json!({ "titulo": "Local", "contenido": "del ejecutor", "ruta": "notas/local.md" })).await;
    reg.llamar(o, oc, "ea_escribir_nota", json!({ "titulo": "No", "contenido": "no" })).await;
    reg.llamar(d, dc, "ea_escribir_nota", json!({ "titulo": "No", "contenido": "no", "simular": true })).await;
    reg.llamar(x, xc, "ea_buscar_notas", json!({ "consulta": "puerto" })).await;
    reg.llamar(p, pc, "ea_buscar_notas", json!({ "consulta": "local", "region": "modelo-local", "limite": 1 })).await;
    reg.llamar(p, pc, "ea_leer_nota", json!({ "ruta": "claude-code/puerto-del-mcp.md" })).await;
    reg.llamar(x, xc, "ea_leer_nota", json!({ "ruta": "claude-code/no.md", "desde": 2 })).await;
    reg.llamar(p, pc, "ea_promover_nota", json!({ "ruta": "claude-code/puerto-del-mcp.md", "simular": true })).await;
    reg.llamar(x, xc, "ea_promover_nota", json!({ "ruta": "modelo-local/notas/local.md", "destino": "notas/local.md" })).await;
    reg.llamar(d, dc, "ea_promover_nota", json!({ "ruta": "claude-code/puerto-del-mcp.md" })).await;
    // Mutantes supervisadas: simular, proponer y rechazos.
    reg.llamar(p, pc, "ea_encolar_rutina", json!({ "slug": "nocturna", "simular": true })).await;
    reg.llamar(p, pc, "ea_encolar_rutina", json!({ "slug": "nocturna", "entradas": {} })).await;
    reg.llamar(p, pc, "ea_encolar_rutina", json!({ "slug": "nocturna" })).await;
    reg.llamar(p, pc, "ea_encolar_rutina", json!({ "slug": "nocturna", "entradas": { "x": "1" }, "simular": true })).await;
    reg.llamar(x, xc, "ea_encolar_rutina", json!({ "slug": "nocturna" })).await;
    reg.llamar(p, pc, "ea_registrar_rutina", json!({ "toml": rutina.replace("version = 1", "version = 2"), "simular": true })).await;
    reg.llamar(p, pc, "ea_registrar_rutina", json!({ "toml": rutina.replace("version = 1", "version = 2") })).await;
    reg.llamar(p, pc, "ea_registrar_rutina", json!({ "toml": "[rutina]\n" })).await;
    reg.llamar(p, pc, "ea_eliminar_rutina", json!({ "slug": "retirada", "simular": true })).await;
    reg.llamar(p, pc, "ea_eliminar_rutina", json!({ "slug": "retirada" })).await;
    reg.llamar(p, pc, "ea_restaurar_rutina", json!({ "slug": "retirada", "simular": false })).await;
    reg.llamar(p, pc, "ea_restaurar_rutina", json!({ "slug": "nocturna" })).await;
    reg.llamar(p, pc, "ea_retirar_rutina", json!({ "slug": "../x" })).await;
    reg.llamar(p, pc, "ea_desprogramar_rutina", json!({ "slug": "a b", "simular": true })).await;
    reg.llamar(p, pc, "ea_programar_rutina", json!({ "slug": "../x", "calendario": "a%b", "simular": true })).await;
    reg.llamar(p, pc, "ea_reiniciar_servicio", json!({ "servicio": "sshd" })).await;
    reg.llamar(o, oc, "ea_reiniciar_servicio", json!({ "servicio": "ea-mcp", "simular": true })).await;
    reg.llamar(p, pc, "ea_limpiar_worktree", json!({ "trabajo": ids[0], "simular": true })).await;
    reg.llamar(p, pc, "ea_limpiar_worktree", json!({ "trabajo": "../x" })).await;
    reg.llamar(p, pc, "ea_lanzar_sesion", json!({ "agente": "emacs", "herramientas": ["x"], "simular": true })).await;
    reg.llamar(p, pc, "ea_lanzar_sesion", json!({ "agente": "codex", "herramientas": [], "directorio": "/tmp" })).await;
    reg.llamar(p, pc, "ea_activar_herramienta", json!({ "elemento": "../../etc/passwd", "simular": true })).await;
    reg.llamar(p, pc, "ea_desactivar_herramienta", json!({ "elemento": "x" })).await;
    reg.llamar(x, xc, "ea_activar_herramienta", json!({ "elemento": "codex/mcp/usuario/docs" })).await;
    // Argumentos que no casan con el esquema.
    reg.llamar(p, pc, "ea_ver_trabajo", json!({})).await;
    reg.llamar(p, pc, "ea_encolar_rutina", json!({ "slug": 3 })).await;
    reg.llamar(p, pc, "ea_inexistente", json!({})).await;
    // El último vistazo ve las llamadas anteriores.
    reg.llamar(o, oc, "ea_status", json!({})).await;

    // El diario entero, del más antiguo al más reciente.
    let s = Store::open_at(&db).unwrap();
    let mut eventos = s.list_events(&EventFilter { limit: Some(1000), ..Default::default() }).unwrap();
    eventos.reverse();
    writeln!(reg.texto, "== diario").unwrap();
    for e in eventos {
        let linea = format!(
            "{} {} {} trabajo={:?} {:?} {}\n",
            e.id,
            e.actor,
            e.action,
            e.trabajo,
            e.outcome,
            e.detail.as_deref().unwrap_or_default()
        );
        reg.texto.push_str(&normalizar(&linea, &reg.tmp));
    }
    for c in [planificador, ejecutor, orquestador, desconocido] {
        c.cancel().await.ok();
    }

    let fichero = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fijado/mcp.txt");
    if std::env::var_os("EA_FIJAR").is_some() {
        fs::create_dir_all(fichero.parent().unwrap()).unwrap();
        fs::write(&fichero, &reg.texto).unwrap();
    }
    let esperado = fs::read_to_string(&fichero).unwrap_or_default();
    if let Some((n, (a, e))) = reg.texto.lines().zip(esperado.lines()).enumerate().find(|(_, (a, e))| a != e) {
        panic!("la línea {} cambió:\n  antes: {e}\n  ahora: {a}", n + 1);
    }
    assert_eq!(reg.texto.lines().count(), esperado.lines().count(), "cambió el número de líneas");
}

/// La credencial de las pruebas, la que manda `conectar`.
fn credencial() -> ea_mcp::Credencial {
    ea_mcp::Credencial::de_texto(ea_pruebas::mcp::CREDENCIAL).unwrap()
}
