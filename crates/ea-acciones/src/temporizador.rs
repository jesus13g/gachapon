//! El timer de systemd de una rutina (spec 004, H5): el fichero de
//! instancia `ea-rutina@<slug>.timer` en el directorio de unidades de
//! `systemd --user`, con su `OnCalendar=`. El servicio `ea-rutina@.service`
//! ya existe y solo ejecuta `ea rutina lanzar <slug>` (art. V: el timer solo
//! crea el trabajo).
//!
//! Sin shell: `systemctl` y `systemd-analyze` se lanzan con los argumentos
//! como vector, y el calendario llega validado ([`comprobar_calendario`]):
//! sin caracteres de control (un salto de línea inyectaría líneas en el
//! unit), sin `%` (especificadores de systemd) ni `\`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

use crate::sistema::slug_valido;

/// Longitud máxima de una expresión `OnCalendar`.
pub const MAX_CALENDARIO: usize = 200;

/// Cuántas ejecuciones próximas enseña la simulación.
pub const ITERACIONES: u32 = 3;

const LIMITE: Duration = Duration::from_secs(15);

/// Una expresión `OnCalendar` que `systemd-analyze calendar` acepta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calendario {
    /// La forma normalizada que da systemd (`*-*-* 08:00:00`).
    pub normalizado: String,
    /// Las próximas ejecuciones, tal como las escribe systemd.
    pub proximas: Vec<String>,
}

/// Cómo está ahora el timer de una rutina.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EstadoTimer {
    /// Existe el fichero de instancia `ea-rutina@<slug>.timer`.
    pub fichero: bool,
    /// Su `OnCalendar=`, si existe el fichero y lo tiene.
    pub calendario: Option<String>,
    /// `active`, `inactive`… según `systemctl --user is-active`; `None` si
    /// no se pudo preguntar.
    pub activo: Option<String>,
}

impl EstadoTimer {
    pub fn programado(&self) -> bool {
        self.fichero || self.activo.as_deref() == Some("active")
    }
}

/// Validación del texto de un calendario, sin preguntar a systemd. Devuelve
/// el calendario recortado.
pub fn comprobar_calendario(calendario: &str) -> Result<&str, String> {
    // Antes de recortar: un «\r» o un «\n» final también se rechaza.
    if calendario.chars().any(char::is_control) {
        return Err("el calendario no admite saltos de línea ni caracteres de control".into());
    }
    let c = calendario.trim();
    if c.is_empty() {
        return Err("el calendario está vacío (p. ej. «*-*-* 08:00:00»)".into());
    }
    if c.len() > MAX_CALENDARIO {
        return Err(format!("el calendario es demasiado largo ({} bytes; como mucho {MAX_CALENDARIO})", c.len()));
    }
    if let Some(x) = c.chars().find(|ch| matches!(ch, '%' | '\\')) {
        return Err(format!("el calendario no admite «{x}» (especificadores y escapes de systemd)"));
    }
    Ok(c)
}

/// Nombre de la unidad del timer de una rutina.
pub fn unidad_timer(slug: &str) -> String {
    format!("ea-rutina@{slug}.timer")
}

/// El contenido del fichero de instancia: el de la plantilla
/// `crates/ea-orchestrator/systemd/ea-rutina@.timer` con otro `OnCalendar`.
pub fn contenido_timer(slug: &str, calendario: &str) -> String {
    format!(
        "# Generado por ea_programar_rutina; se quita con ea_desprogramar_rutina.\n\
         [Unit]\n\
         Description=Disparador temporal de la rutina '{slug}' del ecosistema de asistente\n\
         \n\
         [Timer]\n\
         OnCalendar={calendario}\n\
         # Como la plantilla: además, una vez 10 min después de arrancar la sesión\n\
         # (la máquina alterna con Windows). No recupera ejecuciones perdidas.\n\
         OnStartupSec=10min\n\
         Persistent=false\n\
         \n\
         [Install]\n\
         WantedBy=timers.target\n"
    )
}

/// El `OnCalendar=` de un fichero de unidad. Si hubiera varios (systemd los
/// suma), se enseña el último.
pub fn leer_calendario(contenido: &str) -> Option<String> {
    contenido
        .lines()
        .filter_map(|l| l.trim().strip_prefix("OnCalendar="))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .next_back()
}

