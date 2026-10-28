//! Los adaptadores de los agentes, el MCP y el hook de Claude Code, y las
//! skills del ejecutor (spec 023, decisiones 4 a 6).
//!
//! Solo se instala el adaptador de los agentes que están en el `PATH`; al
//! desinstalar se mira el de todos. Los adaptadores se copian siempre, con
//! la ruta real de las guías en lugar de [`recursos::MARCA_FLUJO`] (también
//! desde un clon: un enlace al repo dejaría el marcador sin sustituir). Nada
//! ajeno se pisa: los fragmentos de `AGENTS.md` van entre marcas, el
//! `settings.json` se fusiona (con copia `.bak`) y solo se borra lo que es
//! nuestro (un enlace a nuestro repo, de una instalación anterior, o un
//! fichero con nuestro contenido).

use std::path::{Path, PathBuf};

use ea_acciones::servicios::Paso;
use ea_ajustes::AgenteId;
use serde_json::{json, Map, Value};

use super::recursos::{self, reescribir_ruta_flujo, Recurso, MARCA_FLUJO};
use super::{Contexto, Origen, Sonda};

const MARCA_INICIO: &str = "<!-- gachapon:inicio -->";
/// La ruta de las guías que nombraban los adaptadores antes del marcador: un
/// fragmento añadido a mano con `cat >>` la lleva y se reconoce por ella.
const RUTA_FLUJO_HEREDADA: &str = "~/Proyectos/ecosistema-asistente/agentes/flujo/";
const MARCA_FIN: &str = "<!-- gachapon:fin -->";

/// Nombre del servidor MCP en Claude Code.
const NOMBRE_MCP: &str = "ea-ecosistema";

/// Una carpeta del repo que se instala bajo la carpeta de configuración del
/// agente. La unidad que se enlaza es el primer componente bajo `prefijo` (la
/// carpeta de la skill o el fichero suelto).
struct Montaje {
    prefijo: &'static str,
    destino: &'static str,
}

/// Un fragmento que se añade a un `AGENTS.md` de la carpeta del agente.
struct Fragmento {
    recurso: &'static str,
    destino: &'static str,
}

/// El adaptador de un agente del catálogo (tabla «Adaptadores del
/// planificador» del manual). El binario y la carpeta salen de su ficha.
struct Adaptador {
    id: AgenteId,
    /// Relativos a la carpeta de configuración del agente.
    montajes: &'static [Montaje],
    fragmentos: &'static [Fragmento],
}

impl Adaptador {
    /// El binario por defecto: si está en el `PATH`, se instala el adaptador.
    fn bin(&self) -> &'static str {
        self.id.ficha().bin_por_defecto
    }

    /// Su carpeta de configuración en el HOME.
    fn dir(&self, ctx: &Contexto) -> PathBuf {
        unir(&ctx.home, self.id.ficha().dir_config)
    }
}

const ADAPTADORES: &[Adaptador] = &[
    Adaptador {
        id: AgenteId::ClaudeCode,
        montajes: &[Montaje { prefijo: "claude/skills/", destino: "skills" }],
        fragmentos: &[],
    },
    Adaptador {
        id: AgenteId::Codex,
        montajes: &[Montaje { prefijo: "agentes/codex/skills/", destino: "skills" }],
        fragmentos: &[Fragmento { recurso: "agentes/codex/AGENTS.md", destino: "AGENTS.md" }],
    },
    Adaptador {
        id: AgenteId::OpenCode,
        montajes: &[Montaje { prefijo: "agentes/opencode/commands/", destino: "commands" }],
        fragmentos: &[Fragmento { recurso: "agentes/opencode/AGENTS.md", destino: "AGENTS.md" }],
    },
    Adaptador {
        id: AgenteId::Antigravity,
        montajes: &[
            Montaje { prefijo: "agentes/antigravity/skills/", destino: "config/skills" },
            Montaje { prefijo: "agentes/antigravity/rules/", destino: "config/rules" },
        ],
        fragmentos: &[],
    },
];

// ------------------------------------------------------------- ayudas

/// ¿Es `ruta` un enlace simbólico? Se mira el disco directamente (la
/// [`Sonda`] no distingue enlaces); en las pruebas las rutas no existen y
/// da `false`.
fn es_enlace(ruta: &Path) -> bool {
    std::fs::symlink_metadata(ruta).is_ok_and(|m| m.file_type().is_symlink())
}

/// Adónde apunta un enlace, si lo es.
fn destino_enlace(ruta: &Path) -> Option<PathBuf> {
    if es_enlace(ruta) { std::fs::read_link(ruta).ok() } else { None }
}

/// Une una ruta relativa con `/` a una base.
fn unir(base: &Path, relativa: &str) -> PathBuf {
    relativa.split('/').filter(|c| !c.is_empty()).fold(base.to_path_buf(), |p, c| p.join(c))
}

fn binario_presente(sonda: &dyn Sonda, bin: &str) -> bool {
    sonda.binario(bin).is_some()
}

/// La ruta de `ea` en el destino, con barras `/` (en Windows el JSON y el
/// shell del hook las entienden) y entre comillas si lleva espacios
/// (`~/Library/Application Support/…`).
fn orden_ea(ctx: &Contexto) -> PathBuf {
    let nombre = if ctx.sistema == ea_plataforma::Sistema::Windows { "ea.exe" } else { "ea" };
    ctx.destinos.bin.join(nombre)
}

/// `<ea> <argumentos>` como orden de shell (el hook y el `headersHelper`).
fn orden(ea: &Path, argumentos: &str) -> String {
    let ruta = ea.to_string_lossy().replace('\\', "/");
    let ruta = if ruta.contains(' ') { format!("\"{ruta}\"") } else { ruta };
    format!("{ruta} {argumentos}")
}

fn orden_hook(ea: &Path, accion: &str) -> String {
    orden(ea, &format!("hook subagentes {accion}"))
}

/// ¿Es `contenido` nuestro recurso (tal cual o con la ruta reescrita)?
fn es_nuestro(contenido: &str, r: &Recurso, ctx: &Contexto) -> bool {
    contenido == r.contenido || contenido == reescribir_ruta_flujo(r.contenido, &ctx.destinos.flujo)
}

/// Las unidades de un montaje: (nombre de la unidad, recursos que la forman).
fn unidades(m: &Montaje) -> Vec<(&'static str, Vec<&'static Recurso>)> {
    let mut v: Vec<(&'static str, Vec<&'static Recurso>)> = Vec::new();
    for r in recursos::bajo(m.prefijo) {
        let resto = &r.ruta[m.prefijo.len()..];
        let unidad = resto.split('/').next().unwrap_or(resto);
        match v.iter_mut().find(|(u, _)| *u == unidad) {
            Some((_, rs)) => rs.push(r),
            None => v.push((unidad, vec![r])),
        }
    }
    v
}

