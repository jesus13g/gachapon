//! Los adaptadores de los agentes, el MCP y el hook de Claude Code, y las
//! skills del ejecutor (spec 023, decisiones 4 a 6).
//!
//! Solo se instala el adaptador de los agentes que están en el `PATH`; al
//! desinstalar se mira el de todos. Nada ajeno se pisa: los fragmentos de
//! `AGENTS.md` van entre marcas, el `settings.json` se fusiona (con copia
//! `.bak`) y solo se borra lo que es nuestro (un enlace a nuestro repo o un
//! fichero con nuestro contenido).

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::recursos::{self, reescribir_ruta_flujo, Recurso};
use super::{Contexto, Origen, Sonda};
use crate::servicios::Paso;

const MARCA_INICIO: &str = "<!-- gachapon:inicio -->";
const MARCA_FIN: &str = "<!-- gachapon:fin -->";

/// Nombre del servidor MCP en Claude Code.
const NOMBRE_MCP: &str = "ea-ecosistema";

/// Una carpeta del repo que se instala bajo una carpeta del HOME. La unidad
/// que se enlaza es el primer componente bajo `prefijo` (la carpeta de la
/// skill o el fichero suelto).
struct Montaje {
    prefijo: &'static str,
    destino: &'static str,
}

/// Un fragmento que se añade a un `AGENTS.md` del agente.
struct Fragmento {
    recurso: &'static str,
    destino: &'static str,
}

/// El adaptador de un agente del catálogo (tabla «Adaptadores del
/// planificador» del manual).
struct Adaptador {
    id: &'static str,
    /// El binario por defecto (`ea-ajustes`, `Ajustes::por_defecto`).
    bin: &'static str,
    montajes: &'static [Montaje],
    fragmentos: &'static [Fragmento],
}