/// La salida de `systemd-analyze calendar --iterations=N`.
pub fn parsear_analisis(salida: &str) -> Option<Calendario> {
    let mut normalizado = None;
    let mut proximas = Vec::new();
    for l in salida.lines() {
        let Some((clave, valor)) = l.trim().split_once(": ") else { continue };
        match clave.trim() {
            "Normalized form" => normalizado = Some(valor.trim().to_string()),
            c if c == "Next elapse" || c.starts_with("Iteration #") => proximas.push(valor.trim().to_string()),
            _ => {}
        }
    }
    Some(Calendario { normalizado: normalizado?, proximas })
}

/// Dónde y con qué se programan los timers. `systemctl` y `systemd_analyze`
/// son la orden y sus primeros argumentos (en las pruebas, `sh guion.sh`);
/// el resto de argumentos se añade como vector.
#[derive(Debug, Clone)]
pub struct Temporizadores {
    /// `~/.config/systemd/user`.
    pub unidades_dir: PathBuf,
    pub systemctl: Vec<String>,
    pub systemd_analyze: Vec<String>,
}

impl Temporizadores {
    /// Los de `systemd --user` de esta sesión: `$XDG_CONFIG_HOME/systemd/user`
    /// (o `~/.config/systemd/user`).
    pub fn usuario() -> Self {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from(".config"));
        Self {
            unidades_dir: config.join("systemd/user"),
            systemctl: vec!["systemctl".into()],
            systemd_analyze: vec!["systemd-analyze".into()],
        }
    }

    /// Ruta del fichero de instancia del timer de `slug`.
    pub fn ruta(&self, slug: &str) -> Result<PathBuf, String> {
        if !slug_valido(slug) {
            return Err(format!("slug de rutina inválido: '{slug}'"));
        }
        Ok(self.unidades_dir.join(unidad_timer(slug)))
    }

    fn leer(&self, ruta: &Path) -> Result<Option<String>, String> {
        match std::fs::read_to_string(ruta) {
            Ok(c) => Ok(Some(c)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("no se pudo leer {}: {e}", ruta.display())),
        }
    }

    /// El `OnCalendar` del fichero de instancia, sin preguntar a systemd.
    pub fn calendario_actual(&self, slug: &str) -> Result<Option<String>, String> {
        Ok(self.leer(&self.ruta(slug)?)?.as_deref().and_then(leer_calendario))
    }

    async fn orden(&self, base: &[String], args: &[&str]) -> Result<std::process::Output, String> {
        let (prog, previos) = base.split_first().ok_or("orden vacía")?;
        let mut cmd = Command::new(prog);
        cmd.args(previos).args(args).kill_on_drop(true);
        timeout(LIMITE, cmd.output())
            .await
            .map_err(|_| format!("{prog} tardó demasiado"))?
            .map_err(|e| format!("no se pudo ejecutar {prog}: {e}"))
    }

    async fn systemctl(&self, args: &[&str]) -> Result<(), String> {
        let mut todos = vec!["--user"];
        todos.extend_from_slice(args);
        let salida = self.orden(&self.systemctl, &todos).await?;
        if salida.status.success() {
            Ok(())
        } else {
            Err(format!(
                "systemctl {}: {}",
                todos.join(" "),
                String::from_utf8_lossy(&salida.stderr).trim()
            ))
        }
    }

    /// Fichero, calendario y si está activo.
    pub async fn estado(&self, slug: &str) -> Result<EstadoTimer, String> {
        let contenido = self.leer(&self.ruta(slug)?)?;
        let unidad = unidad_timer(slug);
        let activo = self
            .orden(&self.systemctl, &["--user", "is-active", &unidad])
            .await
            .ok()
            .map(|s| String::from_utf8_lossy(&s.stdout).trim().to_string())
            .filter(|s| !s.is_empty());
        Ok(EstadoTimer {
            fichero: contenido.is_some(),
            calendario: contenido.as_deref().and_then(leer_calendario),
            activo,
        })
    }

    /// Pregunta a `systemd-analyze calendar` (que no cambia nada). `Ok(Err)`
    /// es un calendario que systemd no acepta; `Err`, que no se pudo preguntar.
    pub async fn validar_calendario(&self, calendario: &str) -> Result<Result<Calendario, String>, String> {
        let c = match comprobar_calendario(calendario) {
            Ok(c) => c,
            Err(e) => return Ok(Err(e)),
        };
        let iter = format!("--iterations={ITERACIONES}");
        let salida = self.orden(&self.systemd_analyze, &["calendar", &iter, "--", c]).await?;
        if !salida.status.success() {
            let err = String::from_utf8_lossy(&salida.stderr).trim().to_string();
            return Ok(Err(if err.is_empty() { format!("systemd no acepta el calendario «{c}»") } else { err }));
        }
        Ok(parsear_analisis(&String::from_utf8_lossy(&salida.stdout))
            .ok_or_else(|| format!("respuesta inesperada de systemd-analyze para «{c}»")))
    }

    /// Escribe el fichero de instancia (atómico), `daemon-reload`, `enable`
    /// y `restart` (para que un timer ya activo tome el horario nuevo). Si
    /// systemd falla, deja el fichero como estaba.
    pub async fn programar(&self, slug: &str, calendario: &str) -> Result<(), String> {
        let c = comprobar_calendario(calendario)?;
        let ruta = self.ruta(slug)?;
        let anterior = self.leer(&ruta)?;
        let escribir = |texto: &str| ea_bus::escribir_atomico(&ruta, texto.as_bytes()).map_err(|e| e.to_string());
        escribir(&contenido_timer(slug, c))?;
        let unidad = unidad_timer(slug);
        let aplicar = async {
            self.systemctl(&["daemon-reload"]).await?;
            self.systemctl(&["enable", &unidad]).await?;
            self.systemctl(&["restart", &unidad]).await
        }
        .await;
        if let Err(e) = aplicar {
            let _ = match &anterior {
                Some(t) => escribir(t),
                None => std::fs::remove_file(&ruta).map_err(|e| e.to_string()),
            };
            let _ = self.systemctl(&["daemon-reload"]).await;
            return Err(format!("{e} (el fichero del timer se dejó como estaba)"));
        }
        Ok(())
    }

    /// Desprograma el timer de una rutina: `disable --now`, borra el fichero
    /// de instancia y `daemon-reload`. `Ok(false)` si no había nada que
    /// quitar. Es lo que reutiliza retirar una rutina.
    pub async fn desprogramar(&self, slug: &str) -> Result<bool, String> {
        let estado = self.estado(slug).await?;
        if !estado.programado() {
            return Ok(false);
        }
        let unidad = unidad_timer(slug);
        self.systemctl(&["disable", "--now", &unidad]).await?;
        let ruta = self.ruta(slug)?;
        if estado.fichero {
            match std::fs::remove_file(&ruta) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("no se pudo borrar {}: {e}", ruta.display())),
            }
        }
        self.systemctl(&["daemon-reload"]).await?;
        Ok(true)
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn analiza_la_salida_de_systemd_analyze() {
        let s = "  Original form: daily\nNormalized form: *-*-* 00:00:00\n    Next elapse: Thu 2026-10-22 00:00:00 CEST\n       (in UTC): Wed 2026-10-21 22:00:00 UTC\n       From now: 7h left\n   Iteration #2: Fri 2026-10-23 00:00:00 CEST\n       (in UTC): x\n   Iteration #3: Sat 2026-10-24 00:00:00 CEST\n";
        let c = parsear_analisis(s).unwrap();
        assert_eq!(c.normalizado, "*-*-* 00:00:00");
        assert_eq!(c.proximas.len(), 3);
        assert_eq!(c.proximas[2], "Sat 2026-10-24 00:00:00 CEST");
        assert!(parsear_analisis("nada").is_none());
    }

    #[test]
    fn el_calendario_no_admite_inyecciones() {
        assert_eq!(comprobar_calendario("  *-*-* 08:00:00 ").unwrap(), "*-*-* 08:00:00");
        for malo in ["", "  ", "daily\nExecStart=/bin/sh", "daily\r", "a\tb", "%h", "a\\b", &"x".repeat(201)] {
            assert!(comprobar_calendario(malo).is_err(), "{malo:?}");
        }
    }

    #[test]
    fn el_fichero_del_timer_lleva_el_calendario() {
        let t = contenido_timer("x", "Mon *-*-* 09:00");
        assert_eq!(leer_calendario(&t).as_deref(), Some("Mon *-*-* 09:00"));
        assert!(t.contains("Persistent=false") && t.contains("WantedBy=timers.target"));
    }
}