// ------------------------------------------------------------- fragmentos

fn bloque(fragmento: &str) -> String {
    format!("{MARCA_INICIO}\n{}\n{MARCA_FIN}\n", fragmento.trim_end())
}

/// Pone `fragmento` entre las marcas de gachapon. Si ya hay un bloque
/// marcado, lo sustituye; si el texto de alguna `alternativa` (el fragmento
/// añadido a mano con `cat >>`) está suelto, lo sustituye por el bloque; si
/// no, lo añade al final.
fn poner_fragmento_con(actual: Option<&str>, fragmento: &str, alternativas: &[&str]) -> String {
    let nuevo = bloque(fragmento);
    let actual = actual.unwrap_or("");
    if let (Some(i), Some(f)) = (actual.find(MARCA_INICIO), actual.find(MARCA_FIN))
        && f > i
    {
        let mut fin = f + MARCA_FIN.len();
        if actual[fin..].starts_with('\n') {
            fin += 1;
        }
        return format!("{}{nuevo}{}", &actual[..i], &actual[fin..]);
    }
    for alt in alternativas.iter().chain(std::iter::once(&fragmento)) {
        let suelto = alt.trim_end();
        if !suelto.is_empty()
            && let Some(i) = actual.find(suelto)
        {
            let mut fin = i + suelto.len();
            if actual[fin..].starts_with('\n') {
                fin += 1;
            }
            return format!("{}{nuevo}{}", &actual[..i], &actual[fin..]);
        }
    }
    if actual.is_empty() {
        nuevo
    } else if actual.ends_with("\n\n") {
        format!("{actual}{nuevo}")
    } else if actual.ends_with('\n') {
        format!("{actual}\n{nuevo}")
    } else {
        format!("{actual}\n\n{nuevo}")
    }
}

/// Pone `fragmento` entre `<!-- gachapon:inicio -->` y `<!-- gachapon:fin -->`:
/// sustituye el bloque si ya está (o el mismo texto suelto, añadido a mano)
/// y si no lo añade al final. Poner dos veces deja lo mismo que una. El
/// plan usa [`poner_fragmento_con`]; esta forma corta solo la usan las pruebas.
#[cfg(test)]
pub fn poner_fragmento(actual: Option<&str>, fragmento: &str) -> String {
    poner_fragmento_con(actual, fragmento, &[])
}

/// Quita el bloque marcado (y la línea en blanco que lo separaba). `None`
/// si no había.
pub fn quitar_fragmento(actual: &str) -> Option<String> {
    let i = actual.find(MARCA_INICIO)?;
    let f = actual[i..].find(MARCA_FIN)? + i;
    let mut fin = f + MARCA_FIN.len();
    if actual[fin..].starts_with('\n') {
        fin += 1;
    }
    let antes = &actual[..i];
    let antes = if antes.ends_with("\n\n") { &antes[..antes.len() - 1] } else { antes };
    Some(format!("{antes}{}", &actual[fin..]))
}

// ------------------------------------------------------------- hook

fn objeto<'a>(v: &'a mut Value, que: &str) -> Result<&'a mut Map<String, Value>, String> {
    v.as_object_mut().ok_or_else(|| format!("{que} no es un objeto JSON"))
}

fn analizar(settings: Option<&str>) -> Result<Value, String> {
    match settings.map(str::trim) {
        None | Some("") => Ok(json!({})),
        Some(t) => serde_json::from_str(t).map_err(|e| format!("settings.json no es JSON válido: {e}")),
    }
}

fn ordenes(entrada: &Value) -> impl Iterator<Item = &str> {
    entrada.get("hooks").and_then(Value::as_array).into_iter().flatten().filter_map(|h| h.get("command").and_then(Value::as_str))
}

fn serializar(v: &Value) -> Result<String, String> {
    serde_json::to_string_pretty(v).map(|t| t + "\n").map_err(|e| e.to_string())
}

/// El envoltorio de shell que registraban las instalaciones antiguas
/// (`claude/hooks/ea-subagentes.sh`, ya retirado del repo).
const HOOK_HEREDADO: &str = "ea-subagentes.sh";

/// Añade a `hooks.SubagentStart` y `hooks.SubagentStop` la orden
/// `<ea> hook subagentes start|stop` si no hay ya una. Una orden del
/// envoltorio antiguo `ea-subagentes.sh` se cambia por la nuestra (o se
/// quita, si ya estaba también la nuestra), con su entrada y su `timeout`.
/// Conserva todo lo demás; `serde_json` sin `preserve_order` deja las claves
/// en orden alfabético. `None`: no cambia nada; `Err`: el JSON no es válido
/// y no se toca.
pub fn fusionar_hook(settings: Option<&str>, ea: &Path) -> Result<Option<String>, String> {
    let mut v = analizar(settings)?;
    let raiz = objeto(&mut v, "settings.json")?;
    let hooks = raiz.entry("hooks").or_insert_with(|| json!({}));
    let hooks = objeto(hooks, "hooks")?;
    let mut cambia = false;
    for (evento, accion) in [("SubagentStart", "start"), ("SubagentStop", "stop")] {
        let lista = hooks.entry(evento).or_insert_with(|| json!([]));
        let lista = lista.as_array_mut().ok_or_else(|| format!("hooks.{evento} no es una lista"))?;
        let buscada = format!("hook subagentes {accion}");
        let mut ya = lista.iter().any(|e| ordenes(e).any(|o| o.contains(&buscada)));
        for e in lista.iter_mut() {
            let Some(hs) = e.get_mut("hooks").and_then(Value::as_array_mut) else { continue };
            hs.retain_mut(|h| {
                if !h.get("command").and_then(Value::as_str).is_some_and(|o| o.contains(HOOK_HEREDADO)) {
                    return true;
                }
                cambia = true;
                if ya {
                    return false;
                }
                h["command"] = json!(orden_hook(ea, accion));
                ya = true;
                true
            });
        }
        lista.retain(|e| e.get("hooks").and_then(Value::as_array).is_none_or(|hs| !hs.is_empty()));
        if !ya {
            lista.push(json!({"matcher": "*", "hooks": [{"type": "command", "command": orden_hook(ea, accion), "timeout": 5}]}));
            cambia = true;
        }
    }
    if cambia { serializar(&v).map(Some) } else { Ok(None) }
}

