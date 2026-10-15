//! Ejecutor del agente local: toma un trabajo de la cola `modelo-local`, lo
//! pasa a Qwen Code headless dentro del worktree, comprueba el criterio de
//! éxito POR SU CUENTA (Qwen puede decir «success» sin haber hecho nada) y
//! entrega el informe a la revisión.
//!
//! - Misión: si el criterio falla y quedan iteraciones, repite la llamada en
//!   el mismo worktree con la evidencia del fallo; solo entrega al pasar, al
//!   agotarlas o con el código 55 de los topes.
//! - Obra: recorre las fases con ese mismo bucle, guarda una instantánea del
//!   árbol tras cada una y se detiene para revisión cada `revisar_cada`.
//! - Rutina: si pasa, se cierra sola; si falla, deja un paquete de
//!   corrección en espera hasta que el usuario lo confirme.
//!
//! Seguridad: Qwen Code 0.25 solo trae sandbox con Docker/seatbelt, y aquí no
//! se usa. Con `sandbox = true` (por defecto; probado con Qwen real), `qwen`
//! se lanza dentro de bubblewrap (sistema de ficheros de solo lectura salvo el
//! worktree, `/tmp` propio y `~/.qwen`; la red queda abierta para llegar a
//! llama-server). Con `sandbox = false` el agente corre con
//! `--approval-mode yolo`: puede escribir y ejecutar en cualquier sitio donde
//! escriba el usuario. Se limita además con los topes de la orden, el
//! aislamiento por worktree y `--exclude-tools` para las herramientas web.
//! También se excluyen las de objetivos y notebooks: no le sirven y cada
//! herramienta declarada cuesta tokens en todos los turnos (~900 entre las
//! tres, medido con `bench/skills`).

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use ea_bus::{
    Agente, Bus, CandadoWorktree, ConfigOrquestador, Evento, FaseInforme, Informe, Modo, Ocupante,
    Orden, ResultadoInforme, Topes,
};

const ESPERA_CANDADO: Duration = Duration::from_secs(60);
const TIEMPO_CRITERIO_S: u64 = 120;
const MAX_EVIDENCIA: usize = 2048;
/// Código con el que Qwen Code sale al agotar un tope.
const SALIDA_TOPES: i32 = 55;

/// Lo que devuelve el agente tras una ejecución.
#[derive(Debug, Clone, Default)]
pub struct SalidaAgente {
    /// Código de salida (`None` si murió por señal, por tiempo, o no arrancó).
    pub codigo: Option<i32>,
    /// Salida en bruto (para `salida-qwen.json`).
    pub bruta: String,
    /// Texto final del agente (resumen), si se pudo extraer.
    pub resumen: String,
}

pub trait AgenteLocal {
    /// `sistema` se añade al system prompt (las skills); vacío si no hay.
    fn ejecutar(&self, prompt: &str, sistema: &str, worktree: &Path, topes: &Topes, config: &ConfigOrquestador) -> SalidaAgente;
}

/// Qwen Code real, en modo headless.
pub struct QwenCode;

impl AgenteLocal for QwenCode {
    fn ejecutar(&self, prompt: &str, sistema: &str, worktree: &Path, topes: &Topes, config: &ConfigOrquestador) -> SalidaAgente {
        let mut args: Vec<String> = Vec::new();
        let mut cmd = if config.sandbox {
            let home = std::env::var("HOME").unwrap_or_default();
            let wt = worktree.to_string_lossy().to_string();
            for a in [
                "--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp",
                "--bind", &format!("{home}/.qwen"), &format!("{home}/.qwen"),
                "--bind", &wt, &wt, "--chdir", &wt, "--unshare-pid", "--die-with-parent",
            ] {
                args.push(a.into());
            }
            args.push(config.qwen_bin.clone());
            Command::new("bwrap")
        } else {
            Command::new(&config.qwen_bin)
        };
        // `--exclude-tools` es un array: va antes de las demás opciones para
        // que no se trague la orden, que va la última.
        for a in [
            "--exclude-tools", "web_fetch", "web_search", "get_goal", "update_goal", "notebook_edit", "--bare", "--auth-type", "openai",
            "--openai-base-url", &config.base_url, "--openai-api-key", "local", "-m", &config.modelo,
            "--approval-mode", "yolo",
        ] {
            args.push(a.into());
        }
        if !sistema.is_empty() {
            args.push("--append-system-prompt".into());
            args.push(sistema.into());
        }
        args.push("--max-session-turns".into());
        args.push(topes.turnos.to_string());
        args.push("--max-wall-time".into());
        args.push(format!("{}s", topes.tiempo_s));
        args.push("--max-tool-calls".into());
        args.push(topes.llamadas_herramienta.to_string());
        for a in ["--output-format", "json", prompt] {
            args.push(a.into());
        }
        cmd.args(&args).current_dir(worktree);
        // tope de seguridad del proceso: algo más que el de Qwen
        let (codigo, bruta, _) = correr(cmd, topes.tiempo_s + 120);
        let resumen = texto_final(&bruta);
        SalidaAgente { codigo, bruta, resumen }
    }
}

/// Texto del último elemento `type=result` del array JSON de Qwen.
fn texto_final(bruta: &str) -> String {
    serde_json::from_str::<serde_json::Value>(bruta)
        .ok()
        .and_then(|v| {
            v.as_array()?
                .iter()
                .rev()
                .find(|m| m["type"] == "result")
                .and_then(|m| m["result"].as_str().map(str::to_string))
        })
        .unwrap_or_default()
}

