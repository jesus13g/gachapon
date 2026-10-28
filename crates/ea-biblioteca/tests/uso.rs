//! Pruebas del uso (F3): todo en directorios de `tempfile`, nunca el HOME real.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use ea_biblioteca::uso::{calcular_uso, FuentesUso};
use ea_biblioteca::{Ambito, Elemento, Inventario, Raices, Tipo};
use ea_pruebas::escribe;

const SECRETO: &str = "TEXTO_PRIVADO_DEL_MENSAJE_9f3a";

struct Mundo {
    _d: tempfile::TempDir,
    raiz: PathBuf,
    raices: Raices,
    fuentes: FuentesUso,
    inv: Inventario,
}

fn elem(agente: &str, tipo: Tipo, ambito: Ambito, nombre: &str) -> Elemento {
    Elemento::nuevo(agente, tipo, ambito, nombre, PathBuf::from("/x")).unwrap()
}

fn mundo() -> Mundo {
    let d = tempfile::tempdir().unwrap();
    let raiz = d.path().to_path_buf();
    let home = raiz.join("home");
    let estado = raiz.join("estado");
    fs::create_dir_all(&home).unwrap();
    let raices = Raices::nuevo(home, None, estado, None);
    let mut fuentes = FuentesUso::desde_raices(&raices);
    fuentes.diario = raiz.join("diario.db");
    let inv = Inventario {
        elementos: vec![
            elem("claude-code", Tipo::Skill, Ambito::Usuario, "x"),
            elem("claude-code", Tipo::Skill, Ambito::Plugin("pl@mkt".into()), "dentro"),
            elem("claude-code", Tipo::Mcp, Ambito::Usuario, "srv"),
            elem("claude-code", Tipo::Subagente, Ambito::Usuario, "revisor"),
            elem("claude-code", Tipo::Plugin, Ambito::Usuario, "pl@mkt"),
            elem("ecosistema", Tipo::Mcp, Ambito::Ecosistema, "ea-ecosistema"),
            elem("qwen-code", Tipo::Skill, Ambito::Ecosistema, "depurar"),
        ],
        avisos: vec![],
    };
    Mundo { _d: d, raiz, raices, fuentes, inv }
}

fn linea(ts: &str, bloques: &str) -> String {
    format!(
        r#"{{"type":"assistant","timestamp":"{ts}","message":{{"role":"assistant","content":[{{"type":"text","text":"{SECRETO}"}},{bloques}]}}}}"#
    ) + "\n"
}

