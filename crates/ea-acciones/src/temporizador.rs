//! El timer de systemd de una rutina (spec 004, H5): el fichero de
//! instancia `ea-rutina@<slug>.timer` en el directorio de unidades de
//! `systemd --user`, con su `OnCalendar=`. El servicio `ea-rutina@.service`
//! ya existe y solo ejecuta `ea rutina lanzar <slug>` (art. V: el timer solo
//! crea el trabajo).
//!
//! Fuera de Linux (spec 020, decisión 6) el mismo contrato lo cumplen un
//! LaunchAgent por rutina (macOS) o una tarea del Programador de tareas
//! (Windows), con el subconjunto portable de [`crate::calendario`].
//!
//! Sin shell: `systemctl` y `systemd-analyze` se lanzan con los argumentos
//! como vector, y el calendario llega validado ([`comprobar_calendario`]):
//! sin caracteres de control (un salto de línea inyectaría líneas en el
//! unit), sin `%` (especificadores de systemd) ni `\`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ea_plataforma::Sistema;
use time::{OffsetDateTime, PrimitiveDateTime};
use tokio::process::Command;
use tokio::time::timeout;

use crate::calendario::{self, Horario, IntervaloLaunchd};
use crate::plist;
use crate::servicios::{bloques_show, cita_systemd, dir_unidades, Backend};
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

/// El servicio de instancia `ea-rutina@.service` con la ruta real de `ea`
/// (la plantilla de `crates/ea-orchestrator/systemd/` tiene una fija). No
/// ejecuta la rutina: solo crea el trabajo en el bus (art. V).
pub fn unidad_servicio_rutina(ea: &Path) -> String {
    format!(
        "# Generado por el ecosistema; apunta al `ea` real de este equipo.\n\
         [Unit]\n\
         Description=Dispara la rutina '%i' del ecosistema de asistente\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         # No ejecuta la rutina: solo crea el trabajo en el bus (§10.1 de la especificación).\n\
         # El orquestador residente (ea-orchestrator.service) la recoge cuando puede.\n\
         ExecStart={} rutina lanzar %i\n",
        cita_systemd(&ea.display().to_string())
    )
}

/// `es.gachapon.ea-rutina.<slug>`: la etiqueta del LaunchAgent de una rutina.
pub fn etiqueta_rutina(slug: &str) -> String {
    format!("es.gachapon.ea-rutina.{slug}")
}

/// `ecosistema-asistente\rutina-<slug>`: la tarea de una rutina en Windows.
pub fn tarea_rutina(slug: &str) -> String {
    format!("ecosistema-asistente\\rutina-{slug}")
}

/// El LaunchAgent de una rutina: ejecuta `<ea> rutina lanzar <slug>` según el
/// horario. No arranca al cargarse (`RunAtLoad` falso).
pub fn plist_rutina(slug: &str, ea: &Path, horario: &Horario) -> String {
    let mut c = String::new();
    c.push_str(&plist::cadena("  ", "Label", &etiqueta_rutina(slug)));
    let args = [ea.display().to_string(), "rutina".into(), "lanzar".into(), slug.to_string()];
    c.push_str(&plist::lista_de_cadenas("  ", "ProgramArguments", &args));
    c.push_str(&plist::booleano("  ", "RunAtLoad", false));
    match horario.launchd() {
        IntervaloLaunchd::Cada(s) => c.push_str(&plist::entero("  ", "StartInterval", i64::from(s))),
        IntervaloLaunchd::Calendario(disparos) => {
            c.push_str("  <key>StartCalendarInterval</key>\n  <array>\n");
            for d in disparos {
                c.push_str("    <dict>\n");
                for (clave, valor) in d {
                    c.push_str(&plist::entero("      ", clave, i64::from(valor)));
                }
                c.push_str("    </dict>\n");
            }
            c.push_str("  </array>\n");
        }
    }
    plist::documento(&c)
}

/// Los argumentos de `schtasks /Create` de una rutina (sin privilegios
/// elevados; `/F` reemplaza la tarea si ya existía).
pub fn schtasks_crear_rutina_args(slug: &str, ea: &Path, horario: &Horario) -> Vec<String> {
    let mut v: Vec<String> = vec!["/Create".into(), "/F".into()];
    v.extend(horario.schtasks());
    v.extend(["/RL".into(), "LIMITED".into(), "/TN".into(), tarea_rutina(slug), "/TR".into()]);
    v.push(format!("\"{}\" rutina lanzar {slug}", ea.display()));
    v
}