/// Ejecuta con `timeout`; devuelve (código, stdout, stderr). Código `None`
/// si no arrancó o murió por señal (incluido el 124 del timeout, que se
/// convierte en `None`).
fn correr(cmd: Command, tope_s: u64) -> (Option<i32>, String, String) {
    let prog = cmd.get_program().to_os_string();
    let args: Vec<_> = cmd.get_args().map(|a| a.to_os_string()).collect();
    let dir = cmd.get_current_dir().map(|d| d.to_path_buf());
    let mut c = Command::new("timeout");
    c.arg("-k").arg("10").arg(tope_s.to_string()).arg(prog).args(args);
    if let Some(d) = dir {
        c.current_dir(d);
    }
    match c.stdin(Stdio::null()).output() {
        Ok(o) => {
            let codigo = o.status.code().filter(|c| *c != 124 && *c != 137);
            (codigo, String::from_utf8_lossy(&o.stdout).into(), String::from_utf8_lossy(&o.stderr).into())
        }
        Err(e) => (None, String::new(), e.to_string()),
    }
}

/// Últimos `MAX_EVIDENCIA` bytes (en límite de carácter): la cola de la salida
/// es donde suele estar el fallo.
fn recortar(s: &str) -> String {
    let s = s.trim();
    if s.len() <= MAX_EVIDENCIA {
        return s.to_string();
    }
    let mut i = s.len() - MAX_EVIDENCIA;
    while !s.is_char_boundary(i) {
        i += 1;
    }
    format!("…{}", &s[i..])
}

fn git(wt: &Path, args: &[&str]) -> Option<String> {
    git_con(wt, args, None)
}

fn git_con(wt: &Path, args: &[&str], indice: Option<&Path>) -> Option<String> {
    let mut c = Command::new("git");
    c.args(args).current_dir(wt);
    if let Some(i) = indice {
        c.env("GIT_INDEX_FILE", i);
    }
    let o = c.output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Árbol de git del estado actual del worktree SIN tocar el índice del
/// usuario: se usa un índice temporal.
fn instantanea(wt: &Path, dir: &Path) -> Option<String> {
    let idx = dir.join(".indice-tmp");
    let _ = std::fs::remove_file(&idx);
    let arbol = git_con(wt, &["add", "-A"], Some(&idx)).and_then(|_| git_con(wt, &["write-tree"], Some(&idx)));
    let _ = std::fs::remove_file(&idx);
    arbol
}

const SIN_PYCACHE: &str = ":(exclude,glob)**/__pycache__/**";

fn diff_entre(wt: &Path, a: &str, b: &str, stat: bool) -> String {
    let mut args = vec!["diff"];
    if stat {
        args.push("--stat");
    }
    args.extend([a, b, "--", ".", SIN_PYCACHE]);
    git(wt, &args).unwrap_or_default()
}

fn construir_prompt(orden: &Orden, titulo: &str, cuerpo: &str, criterio: &str) -> String {
    let mut p = format!("# {titulo}\n\n{cuerpo}\n");
    if !orden.notas_revision.is_empty() {
        p.push_str("\n## Notas de la revisión anterior (tenlas en cuenta)\n");
        for n in &orden.notas_revision {
            p.push_str(&format!("- {n}\n"));
        }
    }
    p.push_str(&format!(
        "\n## Reglas\n\
         - Trabaja SOLO dentro del directorio actual (el worktree); no toques nada fuera.\n\
         - No hagas commits.\n\
         - Para crear un fichero nuevo usa run_shell_command con `cat > ruta <<'EOF'`: no hay herramienta para escribir ficheros. `edit` solo cambia ficheros que ya has leído con read_file.\n\
         - El criterio de éxito es el comando `{criterio}` y debe pasar.\n\
         - Cuando termines, explica en 3 líneas qué hiciste.\n"
    ));
    p
}

fn avisar(config: &ConfigOrquestador, titulo: &str, resultado: &str) {
    if config.avisos_escritorio {
        let _ = Command::new("notify-send").arg(format!("ea: {titulo}")).arg(resultado).status();
    }
}

fn bloqueado(motivo: &str, evidencia: String) -> Informe {
    Informe { evidencia, motivo: Some(motivo.into()), ..Informe::nuevo(ResultadoInforme::Bloqueado) }
}

/// Toma y ejecuta un trabajo de la cola `modelo-local`. Devuelve su id, o
/// `None` si no había nada (o la cola falló).
pub fn ejecutar_uno(bus: &Bus, config: &ConfigOrquestador, agente: &dyn AgenteLocal) -> Option<String> {
    ejecutar_uno_con_espera(bus, config, agente, ESPERA_CANDADO)
}

fn ejecutar_uno_con_espera(bus: &Bus, config: &ConfigOrquestador, agente: &dyn AgenteLocal, espera: Duration) -> Option<String> {
    let id = match bus.tomar(Agente::ModeloLocal) {
        Ok(Some(id)) => id,
        Ok(None) => return None,
        Err(e) => {
            tracing::warn!(error = %e, "no se pudo leer la cola modelo-local");
            return None;
        }
    };
    let orden = match bus.leer_orden(&id) {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!(%id, error = %e, "orden ilegible; se descarta la entrada");
            return Some(id);
        }
    };
    let wt = Path::new(&orden.worktree);

    // Candado antes de pasar a Ejecutando: si está ocupado, el trabajo sigue
    // EnCola y solo se vuelve a poner en la cola.
    let ocupante = Ocupante { agente: "modelo-local".into(), trabajo: id.clone(), pid: 0, desde: String::new() };
    let candado = match CandadoWorktree::adquirir_en(bus.raiz(), wt, ocupante, espera) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(%id, error = %e, "worktree ocupado; se vuelve a encolar");
            let _ = bus.encolar(Agente::ModeloLocal, &id, "worktree ocupado");
            return Some(id);
        }
    };
    let fase_actual = match bus.aplicar(&id, Evento::Tomar) {
        Ok(e) => e.fase_actual,
        Err(e) => {
            tracing::warn!(%id, error = %e, "no se pudo pasar a ejecutando");
            return Some(id);
        }
    };
    let inicio = Instant::now();

    // El candado se mantiene durante todo el bucle de iteraciones y fases.
    let informe = trabajar(bus, config, agente, &id, &orden, fase_actual, wt, inicio);
    entregar(bus, config, &id, &orden, informe);
    drop(candado);
    Some(id)
}

