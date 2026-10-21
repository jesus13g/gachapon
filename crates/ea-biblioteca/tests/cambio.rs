//! Pruebas de activar y desactivar sobre un HOME de `tempfile`: jamás se
//! toca el real. Los comandos oficiales se prueban con un doble.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use ea_ajustes::{Canal, Fuentes};
use ea_biblioteca::*;

struct Mundo {
    _d: tempfile::TempDir,
    home: PathBuf,
    proyecto: PathBuf,
    ctx: Contexto,
}

fn escribe(ruta: impl AsRef<Path>, texto: &str) {
    let ruta = ruta.as_ref();
    fs::create_dir_all(ruta.parent().unwrap()).unwrap();
    fs::write(ruta, texto).unwrap();
}

fn lee(ruta: impl AsRef<Path>) -> String {
    fs::read_to_string(ruta).unwrap()
}

const CODEX: &str = "# Configuración de Codex\nmodel = \"x\"  # modelo\n\n# El primero\n[mcp_servers.docs]\ncommand = \"docs-mcp\"  # comando\nargs = [\"--a\"]\n\n[mcp_servers.docs.env]\nTOKEN = \"t\"\n\n# El apagado\n[mcp_servers.apagado]\nurl = \"https://h.ejemplo.com/mcp\"\nenabled = false # a propósito\n\n[mcp_servers.ultimo]\ncommand = \"u\"";

const OPENCODE_USUARIO: &str = "{\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"mcp\": {\n    \"servers\": {\n      \"remoto\": {\n        \"type\": \"remote\",\n        \"url\": \"https://oc.ejemplo.com/mcp\"\n      },\n      \"local\": { \"type\": \"local\", \"command\": [\"bun\", \"x\"] }\n    }\n  },\n  \"theme\": \"dark\"\n}\n";

const OPENCODE_PROYECTO: &str = "{\"mcp\":{\"plano\":{\"type\":\"local\",\"command\":[\"p\"]}}}";

