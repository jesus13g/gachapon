//! El bucle del ejecutor: toma un trabajo de la cola del ejecutor
//! (`modelo-local`), lo pasa headless al agente que ocupa ese rol (Qwen
//! Code, Codex u OpenCode: ver [`crate::agentes`]) dentro del worktree,
//! comprueba el criterio de éxito POR SU CUENTA (el agente puede decir
//! «success» sin haber hecho nada) y entrega el informe a la revisión.
//!
//! - Misión: si el criterio falla y quedan iteraciones, repite la llamada en
//!   el mismo worktree con la evidencia del fallo; solo entrega al pasar, al
//!   agotarlas o cuando el agente dice que agotó sus topes.
//! - Obra: recorre las fases con ese mismo bucle, guarda una instantánea del
//!   árbol tras cada una y se detiene para revisión cada `revisar_cada`.
//! - Rutina: si pasa, se cierra sola; si falla, deja un paquete de
//!   corrección en espera hasta que el usuario lo confirme.
//!
//! Seguridad: con `sandbox = true` (por defecto) el agente se lanza dentro de
//! bubblewrap: sistema de ficheros de solo lectura salvo el worktree, `/tmp`
//! propio y sus directorios de estado; `$XDG_RUNTIME_DIR` tapado, para que no
//! lea la clave del panel web; la red queda abierta para llegar a
//! llama-server. Con `sandbox = false` Qwen y OpenCode corren con aprobación
//! automática y pueden escribir y ejecutar donde escriba el usuario. Además
//! limitan los topes de la orden (recortados al techo de los ajustes,
//! `agente.techo_topes`), el aislamiento por worktree y, en cada adaptador,
//! la exclusión de las herramientas web.
//!
//! El agente se elige en cada ciclo ([`fabricar`]) y queda anotado en el
//! trabajo al tomarlo (`ejecutado_por`); un agente que no se puede lanzar
//! deja el trabajo bloqueado con el motivo.
//!
//! Cancelación cooperativa: el estado se relee antes de cada iteración y de
//! cada fase, y mientras corre el agente se sondea cada `SONDEO_CANCELACION`.
//! El agente va en su propio grupo de procesos: si el trabajo pasa a
//! `cancelado`, el grupo recibe `SIGTERM` y, tras `MARGEN_TERM`, `SIGKILL`.
//! Tras una cancelación no se corre el criterio ni se entrega informe; lo que
//! el agente ya escribió en el worktree se queda como está.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ea_bus::{
    Bus, Rol, CandadoWorktree, ConfigOrquestador, Evento, FaseInforme, Informe, Modo, Ocupante,
    Orden, ResultadoInforme,
};

const ESPERA_CANDADO: Duration = Duration::from_secs(60);
const TIEMPO_CRITERIO_S: u64 = 120;
const MAX_EVIDENCIA: usize = 2048;
/// Cada cuánto se relee `estado.json` mientras corre el agente.
const SONDEO_CANCELACION: Duration = Duration::from_secs(2);
/// Lo que se espera tras el `SIGTERM` al grupo antes del `SIGKILL`.
const MARGEN_TERM: Duration = Duration::from_secs(5);
/// Lo que se espera a que se cierren stdout y stderr tras la salida del
/// proceso (un nieto escapado del grupo podría tenerlos abiertos).
const ESPERA_SALIDA: Duration = Duration::from_secs(5);

/// Lo que devuelve el agente tras una ejecución.
#[derive(Debug, Clone, Default)]
pub struct SalidaAgente {
    /// Código de salida (`None` si murió por señal, por tiempo, o no arrancó).
    pub codigo: Option<i32>,
    /// Salida en bruto (para `salida-agente.json`).
    pub bruta: String,
    /// Texto final del agente (resumen), si se pudo extraer.
    pub resumen: String,
    /// `true` si se cortó porque el trabajo se canceló mientras corría.
    pub cancelado: bool,
}

pub use crate::agentes::{fabricar, Ejecutor, QwenCode};

/// Ejecuta con `timeout`; devuelve (código, stdout, stderr). Código `None`
/// si no arrancó o murió por señal (incluido el 124 del timeout, que se
/// convierte en `None`).
fn correr(cmd: Command, tope_s: u64) -> (Option<i32>, String, String) {
    let c = correr_cancelable(cmd, tope_s, &|| false);
    (c.codigo, c.salida, c.errores)
}

/// Resultado de [`correr_cancelable`].
pub(crate) struct Corrida {
    pub codigo: Option<i32>,
    pub salida: String,
    pub errores: String,
    pub cancelado: bool,
}

/// Como [`correr`], pero el proceso (`timeout` y todo lo que cuelga de él,
/// bwrap incluido) va en su propio grupo, y cada `SONDEO_CANCELACION` se
/// pregunta a `cancelado`: si dice que sí, se manda `SIGTERM` al grupo
/// entero y, si el líder no ha salido tras `MARGEN_TERM`, `SIGKILL`. El
/// `SIGKILL` final va siempre, para no dejar nietos sueltos.
pub(crate) fn correr_cancelable(cmd: Command, tope_s: u64, cancelado: &dyn Fn() -> bool) -> Corrida {
    let prog = cmd.get_program().to_os_string();
    let args: Vec<_> = cmd.get_args().map(|a| a.to_os_string()).collect();
    let dir = cmd.get_current_dir().map(|d| d.to_path_buf());
    let mut c = Command::new("timeout");
    c.arg("-k").arg("10").arg(tope_s.to_string()).arg(prog).args(args);
    if let Some(d) = dir {
        c.current_dir(d);
    }
    for (k, v) in cmd.get_envs() {
        match v {
            Some(v) => c.env(k, v),
            None => c.env_remove(k),
        };
    }
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0);
    let mut hijo = match c.spawn() {
        Ok(h) => h,
        Err(e) => return Corrida { codigo: None, salida: String::new(), errores: e.to_string(), cancelado: false },
    };
    let rx_salida = leer_en_hilo(hijo.stdout.take());
    let rx_errores = leer_en_hilo(hijo.stderr.take());
    let mut ultimo_sondeo = Instant::now();
    let mut cancelada = false;
    let estado = loop {
        match hijo.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {}
            Err(_) => break None,
        }
        if ultimo_sondeo.elapsed() >= SONDEO_CANCELACION {
            ultimo_sondeo = Instant::now();
            if cancelado() {
                cancelada = true;
                break matar_grupo(&mut hijo);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let salida = rx_salida.recv_timeout(ESPERA_SALIDA).unwrap_or_default();
    let errores = rx_errores.recv_timeout(ESPERA_SALIDA).unwrap_or_default();
    let codigo = if cancelada { None } else { estado.and_then(|s| s.code()).filter(|c| *c != 124 && *c != 137) };
    Corrida { codigo, salida, errores, cancelado: cancelada }
}

/// Lee todo un flujo del hijo en un hilo; el texto llega por el canal.
fn leer_en_hilo<R: Read + Send + 'static>(flujo: Option<R>) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(mut f) = flujo {
            let _ = f.read_to_end(&mut b);
        }
        let _ = tx.send(String::from_utf8_lossy(&b).into_owned());
    });
    rx
}