/// Quita solo las entradas de `ea hook subagentes` (las demás, también el
/// envoltorio puesto a mano, se quedan). Las listas que quedan vacías, y
/// `hooks` si queda vacío, desaparecen. `None` si no había nada nuestro.
pub fn quitar_hook(settings: &str) -> Result<Option<String>, String> {
    let mut v = analizar(Some(settings))?;
    let raiz = objeto(&mut v, "settings.json")?;
    let Some(hooks) = raiz.get_mut("hooks").and_then(Value::as_object_mut) else { return Ok(None) };
    let mut cambia = false;
    for (evento, accion) in [("SubagentStart", "start"), ("SubagentStop", "stop")] {
        let buscada = format!("hook subagentes {accion}");
        if let Some(lista) = hooks.get_mut(evento).and_then(Value::as_array_mut) {
            // Dentro de cada entrada solo se quitan nuestras órdenes; la
            // entrada desaparece solo si se queda sin ninguna.
            for e in lista.iter_mut() {
                if let Some(hs) = e.get_mut("hooks").and_then(Value::as_array_mut) {
                    let antes = hs.len();
                    hs.retain(|h| !h.get("command").and_then(Value::as_str).is_some_and(|o| o.contains(&buscada)));
                    cambia |= hs.len() != antes;
                }
            }
            let antes = lista.len();
            lista.retain(|e| e.get("hooks").and_then(Value::as_array).is_none_or(|hs| !hs.is_empty()));
            if lista.len() != antes && lista.is_empty() {
                hooks.remove(evento);
            }
        }
    }
    if !cambia {
        return Ok(None);
    }
    if hooks.is_empty() {
        raiz.remove("hooks");
    }
    serializar(&v).map(Some)
}

fn ruta_settings(ctx: &Contexto) -> PathBuf {
    unir(&ctx.home, AgenteId::ClaudeCode.ficha().dir_config).join("settings.json")
}

/// El enlace `~/.claude/hooks/ea-subagentes.sh` que ponían las
/// instalaciones antiguas, si apunta a nuestro envoltorio (ya no existe en el
/// repo: el enlace quedaría roto).
fn enlace_hook_heredado(ctx: &Contexto) -> Option<PathBuf> {
    let enlace = unir(&ctx.home, AgenteId::ClaudeCode.ficha().dir_config).join("hooks").join(HOOK_HEREDADO);
    destino_enlace(&enlace).is_some_and(|d| d.ends_with(Path::new("claude").join("hooks").join(HOOK_HEREDADO))).then_some(enlace)
}

// ------------------------------------------------------------- planes

/// `dir`: la carpeta de configuración del agente. Una carpeta real ajena
/// no se toca: se escribe dentro solo nuestro fichero.
fn plan_montaje(ctx: &Contexto, sonda: &dyn Sonda, dir: &Path, m: &Montaje, p: &mut Vec<Paso>) {
    let base = unir(dir, m.destino);
    for (unidad, rs) in unidades(m) {
        let destino = base.join(unidad);
        // Un enlace (el de una instalación anterior desde un clon, o a otro
        // sitio) se quita antes de escribir: si no, se escribiría a través
        // de él, dentro del repo enlazado.
        if es_enlace(&destino) {
            p.push(Paso::Borrar(destino.clone()));
        }
        for r in rs {
            let ruta = unir(&base, &r.ruta[m.prefijo.len()..]);
            let contenido = reescribir_ruta_flujo(r.contenido, &ctx.destinos.flujo);
            if sonda.leer(&ruta).as_deref() != Some(contenido.as_str()) {
                p.push(Paso::Escribir { ruta, contenido });
            }
        }
    }
}

fn plan_fragmento(ctx: &Contexto, sonda: &dyn Sonda, dir: &Path, f: &Fragmento, p: &mut Vec<Paso>) {
    let Some(r) = recursos::recurso(f.recurso) else { return };
    let ruta = unir(dir, f.destino);
    let Ok(actual) = super::leer_para_reescribir(sonda, &ruta) else { return };
    let fragmento = reescribir_ruta_flujo(r.contenido, &ctx.destinos.flujo);
    let heredado = r.contenido.replace(MARCA_FLUJO, RUTA_FLUJO_HEREDADA);
    let nuevo = poner_fragmento_con(actual.as_deref(), &fragmento, &[r.contenido, &heredado]);
    if actual.as_deref() != Some(nuevo.as_str()) {
        p.push(Paso::EditarPersonal { ruta, contenido: nuevo });
    }
}

/// El registro del MCP en Claude Code (ámbito de usuario): la URL y el
/// `headersHelper` que entrega la credencial del planificador en cada
/// conexión (`ea mcp cabeceras`, U-02). La credencial no se escribe nunca en
/// `~/.claude.json`: el sandbox del ejecutor puede leer el HOME, pero no el
/// directorio de ejecución donde está.
fn registro_mcp(ctx: &Contexto) -> Value {
    json!({
        "type": "http",
        "url": format!("http://127.0.0.1:{}/mcp", ctx.puerto_mcp),
        "headersHelper": orden(&orden_ea(ctx), "mcp cabeceras"),
    })
}

/// ¿Está ya en `~/.claude.json` nuestro registro, con la URL y el
/// `headersHelper` de ahora?
fn registro_mcp_al_dia(ctx: &Contexto, sonda: &dyn Sonda) -> bool {
    let quiero = registro_mcp(ctx);
    let Some(v) = sonda.leer(&ctx.home.join(".claude.json")).and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { return false };
    let hay = &v["mcpServers"][NOMBRE_MCP];
    ["url", "headersHelper"].iter().all(|k| hay[*k] == quiero[*k])
}

fn plan_claude(ctx: &Contexto, sonda: &dyn Sonda, p: &mut Vec<Paso>) {
    if !registro_mcp_al_dia(ctx, sonda) {
        // Uno anterior (sin `headersHelper`, o con otra URL o binario) se
        // quita antes de volver a añadirlo: `add-json` no lo sustituye.
        if sonda.salida(&["claude", "mcp", "get", NOMBRE_MCP]).is_some() {
            let argv = ["claude", "mcp", "remove", "--scope", "user", NOMBRE_MCP];
            p.push(Paso::Ejecutar { argv: argv.iter().map(|s| s.to_string()).collect(), tolerar: true, reintentos: 0 });
        }
        let registro = registro_mcp(ctx).to_string();
        let argv = ["claude", "mcp", "add-json", "--scope", "user", NOMBRE_MCP, &registro];
        p.push(Paso::Ejecutar { argv: argv.iter().map(|s| s.to_string()).collect(), tolerar: false, reintentos: 0 });
    }
    let ruta = ruta_settings(ctx);
    let Ok(actual) = super::leer_para_reescribir(sonda, &ruta) else { return };
    if let Ok(Some(nuevo)) = fusionar_hook(actual.as_deref(), &orden_ea(ctx)) {
        if actual.is_some() {
            p.push(Paso::Copiar { origen: ruta.clone(), destino: ruta.with_extension("json.gachapon.bak") });
        }
        p.push(Paso::EditarPersonal { ruta, contenido: nuevo });
    }
    if let Some(enlace) = enlace_hook_heredado(ctx) {
        p.push(Paso::Borrar(enlace));
    }
}