fn mundo() -> Mundo {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    let proyecto = d.path().join("proyecto");
    fs::create_dir_all(&proyecto).unwrap();
    let estado = d.path().join("estado");

    // Codex
    escribe(home.join(".codex/config.toml"), CODEX);
    fs::set_permissions(home.join(".codex/config.toml"), fs::Permissions::from_mode(0o600)).unwrap();
    escribe(home.join(".codex/skills/cx/SKILL.md"), "---\ndescription: Skill de Codex\n---\n");
    // OpenCode
    escribe(home.join(".config/opencode/opencode.json"), OPENCODE_USUARIO);
    escribe(proyecto.join("opencode.json"), OPENCODE_PROYECTO);
    escribe(home.join(".config/opencode/opencode.jsonc"), "{\n // comentario\n \"mcp\": { \"conc\": { \"type\": \"local\", \"command\": [\"x\"] } }\n}\n");
    escribe(home.join(".config/opencode/agent/oa.md"), "---\ndescription: Agente\n---\n");
    // Claude Code
    let repo = d.path().join("repo");
    escribe(repo.join("Cargo.toml"), "[workspace]\n");
    escribe(repo.join("claude/skills/mision/SKILL.md"), "---\ndescription: Misión\n---\n");
    escribe(repo.join("qwen/skills/depurar.md"), "---\ndescripcion: x\n---\n");
    fs::create_dir_all(home.join(".claude/skills")).unwrap();
    std::os::unix::fs::symlink(repo.join("claude/skills/mision"), home.join(".claude/skills/mision")).unwrap();
    escribe(home.join(".claude/skills/propia/SKILL.md"), "---\ndescription: Propia\n---\n");
    escribe(home.join(".claude/skills/propia/extra.txt"), "extra");
    escribe(home.join(".claude/agents/revisor.md"), "---\ndescription: Revisa\n---\n");
    escribe(home.join(".claude/commands/saludo.md"), "---\ndescription: Saluda\n---\n");
    escribe(proyecto.join(".claude/skills/local/SKILL.md"), "---\ndescription: Local\n---\n");
    escribe(
        home.join(".claude/plugins/installed_plugins.json"),
        r#"{"version":2,"plugins":{"foo@mkt":[{"scope":"user","installPath":"/no/existe","version":"1"}]}}"#,
    );
    escribe(home.join(".claude/settings.json"), r#"{"enabledPlugins":{"foo@mkt":true}}"#);
    escribe(home.join(".claude.json"), r#"{"mcpServers":{"ea-ecosistema":{"type":"http","url":"http://127.0.0.1:8765/mcp"}}}"#);
    // Qwen y Antigravity
    escribe(home.join(".qwen/extensions/ext1/qwen-extension.json"), r#"{"name":"ext1"}"#);
    escribe(home.join(".gemini/config/mcp_config.json"), r#"{"mcpServers":{"agy-mcp":{"serverUrl":"https://agy.ejemplo.com/x"}}}"#);
    // Skills del ejecutor y sus ajustes
    let skills = d.path().join("qwen-skills");
    escribe(skills.join("rust.md"), "---\ndescripcion: Rust\n---\n");
    escribe(skills.join("tests.md"), "---\ndescripcion: Tests\n---\n");
    let fuentes = Fuentes::aislada(d.path());
    escribe(fuentes.fichero(), &format!("# mis ajustes\n[agente.skills]\ndir = \"{}\"\n", skills.display()));

    let raices = Raices::nuevo(home.clone(), Some(proyecto.clone()), estado, None);
    let ctx = Contexto::nuevo(raices, fuentes, Canal::Terminal);
    Mundo { home, proyecto, ctx, _d: d }
}

fn copias(m: &Mundo) -> Vec<PathBuf> {
    let dir = m.ctx.raices.estado.join("biblioteca/copias");
    let mut v: Vec<PathBuf> = fs::read_dir(dir).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    v.sort();
    v
}

/// Planifica, aplica y devuelve el hecho.
fn cambiar(m: &Mundo, id: &str, activar: bool, o: &dyn Ordenes) -> Result<Hecho, ErrorBiblioteca> {
    let c = planear(id, activar, &m.ctx)?;
    aplicar_con(&c, &m.ctx, o)
}

fn inventario(m: &Mundo) -> Inventario {
    Biblioteca::cargar(&m.ctx.raices, &ea_ajustes::cargar_de(&m.ctx.fuentes).ajustes)
}

#[test]
fn codex_ida_y_vuelta_deja_el_fichero_igual_y_conserva_los_comentarios() {
    let m = mundo();
    let f = m.home.join(".codex/config.toml");
    let o = OrdenesDoble::default();
    let c = planear("codex/mcp/usuario/docs", false, &m.ctx).unwrap();
    assert_eq!(c.mecanismo, Mecanismo::Campo);
    assert_eq!((c.antes.clone(), c.despues.clone()), (serde_json::Value::Null, serde_json::json!(false)));
    assert!(matches!(&c.objetivo, Objetivo::Fichero { campo, .. } if campo == &["mcp_servers", "docs", "enabled"]));
    // Simular no escribe nada.
    assert_eq!(lee(&f), CODEX);
    let h = aplicar_con(&c, &m.ctx, &o).unwrap();
    let despues = lee(&f);
    assert!(despues.contains("[mcp_servers.docs]\ncommand = \"docs-mcp\"  # comando\nargs = [\"--a\"]\nenabled = false\n"), "{despues}");
    assert!(despues.contains("# Configuración de Codex") && despues.contains("# El apagado") && despues.contains("enabled = false # a propósito"));
    // Copia previa con el contenido original.
    let copia = h.copia.clone().unwrap();
    assert_eq!(lee(&copia), CODEX);
    assert!(copia.starts_with(m.ctx.raices.estado.join("biblioteca/copias")));
    assert!(copia.file_name().unwrap().to_str().unwrap().ends_with("-config.toml"));
    // Permisos conservados.
    assert_eq!(fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
    // La biblioteca lo refleja y activar deja los bytes originales.
    assert_eq!(inventario(&m).buscar("codex/mcp/usuario/docs").unwrap().activado, Activado::No);
    cambiar(&m, "codex/mcp/usuario/docs", true, &o).unwrap();
    assert_eq!(lee(&f), CODEX);
    assert_eq!(copias(&m).len(), 2);
}

#[test]
fn codex_activar_quita_enabled_false_y_el_ultimo_sin_salto_final_se_edita() {
    let m = mundo();
    let f = m.home.join(".codex/config.toml");
    let o = OrdenesDoble::default();
    cambiar(&m, "codex/mcp/usuario/apagado", true, &o).unwrap();
    let t = lee(&f);
    assert!(t.contains("url = \"https://h.ejemplo.com/mcp\"\n") && !t.contains("a propósito"), "{t}");
    // El último servidor no acaba en salto de línea.
    cambiar(&m, "codex/mcp/usuario/ultimo", false, &o).unwrap();
    let t = lee(&f);
    assert!(t.ends_with("command = \"u\"\nenabled = false"), "{t:?}");
    cambiar(&m, "codex/mcp/usuario/ultimo", true, &o).unwrap();
    let t = lee(&f);
    assert!(t.ends_with("[mcp_servers.ultimo]\ncommand = \"u\""), "{t:?}");
    // Y sigue siendo TOML válido con los tres servidores.
    let v: toml::Table = t.parse().unwrap();
    assert_eq!(v["mcp_servers"].as_table().unwrap().len(), 3);
}

#[test]
fn opencode_en_sus_dos_formas_ida_y_vuelta_byte_a_byte() {
    let m = mundo();
    let o = OrdenesDoble::default();
    let usuario = m.home.join(".config/opencode/opencode.json");
    let proyecto = m.proyecto.join("opencode.json");
    for (id, fichero, original) in [
        ("opencode/mcp/usuario/remoto", &usuario, OPENCODE_USUARIO),
        ("opencode/mcp/usuario/local", &usuario, OPENCODE_USUARIO),
        ("opencode/mcp/proyecto/plano", &proyecto, OPENCODE_PROYECTO),
    ] {
        let c = planear(id, false, &m.ctx).unwrap();
        assert!(matches!(&c.objetivo, Objetivo::Fichero { campo, .. } if campo.last().unwrap() == "enabled"));
        cambiar(&m, id, false, &o).unwrap();
        let t = lee(fichero);
        let v: serde_json::Value = serde_json::from_str(&t).expect("sigue siendo JSON válido");
        let nombre = id.rsplit('/').next().unwrap();
        let def = v["mcp"].get(nombre).or_else(|| v["mcp"]["servers"].get(nombre)).unwrap();
        assert_eq!(def["enabled"], false, "{id}: {t}");
        assert_ne!(t, original);
        cambiar(&m, id, true, &o).unwrap();
        assert_eq!(lee(fichero), original, "{id}");
    }
    // El resto del fichero de usuario (orden de claves, `$schema`, `theme`) no se movió.
    assert!(lee(&usuario).contains("\"$schema\"") && lee(&usuario).ends_with("\"theme\": \"dark\"\n}\n"));
}

#[test]
fn opencode_con_comentarios_se_rechaza() {
    let m = mundo();
    let e = planear("opencode/mcp/usuario/conc", false, &m.ctx).unwrap_err();
    assert!(matches!(e, ErrorBiblioteca::NoSePuede(_)), "{e}");
    assert!(e.to_string().starts_with("no se puede cambiar: "), "{e}");
    // Y si alguien fuerza el mecanismo, la comprobación del propio campo también lo para.
    let mut el = inventario(&m).buscar("opencode/mcp/usuario/conc").unwrap().clone();
    el.mecanismo = Mecanismo::Campo;
    let e = plan_cambio_con(&el, false, &m.ctx).unwrap_err();
    assert!(e.to_string().contains("JSONC con comentarios"), "{e}");
    assert!(lee(m.home.join(".config/opencode/opencode.jsonc")).contains("// comentario"));
}

#[test]
fn mover_skill_enlace_subagente_y_comando_ida_y_vuelta() {
    let m = mundo();
    let o = OrdenesDoble::default();
    let almacen = m.ctx.raices.estado.join("biblioteca/desactivadas");

    // Directorio con contenido.
    let propia = m.home.join(".claude/skills/propia");
    cambiar(&m, "claude-code/skill/usuario/propia", false, &o).unwrap();
    assert!(!propia.exists());
    assert_eq!(lee(almacen.join("claude-code/skill/propia/extra.txt")), "extra");
    let idx = lee(almacen.join("indice.json"));
    assert!(idx.contains("claude-code/skill/usuario/propia") && idx.contains(&*propia.to_string_lossy()), "{idx}");
    // Ya no está en el inventario, pero sí entre las desactivadas.
    let d = desactivadas(&m.ctx.raices);
    assert_eq!(d.len(), 1);
    assert_eq!((d[0].activado, d[0].mecanismo), (Activado::No, Mecanismo::Mover));
    cambiar(&m, "claude-code/skill/usuario/propia", true, &o).unwrap();
    assert_eq!(lee(propia.join("extra.txt")), "extra");
    assert!(desactivadas(&m.ctx.raices).is_empty());

    // Enlace simbólico (skill del ecosistema): se mueve el enlace, no el destino.
    let enlace = m.home.join(".claude/skills/mision");
    let destino = fs::read_link(&enlace).unwrap();
    cambiar(&m, "claude-code/skill/ecosistema/mision", false, &o).unwrap();
    assert!(fs::symlink_metadata(&enlace).is_err());
    let movido = almacen.join("claude-code/skill/mision");
    assert!(fs::symlink_metadata(&movido).unwrap().file_type().is_symlink());
    assert!(destino.join("SKILL.md").exists(), "el destino del enlace no se toca");
    cambiar(&m, "claude-code/skill/ecosistema/mision", true, &o).unwrap();
    assert!(fs::symlink_metadata(&enlace).unwrap().file_type().is_symlink());
    assert_eq!(fs::read_link(&enlace).unwrap(), destino);

    // Subagente y comando de fichero, y skills de otros agentes y de proyecto.
    for (id, ruta) in [
        ("claude-code/subagente/usuario/revisor", m.home.join(".claude/agents/revisor.md")),
        ("claude-code/comando/usuario/saludo", m.home.join(".claude/commands/saludo.md")),
        ("opencode/subagente/usuario/oa", m.home.join(".config/opencode/agent/oa.md")),
        ("codex/skill/usuario/cx", m.home.join(".codex/skills/cx")),
        ("claude-code/skill/proyecto/local", m.proyecto.join(".claude/skills/local")),
    ] {
        let antes = fs::read_dir(ruta.parent().unwrap()).unwrap().count();
        cambiar(&m, id, false, &o).unwrap();
        assert!(!ruta.exists(), "{id}");
        cambiar(&m, id, true, &o).unwrap();
        assert!(ruta.exists(), "{id}");
        assert_eq!(fs::read_dir(ruta.parent().unwrap()).unwrap().count(), antes);
    }
    assert!(desactivadas(&m.ctx.raices).is_empty());
}

#[test]
fn mover_con_el_destino_ocupado_se_rechaza() {
    let m = mundo();
    let o = OrdenesDoble::default();
    let propia = m.home.join(".claude/skills/propia");
    cambiar(&m, "claude-code/skill/usuario/propia", false, &o).unwrap();
    // Alguien creó otra cosa en el sitio original.
    escribe(propia.join("SKILL.md"), "---\ndescription: Otra\n---\n");
    let e = planear("claude-code/skill/usuario/propia", true, &m.ctx).unwrap_err();
    assert!(matches!(e, ErrorBiblioteca::Ocupado(_)), "{e}");
    assert!(lee(propia.join("SKILL.md")).contains("Otra"));
    assert_eq!(desactivadas(&m.ctx.raices).len(), 1, "sigue guardada");
    // Y desactivar otra con el mismo nombre en el almacén (p. ej. de proyecto) también.
    escribe(m.proyecto.join(".claude/skills/propia/SKILL.md"), "---\ndescription: De proyecto\n---\n");
    let e = planear("claude-code/skill/proyecto/propia", false, &m.ctx).unwrap_err();
    assert!(matches!(e, ErrorBiblioteca::Ocupado(_)), "{e}");
}

#[test]
fn la_revalidacion_falla_si_el_fichero_cambio_entre_simular_y_aplicar() {
    let m = mundo();
    let f = m.home.join(".codex/config.toml");
    let o = OrdenesDoble::default();
    let c = planear("codex/mcp/usuario/docs", false, &m.ctx).unwrap();
    let cambiado = format!("{CODEX}\n# alguien editó\n");
    fs::write(&f, &cambiado).unwrap();
    let e = aplicar_con(&c, &m.ctx, &o).unwrap_err();
    assert!(matches!(e, ErrorBiblioteca::Revalidacion(_)), "{e}");
    assert_eq!(lee(&f), cambiado, "no se escribió nada");
    assert!(copias(&m).is_empty());

    // El campo cambió: el valor de «antes» ya no es el simulado.
    fs::write(&f, CODEX).unwrap();
    let c = planear("codex/mcp/usuario/docs", false, &m.ctx).unwrap();
    fs::write(&f, CODEX.replace("[mcp_servers.docs]\n", "[mcp_servers.docs]\nenabled = false\n")).unwrap();
    assert!(matches!(aplicar_con(&c, &m.ctx, &o), Err(ErrorBiblioteca::Revalidacion(_))));

    // Mover: el origen desapareció.
    let c = planear("claude-code/skill/usuario/propia", false, &m.ctx).unwrap();
    fs::remove_dir_all(m.home.join(".claude/skills/propia")).unwrap();
    assert!(matches!(aplicar_con(&c, &m.ctx, &o), Err(ErrorBiblioteca::Revalidacion(_))));
}

#[test]
fn comandos_oficiales_con_el_argv_exacto() {
    let m = mundo();
    let o = OrdenesDoble::default();
    let h = cambiar(&m, "claude-code/plugin/usuario/foo@mkt", false, &o).unwrap();
    assert_eq!(h.mecanismo, Mecanismo::ComandoOficial);
    assert_eq!((h.antes.clone(), h.despues.clone()), (serde_json::json!("si"), serde_json::json!("no")));
    assert!(h.copia.is_none());
    cambiar(&m, "antigravity/mcp/usuario/agy-mcp", false, &o).unwrap();
    cambiar(&m, "qwen-code/extension/usuario/ext1", false, &o).unwrap();
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        o.argvs(),
        vec![
            v(&["claude", "plugin", "disable", "--scope", "user", "foo@mkt"]),
            v(&["agy", "mcp", "disable", "agy-mcp"]),
            v(&["qwen", "extensions", "disable", "--scope", "User", "ext1"]),
        ]
    );
    assert_eq!(o.llamadas.lock().unwrap()[0].1.as_deref(), Some(m.home.as_path()));
    // Activar usa «enable» (el doble no cambia los ficheros, así que se planea a mano).
    let mut el = inventario(&m).buscar("claude-code/plugin/usuario/foo@mkt").unwrap().clone();
    el.activado = Activado::No;
    let c = plan_cambio_con(&el, true, &m.ctx).unwrap();
    assert!(matches!(&c.objetivo, Objetivo::Orden { argv, .. } if argv[2] == "enable"));
    // El doble no toca los ficheros de Claude.
    assert_eq!(lee(m.home.join(".claude/settings.json")), r#"{"enabledPlugins":{"foo@mkt":true}}"#);
}

#[test]
fn comando_sin_binario_o_que_falla_es_error_claro() {
    let m = mundo();
    let sin = OrdenesDoble::default().solo_con(&["agy"]);
    let e = cambiar(&m, "claude-code/plugin/usuario/foo@mkt", false, &sin).unwrap_err();
    assert_eq!(e, ErrorBiblioteca::BinarioNoDisponible("claude".into()));
    assert!(e.to_string().contains("binario no disponible"));
    assert!(sin.argvs().is_empty());
    let falla = OrdenesDoble::default().fallando(1, "plugin no encontrado");
    let e = cambiar(&m, "qwen-code/extension/usuario/ext1", false, &falla).unwrap_err();
    assert!(matches!(&e, ErrorBiblioteca::Orden(t) if t.contains("plugin no encontrado")), "{e}");
}

#[test]
fn solo_sesion_y_sin_interruptor_se_rechazan_y_claude_json_no_se_toca() {
    let m = mundo();
    let claude_json = lee(m.home.join(".claude.json"));
    let e = planear("claude-code/mcp/usuario/ea-ecosistema", false, &m.ctx).unwrap_err();
    assert!(e.to_string().starts_with("no se puede cambiar: ") && e.to_string().contains("sesión"), "{e}");
    // Sin interruptor (la skill del ecosistema de origen) y ya en ese estado.
    let e = planear("ecosistema/skill/ecosistema/mision", false, &m.ctx).unwrap_err();
    assert!(matches!(e, ErrorBiblioteca::NoSePuede(_)), "{e}");
    let e = planear("codex/mcp/usuario/apagado", false, &m.ctx).unwrap_err();
    assert!(e.to_string().contains("ya está desactivado"), "{e}");
    assert!(matches!(planear("codex/mcp/usuario/no-existe", false, &m.ctx), Err(ErrorBiblioteca::NoExiste(_))));
    assert!(matches!(planear("esto no es un id", false, &m.ctx), Err(ErrorBiblioteca::IdInvalido { .. })));
    assert_eq!(lee(m.home.join(".claude.json")), claude_json);
}

#[test]
fn skills_del_ejecutor_por_ajustes_ida_y_vuelta() {
    let m = mundo();
    let o = OrdenesDoble::default();
    let f = m.ctx.fuentes.fichero();
    let original = lee(&f);
    // Vacío = todas habilitadas: desactivar una deja las demás.
    let c = planear("qwen-code/skill/ecosistema/tests", false, &m.ctx).unwrap();
    assert_eq!(c.mecanismo, Mecanismo::Ajustes);
    assert_eq!((c.antes.clone(), c.despues.clone()), (serde_json::json!([]), serde_json::json!(["rust"])));
    // Sin terminal real, ea-ajustes lo rechaza y no escribe nada.
    let web = Contexto::nuevo(m.ctx.raices.clone(), m.ctx.fuentes.clone(), Canal::Web);
    let e = aplicar_con(&c, &web, &o).unwrap_err();
    assert!(matches!(e, ErrorBiblioteca::Ajustes(_)), "{e}");
    assert_eq!(lee(&f), original);
    // Desde terminal sí.
    aplicar_con(&c, &m.ctx, &o).unwrap();
    assert!(lee(&f).contains("habilitadas = [\"rust\"]") && lee(&f).starts_with("# mis ajustes"), "{}", lee(&f));
    // La única habilitada no se puede desactivar.
    let e = planear("qwen-code/skill/ecosistema/rust", false, &m.ctx).unwrap_err();
    assert!(e.to_string().contains("única"), "{e}");
    // Activar la que falta equivale a «todas»: la clave se quita y el fichero vuelve a ser el original.
    cambiar(&m, "qwen-code/skill/ecosistema/tests", true, &o).unwrap();
    assert_eq!(lee(&f), original);
}

#[test]
fn escribe_el_fichero_real_si_la_configuracion_es_un_enlace() {
    let m = mundo();
    let o = OrdenesDoble::default();
    let f = m.home.join(".codex/config.toml");
    let real = m.home.join("dotfiles/codex.toml");
    fs::create_dir_all(real.parent().unwrap()).unwrap();
    fs::rename(&f, &real).unwrap();
    std::os::unix::fs::symlink(&real, &f).unwrap();
    cambiar(&m, "codex/mcp/usuario/docs", false, &o).unwrap();
    assert!(fs::symlink_metadata(&f).unwrap().file_type().is_symlink(), "el enlace sigue siendo un enlace");
    assert!(lee(&real).contains("enabled = false"));
    cambiar(&m, "codex/mcp/usuario/docs", true, &o).unwrap();
    assert_eq!(lee(&real), CODEX);
}