/// Entrega el informe según el modo. Rutina: éxito = se cierra sola; fallo =
/// paquete de corrección en espera, sin encolar a nadie.
fn entregar(bus: &Bus, config: &ConfigOrquestador, id: &str, orden: &Orden, informe: Informe) {
    let pasa = informe.resultado == ResultadoInforme::Terminado;
    let motivo = informe.motivo.clone();
    let paquete = (orden.modo == Modo::Rutina && !pasa).then(|| paquete_correccion(bus, id, orden, &informe));
    if let Err(e) = bus.aplicar(id, Evento::Entregar(informe)) {
        tracing::error!(%id, error = %e, "no se pudo entregar el informe");
        return;
    }
    if orden.modo != Modo::Rutina {
        avisar(config, &orden.titulo, &motivo.unwrap_or_else(|| "terminado".into()));
        return;
    }
    let slug = orden.rutina.as_ref().map(|r| r.slug.as_str()).unwrap_or("?");
    if let Some(texto) = paquete {
        if let Err(e) = std::fs::write(bus.raiz().join("trabajos").join(id).join("paquete-correccion.md"), texto) {
            tracing::error!(%id, error = %e, "no se pudo escribir el paquete de corrección");
        }
        if let Err(e) = bus.aplicar(id, Evento::PonerEnEspera) {
            tracing::error!(%id, error = %e, "no se pudo poner en espera");
        }
        avisar(config, &format!("rutina {slug} falló"), "paquete en espera");
    } else {
        if let Err(e) = bus.aplicar(id, Evento::Cerrar) {
            tracing::error!(%id, error = %e, "no se pudo cerrar la rutina");
        }
        avisar(config, &format!("rutina {slug}"), "terminada");
    }
}

fn paquete_correccion(bus: &Bus, id: &str, orden: &Orden, inf: &Informe) -> String {
    let (slug, version) = orden.rutina.as_ref().map(|r| (r.slug.clone(), r.version)).unwrap_or_default();
    let definicion = bus.definicion_rutina(&slug, version).unwrap_or_else(|| "(definición no disponible)".into());
    format!(
        "# Paquete de corrección: rutina {slug} (versión {version})\n\n\
         Trabajo: {id}\n\
         Motivo: {}\n\n\
         ## Definición\n\n```toml\n{}\n```\n\n\
         ## Ejecución (entradas y pasos tal como se le dieron al agente)\n\n{}\n\n\
         ## Criterio de éxito\n\n`{}`\n\n\
         ## Salida y evidencia del error\n\n```\n{}\n```\n\n\
         ## Qué se espera\n\n\
         Corrige la definición de la rutina y sube su versión (`ea rutina registrar <fichero>`).\n",
        inf.motivo.as_deref().unwrap_or("el criterio no pasa"),
        definicion.trim(),
        orden.objetivo.trim(),
        orden.criterio_exito.as_deref().unwrap_or(""),
        inf.evidencia.trim(),
    )
}

/// Resultado de un bucle de iteraciones.
struct Bucle {
    pasa: bool,
    evidencia: String,
    motivo: Option<String>,
}

fn correr_criterio(wt: &Path, criterio: &str) -> (bool, String) {
    let mut c = Command::new("sh");
    c.arg("-c").arg(format!("exec 2>&1; {criterio}")).current_dir(wt);
    let (codigo, out, err) = correr(c, TIEMPO_CRITERIO_S);
    let mut texto = format!("{out}{err}");
    if codigo.is_none() {
        texto.push_str("\n(criterio interrumpido por tiempo)");
    }
    (codigo == Some(0), recortar(&texto))
}