fn plan_skills_ejecutor(ctx: &Contexto, sonda: &dyn Sonda, p: &mut Vec<Paso>) {
    if let Origen::Clon { repo } = &ctx.origen
        && !sonda.existe(&ctx.skills_dir)
    {
        p.push(Paso::Enlazar { origen: repo.join("qwen").join("skills"), destino: ctx.skills_dir.clone() });
        return;
    }
    for r in recursos::bajo("qwen/skills/") {
        let ruta = ctx.skills_dir.join(&r.ruta["qwen/skills/".len()..]);
        if sonda.leer(&ruta).as_deref() != Some(r.contenido) {
            p.push(Paso::Escribir { ruta, contenido: r.contenido.to_string() });
        }
    }
}

/// Los pasos para instalar el adaptador de cada agente presente, el MCP y
/// el hook de Claude Code, y las skills del ejecutor. Lo que ya está igual
/// no genera pasos (repetir es idempotente).
pub fn plan_agentes(ctx: &Contexto, sonda: &dyn Sonda) -> Result<Vec<Paso>, String> {
    let mut p = Vec::new();
    for a in ADAPTADORES.iter().filter(|a| binario_presente(sonda, a.bin())) {
        let dir = a.dir(ctx);
        for m in a.montajes {
            plan_montaje(ctx, sonda, &dir, m, &mut p);
        }
        for f in a.fragmentos {
            plan_fragmento(ctx, sonda, &dir, f, &mut p);
        }
        if a.id == AgenteId::ClaudeCode {
            plan_claude(ctx, sonda, &mut p);
        }
    }
    plan_skills_ejecutor(ctx, sonda, &mut p);
    Ok(p)
}

/// Lo que la CLI tiene que enseñar además del plan: lo que no se pudo
/// planificar y queda en manos del usuario.
pub fn avisos_agentes(ctx: &Contexto, sonda: &dyn Sonda) -> Vec<String> {
    let mut v = Vec::new();
    if binario_presente(sonda, "claude") {
        let ruta = ruta_settings(ctx);
        let leido = super::leer_para_reescribir(sonda, &ruta);
        if let Err(e) = leido.and_then(|t| fusionar_hook(t.as_deref(), &orden_ea(ctx))) {
            v.push(format!(
                "{}: {e}. No se toca; añade a mano el hook de subagentes (SubagentStart/SubagentStop) con «{}» y «{}».",
                ruta.display(),
                orden_hook(&orden_ea(ctx), "start"),
                orden_hook(&orden_ea(ctx), "stop"),
            ));
        }
    }
    for a in ADAPTADORES.iter().filter(|a| binario_presente(sonda, a.bin())) {
        for f in a.fragmentos {
            if let Err(e) = super::leer_para_reescribir(sonda, &unir(&a.dir(ctx), f.destino)) {
                v.push(format!("{e}; el fragmento de gachapon queda por poner"));
            }
        }
    }
    let ausentes: Vec<&str> = ADAPTADORES.iter().filter(|a| !binario_presente(sonda, a.bin())).map(|a| a.id.as_str()).collect();
    if !ausentes.is_empty() {
        v.push(format!("sin adaptador (no están en el PATH): {}; vuelve a ejecutar `ea instalar` cuando los instales", ausentes.join(", ")));
    }
    v
}

fn quitar_montaje(ctx: &Contexto, sonda: &dyn Sonda, dir: &Path, m: &Montaje, p: &mut Vec<Paso>) {
    let base = unir(dir, m.destino);
    for (unidad, rs) in unidades(m) {
        let destino = base.join(unidad);
        let sufijo = Path::new(m.prefijo.trim_end_matches('/')).join(unidad);
        if destino_enlace(&destino).is_some_and(|d| d.ends_with(&sufijo)) {
            p.push(Paso::Borrar(destino));
            continue;
        }
        let mut es_carpeta = false;
        for r in rs {
            let ruta = unir(&base, &r.ruta[m.prefijo.len()..]);
            es_carpeta |= ruta != destino;
            if sonda.leer(&ruta).is_some_and(|c| es_nuestro(&c, r, ctx)) {
                p.push(Paso::Borrar(ruta));
            }
        }
        if es_carpeta {
            p.push(Paso::QuitarDirVacio(destino));
        }
    }
}