/// Dónde y con qué se programan los timers de las rutinas, según el sistema:
/// timers de systemd (Linux, con toda la sintaxis `OnCalendar`), LaunchAgents
/// (macOS) o tareas del Programador de tareas (Windows), estos dos con el
/// subconjunto portable de [`crate::calendario`].
///
/// `systemctl` y `systemd_analyze` son la orden y sus primeros argumentos (en
/// las pruebas, `sh guion.sh`); el resto de argumentos se añade como vector.
#[derive(Debug, Clone)]
pub struct Temporizadores {
    pub backend: Backend,
    /// `~/.config/systemd/user` (systemd) o `~/Library/LaunchAgents` (launchd).
    pub unidades_dir: PathBuf,
    pub systemctl: Vec<String>,
    pub systemd_analyze: Vec<String>,
    /// `launchctl` o `schtasks` (fuera de Linux).
    pub gestor: Vec<String>,
    /// Donde launchd y el Programador de tareas guardan el calendario tal
    /// como se pidió (ninguno de los dos lo devuelve): `<slug>.calendario`.
    pub datos_dir: PathBuf,
    /// El `ea` real (junto al binario que corre): lo que ejecutan las
    /// tareas. Sin él no se programa fuera de Linux ni se escribe
    /// `ea-rutina@.service`.
    pub ea: Option<PathBuf>,
    /// El uid de launchd (`gui/<uid>`).
    pub uid: Option<u32>,
    /// «Ahora» (hora local sin zona y desfase respecto de UTC en segundos)
    /// para las pruebas; `None` = el reloj del sistema.
    pub reloj: Option<(PrimitiveDateTime, i32)>,
}

impl Temporizadores {
    /// Los del sistema en el que corre este binario.
    pub fn usuario() -> Self {
        Self::para(Sistema::actual())
    }