/// Llama al agente hasta que el criterio pase, salgan los topes (55) o se
/// agoten las iteraciones. El criterio lo ejecuta el SISTEMA, no el agente, y
/// su ausencia nunca cuenta como éxito. Las `skills` se vuelven a validar
/// contra la configuración del momento: si ya no valen, no se llama a Qwen.
#[allow(clippy::too_many_arguments)]
fn bucle(
    bus: &Bus,
    config: &ConfigOrquestador,
    agente: &dyn AgenteLocal,
    id: &str,
    orden: &Orden,
    wt: &Path,
    titulo: &str,
    cuerpo: &str,
    criterio: &str,
    skills: &[String],
) -> Bucle {
    if criterio.trim().is_empty() {
        return Bucle { pasa: false, evidencia: "la orden no trae criterio de éxito".into(), motivo: Some("sin criterio de éxito".into()) };
    }
    let dir = bus.raiz().join("trabajos").join(id);
    let sistema = match config.skills.resolver(&orden.creada_por, skills) {
        Ok(s) => ea_bus::bloque_prompt(&s),
        Err(e) => return Bucle { pasa: false, evidencia: e.to_string(), motivo: Some(format!("skills: {e}")) },
    };
    if !sistema.is_empty() {
        let _ = std::fs::write(dir.join("skills-qwen.md"), &sistema);
    }
    let base = construir_prompt(orden, titulo, cuerpo, criterio);
    let max = orden.topes.iteraciones.max(1);
    let mut previa: Option<String> = None;
    for it in 1..=max {
        let _ = bus.anotar_iteracion(id);
        let prompt = match &previa {
            None => base.clone(),
            Some(ev) => format!(
                "{base}\n## Intento anterior (iteración {})\nEl criterio sigue fallando con:\n\n```\n{ev}\n```\n\nCorrige lo que falta.\n",
                it - 1
            ),
        };
        let salida = agente.ejecutar(&prompt, &sistema, wt, &orden.topes, config);
        let _ = std::fs::write(dir.join("salida-qwen.json"), &salida.bruta);
        let (pasa, ev) = correr_criterio(wt, criterio);
        let mut evidencia = ev.clone();
        if !salida.resumen.is_empty() {
            evidencia = recortar_resumen(&format!("{ev}\n--- resumen del agente ---\n{}", salida.resumen.trim()));
        }
        if pasa {
            return Bucle { pasa: true, evidencia, motivo: None };
        }
        if salida.codigo == Some(SALIDA_TOPES) {
            return Bucle { pasa: false, evidencia, motivo: Some("topes agotados (55)".into()) };
        }
        if it == max {
            if salida.codigo != Some(0) {
                evidencia.push_str(&format!("\n(qwen salió con código {:?})", salida.codigo));
            }
            return Bucle { pasa: false, evidencia, motivo: Some("iteraciones agotadas".into()) };
        }
        previa = Some(ev);
    }
    unreachable!("el bucle siempre devuelve en la última iteración")
}

#[allow(clippy::too_many_arguments)]
fn trabajar(
    bus: &Bus,
    config: &ConfigOrquestador,
    agente: &dyn AgenteLocal,
    id: &str,
    orden: &Orden,
    fase_actual: Option<u32>,
    wt: &Path,
    inicio: Instant,
) -> Informe {
    let fin = |mut i: Informe| {
        i.duracion_s = inicio.elapsed().as_secs();
        i
    };
    if !wt.is_dir() {
        return fin(bloqueado("el worktree no existe", orden.worktree.clone()));
    }
    let Some(commit_inicio) = git(wt, &["rev-parse", "HEAD"]) else {
        return fin(bloqueado("el worktree no es un repo git", String::new()));
    };
    if orden.modo == Modo::Obra && !orden.fases.is_empty() {
        return fin(trabajar_obra(bus, config, agente, id, orden, fase_actual, wt, commit_inicio));
    }

    let criterio = orden.criterio_exito.clone().unwrap_or_default();
    let r = bucle(bus, config, agente, id, orden, wt, &orden.titulo, &orden.objetivo, &criterio, &orden.skills);

    let mut diff = git(wt, &["diff", "--stat", &commit_inicio]).unwrap_or_default();
    let nuevos: Vec<String> = git(wt, &["ls-files", "--others", "--exclude-standard"])
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.contains("__pycache__"))
        .map(str::to_string)
        .collect();
    if !nuevos.is_empty() {
        diff.push_str(&format!("\nnuevos: {}", nuevos.join(", ")));
    }
    fin(Informe {
        evidencia: r.evidencia,
        commit_inicio: Some(commit_inicio),
        diff_stat: Some(diff.trim().to_string()),
        motivo: r.motivo,
        obra_completa: r.pasa,
        ..Informe::nuevo(if r.pasa { ResultadoInforme::Terminado } else { ResultadoInforme::Bloqueado })
    })
}