/// Lo inverso de [`plan_agentes`], para todos los agentes del catálogo
/// (estén o no en el `PATH`): solo se quita lo nuestro.
pub fn plan_quitar_agentes(ctx: &Contexto, sonda: &dyn Sonda) -> Result<Vec<Paso>, String> {
    let mut p = Vec::new();
    for a in ADAPTADORES {
        let dir = a.dir(ctx);
        for m in a.montajes {
            quitar_montaje(ctx, sonda, &dir, m, &mut p);
        }
        for f in a.fragmentos {
            let ruta = unir(&dir, f.destino);
            if let Some(nuevo) = super::leer_para_reescribir(sonda, &ruta).ok().flatten().as_deref().and_then(quitar_fragmento) {
                p.push(if nuevo.trim().is_empty() { Paso::Borrar(ruta) } else { Paso::EditarPersonal { ruta, contenido: nuevo } });
            }
        }
    }
    if binario_presente(sonda, "claude") && sonda.salida(&["claude", "mcp", "get", NOMBRE_MCP]).is_some() {
        let argv = ["claude", "mcp", "remove", "--scope", "user", NOMBRE_MCP];
        p.push(Paso::Ejecutar { argv: argv.iter().map(|s| s.to_string()).collect(), tolerar: true, reintentos: 0 });
    }
    let ruta = ruta_settings(ctx);
    if let Some(Ok(Some(nuevo))) = super::leer_para_reescribir(sonda, &ruta).ok().flatten().map(|t| quitar_hook(&t)) {
        p.push(Paso::Copiar { origen: ruta.clone(), destino: ruta.with_extension("json.gachapon.bak") });
        p.push(Paso::EditarPersonal { ruta, contenido: nuevo });
    }
    if destino_enlace(&ctx.skills_dir).is_some_and(|d| d.ends_with(Path::new("qwen").join("skills"))) {
        p.push(Paso::Borrar(ctx.skills_dir.clone()));
    } else {
        for r in recursos::bajo("qwen/skills/") {
            let ruta = ctx.skills_dir.join(&r.ruta["qwen/skills/".len()..]);
            if sonda.leer(&ruta).as_deref() == Some(r.contenido) {
                p.push(Paso::Borrar(ruta));
            }
        }
    }
    Ok(p)
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use crate::instalacion::Destinos;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Falsa {
        ficheros: HashMap<PathBuf, String>,
        binarios: Vec<&'static str>,
        salidas: HashMap<String, String>,
        ilegibles: Vec<PathBuf>,
    }

    impl Sonda for Falsa {
        fn ilegible(&self, r: &Path) -> bool {
            self.ilegibles.iter().any(|i| i == r)
        }
        fn binario(&self, n: &str) -> Option<PathBuf> {
            self.binarios.contains(&n).then(|| PathBuf::from("/usr/bin").join(n))
        }
        fn existe(&self, r: &Path) -> bool {
            self.ficheros.keys().any(|k| k.starts_with(r))
        }
        fn leer(&self, r: &Path) -> Option<String> {
            self.ficheros.get(r).cloned()
        }
        fn salida(&self, argv: &[&str]) -> Option<String> {
            self.salidas.get(&argv.join(" ")).cloned()
        }
        fn http_ok(&self, _: &str) -> bool {
            false
        }
    }

    const HOME: &str = "/nohay/home-pruebas-023";

    fn ctx(origen: Origen) -> Contexto {
        let home = PathBuf::from(HOME);
        Contexto {
            sistema: ea_plataforma::Sistema::Linux,
            origen,
            exes_origen: PathBuf::from("/tmp/paquete"),
            destinos: Destinos { bin: home.join(".local/share/ea/bin"), flujo: home.join(".local/share/ea/flujo"), datos: home.join(".local/share/ea") },
            skills_dir: home.join(".config/ea/qwen-skills"),
            home,
            puerto_mcp: 8765,
            con_escritorio: true,
        }
    }

    fn escrituras(p: &[Paso]) -> Vec<&PathBuf> {
        p.iter().filter_map(|x| if let Paso::Escribir { ruta, .. } | Paso::EditarPersonal { ruta, .. } = x { Some(ruta) } else { None }).collect()
    }

    #[test]
    fn sin_agentes_solo_las_skills_del_ejecutor() {
        let c = ctx(Origen::Paquete);
        let p = plan_agentes(&c, &Falsa::default()).unwrap();
        assert_eq!(p.len(), recursos::bajo("qwen/skills/").count());
        assert!(escrituras(&p).iter().all(|r| r.starts_with(&c.skills_dir)));
    }

    #[test]
    fn claude_en_paquete_escribe_con_la_ruta_real_mcp_y_hook() {
        let c = ctx(Origen::Paquete);
        let s = Falsa { binarios: vec!["claude"], ..Default::default() };
        let p = plan_agentes(&c, &s).unwrap();
        let skill = c.home.join(".claude/skills/mision/SKILL.md");
        let contenido = p.iter().find_map(|x| match x {
            Paso::Escribir { ruta, contenido } | Paso::EditarPersonal { ruta, contenido } if *ruta == skill => Some(contenido),
            _ => None,
        });
        let contenido = contenido.expect("escribe la skill");
        assert!(!contenido.contains(MARCA_FLUJO));
        assert!(contenido.contains(&format!("{HOME}/.local/share/ea/flujo/mision.md")));
        // El MCP se registra con el `headersHelper` que entrega la
        // credencial del planificador (U-02); la credencial no va en la orden.
        let registro = p.iter().find_map(|x| match x {
            Paso::Ejecutar { argv, tolerar: false, .. } if argv[..6].join(" ") == "claude mcp add-json --scope user ea-ecosistema" => Some(&argv[6]),
            _ => None,
        });
        let registro: Value = serde_json::from_str(registro.expect("registra el MCP")).unwrap();
        assert_eq!(
            registro,
            json!({"type": "http", "url": "http://127.0.0.1:8765/mcp", "headersHelper": format!("{HOME}/.local/share/ea/bin/ea mcp cabeceras")})
        );
        assert!(!p.iter().any(|x| matches!(x, Paso::Ejecutar { argv, .. } if argv.join(" ").contains("remove"))), "no había nada que quitar");
        let settings = c.home.join(".claude/settings.json");
        assert!(escrituras(&p).contains(&&settings));
        // Sin settings previo no hay copia.
        assert!(!p.iter().any(|x| matches!(x, Paso::Copiar { .. })));
    }

    #[test]
    fn claude_con_mcp_previo_sin_credencial_lo_rehace_y_despues_es_idempotente() {
        let c = ctx(Origen::Paquete);
        let mut s = Falsa { binarios: vec!["claude"], ..Default::default() };
        s.salidas.insert("claude mcp get ea-ecosistema".into(), "ea-ecosistema: http…".into());
        // El registro de antes de U-02: la URL sola, sin `headersHelper`.
        let claude_json = c.home.join(".claude.json");
        s.ficheros.insert(claude_json.clone(), r#"{"mcpServers":{"ea-ecosistema":{"type":"http","url":"http://127.0.0.1:8765/mcp"}}}"#.into());
        let p = plan_agentes(&c, &s).unwrap();
        let ordenes: Vec<String> = p.iter().filter_map(|x| if let Paso::Ejecutar { argv, .. } = x { Some(argv[..6].join(" ")) } else { None }).collect();
        assert_eq!(ordenes, ["claude mcp remove --scope user ea-ecosistema", "claude mcp add-json --scope user ea-ecosistema"]);
        // Simula lo ya instalado: aplicar las escrituras y el registro en la sonda.
        for x in p {
            match x {
                Paso::Escribir { ruta, contenido } | Paso::EditarPersonal { ruta, contenido } => {
                    s.ficheros.insert(ruta, contenido);
                }
                Paso::Ejecutar { argv, .. } if argv[2] == "add-json" => {
                    s.ficheros.insert(claude_json.clone(), json!({"mcpServers": {"ea-ecosistema": serde_json::from_str::<Value>(&argv[6]).unwrap()}}).to_string());
                }
                _ => {}
            }
        }
        let p = plan_agentes(&c, &s).unwrap();
        assert!(p.is_empty(), "repetir no hace nada: {p:?}");
    }

    #[test]
    fn hook_en_settings_inexistente_otros_hooks_y_ya_presente() {
        let ea = Path::new("/opt/g bin/ea");
        let n = fusionar_hook(None, ea).unwrap().unwrap();
        let v: Value = serde_json::from_str(&n).unwrap();
        assert_eq!(v["hooks"]["SubagentStart"][0]["hooks"][0]["command"], "\"/opt/g bin/ea\" hook subagentes start");
        assert_eq!(v["hooks"]["SubagentStop"][0]["hooks"][0]["timeout"], 5);
        assert_eq!(fusionar_hook(Some(&n), ea).unwrap(), None);

        let otros = r#"{"model":"opus","hooks":{"SubagentStart":[{"matcher":"x","hooks":[{"type":"command","command":"otro"}]}],"Stop":[]}}"#;
        let n = fusionar_hook(Some(otros), Path::new("/b/ea")).unwrap().unwrap();
        let v: Value = serde_json::from_str(&n).unwrap();
        assert_eq!(v["model"], "opus");
        assert_eq!(v["hooks"]["SubagentStart"].as_array().unwrap().len(), 2);
        assert!(v["hooks"]["Stop"].is_array());

        // El envoltorio antiguo se cambia por la orden de `ea`, en su sitio.
        let antiguo = r#"{"hooks":{"SubagentStart":[{"matcher":"*","hooks":[{"type":"command","command":"sh \"$HOME/.claude/hooks/ea-subagentes.sh\" start","timeout":5}]}],"SubagentStop":[{"hooks":[{"command":"~/.claude/hooks/ea-subagentes.sh stop"}]}]}}"#;
        let n = fusionar_hook(Some(antiguo), Path::new("/b/ea")).unwrap().unwrap();
        let v: Value = serde_json::from_str(&n).unwrap();
        assert_eq!(v["hooks"]["SubagentStart"], json!([{"matcher": "*", "hooks": [{"type": "command", "command": "/b/ea hook subagentes start", "timeout": 5}]}]));
        assert_eq!(v["hooks"]["SubagentStop"], json!([{"hooks": [{"command": "/b/ea hook subagentes stop"}]}]));
        assert!(!n.contains("ea-subagentes.sh"));
        assert_eq!(fusionar_hook(Some(&n), Path::new("/b/ea")).unwrap(), None, "migrar dos veces es lo mismo que una");
        // Con la nuestra ya puesta, el envoltorio sobra y se quita (con su
        // entrada si se queda vacía); lo ajeno de la misma entrada, no.
        let los_dos = r#"{"hooks":{"SubagentStart":[{"hooks":[{"command":"/b/ea hook subagentes start"}]},{"hooks":[{"command":"x/ea-subagentes.sh start"}]},{"hooks":[{"command":"x/ea-subagentes.sh start"},{"command":"mio.sh"}]}],"SubagentStop":[{"hooks":[{"command":"/b/ea hook subagentes stop"}]}]}}"#;
        let v: Value = serde_json::from_str(&fusionar_hook(Some(los_dos), Path::new("/b/ea")).unwrap().unwrap()).unwrap();
        assert_eq!(v["hooks"]["SubagentStart"], json!([{"hooks": [{"command": "/b/ea hook subagentes start"}]}, {"hooks": [{"command": "mio.sh"}]}]));

        assert!(fusionar_hook(Some("{ no"), Path::new("/b/ea")).is_err());
        assert!(fusionar_hook(Some("[1]"), Path::new("/b/ea")).is_err());
    }

    #[test]
    fn hook_windows_con_barras() {
        assert_eq!(orden_hook(Path::new("C:\\Users\\ana\\ea.exe"), "stop"), "C:/Users/ana/ea.exe hook subagentes stop");
    }

    #[test]
    fn settings_invalido_no_se_toca_y_se_avisa() {
        let c = ctx(Origen::Paquete);
        let mut s = Falsa { binarios: vec!["claude"], ..Default::default() };
        let settings = c.home.join(".claude/settings.json");
        s.ficheros.insert(settings.clone(), "{ roto".into());
        let p = plan_agentes(&c, &s).unwrap();
        assert!(!escrituras(&p).contains(&&settings));
        let avisos = avisos_agentes(&c, &s);
        assert!(avisos.iter().any(|a| a.contains("settings.json") && a.contains("hook subagentes start")));
    }

    #[test]
    fn un_settings_ilegible_no_se_reescribe_y_se_avisa() {
        let c = ctx(Origen::Paquete);
        let settings = c.home.join(".claude/settings.json");
        let s = Falsa { binarios: vec!["claude"], ilegibles: vec![settings.clone()], ..Default::default() };
        let p = plan_agentes(&c, &s).unwrap();
        assert!(!p.iter().any(|x| matches!(x, Paso::Escribir { ruta, .. } | Paso::EditarPersonal { ruta, .. } | Paso::Copiar { destino: ruta, .. } if ruta.starts_with(&settings))));
        assert!(avisos_agentes(&c, &s).iter().any(|a| a.contains("no se puede leer")));
    }

    #[test]
    fn quitar_hook_respeta_las_ordenes_ajenas_de_la_misma_entrada() {
        let t = r#"{"hooks":{"SubagentStart":[{"matcher":"*","hooks":[
            {"type":"command","command":"/x/ea hook subagentes start"},
            {"type":"command","command":"mi-script.sh"}]}]}}"#;
        let n = quitar_hook(t).unwrap().unwrap();
        assert!(n.contains("mi-script.sh"));
        assert!(!n.contains("hook subagentes start"));
        assert!(n.contains("SubagentStart"));
    }

    #[test]
    fn settings_previo_se_copia_antes() {
        let c = ctx(Origen::Paquete);
        let mut s = Falsa { binarios: vec!["claude"], ..Default::default() };
        let settings = c.home.join(".claude/settings.json");
        s.ficheros.insert(settings.clone(), "{}".into());
        let p = plan_agentes(&c, &s).unwrap();
        let i = p.iter().position(|x| matches!(x, Paso::Copiar { destino, .. } if destino.ends_with("settings.json.gachapon.bak"))).unwrap();
        let j = p.iter().position(|x| matches!(x, Paso::Escribir { ruta, .. } | Paso::EditarPersonal { ruta, .. } if *ruta == settings)).unwrap();
        assert!(i < j);
    }

    #[test]
    fn fragmentos_idempotentes_y_a_mano() {
        let f = "## gachapon\nsigue la guía\n";
        let una = poner_fragmento(Some("# Mío\n"), f);
        assert_eq!(poner_fragmento(Some(&una), f), una);
        assert!(una.starts_with("# Mío\n\n<!-- gachapon:inicio -->\n"));
        let otra = poner_fragmento(Some(&una), "## gachapon\nnueva\n");
        assert!(otra.contains("nueva") && !otra.contains("sigue la guía"));
        assert_eq!(poner_fragmento(None, f), format!("{MARCA_INICIO}\n## gachapon\nsigue la guía\n{MARCA_FIN}\n"));
        // Añadido a mano con `cat >>`: se sustituye, no se duplica.
        let a_mano = format!("# Mío\n{f}");
        let n = poner_fragmento(Some(&a_mano), f);
        assert_eq!(n.matches("sigue la guía").count(), 1);
        assert!(n.contains(MARCA_INICIO));
        // Quitar deja lo de antes.
        assert_eq!(quitar_fragmento(&una).unwrap(), "# Mío\n");
        assert_eq!(quitar_fragmento("# nada\n"), None);
    }

    #[test]
    fn fragmento_a_mano_con_la_ruta_fija_se_sustituye_por_el_reescrito() {
        let c = ctx(Origen::Paquete);
        let r = recursos::recurso("agentes/codex/AGENTS.md").unwrap();
        let ruta = c.home.join(".codex/AGENTS.md");
        // Añadido a mano con `cat >>`: tal cual (con el marcador) o como lo
        // daban las versiones anteriores (con la ruta fija de entonces).
        for a_mano in [r.contenido.to_string(), r.contenido.replace(MARCA_FLUJO, RUTA_FLUJO_HEREDADA)] {
            let mut s = Falsa { binarios: vec!["codex"], ..Default::default() };
            s.ficheros.insert(ruta.clone(), format!("# Reglas mías\n{a_mano}"));
            let p = plan_agentes(&c, &s).unwrap();
            let nuevo = p.iter().find_map(|x| match x {
                Paso::Escribir { ruta: q, contenido } | Paso::EditarPersonal { ruta: q, contenido } if *q == ruta => Some(contenido.clone()),
                _ => None,
            });
            let nuevo = nuevo.unwrap();
            assert!(nuevo.starts_with("# Reglas mías\n"));
            assert!(!nuevo.contains(MARCA_FLUJO) && !nuevo.contains(RUTA_FLUJO_HEREDADA));
            assert_eq!(nuevo.matches(MARCA_INICIO).count(), 1);
            assert_eq!(nuevo.matches("## Ecosistema asistente").count(), 1, "{nuevo}");
        }
    }

    #[cfg(unix)] // enlaces simbólicos de verdad
    #[test]
    fn un_clon_copia_los_adaptadores_y_sustituye_los_enlaces_de_antes() {
        // HOME y clon de verdad (temporales): el enlace de una instalación
        // anterior al repo se quita y en su lugar va la copia con la ruta
        // real de las guías, sin el marcador.
        let d = tempfile::tempdir().unwrap();
        let repo = d.path().join("clon");
        let mut c = ctx(Origen::Clon { repo: repo.clone() });
        c.home = d.path().join("home");
        c.destinos.flujo = repo.join("agentes/flujo");
        c.skills_dir = c.home.join(".config/ea/qwen-skills");
        std::fs::create_dir_all(repo.join("claude/skills/mision")).unwrap();
        std::fs::create_dir_all(c.home.join(".claude/skills")).unwrap();
        let enlace = c.home.join(".claude/skills/mision");
        ea_plataforma::fs::enlazar(&repo.join("claude/skills/mision"), &enlace).unwrap();
        let s = Falsa { binarios: vec!["claude", "opencode"], ..Default::default() };
        let p = plan_agentes(&c, &s).unwrap();
        let borrar = p.iter().position(|x| *x == Paso::Borrar(enlace.clone())).expect("quita el enlace");
        let skill = enlace.join("SKILL.md");
        let escribir = p.iter().position(|x| matches!(x, Paso::Escribir { ruta, .. } if *ruta == skill)).expect("copia la skill");
        assert!(borrar < escribir);
        assert!(!p.iter().any(|x| matches!(x, Paso::Enlazar { destino, .. } if destino.starts_with(c.home.join(".claude")))));
        let comando = c.home.join(".config/opencode/commands/ea-obra.md");
        let obra = p.iter().find_map(|x| match x {
            Paso::Escribir { ruta, contenido } if *ruta == comando => Some(contenido),
            _ => None,
        });
        let obra = obra.expect("copia el comando de OpenCode");
        assert!(obra.contains(&format!("{}/obra.md", repo.join("agentes/flujo").display())) && !obra.contains(MARCA_FLUJO));
        // Las skills del ejecutor no llevan marcador: siguen enlazadas al repo.
        assert!(p.contains(&Paso::Enlazar { origen: repo.join("qwen/skills"), destino: c.skills_dir.clone() }));
        // Aplicado de verdad, el enlace es ahora una carpeta con la copia.
        let mut registro = Vec::new();
        ea_acciones::servicios::aplicar(&p, &mut |argv| {
            registro.push(argv.join(" "));
            Ok(())
        })
        .unwrap();
        assert!(!es_enlace(&enlace) && enlace.is_dir());
        let copia = std::fs::read_to_string(&skill).unwrap();
        assert!(copia.starts_with("---\nname: mision\n") && !copia.contains(MARCA_FLUJO), "{copia}");
        assert!(std::fs::read_dir(repo.join("claude/skills/mision")).unwrap().next().is_none(), "no escribe dentro del repo");
    }

    #[test]
    fn clon_con_carpeta_real_ajena_escribe_dentro() {
        let repo = PathBuf::from("/srv/gachapon");
        let c = ctx(Origen::Clon { repo });
        let mut s = Falsa { binarios: vec!["claude"], ..Default::default() };
        s.ficheros.insert(c.home.join(".claude/skills/mision/notas.md"), "mías".into());
        let p = plan_agentes(&c, &s).unwrap();
        assert!(escrituras(&p).contains(&&c.home.join(".claude/skills/mision/SKILL.md")));
    }

    #[test]
    fn clon_en_otra_ruta_copia_con_la_ruta_del_repo() {
        let repo = PathBuf::from("/srv/gachapon");
        let mut c = ctx(Origen::Clon { repo: repo.clone() });
        c.destinos.flujo = repo.join("agentes/flujo");
        let s = Falsa { binarios: vec!["agy"], ..Default::default() };
        let p = plan_agentes(&c, &s).unwrap();
        assert!(!p.iter().any(|x| matches!(x, Paso::Enlazar { destino, .. } if destino.starts_with(c.home.join(".gemini")))));
        let regla = p.iter().find_map(|x| match x {
            Paso::Escribir { ruta, contenido } | Paso::EditarPersonal { ruta, contenido } if ruta.ends_with("rules/ea-planificador.md") => Some(contenido),
            _ => None,
        });
        assert!(regla.unwrap().contains("/srv/gachapon/agentes/flujo/"));
    }

    #[test]
    fn quitar_es_simetrico() {
        let c = ctx(Origen::Paquete);
        let mut s = Falsa { binarios: vec!["claude", "codex", "opencode", "agy"], ..Default::default() };
        s.ficheros.insert(c.home.join(".codex/AGENTS.md"), "# Mío\n".into());
        s.ficheros.insert(c.home.join(".claude/settings.json"), r#"{"model":"opus"}"#.into());
        s.ficheros.insert(c.home.join(".claude/skills/otra/SKILL.md"), "ajena".into());
        for x in plan_agentes(&c, &s).unwrap() {
            if let Paso::Escribir { ruta, contenido } | Paso::EditarPersonal { ruta, contenido } = x {
                s.ficheros.insert(ruta, contenido);
            }
        }
        s.salidas.insert("claude mcp get ea-ecosistema".into(), "sí".into());
        let q = plan_quitar_agentes(&c, &s).unwrap();
        // Se borran las skills nuestras, no la ajena.
        assert!(q.contains(&Paso::Borrar(c.home.join(".claude/skills/mision/SKILL.md"))));
        assert!(!q.contains(&Paso::Borrar(c.home.join(".claude/skills/otra/SKILL.md"))));
        assert!(q.contains(&Paso::Borrar(c.skills_dir.join("rust.md"))));
        // El AGENTS.md vuelve a lo de antes; el settings, sin hooks.
        assert!(q.contains(&Paso::EditarPersonal { ruta: c.home.join(".codex/AGENTS.md"), contenido: "# Mío\n".into() }));
        let settings = q.iter().find_map(|x| match x {
            Paso::Escribir { ruta, contenido } | Paso::EditarPersonal { ruta, contenido } if ruta.ends_with("settings.json") => Some(contenido.clone()),
            _ => None,
        });
        let v: Value = serde_json::from_str(&settings.unwrap()).unwrap();
        assert_eq!(v, json!({"model": "opus"}));
        assert!(q.iter().any(|x| matches!(x, Paso::Ejecutar { argv, tolerar: true, .. } if argv.join(" ") == "claude mcp remove --scope user ea-ecosistema")));
        // El AGENTS.md que solo tenía lo nuestro se borra entero.
        assert!(q.contains(&Paso::Borrar(c.home.join(".config/opencode/AGENTS.md"))));
    }

    #[test]
    fn cada_adaptador_con_su_binario_y_sus_carpetas() {
        let c = ctx(Origen::Paquete);
        // Sin ninguno en el PATH, el aviso los nombra por su id.
        let avisos = avisos_agentes(&c, &Falsa::default());
        let esperado = "sin adaptador (no están en el PATH): claude-code, codex, opencode, antigravity; vuelve a ejecutar `ea instalar` cuando los instales";
        assert!(avisos.iter().any(|a| a == esperado), "{avisos:?}");
        // Con su binario, cada adaptador escribe en sus carpetas de siempre y solo en ellas.
        for (bin, carpetas) in [
            ("claude", &[".claude/skills", ".claude/settings.json"][..]),
            ("codex", &[".codex/skills", ".codex/AGENTS.md"]),
            ("opencode", &[".config/opencode/commands", ".config/opencode/AGENTS.md"]),
            ("agy", &[".gemini/config/skills", ".gemini/config/rules"]),
        ] {
            let p = plan_agentes(&c, &Falsa { binarios: vec![bin], ..Default::default() }).unwrap();
            let rutas: Vec<&Path> =
                escrituras(&p).into_iter().filter(|r| !r.starts_with(&c.skills_dir)).map(|r| r.strip_prefix(&c.home).unwrap()).collect();
            assert!(rutas.iter().all(|r| carpetas.iter().any(|c| r.starts_with(c))), "{bin}: {rutas:?}");
            assert!(carpetas.iter().all(|c| rutas.iter().any(|r| r.starts_with(c))), "{bin}: {rutas:?}");
        }
    }

    #[cfg(unix)] // enlaces simbólicos de verdad
    #[test]
    fn instalar_migra_el_hook_de_shell_en_un_home_de_verdad() {
        // HOME temporal con lo que dejaba la instalación a mano de antes: el
        // enlace `~/.claude/hooks/ea-subagentes.sh` al envoltorio del repo
        // (ya roto) y el settings.json que lo llama.
        let d = tempfile::tempdir().unwrap();
        let mut c = ctx(Origen::Paquete);
        c.home = d.path().join("home");
        let hooks = c.home.join(".claude/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        let enlace = hooks.join("ea-subagentes.sh");
        ea_plataforma::fs::enlazar(&d.path().join("clon/claude/hooks/ea-subagentes.sh"), &enlace).unwrap();
        let ajeno = hooks.join("otro.sh");
        ea_plataforma::fs::enlazar(&d.path().join("x/otro.sh"), &ajeno).unwrap();
        let settings = c.home.join(".claude/settings.json");
        let antes = r#"{"model":"opus","hooks":{"SubagentStart":[{"matcher":"*","hooks":[{"type":"command","command":"sh \"$HOME/.claude/hooks/ea-subagentes.sh\" start","timeout":5}]}],"SubagentStop":[{"matcher":"*","hooks":[{"type":"command","command":"sh \"$HOME/.claude/hooks/ea-subagentes.sh\" stop","timeout":5}]}]}}"#;
        std::fs::write(&settings, antes).unwrap();
        let mut s = Falsa { binarios: vec!["claude"], ..Default::default() };
        s.ficheros.insert(settings.clone(), antes.into());
        let p = plan_agentes(&c, &s).unwrap();
        assert!(p.contains(&Paso::Borrar(enlace.clone())));
        assert!(!p.contains(&Paso::Borrar(ajeno.clone())), "un enlace ajeno de ~/.claude/hooks se queda");
        ea_acciones::servicios::aplicar(&p, &mut |_| Ok(())).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        let ea = c.destinos.bin.join("ea");
        for (evento, accion) in [("SubagentStart", "start"), ("SubagentStop", "stop")] {
            let orden = format!("{} hook subagentes {accion}", ea.display());
            assert_eq!(v["hooks"][evento], json!([{"matcher": "*", "hooks": [{"type": "command", "command": orden, "timeout": 5}]}]));
        }
        assert_eq!(v["model"], "opus");
        assert!(std::fs::symlink_metadata(&enlace).is_err(), "el enlace roto se quita");
        assert!(std::fs::symlink_metadata(&ajeno).is_ok());
        assert!(c.home.join(".claude/settings.json.gachapon.bak").is_file());
        // Repetir no cambia nada más del hook.
        s.ficheros.insert(settings.clone(), std::fs::read_to_string(&settings).unwrap());
        let p = plan_agentes(&c, &s).unwrap();
        assert!(!p.iter().any(|x| matches!(x, Paso::EditarPersonal { ruta, .. } | Paso::Borrar(ruta) if ruta.starts_with(c.home.join(".claude/settings.json")) || ruta.starts_with(&hooks))));
    }

    #[test]
    fn quitar_hook_deja_el_envoltorio_a_mano() {
        let t = r#"{"hooks":{"SubagentStart":[{"hooks":[{"command":"/b/ea hook subagentes start"}]},{"hooks":[{"command":"x/ea-subagentes.sh start"}]}]}}"#;
        let v: Value = serde_json::from_str(&quitar_hook(t).unwrap().unwrap()).unwrap();
        assert_eq!(v["hooks"]["SubagentStart"].as_array().unwrap().len(), 1);
        assert_eq!(quitar_hook(r#"{"model":"x"}"#).unwrap(), None);
    }
}