/// `SIGTERM` al grupo del hijo (su pgid es su pid: `process_group(0)`),
/// espera hasta `MARGEN_TERM` a que salga el líder y luego `SIGKILL` al
/// grupo, por si queda alguien. Devuelve el estado de salida del líder.
fn matar_grupo(hijo: &mut Child) -> Option<std::process::ExitStatus> {
    let grupo = -(hijo.id() as libc::pid_t);
    // SAFETY: `kill` solo recibe enteros; con un pid negativo señala al
    // grupo que creó `process_group(0)` para este hijo, que aún no se ha
    // recogido (su pid, y por tanto el grupo, no se ha podido reutilizar).
    unsafe { libc::kill(grupo, libc::SIGTERM) };
    let limite = Instant::now() + MARGEN_TERM;
    let mut estado = None;
    while Instant::now() < limite {
        if let Ok(Some(st)) = hijo.try_wait() {
            estado = Some(st);
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // SAFETY: solo enteros. Si el líder ya se recogió y no queda nadie en el
    // grupo, `kill` falla con ESRCH; mientras quede algún miembro, el kernel
    // no reutiliza ese id. Reutilizarlo en estos milisegundos exigiría dar la
    // vuelta entera al espacio de pids.
    unsafe { libc::kill(grupo, libc::SIGKILL) };
    estado.or_else(|| hijo.wait().ok())
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

/// El prompt no nombra al agente ni al modelo; `regla` es la de las
/// herramientas propias del agente (vacía si no tiene).
fn construir_prompt(orden: &Orden, titulo: &str, cuerpo: &str, criterio: &str, regla: &str) -> String {
    let mut p = format!("# {titulo}\n\n{cuerpo}\n");
    if !orden.notas_revision.is_empty() {
        p.push_str("\n## Notas de la revisión anterior (tenlas en cuenta)\n");
        for n in &orden.notas_revision {
            p.push_str(&format!("- {n}\n"));
        }
    }
    p.push_str("\n## Reglas\n- Trabaja SOLO dentro del directorio actual (el worktree); no toques nada fuera.\n- No hagas commits.\n");
    if !regla.is_empty() {
        p.push_str(&format!("- {regla}\n"));
    }
    p.push_str(&format!(
        "- El criterio de éxito es el comando `{criterio}` y debe pasar.\n- Cuando termines, explica en 3 líneas qué hiciste.\n"
    ));
    p
}

/// El lanzador del panel web del checkout con que se compiló el ejecutor:
/// abre `ea-web` en un enlace profundo (`ea-web-open '#trabajos/<id>'`).
const EA_WEB_OPEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../ea-web/bin/ea-web-open");

/// La orden del aviso de escritorio. Con `abrir`, el aviso lleva la acción
/// «abrir», que lanza `abrir '#trabajos/<id>'`. `notify-send --action` espera
/// a que se pulse o se descarte el aviso; si no admite acciones (o falla), se
/// avisa sin ella. Los textos van como argumentos, nunca dentro del guion.
fn orden_aviso(titulo: &str, resultado: &str, id: &str, abrir: Option<&Path>) -> Command {
    let titulo = format!("ea: {titulo}");
    let Some(abrir) = abrir else {
        let mut c = Command::new("notify-send");
        c.arg(titulo).arg(resultado);
        return c;
    };
    let mut c = Command::new("sh");
    c.arg("-c")
        .arg(r#"r=$(notify-send --action=abrir=abrir "$1" "$2") || exec notify-send "$1" "$2"; [ "$r" = abrir ] && exec "$3" "$4"; exit 0"#)
        .arg("ea-aviso")
        .arg(titulo)
        .arg(resultado)
        .arg(abrir)
        .arg(format!("#trabajos/{id}"));
    c
}

/// Aviso de escritorio sobre un trabajo. Desacoplado: el ejecutor no lo
/// espera (un hilo recoge el proceso cuando acaba, para no dejar zombis).
fn avisar(config: &ConfigOrquestador, titulo: &str, resultado: &str, id: &str) {
    if !config.avisos_escritorio {
        return;
    }
    let abrir = Path::new(EA_WEB_OPEN);
    let mut c = orden_aviso(titulo, resultado, id, abrir.is_file().then_some(abrir));
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    match c.spawn() {
        Ok(mut hijo) => {
            std::thread::spawn(move || {
                let _ = hijo.wait();
            });
        }
        Err(e) => tracing::debug!(error = %e, "no se pudo lanzar el aviso de escritorio"),
    }
}

fn bloqueado(motivo: &str, evidencia: String) -> Informe {
    Informe { evidencia, motivo: Some(motivo.into()), ..Informe::nuevo(ResultadoInforme::Bloqueado) }
}

/// Toma y ejecuta un trabajo de la cola `modelo-local`. Devuelve su id, o
/// `None` si no había nada (o la cola falló).
pub fn ejecutar_uno(bus: &Bus, config: &ConfigOrquestador, agente: &dyn Ejecutor) -> Option<String> {
    ejecutar_uno_con_espera(bus, config, agente, ESPERA_CANDADO)
}

fn ejecutar_uno_con_espera(bus: &Bus, config: &ConfigOrquestador, agente: &dyn Ejecutor, espera: Duration) -> Option<String> {
    let id = match bus.tomar(Rol::Ejecutor) {
        Ok(Some(id)) => id,
        Ok(None) => return None,
        Err(e) => {
            tracing::warn!(error = %e, "no se pudo leer la cola modelo-local");
            return None;
        }
    };
    let mut orden = match bus.leer_orden(&id) {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!(%id, error = %e, "orden ilegible; se descarta la entrada");
            return Some(id);
        }
    };
    // Ninguna orden pasa del techo de los ajustes, la escriba quien la escriba.
    let acotados = orden.topes.acotar(&config.techo_topes);
    if acotados != orden.topes {
        tracing::warn!(%id, pedidos = ?orden.topes, techo = ?config.techo_topes, "topes acotados al techo de los ajustes");
        orden.topes = acotados;
    }
    let wt = Path::new(&orden.worktree);

    // Candado antes de pasar a Ejecutando: si está ocupado, el trabajo sigue
    // EnCola y solo se vuelve a poner en la cola.
    let ocupante =
        Ocupante { agente: config.ejecutor.id.clone(), trabajo: id.clone(), pid: 0, desde: String::new(), rol: Some(Rol::Ejecutor) };
    let candado = match CandadoWorktree::adquirir_en(bus.raiz(), wt, ocupante, espera) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(%id, error = %e, "worktree ocupado; se vuelve a encolar");
            let _ = bus.encolar(Rol::Ejecutor, &id, "worktree ocupado");
            return Some(id);
        }
    };
    let fase_actual = match bus.tomar_con(&id, config.ejecutor.ejecutado_por()) {
        Ok(e) => e.fase_actual,
        Err(e) => {
            tracing::warn!(%id, error = %e, "no se pudo pasar a ejecutando");
            return Some(id);
        }
    };
    let inicio = Instant::now();

    // Un ejecutor que no se puede lanzar deja el trabajo bloqueado con el
    // motivo, sin gastar iteraciones (spec 017).
    if let Err(m) = agente.disponible(config) {
        let motivo = format!("el ejecutor {} no está disponible: {m}", config.ejecutor.id);
        tracing::warn!(%id, %motivo, "trabajo bloqueado");
        entregar(bus, config, &id, &orden, bloqueado(&motivo, String::new()));
        return Some(id);
    }

    // El candado se mantiene durante todo el bucle de iteraciones y fases.
    match trabajar(bus, config, agente, &id, &orden, fase_actual, wt, inicio) {
        Ok(informe) => entregar(bus, config, &id, &orden, informe),
        Err(c) => cancelado_en_curso(bus, &id, c.fase),
    }
    drop(candado);
    Some(id)
}

/// El trabajo se canceló mientras corría y el ejecutor dejó de trabajar en
/// él (`fase`: la de la obra en que se cortó). Lo deja en el historial y en
/// el diario; el estado ya es `cancelado` y el worktree queda como esté.
fn cancelado_en_curso(bus: &Bus, id: &str, fase: Option<u32>) {
    let iteracion = bus.leer_estado(id).map(|e| e.iteracion).unwrap_or(0);
    tracing::info!(%id, iteracion, ?fase, "trabajo cancelado en curso; se suelta el worktree");
    if let Err(e) = bus.anotar_cancelacion_en_curso(id, iteracion, fase) {
        tracing::warn!(%id, error = %e, "no se pudo anotar la cancelación en curso");
    }
}

/// Entrega el informe según el modo. Rutina: éxito = se cierra sola; fallo =
/// paquete de corrección en espera, sin encolar a nadie.
fn entregar(bus: &Bus, config: &ConfigOrquestador, id: &str, orden: &Orden, informe: Informe) {
    let pasa = informe.resultado == ResultadoInforme::Terminado;
    let motivo = informe.motivo.clone();
    let paquete = (orden.modo == Modo::Rutina && !pasa).then(|| paquete_correccion(bus, id, orden, &informe));
    if let Err(e) = bus.aplicar(id, Evento::Entregar(informe)) {
        // Cancelado después de la última comprobación (p. ej. mientras corría
        // el criterio): no es un fallo, el trabajo se queda cancelado.
        if bus.esta_cancelado(id) {
            cancelado_en_curso(bus, id, None);
        } else {
            tracing::error!(%id, error = %e, "no se pudo entregar el informe");
        }
        return;
    }
    if orden.modo != Modo::Rutina {
        avisar(config, &orden.titulo, &motivo.unwrap_or_else(|| "terminado".into()), id);
        return;
    }
    let slug = orden.rutina.as_ref().map(|r| r.slug.as_str()).unwrap_or("?");
    if let Some(texto) = paquete {
        if let Err(e) = std::fs::write(bus.raiz().join("trabajos").join(id).join("paquete-correccion.md"), texto) {
            tracing::error!(%id, error = %e, "no se pudo escribir el paquete de corrección");
        }
        if let Err(e) = bus.aplicar(id, Evento::PonerEnEspera)
            && !bus.esta_cancelado(id)
        {
            tracing::error!(%id, error = %e, "no se pudo poner en espera");
        }
        avisar(config, &format!("rutina {slug} falló"), "paquete en espera", id);
    } else {
        if let Err(e) = bus.aplicar(id, Evento::Cerrar)
            && !bus.esta_cancelado(id)
        {
            tracing::error!(%id, error = %e, "no se pudo cerrar la rutina");
        }
        avisar(config, &format!("rutina {slug}"), "terminada", id);
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

/// El trabajo se canceló mientras se ejecutaba: no hay informe que entregar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cancelado {
    /// En una obra, la fase en que se cortó.
    fase: Option<u32>,
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
/// `None` si el trabajo se canceló: antes de cada iteración se relee el
/// estado, y tras una cancelación no se corre el criterio.
#[allow(clippy::too_many_arguments)]
fn bucle(
    bus: &Bus,
    config: &ConfigOrquestador,
    agente: &dyn Ejecutor,
    id: &str,
    orden: &Orden,
    wt: &Path,
    titulo: &str,
    cuerpo: &str,
    criterio: &str,
    skills: &[String],
) -> Option<Bucle> {
    if criterio.trim().is_empty() {
        return Some(Bucle { pasa: false, evidencia: "la orden no trae criterio de éxito".into(), motivo: Some("sin criterio de éxito".into()) });
    }
    let dir = bus.raiz().join("trabajos").join(id);
    let sistema = match config.skills.resolver(&orden.creada_por, skills) {
        Ok(s) => ea_bus::bloque_prompt(&s),
        Err(e) => return Some(Bucle { pasa: false, evidencia: e.to_string(), motivo: Some(e.to_string()) }),
    };
    if !sistema.is_empty() {
        let _ = std::fs::write(dir.join(ea_bus::SKILLS_AGENTE), &sistema);
    }
    let base = construir_prompt(orden, titulo, cuerpo, criterio, agente.regla_herramientas());
    let max = orden.topes.iteraciones.max(1);
    let mut previa: Option<String> = None;
    let cancelado = || bus.esta_cancelado(id);
    for it in 1..=max {
        // Solo se anota (y se llama al agente) si sigue `ejecutando`; la
        // comprobación y la escritura van bajo el bloqueo del trabajo.
        if let Ok(None) = bus.anotar_iteracion(id) {
            return None;
        }
        let prompt = match &previa {
            None => base.clone(),
            Some(ev) => format!(
                "{base}\n## Intento anterior (iteración {})\nEl criterio sigue fallando con:\n\n```\n{ev}\n```\n\nCorrige lo que falta.\n",
                it - 1
            ),
        };
        let salida = agente.ejecutar(&prompt, &sistema, wt, &orden.topes, config, &cancelado);
        let _ = std::fs::write(dir.join(ea_bus::SALIDA_AGENTE), &salida.bruta);
        if salida.cancelado || cancelado() {
            return None;
        }
        let (pasa, ev) = correr_criterio(wt, criterio);
        let mut evidencia = ev.clone();
        if !salida.resumen.is_empty() {
            evidencia = recortar_resumen(&format!("{ev}\n--- resumen del agente ---\n{}", salida.resumen.trim()));
        }
        if pasa {
            return Some(Bucle { pasa: true, evidencia, motivo: None });
        }
        if let Some(c) = agente.codigo_topes().filter(|c| salida.codigo == Some(*c)) {
            return Some(Bucle { pasa: false, evidencia, motivo: Some(format!("topes agotados ({c})")) });
        }
        if it == max {
            if salida.codigo != Some(0) {
                evidencia.push_str(&format!("\n(el agente salió con código {:?})", salida.codigo));
            }
            return Some(Bucle { pasa: false, evidencia, motivo: Some("iteraciones agotadas".into()) });
        }
        previa = Some(ev);
    }
    unreachable!("el bucle siempre devuelve en la última iteración")
}

#[allow(clippy::too_many_arguments)]
fn trabajar(
    bus: &Bus,
    config: &ConfigOrquestador,
    agente: &dyn Ejecutor,
    id: &str,
    orden: &Orden,
    fase_actual: Option<u32>,
    wt: &Path,
    inicio: Instant,
) -> Result<Informe, Cancelado> {
    let fin = |mut i: Informe| {
        i.duracion_s = inicio.elapsed().as_secs();
        Ok(i)
    };
    if !wt.is_dir() {
        return fin(bloqueado("el worktree no existe", orden.worktree.clone()));
    }
    let Some(commit_inicio) = git(wt, &["rev-parse", "HEAD"]) else {
        return fin(bloqueado("el worktree no es un repo git", String::new()));
    };
    if orden.modo == Modo::Obra && !orden.fases.is_empty() {
        return fin(trabajar_obra(bus, config, agente, id, orden, fase_actual, wt, commit_inicio)?);
    }

    let criterio = orden.criterio_exito.clone().unwrap_or_default();
    let r = bucle(bus, config, agente, id, orden, wt, &orden.titulo, &orden.objetivo, &criterio, &orden.skills)
        .ok_or(Cancelado { fase: None })?;

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
/// `revisar_cada` fases hechas (o al terminar todas). Antes de cada fase se
/// relee el estado: si se ha cancelado, se corta sin informe.
#[allow(clippy::too_many_arguments)]
fn trabajar_obra(
    bus: &Bus,
    config: &ConfigOrquestador,
    agente: &dyn Ejecutor,
    id: &str,
    orden: &Orden,
    fase_actual: Option<u32>,
    wt: &Path,
    commit_inicio: String,
) -> Result<Informe, Cancelado> {
    let dir = bus.raiz().join("trabajos").join(id);
    let mut fases = orden.fases.clone();
    fases.sort_by_key(|f| f.numero);
    let total = fases.len();
    // `Bus::reformular` deja `fase_actual` en la fase a rehacer (la pedida o
    // la pendiente; en una obra completa, la última). Sin ella, la última.
    let inicio_idx = fase_actual.and_then(|n| fases.iter().position(|f| f.numero == n)).unwrap_or(total - 1);

    let f_ultimo = dir.join("arbol-ultimo.txt");
    let f_revision = dir.join("arbol-revision.txt");
    let Some(mut previo) = leer_texto(&f_ultimo).or_else(|| instantanea(wt, &dir)) else {
        return Ok(bloqueado("no se pudo guardar la instantánea del árbol", String::new()));
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
        if bus.esta_cancelado(id) {
            return Err(Cancelado { fase: Some(f.numero) });
        }
        let titulo = format!("{} · fase {} de {}: {}", orden.titulo, idx + 1, total, f.titulo);
        let cuerpo = format!(
            "## Objetivo general (contexto)\n{}\n\n## Esta fase: {}\n{}\n\nEntregable: {}",
            orden.objetivo, f.titulo, f.instrucciones, f.entregable
        );
        let skills = if f.skills.is_empty() { &orden.skills } else { &f.skills };
        let r = bucle(bus, config, agente, id, orden, wt, &titulo, &cuerpo, &f.criterio, skills)
            .ok_or(Cancelado { fase: Some(f.numero) })?;
        evidencia = r.evidencia.clone();
        if !r.pasa {
            let stat = instantanea(wt, &dir).map(|t| diff_entre(wt, &previo, &t, true)).unwrap_or_default();
            let _ = bus.registrar_fase(id, f.numero, "bloqueada", &serde_json::json!({ "motivo": r.motivo, "diff_stat": stat }));
            informadas.push(FaseInforme { numero: f.numero, titulo: f.titulo.clone(), resultado: "bloqueada".into(), diff_stat: stat, ejecutado_por: Some(config.ejecutor.ejecutado_por()) });
            fallo = Some((format!("fase {}: {}", f.numero, r.motivo.unwrap_or_default()), r.evidencia));
            break;
        }
        let Some(arbol) = instantanea(wt, &dir) else {
            fallo = Some((format!("fase {}: no se pudo guardar la instantánea", f.numero), r.evidencia));
            break;
        };
        let stat = diff_entre(wt, &previo, &arbol, true);
        let _ = bus.registrar_fase(id, f.numero, "hecha", &serde_json::json!({ "diff_stat": stat, "arbol": arbol }));
        informadas.push(FaseInforme { numero: f.numero, titulo: f.titulo.clone(), resultado: "hecha".into(), diff_stat: stat, ejecutado_por: Some(config.ejecutor.ejecutado_por()) });
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
    Ok(Informe {
        evidencia,
        commit_inicio: Some(commit_inicio),
        diff_stat: Some(stat),
        motivo,
        fases_hechas: hechas,
        obra_completa: completa,
        fases: informadas,
        diff: Some(parche),
        ..Informe::nuevo(resultado)
    })
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
    use ea_bus::{EstadoTrabajo, Modo, Topes};
    use std::sync::Mutex;

    /// Doble: ejecuta un script de shell en el worktree en lugar de Qwen.
    struct Doble {
        script: &'static str,
        codigo: i32,
        visto: Mutex<Vec<String>>,
        sistemas: Mutex<Vec<String>>,
        topes: Mutex<Vec<Topes>>,
    }
    impl Ejecutor for Doble {
        fn ejecutar(&self, prompt: &str, sistema: &str, wt: &Path, topes: &Topes, _: &ConfigOrquestador, _: &dyn Fn() -> bool) -> SalidaAgente {
            self.visto.lock().unwrap().push(prompt.to_string());
            self.topes.lock().unwrap().push(*topes);
            self.sistemas.lock().unwrap().push(sistema.to_string());
            let _ = Command::new("sh").arg("-c").arg(self.script).current_dir(wt).status();
            SalidaAgente { codigo: Some(self.codigo), bruta: "[]".into(), resumen: "hecho".into(), cancelado: false }
        }
        /// Como Qwen Code: 55 es «topes agotados».
        fn codigo_topes(&self) -> Option<i32> {
            Some(55)
        }
    }
    fn argumentos(c: &Command) -> Vec<String> {
        c.get_args().map(|a| a.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn el_aviso_sin_lanzador_es_el_de_siempre() {
        let c = orden_aviso("t", "terminado", "x", None);
        assert_eq!(c.get_program(), "notify-send");
        assert_eq!(argumentos(&c), ["ea: t", "terminado"]);
    }

    #[test]
    fn el_aviso_lleva_la_accion_abrir_con_el_enlace_al_trabajo() {
        // El lanzador existe en el checkout (si se mueve, que falle aquí).
        assert!(Path::new(EA_WEB_OPEN).is_file(), "{EA_WEB_OPEN}");
        let c = orden_aviso("misión $(rm -rf ~)", "terminado", "20261020-101500-ab12", Some(Path::new("/x/ea-web-open")));
        assert_eq!(c.get_program(), "sh");
        let a = argumentos(&c);
        assert!(a[1].contains("--action=abrir=abrir"));
        // Los textos van como argumentos posicionales, no dentro del guion.
        assert!(!a[1].contains("rm -rf"));
        assert_eq!(&a[2..], ["ea-aviso", "ea: misión $(rm -rf ~)", "terminado", "/x/ea-web-open", "#trabajos/20261020-101500-ab12"]);
    }

    #[test]
    fn el_guion_del_aviso_lanza_el_enlace_solo_si_se_pulsa_abrir() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let abierto = d.path().join("abierto");
        let escribe = |nombre: &str, cuerpo: String| {
            let r = bin.join(nombre);
            std::fs::write(&r, format!("#!/bin/sh\n{cuerpo}\n")).unwrap();
            std::fs::set_permissions(&r, std::fs::Permissions::from_mode(0o755)).unwrap();
            r
        };
        let abrir = escribe("abrir", format!("printf %s \"$1\" > '{}'", abierto.display()));
        for (respuesta, esperado) in [("abrir", "#trabajos/x"), ("", "")] {
            let _ = std::fs::remove_file(&abierto);
            escribe("notify-send", format!("echo '{respuesta}'"));
            let st = orden_aviso("t", "r", "x", Some(&abrir))
                .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
                .status()
                .unwrap();
            assert!(st.success());
            assert_eq!(std::fs::read_to_string(&abierto).unwrap_or_default(), esperado, "respuesta {respuesta:?}");
        }
    }

    fn doble(script: &'static str, codigo: i32) -> Doble {
        Doble { script, codigo, visto: Mutex::new(vec![]), sistemas: Mutex::new(vec![]), topes: Mutex::new(vec![]) }
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
    fn el_prompt_no_nombra_al_agente_y_lleva_la_regla_del_adaptador() {
        let o = orden(Path::new("/w"), "test -f ok.txt");
        let neutro = construir_prompt(&o, "t", "c", "test -f ok.txt", "");
        assert!(!neutro.to_lowercase().contains("qwen") && !neutro.contains("run_shell_command"), "{neutro}");
        assert!(neutro.contains("`test -f ok.txt`") && neutro.contains("## Reglas"));
        let qwen = construir_prompt(&o, "t", "c", "x", QwenCode.regla_herramientas());
        assert!(qwen.contains("- Para crear un fichero nuevo usa run_shell_command"));
    }

    #[test]
    fn el_entorno_del_adaptador_llega_al_proceso() {
        let mut c = Command::new("sh");
        c.arg("-c").arg("printf %s \"$EA_PRUEBA\"").env("EA_PRUEBA", "hola");
        assert_eq!(correr_cancelable(c, 10, &|| false).salida, "hola");
    }

    /// T-007 de la spec 017: una misión trivial con el agente de verdad,
    /// dentro de bwrap y contra el endpoint local de los ajustes por defecto.
    /// `EA_AGENTE_REAL=qwen-code|codex|opencode cargo test -p ea-orchestrator
    /// -- --ignored --nocapture agente_real` (`EA_AGENTE_BIN` cambia el binario
    /// `EA_AGENTE_OBJETIVO` la tarea y `EA_AGENTE_BASE_URL` el endpoint).
    #[test]
    #[ignore = "lanza un agente real contra llama-server"]
    fn mision_trivial_con_el_agente_real() {
        let id = std::env::var("EA_AGENTE_REAL").unwrap_or_else(|_| "opencode".into());
        let tipo = ea_bus::TipoAgente::desde(&id).expect("qwen-code, codex u opencode");
        let mut cfg = config();
        cfg.ejecutor.id = id.clone();
        cfg.ejecutor.tipo = tipo;
        cfg.ejecutor.bin = std::env::var("EA_AGENTE_BIN").unwrap_or_else(|_| if id == "qwen-code" { "qwen".into() } else { id.clone() });
        if let Ok(url) = std::env::var("EA_AGENTE_BASE_URL") {
            cfg.ejecutor.modelo.base_url = url;
        }
        assert!(cfg.sandbox, "la prueba es dentro de bwrap");
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let mut o = orden(wt.path(), "test -f ok.txt");
        o.topes = Topes { tiempo_s: 300, iteraciones: 1, ..Topes::default() };
        o.notas_revision.clear(); // «usa mayúsculas» haría crear OK.TXT
        // `EA_AGENTE_OBJETIVO` cambia la tarea (p. ej. «lista tus herramientas»).
        if let Ok(obj) = std::env::var("EA_AGENTE_OBJETIVO") {
            o.objetivo = format!("{obj} Después crea ok.txt.");
        }
        let t = bus.crear_trabajo(o).unwrap();
        let agente = fabricar(&cfg.ejecutor);
        let inicio = Instant::now();
        ejecutar_uno(&bus, &cfg, agente.as_ref());
        let inf = bus.leer_informe(&t).unwrap().unwrap();
        let bruta = std::fs::read_to_string(e.path().join("estado/trabajos").join(&t).join(ea_bus::SALIDA_AGENTE)).unwrap_or_default();
        println!("{id}: {:?} en {:?}\nmotivo: {:?}\nevidencia:\n{}\n--- salida ({} bytes), final:\n{}", inf.resultado, inicio.elapsed(), inf.motivo, inf.evidencia, bruta.len(), &bruta[bruta.len().saturating_sub(1500)..]);
        assert_eq!(inf.resultado, ResultadoInforme::Terminado);
        assert_eq!(inf.ejecutado_por.unwrap().agente, id);
        assert!(inf.evidencia.contains("resumen del agente"), "el resumen se extrajo de la salida");
    }

    /// Doble que, mientras ejecuta, cambia `roles.ejecutor` en los ajustes
    /// (como haría el usuario desde la web) y luego hace el trabajo.
    struct CambiaEjecutor {
        ajustes: std::path::PathBuf,
    }
    impl Ejecutor for CambiaEjecutor {
        fn ejecutar(&self, _: &str, _: &str, wt: &Path, _: &Topes, _: &ConfigOrquestador, _: &dyn Fn() -> bool) -> SalidaAgente {
            std::fs::create_dir_all(self.ajustes.parent().unwrap()).unwrap();
            std::fs::write(&self.ajustes, "[roles]\nejecutor = \"opencode\"\n").unwrap();
            std::fs::write(wt.join("ok.txt"), "").unwrap();
            SalidaAgente { codigo: Some(0), bruta: "[]".into(), resumen: "hecho".into(), cancelado: false }
        }
    }

    #[test]
    fn cambiar_de_ejecutor_en_caliente_no_toca_el_trabajo_en_curso() {
        // Lo que hace `main` en cada ciclo, sobre unos ajustes aislados.
        let d = tempfile::tempdir().unwrap();
        let f = ea_ajustes::Fuentes::aislada(d.path());
        let ciclo = |bus: &Bus, agente: &dyn Ejecutor| {
            let cfg = ConfigOrquestador { avisos_escritorio: false, ..ConfigOrquestador::desde_ajustes(&ea_ajustes::cargar_de(&f).ajustes) };
            ejecutar_uno(bus, &cfg, agente)
        };
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let a = bus.crear_trabajo(orden(wt.path(), "test -f ok.txt")).unwrap();
        let b = bus.crear_trabajo(orden(wt.path(), "test -f ok.txt")).unwrap();
        let cambia = CambiaEjecutor { ajustes: f.dir_config.join("ajustes.toml") };
        assert_eq!(ciclo(&bus, &cambia), Some(a.clone()));
        assert_eq!(ciclo(&bus, &doble("true", 0)), Some(b.clone()));
        let quien = |id: &str| bus.leer_informe(id).unwrap().unwrap().ejecutado_por.unwrap().agente;
        assert_eq!(quien(&a), "qwen-code", "el trabajo en curso termina con el ejecutor con que se tomó");
        assert_eq!(quien(&b), "opencode", "el siguiente sale con el nuevo");
        assert_eq!(bus.leer_estado(&b).unwrap().ejecutado_por.unwrap().modelo, "qwen3-coder-30b");
        // Y en el ciclo siguiente se fabrica el adaptador del nuevo ejecutor.
        let cfg = ConfigOrquestador::desde_ajustes(&ea_ajustes::cargar_de(&f).ajustes);
        assert_eq!(cfg.ejecutor.tipo, ea_bus::TipoAgente::OpenCode);
        assert!(fabricar(&cfg.ejecutor).codigo_topes().is_none(), "no es el de Qwen");
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
        assert_eq!(bus.listar_cola(Rol::Planificador).unwrap()[0].trabajo, id);
        assert!(e.path().join("trabajos").join(&id).join(ea_bus::SALIDA_AGENTE).exists());
        let por = config().ejecutor.ejecutado_por();
        assert_eq!(bus.leer_estado(&id).unwrap().ejecutado_por.as_ref(), Some(&por));
        assert_eq!(inf.ejecutado_por.as_ref(), Some(&por), "el informe dice quién lo hizo");
        assert_eq!(ejecutar_uno(&bus, &config(), &ag), None);
    }

    #[test]
    fn los_topes_de_la_orden_se_recortan_al_techo() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let topes = Topes { turnos: 500, tiempo_s: 9000, llamadas_herramienta: 20, iteraciones: 5 };
        bus.crear_trabajo(Orden { topes, ..orden(wt.path(), "false") }).unwrap();
        let cfg = ConfigOrquestador {
            techo_topes: Topes { turnos: 10, tiempo_s: 120, llamadas_herramienta: 200, iteraciones: 1 },
            ..config()
        };
        let ag = doble("true", 0);
        ejecutar_uno(&bus, &cfg, &ag);
        let vistos = ag.topes.lock().unwrap();
        // techo de iteraciones 1: una sola llamada aunque el criterio falle
        assert_eq!(vistos.len(), 1);
        assert_eq!(vistos[0], Topes { turnos: 10, tiempo_s: 120, llamadas_herramienta: 20, iteraciones: 1 });
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
        let bus = Bus::abrir_en(e.path()).unwrap().con_skills(cfg.skills.clone());
        let o = Orden { skills: vec!["rust".into()], creada_por: "claude-code".into(), ..orden(wt.path(), "test -f ok.txt") };
        let id = bus.crear_trabajo(o).unwrap();
        let ag = doble("echo hola > ok.txt", 0);
        ejecutar_uno(&bus, &cfg, &ag);
        let s = &ag.sistemas.lock().unwrap()[0];
        assert!(s.contains("Usa cargo test."));
        assert!(!s.contains("Prueba primero."));
        assert!(!ag.visto.lock().unwrap()[0].contains("Usa cargo test."));
        assert!(e.path().join("trabajos").join(&id).join(ea_bus::SKILLS_AGENTE).exists());
        assert_eq!(bus.leer_informe(&id).unwrap().unwrap().resultado, ResultadoInforme::Terminado);
    }

    #[test]
    fn sin_llave_las_skills_bloquean_sin_llamar_a_qwen() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let sk = tempfile::tempdir().unwrap();
        let cfg = config_con_skills(sk.path());
        // Al crear, "test" aún tenía la llave; luego los ajustes cambian y el
        // ejecutor vuelve a validar con los de ahora.
        let mut al_crear = cfg.skills.clone();
        al_crear.pueden_elegir.push("test".into());
        let bus = Bus::abrir_en(e.path()).unwrap().con_skills(al_crear);
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
        let bus = Bus::abrir_en(e.path()).unwrap().con_skills(cfg.skills.clone());
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
        assert_eq!(bus.listar_cola(Rol::Planificador).unwrap()[0].motivo, "bloqueado");
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
        let ocupante = Ocupante { agente: "claude-code".into(), trabajo: "x".into(), pid: 0, desde: String::new(), rol: Some(Rol::Planificador) };
        let _c = CandadoWorktree::adquirir_en(e.path(), wt.path(), ocupante, Duration::from_secs(1)).unwrap();
        assert_eq!(
            ejecutar_uno_con_espera(&bus, &config(), &doble("true", 0), Duration::from_millis(200)),
            Some(id.clone())
        );
        assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::EnCola);
        assert_eq!(bus.listar_cola(Rol::Ejecutor).unwrap()[0].motivo, "worktree ocupado");
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
        assert_eq!(bus.listar_cola(Rol::Planificador).unwrap().len(), 1);
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

        bus.reformular(&id, "", None).unwrap(); // = ea trabajo continuar
        // Entre medias cambia el ejecutor: la fase 3 la hace el nuevo.
        let mut otro = config();
        otro.ejecutor.id = "codex".into();
        ejecutar_uno(&bus, &otro, &ag);
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!((inf.fases_hechas, inf.obra_completa, inf.motivo), (3, true, None));
        assert_eq!(inf.fases.len(), 3);
        let quien: Vec<_> = inf.fases.iter().map(|f| f.ejecutado_por.as_ref().unwrap().agente.as_str()).collect();
        assert_eq!(quien, ["qwen-code", "qwen-code", "codex"]);
        let d = inf.diff.unwrap();
        assert!(d.contains("+++ b/f3") && !d.contains("+++ b/f1"));
        assert_eq!(bus.leer_estado(&id).unwrap().fase_actual, Some(4));

        // Un ejecutor que no se puede lanzar no gasta iteraciones: el trabajo
        // queda bloqueado con el motivo.
        let id = bus.crear_trabajo(fases3(wt.path(), None)).unwrap();
        otro.ejecutor.bin = "/no/existe/codex".into();
        ejecutar_uno(&bus, &otro, &crate::agentes::Codex);
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!((inf.resultado, inf.fases_hechas), (ResultadoInforme::Bloqueado, 0));
        let motivo = inf.motivo.unwrap();
        assert!(motivo.contains("codex no está disponible") && motivo.contains("/no/existe/codex"), "{motivo}");
        assert_eq!(bus.leer_estado(&id).unwrap().iteracion, 0);
    }

    #[test]
    fn reformular_una_obra_completa_desde_la_fase_2_rehace_la_2_y_la_3() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let id = bus.crear_trabajo(fases3(wt.path(), None)).unwrap();
        let ag = doble(SIGUIENTE, 0);
        ejecutar_uno(&bus, &config(), &ag);
        assert!(inf_obra_completa(&bus, &id));
        assert_eq!(ag.visto.lock().unwrap().len(), 3);

        bus.reformular(&id, "rehaz desde la 2", Some(2)).unwrap();
        ejecutar_uno(&bus, &config(), &ag);
        let vistos = ag.visto.lock().unwrap();
        assert_eq!(vistos.len(), 5, "solo las fases 2 y 3 vuelven al agente");
        assert!(vistos[3].contains("fase 2 de 3") && vistos[4].contains("fase 3 de 3"));
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!((inf.fases_hechas, inf.obra_completa), (3, true));
        assert_eq!(inf.fases.iter().map(|f| f.numero).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(bus.leer_estado(&id).unwrap().fase_actual, Some(4));
        let fases = ea_store::Store::open_at(&e.path().join("ea.db")).unwrap().listar_fases(&id).unwrap();
        assert!(fases.iter().all(|f| f.estado == "hecha"));
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
        assert!(bus.listar_cola(Rol::Planificador).unwrap().is_empty());
        assert!(bus.listar_cola(Rol::Ejecutor).unwrap().is_empty());
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
        assert!(bus.listar_cola(Rol::Planificador).unwrap().is_empty());
        let p = std::fs::read_to_string(e.path().join("estado/trabajos").join(&id).join("paquete-correccion.md")).unwrap();
        assert!(p.contains("slug = \"prueba\"") && p.contains("test -f ok.txt") && p.contains("iteraciones agotadas"));
        bus.confirmar(&id, "revisa la ruta").unwrap();
        let c = bus.listar_cola(Rol::Planificador).unwrap();
        assert_eq!((c.len(), c[0].trabajo.as_str(), c[0].motivo.as_str()), (1, id.as_str(), "corrección de rutina"));
        let p = std::fs::read_to_string(e.path().join("estado/trabajos").join(&id).join("paquete-correccion.md")).unwrap();
        assert!(p.contains("revisa la ruta"));
    }

    // --- cancelación en curso ----------------------------------------------

    /// Doble que cuenta sus llamadas y, en la primera, cancela el trabajo
    /// desde fuera (con otro `Bus`, como la web o la CLI) antes de volver.
    struct Cancelador {
        raiz: std::path::PathBuf,
        llamadas: std::sync::atomic::AtomicUsize,
    }
    impl Ejecutor for Cancelador {
        fn ejecutar(&self, _: &str, _: &str, _: &Path, _: &Topes, _: &ConfigOrquestador, _: &dyn Fn() -> bool) -> SalidaAgente {
            if self.llamadas.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                let bus = Bus::abrir_en(&self.raiz).unwrap();
                let id = bus.listar_trabajos().unwrap().pop().unwrap();
                bus.aplicar(&id, Evento::Cancelar).unwrap();
            }
            SalidaAgente { codigo: Some(0), bruta: "[]".into(), resumen: "hecho".into(), cancelado: false }
        }
    }

    fn cancelador(bus: &Bus) -> Cancelador {
        Cancelador { raiz: bus.raiz().to_path_buf(), llamadas: Default::default() }
    }

    /// Detalles (JSON) de los `bus_cancelado_en_curso` del diario.
    fn cancelaciones_en_curso(e: &Path) -> Vec<serde_json::Value> {
        ea_store::Store::open_at(&e.join("ea.db"))
            .unwrap()
            .list_events(&Default::default())
            .unwrap()
            .into_iter()
            .filter(|ev| ev.action == "bus_cancelado_en_curso")
            .filter_map(|ev| serde_json::from_str(ev.detail.as_deref()?).ok())
            .collect()
    }

    /// Lo que debe quedar tras cortar un trabajo cancelado: cancelado, sin
    /// informe, en ninguna cola y con el worktree libre.
    fn queda_cancelado_y_suelto(bus: &Bus, id: &str, wt: &Path) {
        assert_eq!(bus.leer_estado(id).unwrap().estado, EstadoTrabajo::Cancelado);
        assert!(bus.leer_informe(id).unwrap().is_none());
        assert!(bus.listar_cola(Rol::Planificador).unwrap().is_empty());
        assert!(bus.listar_cola(Rol::Ejecutor).unwrap().is_empty());
        assert!(CandadoWorktree::ocupante_en(bus.raiz(), wt).is_none(), "el semáforo sigue cogido");
    }

    #[test]
    fn cancelar_entre_iteraciones_no_vuelve_a_llamar_al_agente() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let mut o = orden(wt.path(), "touch criterio-corrio; false");
        o.topes.iteraciones = 3;
        let id = bus.crear_trabajo(o).unwrap();
        let ag = cancelador(&bus);
        assert_eq!(ejecutar_uno(&bus, &config(), &ag), Some(id.clone()));
        assert_eq!(ag.llamadas.load(std::sync::atomic::Ordering::SeqCst), 1);
        // Tras la cancelación no se corre el criterio.
        assert!(!wt.path().join("criterio-corrio").exists());
        queda_cancelado_y_suelto(&bus, &id, wt.path());
        assert_eq!(bus.leer_estado(&id).unwrap().iteracion, 1);
        let c = cancelaciones_en_curso(e.path());
        assert_eq!(c.len(), 1);
        assert_eq!((c[0]["trabajo"].as_str(), c[0]["iteracion"].as_u64(), c[0]["fase"].is_null()), (Some(id.as_str()), Some(1), true));
        let historial = std::fs::read_to_string(bus.raiz().join("trabajos").join(&id).join("historial.jsonl")).unwrap();
        assert!(historial.lines().last().unwrap().contains("cancelado_en_curso"));
    }

    #[test]
    fn cancelar_una_obra_no_empieza_la_fase_siguiente() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let id = bus.crear_trabajo(fases3(wt.path(), None)).unwrap();
        let ag = cancelador(&bus);
        ejecutar_uno(&bus, &config(), &ag);
        assert_eq!(ag.llamadas.load(std::sync::atomic::Ordering::SeqCst), 1);
        queda_cancelado_y_suelto(&bus, &id, wt.path());
        assert_eq!(cancelaciones_en_curso(e.path())[0]["fase"].as_u64(), Some(1));
        // La fase cortada no se registra como hecha ni bloqueada.
        let fases = ea_store::Store::open_at(&e.path().join("ea.db")).unwrap().listar_fases(&id).unwrap();
        assert!(fases.iter().all(|f| f.informe.is_none()), "{fases:?}");
    }

    /// Espera (como mucho `max`) a que exista un fichero.
    fn esperar_fichero(p: &Path, max: Duration) -> bool {
        let limite = Instant::now() + max;
        while Instant::now() < limite {
            if p.exists() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    /// Cancela el trabajo `id` desde otro hilo en cuanto aparezca `marca`.
    fn cancelar_al_ver(raiz: &Path, id: &str, marca: std::path::PathBuf) -> std::thread::JoinHandle<bool> {
        let (raiz, id) = (raiz.to_path_buf(), id.to_string());
        std::thread::spawn(move || {
            if !esperar_fichero(&marca, Duration::from_secs(20)) {
                return false;
            }
            Bus::abrir_en(&raiz).unwrap().aplicar(&id, Evento::Cancelar).is_ok()
        })
    }

    #[test]
    fn cancelar_durante_el_criterio_no_da_error_al_entregar() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        let mut o = orden(wt.path(), "touch marca; sleep 1; true");
        o.topes.iteraciones = 1;
        let id = bus.crear_trabajo(o).unwrap();
        let hilo = cancelar_al_ver(bus.raiz(), &id, wt.path().join("marca"));
        ejecutar_uno(&bus, &config(), &doble("true", 0));
        assert!(hilo.join().unwrap(), "no se pudo cancelar");
        // La entrega llega tarde: el trabajo se queda cancelado, sin informe.
        queda_cancelado_y_suelto(&bus, &id, wt.path());
        assert_eq!(cancelaciones_en_curso(e.path()).len(), 1);
    }

    #[test]
    fn cancelar_durante_la_ejecucion_mata_al_agente() {
        use std::os::unix::fs::PermissionsExt;
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = con_store(e.path());
        // Un «qwen» que deja un nieto (`sleep` largo) en su mismo grupo y lo
        // espera: si solo muriera el hijo, el nieto seguiría vivo.
        let bin = tempfile::tempdir().unwrap();
        let qwen = bin.path().join("qwen-falso");
        std::fs::write(&qwen, "#!/bin/sh\nsleep 600 &\necho $! > pid-nieto\nwait\n").unwrap();
        std::fs::set_permissions(&qwen, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut cfg = ConfigOrquestador { sandbox: false, ..config() };
        cfg.ejecutor.bin = qwen.to_string_lossy().into();
        let mut o = orden(wt.path(), "touch criterio-corrio; false");
        o.topes.iteraciones = 3;
        let id = bus.crear_trabajo(o).unwrap();
        let hilo = cancelar_al_ver(bus.raiz(), &id, wt.path().join("pid-nieto"));
        let t = Instant::now();
        ejecutar_uno(&bus, &cfg, &QwenCode);
        assert!(hilo.join().unwrap(), "no se pudo cancelar");
        // Sondeo (2 s) + margen del TERM (5 s), con holgura.
        assert!(t.elapsed() < Duration::from_secs(15), "tardó {:?}", t.elapsed());
        let nieto: u32 = std::fs::read_to_string(wt.path().join("pid-nieto")).unwrap().trim().parse().unwrap();
        let vivo = |pid: u32| {
            std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .is_ok_and(|s| s.rsplit(')').next().is_some_and(|r| !r.trim_start().starts_with('Z')))
        };
        let limite = Instant::now() + Duration::from_secs(3);
        while vivo(nieto) && Instant::now() < limite {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!vivo(nieto), "el nieto {nieto} sigue vivo");
        assert!(!wt.path().join("criterio-corrio").exists());
        queda_cancelado_y_suelto(&bus, &id, wt.path());
        assert_eq!(bus.leer_estado(&id).unwrap().iteracion, 1);
        assert_eq!(cancelaciones_en_curso(e.path()).len(), 1);
    }
}
