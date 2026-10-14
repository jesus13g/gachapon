//! Ejecutor del agente local: toma un trabajo de la cola `modelo-local`, lo
//! pasa a Qwen Code headless dentro del worktree, comprueba el criterio de
//! éxito POR SU CUENTA (Qwen puede decir «success» sin haber hecho nada) y
//! entrega el informe a la revisión.
//!
//! Seguridad: Qwen Code 0.25 solo trae sandbox con Docker/seatbelt, y aquí no
//! se usa. Con `sandbox = true` en la configuración, `qwen` se lanza dentro de
//! bubblewrap (sistema de ficheros de solo lectura salvo el worktree, `/tmp`
//! y `~/.qwen`). Con `sandbox = false` (por defecto) el agente corre con
//! `--approval-mode yolo`: puede escribir y ejecutar en cualquier sitio donde
//! escriba el usuario. Riesgo asumido; se limita con los topes de la orden,
//! el aislamiento por worktree y `--exclude-tools` para las herramientas web.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use ea_bus::{
    Agente, Bus, CandadoWorktree, ConfigOrquestador, Evento, Informe, Ocupante, Orden, ResultadoInforme, Topes,
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
    fn ejecutar(&self, prompt: &str, worktree: &Path, topes: &Topes, config: &ConfigOrquestador) -> SalidaAgente;
}

/// Qwen Code real, en modo headless.
pub struct QwenCode;

impl AgenteLocal for QwenCode {
    fn ejecutar(&self, prompt: &str, worktree: &Path, topes: &Topes, config: &ConfigOrquestador) -> SalidaAgente {
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
            "--exclude-tools", "web_fetch", "web_search", "--bare", "--auth-type", "openai",
            "--openai-base-url", &config.base_url, "--openai-api-key", "local", "-m", &config.modelo,
            "--approval-mode", "yolo",
        ] {
            args.push(a.into());
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
    let o = Command::new("git").args(args).current_dir(wt).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn construir_prompt(orden: &Orden, criterio: &str) -> String {
    let mut p = format!("# {}\n\n{}\n", orden.titulo, orden.objetivo);
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
    Informe {
        resultado: ResultadoInforme::Bloqueado,
        evidencia,
        commit_inicio: None,
        diff_stat: None,
        motivo: Some(motivo.into()),
        fases_hechas: 0,
        duracion_s: 0,
    }
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
    let _candado = match CandadoWorktree::adquirir_en(bus.raiz(), wt, ocupante, espera) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(%id, error = %e, "worktree ocupado; se vuelve a encolar");
            let _ = bus.encolar(Agente::ModeloLocal, &id, "worktree ocupado");
            return Some(id);
        }
    };
    if let Err(e) = bus.aplicar(&id, Evento::Tomar) {
        tracing::warn!(%id, error = %e, "no se pudo pasar a ejecutando");
        return Some(id);
    }
    let inicio = Instant::now();

    let informe = trabajar(bus, config, agente, &id, &orden, wt, inicio);
    let (titulo, resultado) = (orden.titulo.clone(), informe.motivo.clone().unwrap_or_else(|| "terminado".into()));
    if let Err(e) = bus.aplicar(&id, Evento::Entregar(informe)) {
        tracing::error!(%id, error = %e, "no se pudo entregar el informe");
    }
    drop(_candado);
    avisar(config, &titulo, &resultado);
    Some(id)
}

fn trabajar(
    bus: &Bus,
    config: &ConfigOrquestador,
    agente: &dyn AgenteLocal,
    id: &str,
    orden: &Orden,
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
    let criterio = orden.criterio_exito.clone().unwrap_or_default();
    let prompt = construir_prompt(orden, if criterio.is_empty() { "(sin criterio: comprueba tú mismo tu trabajo)" } else { &criterio });

    let salida = agente.ejecutar(&prompt, wt, &orden.topes, config);
    let dir = bus.raiz().join("trabajos").join(id);
    let _ = std::fs::write(dir.join("salida-qwen.json"), &salida.bruta);

    // El criterio lo ejecuta el SISTEMA, no el agente.
    let (pasa, evidencia) = if criterio.is_empty() {
        (salida.codigo == Some(0), "sin criterio de éxito en la orden".to_string())
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(format!("exec 2>&1; {criterio}")).current_dir(wt);
        let (codigo, out, err) = correr(c, TIEMPO_CRITERIO_S);
        let mut texto = format!("{out}{err}");
        if codigo.is_none() {
            texto.push_str("\n(criterio interrumpido por tiempo)");
        }
        (codigo == Some(0), recortar(&texto))
    };

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

    let motivo = if pasa {
        None
    } else {
        Some(match salida.codigo {
            Some(SALIDA_TOPES) => "topes agotados (55)",
            Some(0) => "criterio falla",
            _ => "error de qwen",
        })
    };
    // Si falló qwen pero el criterio pasa, manda el criterio.
    let mut evidencia = evidencia;
    if !salida.resumen.is_empty() {
        evidencia = format!("{evidencia}\n--- resumen del agente ---\n{}", salida.resumen.trim());
        evidencia = recortar_resumen(&evidencia);
    }
    fin(Informe {
        resultado: if pasa { ResultadoInforme::Terminado } else { ResultadoInforme::Bloqueado },
        evidencia,
        commit_inicio: Some(commit_inicio),
        diff_stat: Some(diff.trim().to_string()),
        motivo: motivo.map(str::to_string),
        fases_hechas: 0,
        duracion_s: 0,
    })
}

/// La evidencia lleva el criterio (≤2 KB) y el resumen del agente (≤1 KB más).
fn recortar_resumen(s: &str) -> String {
    let max = MAX_EVIDENCIA + 1024;
    if s.len() <= max {
        return s.to_string();
    }
    let mut i = max;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    format!("{}…", &s[..i])
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
        visto: Mutex<String>,
    }
    impl AgenteLocal for Doble {
        fn ejecutar(&self, prompt: &str, wt: &Path, _: &Topes, _: &ConfigOrquestador) -> SalidaAgente {
            *self.visto.lock().unwrap() = prompt.to_string();
            let _ = Command::new("sh").arg("-c").arg(self.script).current_dir(wt).status();
            SalidaAgente { codigo: Some(self.codigo), bruta: "[]".into(), resumen: "hecho".into() }
        }
    }
    fn doble(script: &'static str, codigo: i32) -> Doble {
        Doble { script, codigo, visto: Mutex::new(String::new()) }
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
            creada_en: String::new(),
            creada_por: "test".into(),
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
        assert!(ag.visto.lock().unwrap().contains("usa mayúsculas"));
        assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::EnRevision);
        let inf = bus.leer_informe(&id).unwrap().unwrap();
        assert_eq!(inf.resultado, ResultadoInforme::Terminado);
        assert!(inf.diff_stat.unwrap().contains("ok.txt"));
        assert!(inf.commit_inicio.is_some());
        assert_eq!(bus.listar_cola(Agente::ClaudeCode).unwrap()[0].trabajo, id);
        assert!(e.path().join("trabajos").join(&id).join("salida-qwen.json").exists());
        assert_eq!(ejecutar_uno(&bus, &config(), &ag), None);
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
        assert_eq!(inf.motivo.as_deref(), Some("criterio falla"));
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
        assert_eq!(bus.leer_informe(&b).unwrap().unwrap().motivo.as_deref(), Some("error de qwen"));
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
}