fn tool(nombre: &str, entrada: &str) -> String {
    format!(r#"{{"type":"tool_use","id":"toolu_1","name":"{nombre}","input":{entrada}}}"#)
}

fn añade(p: &Path, t: &str) {
    let mut f = fs::OpenOptions::new().append(true).open(p).unwrap();
    f.write_all(t.as_bytes()).unwrap();
}

fn transcripcion(m: &Mundo) -> PathBuf {
    m.raices.home.join(".claude/projects/-p/sesion1.jsonl")
}

fn contenido_basico() -> String {
    let mut t = String::new();
    t += &linea("2026-10-01T10:00:00.000Z", &tool("Skill", &format!(r#"{{"skill":"x","args":"{SECRETO}"}}"#)));
    t += &linea("2026-10-02T10:00:00.000Z", &tool("mcp__srv__una", "{}"));
    t += &linea("2026-10-03T10:00:00.000Z", &tool("mcp__srv__otra", "{}"));
    t += &linea("2026-10-04T10:00:00.000Z", &tool("Agent", &format!(r#"{{"subagent_type":"revisor","prompt":"{SECRETO}"}}"#)));
    t += &linea("2026-10-05T10:00:00.000Z", &tool("mcp__claude_ai_Gmail__buscar", "{}"));
    t += &linea("2026-10-05T11:00:00.000Z", &tool("Read", "{}"));
    t
}

#[test]
fn cuenta_skill_mcp_y_agente_de_una_transcripcion() {
    let m = mundo();
    escribe(transcripcion(&m), &contenido_basico());
    // Y una de subagente.
    escribe(
        m.raices.home.join(".claude/projects/-p/sesion1/subagents/agent-a.jsonl"),
        &linea("2026-10-06T10:00:00.000Z", &tool("Skill", r#"{"skill":"pl:dentro"}"#)),
    );
    let r = calcular_uso(&m.raices, &m.inv, &m.fuentes);

    let x = r.buscar("claude-code/skill/usuario/x").unwrap();
    assert_eq!((x.total, x.ultimo.as_deref()), (1, Some("2026-10-01T10:00:00Z")));
    assert_eq!(x.por[0].actor, "claude-code");
    let srv = r.buscar("claude-code/mcp/usuario/srv").unwrap();
    assert_eq!(srv.total, 2);
    assert_eq!(srv.ultimo.as_deref(), Some("2026-10-03T10:00:00Z"));
    assert_eq!(r.buscar("claude-code/subagente/usuario/revisor").unwrap().total, 1);
    // Skill de un plugin, con el prefijo `plugin:skill`.
    assert_eq!(r.buscar("claude-code/skill/plugin:pl@mkt/dentro").unwrap().total, 1);
    // Lo que no está en el inventario va aparte, y las herramientas ajenas a MCP/Skill/Agent no cuentan.
    assert_eq!(r.sin_elemento.len(), 1);
    assert_eq!(r.sin_elemento[0].nombre, "claude_ai_Gmail");
    assert_eq!(r.sin_elemento[0].tipo, Tipo::Mcp);
    assert!(r.actividad.iter().any(|a| a.agente == "claude-code" && a.ultima.is_some()));
    assert!(r.avisos.is_empty(), "{:?}", r.avisos);
}

#[test]
fn el_informe_no_contiene_texto_de_los_mensajes() {
    let m = mundo();
    escribe(transcripcion(&m), &contenido_basico());
    let r = calcular_uso(&m.raices, &m.inv, &m.fuentes);
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains(SECRETO));
    // Ni la caché en disco.
    let cache = fs::read_to_string(m.raices.estado.join("biblioteca/cache-uso.json")).unwrap();
    assert!(!cache.contains(SECRETO));
    assert!(cache.contains("srv"));
}

#[test]
fn la_cache_es_incremental() {
    let m = mundo();
    let t = transcripcion(&m);
    escribe(&t, &contenido_basico());
    let r1 = calcular_uso(&m.raices, &m.inv, &m.fuentes);
    assert_eq!(r1.estadisticas.lineas_leidas, 6);
    assert_eq!(r1.estadisticas.ficheros_releidos, 1);

    // Sin cambios: no se lee ninguna línea y el resultado es el mismo.
    let r2 = calcular_uso(&m.raices, &m.inv, &m.fuentes);
    assert_eq!(r2.estadisticas.lineas_leidas, 0);
    assert_eq!(serde_json::to_string(&r1.usos).unwrap(), serde_json::to_string(&r2.usos).unwrap());

    // Crece: solo se leen las líneas nuevas.
    añade(&t, &linea("2026-10-09T10:00:00.000Z", &tool("mcp__srv__mas", "{}")));
    añade(&t, &linea("2026-10-09T11:00:00.000Z", &tool("Skill", r#"{"skill":"x"}"#)));
    let r3 = calcular_uso(&m.raices, &m.inv, &m.fuentes);
    assert_eq!(r3.estadisticas.lineas_leidas, 2);
    assert_eq!(r3.estadisticas.ficheros_incrementales, 1);
    assert_eq!(r3.buscar("claude-code/mcp/usuario/srv").unwrap().total, 3);
    let x = r3.buscar("claude-code/skill/usuario/x").unwrap();
    assert_eq!((x.total, x.ultimo.as_deref()), (2, Some("2026-10-09T11:00:00Z")));

    // Una línea a medias no se consume hasta que se completa.
    añade(&t, r#"{"type":"assistant","timestamp":"2026-10-10T10:00:00.000Z","message":{"content":["#);
    let r4 = calcular_uso(&m.raices, &m.inv, &m.fuentes);
    assert_eq!(r4.estadisticas.lineas_leidas, 0);
    assert_eq!(r4.buscar("claude-code/skill/usuario/x").unwrap().total, 2);

    // Encoge (reescrito): se relee entero y no queda nada de antes.
    escribe(&t, &linea("2026-10-11T10:00:00.000Z", &tool("Skill", r#"{"skill":"x"}"#)));
    let r5 = calcular_uso(&m.raices, &m.inv, &m.fuentes);
    assert_eq!(r5.estadisticas.ficheros_releidos, 1);
    assert_eq!(r5.estadisticas.lineas_leidas, 1);
    assert_eq!(r5.buscar("claude-code/skill/usuario/x").unwrap().total, 1);
    assert!(r5.buscar("claude-code/mcp/usuario/srv").is_none());
}

#[test]
fn una_cache_corrupta_se_ignora() {
    let m = mundo();
    escribe(transcripcion(&m), &contenido_basico());
    escribe(m.raices.estado.join("biblioteca/cache-uso.json"), "{no es json");
    let r = calcular_uso(&m.raices, &m.inv, &m.fuentes);
    assert_eq!(r.buscar("claude-code/mcp/usuario/srv").unwrap().total, 2);
    assert_eq!(r.estadisticas.lineas_leidas, 6);
}

#[test]
fn claude_json_corrupto_avisa_y_no_rompe() {
    let m = mundo();
    escribe(&m.fuentes.claude_json, r#"{"skillUsage":{"x":{"usageCount":4,"lastUs"#);
    escribe(transcripcion(&m), &contenido_basico());
    let r = calcular_uso(&m.raices, &m.inv, &m.fuentes);
    assert_eq!(r.avisos.len(), 1);
    assert!(!r.avisos[0].contains("usageCount"));
    // Las transcripciones siguen contando.
    assert_eq!(r.buscar("claude-code/skill/usuario/x").unwrap().total, 1);
}

#[test]
fn claude_json_y_transcripciones_no_se_suman() {
    let m = mundo();
    escribe(
        &m.fuentes.claude_json,
        r#"{"skillUsage":{"x":{"usageCount":5,"lastUsedAt":1791408135818},"/pl:dentro":{"usageCount":0,"lastUsedAt":1790000000000}},
            "pluginUsage":{"pl@mkt":{"usageCount":3,"lastUsedAt":1791000000000}},
            "agentLastUsed":{"revisor":1790875531863,"desconocido":1790875531863},
            "mcpServers":{"srv":{"env":{"K":"SECRETO_NO_LEER"}}}}"#,
    );
    escribe(transcripcion(&m), &contenido_basico());
    let r = calcular_uso(&m.raices, &m.inv, &m.fuentes);
    let x = r.buscar("claude-code/skill/usuario/x").unwrap();
    // Manda el mayor (5 del contador), no 5 + 1.
    assert_eq!(x.total, 5);
    assert!(x.ultimo.as_deref().unwrap() > "2026-10-05T11:00:00Z");
    assert_eq!(r.buscar("claude-code/plugin/usuario/pl@mkt").unwrap().total, 3);
    let rev = r.buscar("claude-code/subagente/usuario/revisor").unwrap();
    assert_eq!(rev.total, 1);
    assert_eq!(rev.ultimo.as_deref(), Some("2026-10-04T10:00:00Z"));
    assert!(r.sin_elemento.iter().any(|s| s.nombre == "desconocido" && s.tipo == Tipo::Subagente));
    assert!(!serde_json::to_string(&r).unwrap().contains("SECRETO_NO_LEER"));
}

#[test]
fn el_diario_cuenta_los_mcp_por_actor() {
    let m = mundo();
    let c = rusqlite::Connection::open(&m.fuentes.diario).unwrap();
    c.execute_batch("CREATE TABLE events (id INTEGER PRIMARY KEY, ts TEXT, actor TEXT, action TEXT, trabajo TEXT, outcome TEXT, detail TEXT);").unwrap();
    let ins = |ts: &str, actor: &str, accion: &str, detalle: &str| {
        c.execute("INSERT INTO events (ts, actor, action, detail) VALUES (?1, ?2, ?3, ?4)", [ts, actor, accion, detalle]).unwrap();
    };
    ins("2026-10-01T10:00:00.123456789Z", "claude-code", "mcp_leer_nota", r#"{"args":{}}"#);
    ins("2026-10-02T10:00:00.5Z", "claude-code", "mcp_status", "no es json");
    ins("2026-10-03T10:00:00Z", "mcp", "mcp_leer_nota", r#"{"agente":"codex"}"#);
    ins("2026-10-04T10:00:00Z", "orquestador", "trabajo_creado", "{}");
    ins("2026-10-04T10:00:00Z", "x", "mcpXleer", "{}"); // el `_` de LIKE no es comodín
    drop(c);
    let r = calcular_uso(&m.raices, &m.inv, &m.fuentes);
    let u = r.buscar("ecosistema/mcp/ecosistema/ea-ecosistema").unwrap();
    assert_eq!(u.total, 3);
    assert_eq!(u.ultimo.as_deref(), Some("2026-10-03T10:00:00Z"));
    let por: Vec<(&str, u64)> = u.por.iter().map(|p| (p.actor.as_str(), p.usos)).collect();
    assert_eq!(por, vec![("claude-code", 2), ("codex", 1)]);
}

#[test]
fn las_skills_del_ejecutor_se_cuentan_por_trabajo() {
    let m = mundo();
    let t = m.raiz.join("estado/trabajos");
    // Entregó las skills: cuenta (y una skill repetida en orden y fase, una vez).
    escribe(t.join("a/orden.json"), r#"{"id":"a","skills":["depurar"],"fases":[{"skills":["depurar","otra"]}],"creada_en":"2026-10-07T08:00:00Z"}"#);
    escribe(t.join("a/skills-agente.md"), "# Skills\n");
    escribe(t.join("b/orden.json"), r#"{"id":"b","skills":["depurar"],"creada_en":"2026-10-08T08:00:00Z"}"#);
    escribe(t.join("b/skills-agente.md"), "x");
    // No llegó a ejecutarse: no cuenta.
    escribe(t.join("c/orden.json"), r#"{"id":"c","skills":["depurar"]}"#);
    // Orden ilegible: aviso.
    escribe(t.join("d/orden.json"), "{roto");
    escribe(t.join("d/skills-agente.md"), "x");
    let r = calcular_uso(&m.raices, &m.inv, &m.fuentes);
    let u = r.buscar("qwen-code/skill/ecosistema/depurar").unwrap();
    assert_eq!((u.total, u.por[0].actor.as_str()), (2, "ejecutor"));
    assert_eq!(u.ultimo.as_deref(), Some("2026-10-08T08:00:00Z"));
    assert!(r.sin_elemento.iter().any(|s| s.nombre == "otra"));
    assert_eq!(r.avisos.len(), 1);
}

#[test]
fn qwen_y_actividad_de_antigravity() {
    let m = mundo();
    let linea_qwen = |n: &str| {
        format!(r#"{{"type":"assistant","timestamp":"2026-10-12T10:00:00.000Z","message":{{"role":"model","parts":[{{"text":"{SECRETO}"}},{{"functionCall":{{"id":"1","name":"{n}","args":{{"c":"{SECRETO}"}}}}}}]}}}}"#) + "\n"
    };
    let mut inv = m.inv.clone();
    inv.elementos.push(elem("qwen-code", Tipo::Mcp, Ambito::Usuario, "srv"));
    escribe(
        m.raices.home.join(".qwen/projects/p/chats/s.jsonl"),
        &(linea_qwen("mcp__srv__cosa") + &linea_qwen("read_file")),
    );
    escribe(m.raices.home.join(".gemini/antigravity-cli/log/cli-1.log"), "x");
    let r = calcular_uso(&m.raices, &inv, &m.fuentes);
    let u = r.buscar("qwen-code/mcp/usuario/srv").unwrap();
    assert_eq!((u.total, u.por[0].actor.as_str()), (1, "qwen-code"));
    assert!(r.actividad.iter().any(|a| a.agente == "antigravity" && a.ultima.is_some()));
    assert!(r.actividad.iter().any(|a| a.agente == "codex" && a.ultima.is_none()));
    let agentes: Vec<_> = r.actividad.iter().map(|a| a.agente.as_str()).collect();
    assert_eq!(agentes, ["claude-code", "qwen-code", "codex", "opencode", "antigravity"]);
    assert!(!serde_json::to_string(&r).unwrap().contains(SECRETO));
}

#[test]
fn sin_nada_en_disco_el_informe_esta_vacio() {
    let m = mundo();
    let r = calcular_uso(&m.raices, &m.inv, &m.fuentes);
    assert!(r.usos.is_empty() && r.sin_elemento.is_empty() && r.avisos.is_empty());
}