fn leer_texto(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Obra por fases desde `fase_actual`, con parada de revisión cada
/// `revisar_cada` fases hechas (o al terminar todas).
#[allow(clippy::too_many_arguments)]
fn trabajar_obra(
    bus: &Bus,
    config: &ConfigOrquestador,
    agente: &dyn AgenteLocal,
    id: &str,
    orden: &Orden,
    fase_actual: Option<u32>,
    wt: &Path,
    commit_inicio: String,
) -> Informe {
    let dir = bus.raiz().join("trabajos").join(id);
    let mut fases = orden.fases.clone();
    fases.sort_by_key(|f| f.numero);
    let total = fases.len();
    // Si la obra ya estaba completa y se reformula, se repite la última fase.
    let inicio_idx = fase_actual.and_then(|n| fases.iter().position(|f| f.numero == n)).unwrap_or(total - 1);

    let f_ultimo = dir.join("arbol-ultimo.txt");
    let f_revision = dir.join("arbol-revision.txt");
    let Some(mut previo) = leer_texto(&f_ultimo).or_else(|| instantanea(wt, &dir)) else {
        return bloqueado("no se pudo guardar la instantánea del árbol", String::new());
    };
    if !f_ultimo.exists() {
        let _ = std::fs::write(&f_ultimo, &previo);
    }
    let base_revision = leer_texto(&f_revision).unwrap_or_else(|| {
        let _ = std::fs::write(&f_revision, &previo);
        previo.clone()
    });
    // Fases ya informadas de ejecuciones anteriores (las que sigan vigentes).
    let mut informadas: Vec<FaseInforme> = std::fs::read(dir.join("fases-informe.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    informadas.retain(|f| f.numero < fases[inicio_idx].numero);

    let mut fallo: Option<(String, String)> = None;
    let mut evidencia = String::new();
    let mut completa = false;
    let mut idx = inicio_idx;
    while idx < total {
        let f = &fases[idx];
        let titulo = format!("{} · fase {} de {}: {}", orden.titulo, idx + 1, total, f.titulo);
        let cuerpo = format!(
            "## Objetivo general (contexto)\n{}\n\n## Esta fase: {}\n{}\n\nEntregable: {}",
            orden.objetivo, f.titulo, f.instrucciones, f.entregable
        );
        let skills = if f.skills.is_empty() { &orden.skills } else { &f.skills };
        let r = bucle(bus, config, agente, id, orden, wt, &titulo, &cuerpo, &f.criterio, skills);
        evidencia = r.evidencia.clone();
        if !r.pasa {
            let stat = instantanea(wt, &dir).map(|t| diff_entre(wt, &previo, &t, true)).unwrap_or_default();
            let _ = bus.registrar_fase(id, f.numero, "bloqueada", &serde_json::json!({ "motivo": r.motivo, "diff_stat": stat }));
            informadas.push(FaseInforme { numero: f.numero, titulo: f.titulo.clone(), resultado: "bloqueada".into(), diff_stat: stat });
            fallo = Some((format!("fase {}: {}", f.numero, r.motivo.unwrap_or_default()), r.evidencia));
            break;
        }
        let Some(arbol) = instantanea(wt, &dir) else {
            fallo = Some((format!("fase {}: no se pudo guardar la instantánea", f.numero), r.evidencia));
            break;
        };
        let stat = diff_entre(wt, &previo, &arbol, true);
        let _ = bus.registrar_fase(id, f.numero, "hecha", &serde_json::json!({ "diff_stat": stat, "arbol": arbol }));
        informadas.push(FaseInforme { numero: f.numero, titulo: f.titulo.clone(), resultado: "hecha".into(), diff_stat: stat });
        previo = arbol;
        let _ = std::fs::write(&f_ultimo, &previo);
        idx += 1;
        let hechas = idx as u32;
        if idx == total {
            completa = true;
            break;
        }
        if orden.revisar_cada.is_some_and(|k| k > 0 && hechas % k == 0) {
            break;
        }
    }
    let _ = std::fs::write(dir.join("fases-informe.json"), serde_json::to_vec_pretty(&informadas).unwrap_or_default());

    let actual = instantanea(wt, &dir).unwrap_or_else(|| previo.clone());
    let stat = diff_entre(wt, &base_revision, &actual, true);
    let parche = diff_entre(wt, &base_revision, &actual, false);
    let parche = if parche.len() > MAX_DIFF { format!("{}\n…(recortado)", &parche[..corte(&parche, MAX_DIFF)]) } else { parche };
    let hechas = informadas.iter().filter(|f| f.resultado == "hecha").count() as u32;
    if fallo.is_none() {
        // Parada de revisión (o fin): la próxima revisión parte de aquí.
        let _ = std::fs::write(&f_revision, &actual);
    }
    let (resultado, motivo) = match &fallo {
        Some((m, _)) => (ResultadoInforme::Bloqueado, Some(m.clone())),
        None if completa => (ResultadoInforme::Terminado, None),
        None => (ResultadoInforme::Terminado, Some(format!("revisión intermedia tras {hechas} fases; quedan {}", total as u32 - hechas))),
    };
    if let Some((_, ev)) = fallo {
        evidencia = ev;
    }
    Informe {
        evidencia,
        commit_inicio: Some(commit_inicio),
        diff_stat: Some(stat),
        motivo,
        fases_hechas: hechas,
        obra_completa: completa,
        fases: informadas,
        diff: Some(parche),
        ..Informe::nuevo(resultado)
    }
}

const MAX_DIFF: usize = 100_000;

fn corte(s: &str, max: usize) -> usize {
    let mut i = max.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// La evidencia lleva el criterio (≤2 KB) y el resumen del agente (≤1 KB más).
fn recortar_resumen(s: &str) -> String {
    let max = MAX_EVIDENCIA + 1024;
    if s.len() <= max {
        return s.to_string();
    }
    format!("{}…", &s[..corte(s, max)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use ea_bus::{EstadoTrabajo, Modo};
    use std::sync::Mutex;

    /// Doble: ejecuta un script de shell en el worktree en lugar de Qwen.
    struct Doble {
        script: &'static str,
        codigo: i32,
        visto: Mutex<Vec<String>>,
        sistemas: Mutex<Vec<String>>,
    }
    impl AgenteLocal for Doble {
        fn ejecutar(&self, prompt: &str, sistema: &str, wt: &Path, _: &Topes, _: &ConfigOrquestador) -> SalidaAgente {
            self.visto.lock().unwrap().push(prompt.to_string());
            self.sistemas.lock().unwrap().push(sistema.to_string());
            let _ = Command::new("sh").arg("-c").arg(self.script).current_dir(wt).status();
            SalidaAgente { codigo: Some(self.codigo), bruta: "[]".into(), resumen: "hecho".into() }
        }
    }
    fn doble(script: &'static str, codigo: i32) -> Doble {
        Doble { script, codigo, visto: Mutex::new(vec![]), sistemas: Mutex::new(vec![]) }
    }

    fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for a in [&["init", "-q"][..], &["config", "user.email", "t@t"], &["config", "user.name", "t"]] {
            Command::new("git").args(a).current_dir(d.path()).status().unwrap();
        }
        std::fs::write(d.path().join("a.txt"), "a").unwrap();
        for a in [&["add", "."][..], &["commit", "-qm", "inicial"]] {
            Command::new("git").args(a).current_dir(d.path()).status().unwrap();
        }
        d
    }

    fn orden(wt: &Path, criterio: &str) -> Orden {
        Orden {
            id: String::new(),
            modo: Modo::Mision,
            titulo: "prueba".into(),
            worktree: wt.to_string_lossy().into(),
            objetivo: "crea ok.txt".into(),
            criterio_exito: Some(criterio.into()),
            fases: vec![],
            revisar_cada: None,
            topes: Topes::default(),
            rutina: None,
            notas_revision: vec!["usa mayúsculas".into()],
            skills: vec![],
            creada_en: String::new(),
            creada_por: "test".into(),
        }
    }

    fn inf_obra_completa(bus: &Bus, id: &str) -> bool {
        bus.leer_informe(id).unwrap().unwrap().obra_completa
    }

    fn con_store(e: &Path) -> Bus {
        let s = ea_store::Store::open_at(&e.join("ea.db")).unwrap();
        Bus::abrir_en(&e.join("estado")).unwrap().con_store(s)
    }

    fn fases3(wt: &Path, revisar_cada: Option<u32>) -> Orden {
        let fase = |n: u32| ea_bus::Fase {
            numero: n,
            titulo: format!("f{n}"),
            instrucciones: format!("crea el fichero f{n}"),
            entregable: format!("f{n}"),
            criterio: format!("test -f f{n}"),
            skills: vec![],
        };
        Orden {
            modo: Modo::Obra,
            criterio_exito: None,
            fases: vec![fase(1), fase(2), fase(3)],
            revisar_cada,
            topes: Topes { iteraciones: 2, ..Topes::default() },
            ..orden(wt, "")
        }
    }

    /// Crea el siguiente `fN` que falte.
    const SIGUIENTE: &str = "for f in f1 f2 f3; do [ -f $f ] || { echo x > $f; break; }; done";

    fn rutina_orden(wt: &Path, criterio: &str) -> Orden {
        Orden {
            modo: Modo::Rutina,
            titulo: "Rutina prueba".into(),
            rutina: Some(ea_bus::RutinaRef { slug: "prueba".into(), version: 1 }),
            notas_revision: vec![],
            skills: vec![],
            topes: Topes { iteraciones: 1, ..Topes::default() },
            ..orden(wt, criterio)
        }
    }

    fn config() -> ConfigOrquestador {
        ConfigOrquestador { avisos_escritorio: false, ..Default::default() }
    }

    #[test]
    fn criterio_que_pasa_da_terminado_y_encola_en_claude_code() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let id = bus.crear_trabajo(orden(wt.path(), "test -f ok.txt")).unwrap();
        let ag = doble("echo hola > ok.txt", 0);
        assert_eq!(ejecutar_uno(&bus, &config(), &ag), Some(id.clone()));
        assert!(ag.visto.lock().unwrap()[0].contains("usa mayúsculas"));
        assert_eq!(ag.visto.lock().unwrap().len(), 1);
        assert!(inf_obra_completa(&bus, &id));
        assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::EnRevision);
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!(inf.resultado, ResultadoInforme::Terminado);
        assert!(inf.diff_stat.unwrap().contains("ok.txt"));
        assert!(inf.commit_inicio.is_some());
        assert_eq!(bus.listar_cola(Agente::ClaudeCode).unwrap()[0].trabajo, id);
        assert!(e.path().join("trabajos").join(&id).join("salida-qwen.json").exists());
        assert_eq!(ejecutar_uno(&bus, &config(), &ag), None);
    }

    fn config_con_skills(dir: &Path) -> ConfigOrquestador {
        std::fs::write(dir.join("rust.md"), "---\ndescripcion: Rust\n---\nUsa cargo test.\n").unwrap();
        std::fs::write(dir.join("tests.md"), "---\ndescripcion: tests\n---\nPrueba primero.\n").unwrap();
        let mut c = config();
        c.skills.dir = dir.to_string_lossy().into();
        c
    }

    #[test]
    fn las_skills_elegidas_van_al_system_prompt_y_solo_esas() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let sk = tempfile::tempdir().unwrap();
        let cfg = config_con_skills(sk.path());
        let bus = Bus::abrir_en(e.path()).unwrap();
        let o = Orden { skills: vec!["rust".into()], creada_por: "claude-code".into(), ..orden(wt.path(), "test -f ok.txt") };
        let id = bus.crear_trabajo(o).unwrap();
        let ag = doble("echo hola > ok.txt", 0);
        ejecutar_uno(&bus, &cfg, &ag);
        let s = &ag.sistemas.lock().unwrap()[0];
        assert!(s.contains("Usa cargo test."));
        assert!(!s.contains("Prueba primero."));
        assert!(!ag.visto.lock().unwrap()[0].contains("Usa cargo test."));
        assert!(e.path().join("trabajos").join(&id).join("skills-qwen.md").exists());
        assert_eq!(bus.leer_informe(&id).unwrap().unwrap().resultado, ResultadoInforme::Terminado);
    }

    #[test]
    fn sin_llave_las_skills_bloquean_sin_llamar_a_qwen() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let sk = tempfile::tempdir().unwrap();
        let cfg = config_con_skills(sk.path());
        let bus = Bus::abrir_en(e.path()).unwrap();
        let o = Orden { skills: vec!["rust".into()], ..orden(wt.path(), "test -f ok.txt") };
        let id = bus.crear_trabajo(o).unwrap();
        let ag = doble("echo hola > ok.txt", 0);
        ejecutar_uno(&bus, &cfg, &ag);
        assert!(ag.visto.lock().unwrap().is_empty());
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!(inf.resultado, ResultadoInforme::Bloqueado);
        assert!(inf.motivo.unwrap().contains("llave"));
    }

    #[test]
    fn cada_fase_de_obra_puede_llevar_sus_skills() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let sk = tempfile::tempdir().unwrap();
        let cfg = config_con_skills(sk.path());
        let bus = Bus::abrir_en(e.path()).unwrap();
        let mut o = Orden { skills: vec!["rust".into()], creada_por: "claude-code".into(), ..fases3(wt.path(), None) };
        o.fases[1].skills = vec!["tests".into()];
        bus.crear_trabajo(o).unwrap();
        let ag = doble(SIGUIENTE, 0);
        ejecutar_uno(&bus, &cfg, &ag);
        let s = ag.sistemas.lock().unwrap();
        assert_eq!(s.len(), 3);
        assert!(s[0].contains("cargo") && !s[0].contains("Prueba"));
        assert!(s[1].contains("Prueba") && !s[1].contains("cargo"));
        assert!(s[2].contains("cargo"));
    }

    #[test]
    fn criterio_que_falla_da_bloqueado_aunque_el_agente_diga_exito() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let id = bus.crear_trabajo(orden(wt.path(), "echo roto; exit 1")).unwrap();
        ejecutar_uno(&bus, &config(), &doble("true", 0));
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!(inf.resultado, ResultadoInforme::Bloqueado);
        assert_eq!(inf.motivo.as_deref(), Some("iteraciones agotadas"));
        assert!(inf.evidencia.contains("roto"));
        assert_eq!(bus.listar_cola(Agente::ClaudeCode).unwrap()[0].motivo, "bloqueado");
    }

    #[test]
    fn topes_y_error_de_qwen_dan_su_motivo() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let a = bus.crear_trabajo(orden(wt.path(), "false")).unwrap();
        ejecutar_uno(&bus, &config(), &doble("true", 55));
        assert_eq!(bus.leer_informe(&a).unwrap().unwrap().motivo.as_deref(), Some("topes agotados (55)"));
        let b = bus.crear_trabajo(orden(wt.path(), "false")).unwrap();
        ejecutar_uno(&bus, &config(), &doble("true", 1));
        assert_eq!(bus.leer_informe(&b).unwrap().unwrap().motivo.as_deref(), Some("iteraciones agotadas"));
        assert!(bus.leer_informe(&b).unwrap().unwrap().evidencia.contains("código"));
    }

    #[test]
    fn sin_repo_git_se_entrega_bloqueado() {
        let wt = tempfile::tempdir().unwrap();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let id = bus.crear_trabajo(orden(wt.path(), "true")).unwrap();
        ejecutar_uno(&bus, &config(), &doble("true", 0));
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!(inf.resultado, ResultadoInforme::Bloqueado);
        assert_eq!(inf.motivo.as_deref(), Some("el worktree no es un repo git"));
    }

    #[test]
    fn candado_ocupado_vuelve_a_encolar() {
        // La espera real es de 60 s; la prueba usa una corta.
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let id = bus.crear_trabajo(orden(wt.path(), "true")).unwrap();
        let ocupante = Ocupante { agente: "claude-code".into(), trabajo: "x".into(), pid: 0, desde: String::new() };
        let _c = CandadoWorktree::adquirir_en(e.path(), wt.path(), ocupante, Duration::from_secs(1)).unwrap();
        assert_eq!(
            ejecutar_uno_con_espera(&bus, &config(), &doble("true", 0), Duration::from_millis(200)),
            Some(id.clone())
        );
        assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::EnCola);
        assert_eq!(bus.listar_cola(Agente::ModeloLocal).unwrap()[0].motivo, "worktree ocupado");
    }

    #[test]
    fn mision_pasa_en_la_segunda_iteracion_con_la_evidencia_del_fallo() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let id = bus.crear_trabajo(orden(wt.path(), "test -f ok.txt || { echo FALTA-OK; exit 1; }")).unwrap();
        let ag = doble("if [ -f marca ]; then echo hola > ok.txt; else touch marca; fi", 0);
        ejecutar_uno(&bus, &config(), &ag);
        let vistos = ag.visto.lock().unwrap();
        assert_eq!(vistos.len(), 2);
        assert!(!vistos[0].contains("sigue fallando"));
        assert!(vistos[1].contains("El criterio sigue fallando con") && vistos[1].contains("FALTA-OK"));
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!(inf.resultado, ResultadoInforme::Terminado);
        assert_eq!(bus.leer_estado(&id).unwrap().iteracion, 2);
        assert_eq!(bus.listar_cola(Agente::ClaudeCode).unwrap().len(), 1);
    }

    #[test]
    fn mision_que_agota_iteraciones_se_entrega_bloqueada() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let mut o = orden(wt.path(), "false");
        o.topes.iteraciones = 3;
        let id = bus.crear_trabajo(o).unwrap();
        let ag = doble("true", 0);
        ejecutar_uno(&bus, &config(), &ag);
        assert_eq!(ag.visto.lock().unwrap().len(), 3);
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!((inf.resultado, inf.motivo.as_deref()), (ResultadoInforme::Bloqueado, Some("iteraciones agotadas")));
        assert_eq!(bus.leer_estado(&id).unwrap().iteracion, 3);
    }

    #[test]
    fn codigo_55_entrega_sin_reintentar() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        bus.crear_trabajo(orden(wt.path(), "false")).unwrap();
        let ag = doble("true", 55);
        ejecutar_uno(&bus, &config(), &ag);
        assert_eq!(ag.visto.lock().unwrap().len(), 1);
    }

    #[test]
    fn obra_de_tres_fases_para_tras_la_segunda_y_continuar_termina() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let id = bus.crear_trabajo(fases3(wt.path(), Some(2))).unwrap();
        let ag = doble(SIGUIENTE, 0);
        ejecutar_uno(&bus, &config(), &ag);
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!((inf.resultado, inf.fases_hechas, inf.obra_completa), (ResultadoInforme::Terminado, 2, false));
        assert_eq!(inf.fases.len(), 2);
        assert!(inf.diff.as_deref().unwrap().contains("f1") && inf.diff.as_deref().unwrap().contains("f2"));
        assert!(!inf.diff.as_deref().unwrap().contains("+++ b/f3"));
        let st = bus.leer_estado(&id).unwrap();
        assert_eq!((st.estado, st.fase_actual), (EstadoTrabajo::EnRevision, Some(3)));
        assert_eq!(ag.visto.lock().unwrap().len(), 2);
        assert!(ag.visto.lock().unwrap()[1].contains("fase 2 de 3"));
        let fases = ea_store::Store::open_at(&e.path().join("ea.db")).unwrap().listar_fases(&id).unwrap();
        assert_eq!(fases.iter().map(|f| f.estado.as_str()).collect::<Vec<_>>(), ["hecha", "hecha", "pendiente"]);
        assert!(fases[1].informe.as_deref().unwrap().contains("f2"));
        // El índice del usuario sigue intacto.
        let staged = git(wt.path(), &["diff", "--cached", "--name-only"]).unwrap();
        assert!(staged.is_empty(), "índice tocado: {staged}");

        bus.reformular(&id, "").unwrap(); // = ea trabajo continuar
        ejecutar_uno(&bus, &config(), &ag);
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!((inf.fases_hechas, inf.obra_completa, inf.motivo), (3, true, None));
        assert_eq!(inf.fases.len(), 3);
        let d = inf.diff.unwrap();
        assert!(d.contains("+++ b/f3") && !d.contains("+++ b/f1"));
        assert_eq!(bus.leer_estado(&id).unwrap().fase_actual, Some(4));
    }

    #[test]
    fn obra_con_una_fase_fallida_se_entrega_bloqueada() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let id = bus.crear_trabajo(fases3(wt.path(), None)).unwrap();
        let ag = doble("[ -f f1 ] || echo x > f1", 0);
        ejecutar_uno(&bus, &config(), &ag);
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!(inf.resultado, ResultadoInforme::Bloqueado);
        assert!(inf.motivo.unwrap().starts_with("fase 2: iteraciones agotadas"));
        assert_eq!((inf.fases_hechas, inf.obra_completa), (1, false));
        assert_eq!(inf.fases.iter().map(|f| f.resultado.as_str()).collect::<Vec<_>>(), ["hecha", "bloqueada"]);
        assert_eq!(bus.leer_estado(&id).unwrap().fase_actual, Some(2));
    }

    #[test]
    fn rutina_que_pasa_se_cierra_sola_y_no_va_a_ninguna_cola() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let id = bus.crear_trabajo(rutina_orden(wt.path(), "test -f ok.txt")).unwrap();
        ejecutar_uno(&bus, &config(), &doble("echo hola > ok.txt", 0));
        assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::Cerrado);
        assert!(bus.listar_cola(Agente::ClaudeCode).unwrap().is_empty());
        assert!(bus.listar_cola(Agente::ModeloLocal).unwrap().is_empty());
        assert!(!e.path().join("trabajos").join(&id).join("paquete-correccion.md").exists());
    }

    #[test]
    fn rutina_que_falla_queda_en_espera_con_paquete_y_confirmar_la_encola() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let toml = format!(
            "[rutina]\nslug = \"prueba\"\nversion = 1\nworktree = \"{}\"\ncriterio_exito = \"test -f ok.txt\"\n[[pasos]]\ntexto = \"crea ok.txt\"\n",
            wt.path().display()
        );
        bus.registrar_rutina(&toml, &e.path().join("rutinas")).unwrap();
        let id = bus.lanzar_rutina("prueba", &[]).unwrap();
        ejecutar_uno(&bus, &config(), &doble("true", 0));
        assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::EnEspera);
        assert!(bus.listar_cola(Agente::ClaudeCode).unwrap().is_empty());
        let p = std::fs::read_to_string(e.path().join("estado/trabajos").join(&id).join("paquete-correccion.md")).unwrap();
        assert!(p.contains("slug = \"prueba\"") && p.contains("test -f ok.txt") && p.contains("iteraciones agotadas"));
        bus.confirmar(&id, "revisa la ruta").unwrap();
        let c = bus.listar_cola(Agente::ClaudeCode).unwrap();
        assert_eq!((c.len(), c[0].trabajo.as_str(), c[0].motivo.as_str()), (1, id.as_str(), "corrección de rutina"));
        let p = std::fs::read_to_string(e.path().join("estado/trabajos").join(&id).join("paquete-correccion.md")).unwrap();
        assert!(p.contains("revisa la ruta"));
    }
}