    /// Los de un sistema dado (parametrizable para las pruebas).
    pub fn para(sistema: Sistema) -> Self {
        let backend = Backend::de(sistema);
        let ea = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(ea_plataforma::rutas::nombre_exe("ea"))));
        let datos_dir = if backend == Backend::Systemd { PathBuf::new() } else { ea_ajustes::cargar().rutas.estado.join("programadas") };
        Self {
            backend,
            unidades_dir: dir_unidades(backend),
            systemctl: vec!["systemctl".into()],
            systemd_analyze: vec!["systemd-analyze".into()],
            gestor: vec![backend.programa().into()],
            datos_dir,
            ea,
            uid: ea_plataforma::usuario::uid(),
            reloj: None,
        }
    }

    /// Un backend de systemd con esas carpeta y órdenes (las pruebas).
    pub fn systemd(unidades_dir: PathBuf, systemctl: Vec<String>, systemd_analyze: Vec<String>) -> Self {
        Self {
            backend: Backend::Systemd,
            unidades_dir,
            systemctl,
            systemd_analyze,
            gestor: Vec::new(),
            datos_dir: PathBuf::new(),
            ea: None,
            uid: None,
            reloj: None,
        }
    }

    /// Un backend de launchd o del Programador de tareas (las pruebas).
    pub fn portable(backend: Backend, unidades_dir: PathBuf, gestor: Vec<String>, datos_dir: PathBuf, ea: PathBuf, uid: Option<u32>) -> Self {
        Self {
            backend,
            unidades_dir,
            systemctl: Vec::new(),
            systemd_analyze: Vec::new(),
            gestor,
            datos_dir,
            ea: Some(ea),
            uid,
            reloj: None,
        }
    }

    /// Ruta del fichero del timer de `slug`: la unidad de systemd, el plist
    /// de launchd o, en Windows (que no tiene fichero), el del calendario.
    pub fn ruta(&self, slug: &str) -> Result<PathBuf, String> {
        if !slug_valido(slug) {
            return Err(format!("slug de rutina inválido: '{slug}'"));
        }
        Ok(match self.backend {
            Backend::Systemd => self.unidades_dir.join(unidad_timer(slug)),
            Backend::Launchd => self.unidades_dir.join(format!("{}.plist", etiqueta_rutina(slug))),
            Backend::Schtasks => self.ruta_calendario(slug),
        })
    }

    fn ruta_calendario(&self, slug: &str) -> PathBuf {
        self.datos_dir.join(format!("{slug}.calendario"))
    }

    fn leer(&self, ruta: &Path) -> Result<Option<String>, String> {
        match std::fs::read_to_string(ruta) {
            Ok(c) => Ok(Some(c)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("no se pudo leer {}: {e}", ruta.display())),
        }
    }

    /// El calendario programado, sin preguntar al sistema: el `OnCalendar`
    /// del fichero de instancia o el guardado junto al plist/la tarea.
    pub fn calendario_actual(&self, slug: &str) -> Result<Option<String>, String> {
        if self.backend == Backend::Systemd {
            return Ok(self.leer(&self.ruta(slug)?)?.as_deref().and_then(leer_calendario));
        }
        self.ruta(slug)?;
        Ok(self.leer(&self.ruta_calendario(slug))?.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()))
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

    async fn orden_args(&self, base: &[String], args: &[String]) -> Result<std::process::Output, String> {
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        self.orden(base, &a).await
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

    /// Una orden de `launchctl`/`schtasks`; el error lleva su salida de error.
    async fn lanzar_gestor(&self, args: &[String]) -> Result<(), String> {
        let salida = self.orden_args(&self.gestor, args).await?;
        if salida.status.success() {
            return Ok(());
        }
        let e = String::from_utf8_lossy(&salida.stderr).trim().to_string();
        Err(format!("{} {}: {}", self.gestor.join(" "), args.join(" "), if e.is_empty() { salida.status.to_string() } else { e }))
    }

    fn uid_launchd(&self) -> Result<u32, String> {
        self.uid.ok_or_else(|| "no se pudo saber el uid del usuario para launchd".to_string())
    }

    fn dominio_launchd(&self, slug: &str) -> Result<String, String> {
        Ok(format!("gui/{}/{}", self.uid_launchd()?, etiqueta_rutina(slug)))
    }

    fn ea(&self) -> Result<&Path, String> {
        self.ea.as_deref().ok_or_else(|| "no se sabe dónde está el binario `ea` que ejecutarán las tareas".to_string())
    }

    /// «Ahora» en hora local sin zona, y el desfase con UTC en segundos.
    fn ahora(&self) -> (PrimitiveDateTime, i32) {
        if let Some(r) = self.reloj {
            return r;
        }
        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        let desfase = ea_plataforma::reloj::desfase_local(unix).unwrap_or(0);
        let dt = OffsetDateTime::from_unix_timestamp(unix + i64::from(desfase)).unwrap_or(OffsetDateTime::UNIX_EPOCH);
        (PrimitiveDateTime::new(dt.date(), dt.time()), desfase)
    }

    /// Fichero, calendario y si está activo.
    pub async fn estado(&self, slug: &str) -> Result<EstadoTimer, String> {
        let contenido = self.leer(&self.ruta(slug)?)?;
        match self.backend {
            Backend::Systemd => {
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
            Backend::Launchd => {
                let cargado = match self.dominio_launchd(slug) {
                    Ok(d) => self.lanzar_gestor(&["print".into(), d]).await.is_ok(),
                    Err(_) => false,
                };
                Ok(EstadoTimer {
                    fichero: contenido.is_some(),
                    calendario: self.calendario_actual(slug)?,
                    activo: Some(if cargado { "active" } else { "inactive" }.into()),
                })
            }
            Backend::Schtasks => {
                let existe = self.lanzar_gestor(&["/Query".into(), "/TN".into(), tarea_rutina(slug)]).await.is_ok();
                Ok(EstadoTimer {
                    fichero: contenido.is_some(),
                    calendario: self.calendario_actual(slug)?,
                    activo: Some(if existe { "active" } else { "inactive" }.into()),
                })
            }
        }
    }

    /// Valida un calendario. Linux: pregunta a `systemd-analyze calendar` (que
    /// no cambia nada). Fuera de Linux: el subconjunto portable, con las
    /// próximas ejecuciones calculadas aquí. `Ok(Err)` es un calendario que
    /// no se acepta; `Err`, que no se pudo preguntar.
    pub async fn validar_calendario(&self, calendario: &str) -> Result<Result<Calendario, String>, String> {
        let c = match comprobar_calendario(calendario) {
            Ok(c) => c,
            Err(e) => return Ok(Err(e)),
        };
        if self.backend != Backend::Systemd {
            return Ok(match calendario::parsear(c) {
                Ok(h) => {
                    let (ahora, _) = self.ahora();
                    Ok(Calendario {
                        normalizado: h.normalizado(),
                        proximas: h.proximas(ahora, ITERACIONES as usize).into_iter().map(calendario::formatear).collect(),
                    })
                }
                Err(e) => Err(e),
            });
        }
        let iter = format!("--iterations={ITERACIONES}");
        let salida = self.orden(&self.systemd_analyze, &["calendar", &iter, "--", c]).await?;
        if !salida.status.success() {
            let err = String::from_utf8_lossy(&salida.stderr).trim().to_string();
            return Ok(Err(if err.is_empty() { format!("systemd no acepta el calendario «{c}»") } else { err }));
        }
        Ok(parsear_analisis(&String::from_utf8_lossy(&salida.stdout))
            .ok_or_else(|| format!("respuesta inesperada de systemd-analyze para «{c}»")))
    }

    /// Programa la rutina con ese calendario (y reemplaza el horario
    /// anterior). Si el sistema falla, deja los ficheros como estaban.
    pub async fn programar(&self, slug: &str, calendario: &str) -> Result<(), String> {
        let c = comprobar_calendario(calendario)?;
        match self.backend {
            Backend::Systemd => self.programar_systemd(slug, c).await,
            Backend::Launchd => self.programar_launchd(slug, c).await,
            Backend::Schtasks => self.programar_schtasks(slug, c).await,
        }
    }

    /// Escribe el fichero de instancia (atómico), `daemon-reload`, `enable`
    /// y `restart` (para que un timer ya activo tome el horario nuevo). Si
    /// systemd falla, deja el fichero como estaba.
    async fn programar_systemd(&self, slug: &str, c: &str) -> Result<(), String> {
        let ruta = self.ruta(slug)?;
        let anterior = self.leer(&ruta)?;
        let escribir = |texto: &str| ea_bus::escribir_atomico(&ruta, texto.as_bytes()).map_err(|e| e.to_string());
        // El servicio que dispara el timer: si no está, con la ruta real de `ea`.
        if let Some(ea) = &self.ea {
            let servicio = self.unidades_dir.join("ea-rutina@.service");
            if std::fs::symlink_metadata(&servicio).is_err() {
                ea_bus::escribir_atomico(&servicio, unidad_servicio_rutina(ea).as_bytes()).map_err(|e| e.to_string())?;
            }
        }
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

    async fn programar_launchd(&self, slug: &str, c: &str) -> Result<(), String> {
        let horario = calendario::parsear(c)?;
        let ea = self.ea()?.to_path_buf();
        let ruta = self.ruta(slug)?;
        let dominio = self.dominio_launchd(slug)?;
        let anterior = self.leer(&ruta)?;
        let escribir = |texto: &str| ea_bus::escribir_atomico(&ruta, texto.as_bytes()).map_err(|e| e.to_string());
        escribir(&plist_rutina(slug, &ea, &horario))?;
        // Si ya estaba cargado, `bootstrap` falla: se descarga antes.
        let _ = self.lanzar_gestor(&["bootout".into(), dominio]).await;
        let cargar = self.lanzar_gestor(&["bootstrap".into(), format!("gui/{}", self.uid_launchd()?), ruta.display().to_string()]).await;
        if let Err(e) = cargar {
            let _ = match &anterior {
                Some(t) => escribir(t),
                None => std::fs::remove_file(&ruta).map_err(|e| e.to_string()),
            };
            return Err(format!("{e} (el plist se dejó como estaba)"));
        }
        ea_bus::escribir_atomico(&self.ruta_calendario(slug), c.as_bytes()).map_err(|e| e.to_string())
    }

    async fn programar_schtasks(&self, slug: &str, c: &str) -> Result<(), String> {
        let horario = calendario::parsear(c)?;
        let ea = self.ea()?.to_path_buf();
        self.ruta(slug)?;
        self.lanzar_gestor(&schtasks_crear_rutina_args(slug, &ea, &horario)).await?;
        ea_bus::escribir_atomico(&self.ruta_calendario(slug), c.as_bytes()).map_err(|e| e.to_string())
    }

    /// Desprograma el timer de una rutina. `Ok(false)` si no había nada que
    /// quitar. Es lo que reutiliza retirar una rutina.
    pub async fn desprogramar(&self, slug: &str) -> Result<bool, String> {
        let estado = self.estado(slug).await?;
        if !estado.programado() {
            return Ok(false);
        }
        let quitar = |ruta: &Path| match std::fs::remove_file(ruta) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("no se pudo borrar {}: {e}", ruta.display())),
        };
        match self.backend {
            Backend::Systemd => {
                let unidad = unidad_timer(slug);
                self.systemctl(&["disable", "--now", &unidad]).await?;
                if estado.fichero {
                    quitar(&self.ruta(slug)?)?;
                }
                self.systemctl(&["daemon-reload"]).await?;
            }
            Backend::Launchd => {
                // Descargarlo puede fallar si ya no estaba cargado.
                let _ = self.lanzar_gestor(&["bootout".into(), self.dominio_launchd(slug)?]).await;
                quitar(&self.ruta(slug)?)?;
                quitar(&self.ruta_calendario(slug))?;
            }
            Backend::Schtasks => {
                let existe = estado.activo.as_deref() == Some("active");
                if existe {
                    self.lanzar_gestor(&["/Delete".into(), "/TN".into(), tarea_rutina(slug), "/F".into()]).await?;
                }
                quitar(&self.ruta_calendario(slug))?;
            }
        }
        Ok(true)
    }

    /// Próxima ejecución (marca unix, hora absoluta) y estado (`active`,
    /// `inactive`…) del timer de una rutina. Linux: lo dice systemd. Fuera
    /// de Linux, el calendario guardado y [`crate::calendario`]. `None` si no
    /// hay dato. `slug` ya debe estar validado.
    pub async fn proxima(&self, slug: &str) -> Option<(Option<i64>, String)> {
        if self.backend == Backend::Systemd {
            let args = ["--user", "--timestamp=unix", "show", &unidad_timer(slug), "-p", "NextElapseUSecRealtime", "-p", "ActiveState"];
            let salida = self.orden(&self.systemctl, &args).await.ok().filter(|s| s.status.success())?;
            let b = bloques_show(&String::from_utf8_lossy(&salida.stdout)).into_iter().next()?;
            let proxima = b.get("NextElapseUSecRealtime").and_then(|v| v.strip_prefix('@')).and_then(|v| v.parse().ok());
            return Some((proxima, b.get("ActiveState").cloned().unwrap_or_default()));
        }
        let c = self.calendario_actual(slug).ok().flatten()?;
        let (ahora, desfase) = self.ahora();
        let primera = calendario::parsear(&c).ok()?.proximas(ahora, 1).into_iter().next()?;
        let activo = self.estado(slug).await.ok()?.activo.unwrap_or_default();
        Some((Some(primera.assume_utc().unix_timestamp() - i64::from(desfase)), activo))
    }

    /// Una línea legible con la próxima ejecución, para `ea rutina listar`.
    /// Linux: la línea de `systemctl list-timers`; fuera, la fecha calculada.
    pub async fn proxima_legible(&self, slug: &str) -> Option<String> {
        if self.backend == Backend::Systemd {
            let unidad = unidad_timer(slug);
            let salida = self.orden(&self.systemctl, &["--user", "list-timers", &unidad, "--no-legend"]).await.ok().filter(|s| s.status.success())?;
            let texto = String::from_utf8_lossy(&salida.stdout);
            let linea = texto.lines().next()?.trim();
            return (!linea.is_empty()).then(|| linea.to_string());
        }
        let c = self.calendario_actual(slug).ok().flatten()?;
        let (ahora, _) = self.ahora();
        calendario::parsear(&c).ok()?.proximas(ahora, 1).into_iter().next().map(calendario::formatear)
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