const ADAPTADORES: &[Adaptador] = &[
    Adaptador {
        id: "claude-code",
        bin: "claude",
        montajes: &[Montaje { prefijo: "claude/skills/", destino: ".claude/skills" }],
        fragmentos: &[],
    },
    Adaptador {
        id: "codex",
        bin: "codex",
        montajes: &[Montaje { prefijo: "agentes/codex/skills/", destino: ".codex/skills" }],
        fragmentos: &[Fragmento { recurso: "agentes/codex/AGENTS.md", destino: ".codex/AGENTS.md" }],
    },
    Adaptador {
        id: "opencode",
        bin: "opencode",
        montajes: &[Montaje { prefijo: "agentes/opencode/commands/", destino: ".config/opencode/commands" }],
        fragmentos: &[Fragmento { recurso: "agentes/opencode/AGENTS.md", destino: ".config/opencode/AGENTS.md" }],
    },
    Adaptador {
        id: "antigravity",
        bin: "agy",
        montajes: &[
            Montaje { prefijo: "agentes/antigravity/skills/", destino: ".gemini/config/skills" },
            Montaje { prefijo: "agentes/antigravity/rules/", destino: ".gemini/config/rules" },
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

fn canonica(p: &Path) -> PathBuf {
    ea_plataforma::rutas::canonica(p).unwrap_or_else(|_| p.to_path_buf())
}

/// El repo del clon, si está justo en la ruta fija que nombran los
/// adaptadores: entonces se enlaza (las skills siguen vivas).
fn repo_enlazable(ctx: &Contexto) -> Option<&Path> {
    match &ctx.origen {
        Origen::Clon { repo } if canonica(repo) == canonica(&ctx.home.join("Proyectos/ecosistema-asistente")) => Some(repo),
        _ => None,
    }
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

fn orden_hook(ea: &Path, accion: &str) -> String {
    let ruta = ea.to_string_lossy().replace('\\', "/");
    let ruta = if ruta.contains(' ') { format!("\"{ruta}\"") } else { ruta };
    format!("{ruta} hook subagentes {accion}")
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
/// y si no lo añade al final. Poner dos veces deja lo mismo que una.
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

/// Añade a `hooks.SubagentStart` y `hooks.SubagentStop` la orden
/// `<ea> hook subagentes start|stop` si no hay ya una (o el envoltorio
/// antiguo `ea-subagentes.sh`). Conserva todo lo demás; `serde_json` sin
/// `preserve_order` deja las claves en orden alfabético. `None`: no cambia
/// nada; `Err`: el JSON no es válido y no se toca.
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
        let ya = lista.iter().any(|e| ordenes(e).any(|o| o.contains(&buscada) || o.contains("ea-subagentes.sh")));
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
    ctx.home.join(".claude").join("settings.json")
}

// ------------------------------------------------------------- planes

fn plan_montaje(ctx: &Contexto, sonda: &dyn Sonda, m: &Montaje, p: &mut Vec<Paso>) {
    let base = unir(&ctx.home, m.destino);
    let repo = repo_enlazable(ctx);
    for (unidad, rs) in unidades(m) {
        let destino = base.join(unidad);
        if let Some(repo) = repo {
            let origen = unir(repo, m.prefijo).join(unidad);
            if destino_enlace(&destino).is_some_and(|d| canonica(&d) == canonica(&origen)) {
                continue;
            }
            // Una copia idéntica ya vale; una carpeta real ajena no se
            // sustituye por un enlace: se escribe solo nuestro fichero.
            let identica = rs.iter().all(|r| sonda.leer(&unir(&ctx.home, m.destino).join(&r.ruta[m.prefijo.len()..])).as_deref() == Some(r.contenido));
            if identica {
                continue;
            }
            if !sonda.existe(&destino) || es_enlace(&destino) {
                p.push(Paso::Enlazar { origen, destino });
                continue;
            }
        }
        // Un enlace (a otro sitio) se quita antes de escribir: si no, se
        // escribiría a través de él, dentro del repo enlazado.
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

fn plan_fragmento(ctx: &Contexto, sonda: &dyn Sonda, f: &Fragmento, p: &mut Vec<Paso>) {
    let Some(r) = recursos::recurso(f.recurso) else { return };
    let ruta = unir(&ctx.home, f.destino);
    let Ok(actual) = super::leer_para_reescribir(sonda, &ruta) else { return };
    let fragmento = reescribir_ruta_flujo(r.contenido, &ctx.destinos.flujo);
    let nuevo = poner_fragmento_con(actual.as_deref(), &fragmento, &[r.contenido]);
    if actual.as_deref() != Some(nuevo.as_str()) {
        p.push(Paso::EditarPersonal { ruta, contenido: nuevo });
    }
}

fn plan_claude(ctx: &Contexto, sonda: &dyn Sonda, p: &mut Vec<Paso>) {
    if sonda.salida(&["claude", "mcp", "get", NOMBRE_MCP]).is_none() {
        let url = format!("http://127.0.0.1:{}/mcp", ctx.puerto_mcp);
        let argv = ["claude", "mcp", "add", "--transport", "http", "--scope", "user", NOMBRE_MCP, &url];
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
    for a in ADAPTADORES.iter().filter(|a| binario_presente(sonda, a.bin)) {
        for m in a.montajes {
            plan_montaje(ctx, sonda, m, &mut p);
        }
        for f in a.fragmentos {
            plan_fragmento(ctx, sonda, f, &mut p);
        }
        if a.id == "claude-code" {
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
    for a in ADAPTADORES.iter().filter(|a| binario_presente(sonda, a.bin)) {
        for f in a.fragmentos {
            if let Err(e) = super::leer_para_reescribir(sonda, &unir(&ctx.home, f.destino)) {
                v.push(format!("{e}; el fragmento de gachapon queda por poner"));
            }
        }
    }
    let ausentes: Vec<&str> = ADAPTADORES.iter().filter(|a| !binario_presente(sonda, a.bin)).map(|a| a.id).collect();
    if !ausentes.is_empty() {
        v.push(format!("sin adaptador (no están en el PATH): {}; vuelve a ejecutar `ea instalar` cuando los instales", ausentes.join(", ")));
    }
    v
}

fn quitar_montaje(ctx: &Contexto, sonda: &dyn Sonda, m: &Montaje, p: &mut Vec<Paso>) {
    let base = unir(&ctx.home, m.destino);
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
        for m in a.montajes {
            quitar_montaje(ctx, sonda, m, &mut p);
        }
        for f in a.fragmentos {
            let ruta = unir(&ctx.home, f.destino);
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
        assert!(!contenido.contains(recursos::RUTA_FLUJO_FIJA));
        assert!(contenido.contains(&format!("{HOME}/.local/share/ea/flujo/mision.md")));
        assert!(p.iter().any(|x| matches!(x, Paso::Ejecutar { argv, .. } if argv.join(" ") == "claude mcp add --transport http --scope user ea-ecosistema http://127.0.0.1:8765/mcp")));
        let settings = c.home.join(".claude/settings.json");
        assert!(escrituras(&p).contains(&&settings));
        // Sin settings previo no hay copia.
        assert!(!p.iter().any(|x| matches!(x, Paso::Copiar { .. })));
    }

    #[test]
    fn claude_con_mcp_previo_no_lo_vuelve_a_registrar_y_es_idempotente() {
        let c = ctx(Origen::Paquete);
        let mut s = Falsa { binarios: vec!["claude"], ..Default::default() };
        s.salidas.insert("claude mcp get ea-ecosistema".into(), "ea-ecosistema: http…".into());
        // Simula lo ya instalado: aplicar las escrituras en la sonda.
        for x in plan_agentes(&c, &s).unwrap() {
            if let Paso::Escribir { ruta, contenido } | Paso::EditarPersonal { ruta, contenido } = x {
                s.ficheros.insert(ruta, contenido);
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

        let antiguo = r#"{"hooks":{"SubagentStart":[{"hooks":[{"command":"~/.claude/hooks/ea-subagentes.sh start"}]}],"SubagentStop":[{"hooks":[{"command":"~/.claude/hooks/ea-subagentes.sh stop"}]}]}}"#;
        assert_eq!(fusionar_hook(Some(antiguo), Path::new("/b/ea")).unwrap(), None);

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
        let mut s = Falsa { binarios: vec!["codex"], ..Default::default() };
        let r = recursos::recurso("agentes/codex/AGENTS.md").unwrap();
        let ruta = c.home.join(".codex/AGENTS.md");
        s.ficheros.insert(ruta.clone(), format!("# Reglas mías\n{}", r.contenido));
        let p = plan_agentes(&c, &s).unwrap();
        let nuevo = p.iter().find_map(|x| match x {
            Paso::Escribir { ruta: q, contenido } | Paso::EditarPersonal { ruta: q, contenido } if *q == ruta => Some(contenido.clone()),
            _ => None,
        });
        let nuevo = nuevo.unwrap();
        assert!(nuevo.starts_with("# Reglas mías\n"));
        assert!(!nuevo.contains(recursos::RUTA_FLUJO_FIJA));
        assert_eq!(nuevo.matches(MARCA_INICIO).count(), 1);
    }

    #[test]
    fn clon_en_la_ruta_fija_enlaza_carpetas() {
        let repo = PathBuf::from(HOME).join("Proyectos/ecosistema-asistente");
        let c = ctx(Origen::Clon { repo: repo.clone() });
        let s = Falsa { binarios: vec!["claude", "opencode"], ..Default::default() };
        let p = plan_agentes(&c, &s).unwrap();
        assert!(p.contains(&Paso::Enlazar { origen: repo.join("claude/skills/mision"), destino: c.home.join(".claude/skills/mision") }));
        assert!(p.contains(&Paso::Enlazar {
            origen: repo.join("agentes/opencode/commands/ea-obra.md"),
            destino: c.home.join(".config/opencode/commands/ea-obra.md")
        }));
        // Skills del ejecutor: una carpeta enlazada.
        assert!(p.contains(&Paso::Enlazar { origen: repo.join("qwen/skills"), destino: c.skills_dir.clone() }));
        assert!(!escrituras(&p).iter().any(|r| r.starts_with(c.home.join(".claude/skills"))));
    }

    #[test]
    fn clon_con_carpeta_real_ajena_escribe_dentro() {
        let repo = PathBuf::from(HOME).join("Proyectos/ecosistema-asistente");
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
    fn quitar_hook_deja_el_envoltorio_a_mano() {
        let t = r#"{"hooks":{"SubagentStart":[{"hooks":[{"command":"/b/ea hook subagentes start"}]},{"hooks":[{"command":"x/ea-subagentes.sh start"}]}]}}"#;
        let v: Value = serde_json::from_str(&quitar_hook(t).unwrap().unwrap()).unwrap();
        assert_eq!(v["hooks"]["SubagentStart"].as_array().unwrap().len(), 1);
        assert_eq!(quitar_hook(r#"{"model":"x"}"#).unwrap(), None);
    }
}
