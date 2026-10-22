//! Los servicios residentes del ecosistema (`ea-web`, `ea-mcp`,
//! `ea-orchestrator` y el `llama-server` del usuario) en cada sistema (spec
//! 020, decisión 5): `systemd --user` en Linux, LaunchAgents de launchd en
//! macOS y tareas del Programador de tareas al iniciar sesión en Windows.
//!
//! [`GestorServicios`] pregunta el estado y reinicia; [`plan_instalar`] y
//! [`plan_desinstalar`] generan los pasos de `ea servicios instalar|
//! desinstalar` con la ruta real de los binarios (puros: devuelven pasos, no
//! tocan nada hasta [`aplicar`]). Sin shell: todo son argumentos.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ea_plataforma::Sistema;
use tokio::process::Command;
use tokio::time::timeout;

use crate::plist;
use crate::sistema::EstadoServicio;

/// Los servicios que se instalan con `ea servicios instalar` (el
/// `llama-server` es del usuario y no se toca).
pub const SERVICIOS_INSTALABLES: [&str; 3] = ["ea-mcp", "ea-orchestrator", "ea-web"];

const LIMITE_CONSULTA: Duration = Duration::from_secs(5);
const LIMITE_REINICIO: Duration = Duration::from_secs(30);

/// Con qué gestor de servicios se habla.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Systemd,
    Launchd,
    Schtasks,
}

impl Backend {
    /// El gestor que corresponde a un sistema; lo desconocido se trata como
    /// Linux (es lo que había).
    pub fn de(sistema: Sistema) -> Self {
        match sistema {
            Sistema::MacOs => Backend::Launchd,
            Sistema::Windows => Backend::Schtasks,
            Sistema::Linux | Sistema::Otro => Backend::Systemd,
        }
    }

    /// El programa por defecto con el que se habla.
    pub fn programa(self) -> &'static str {
        match self {
            Backend::Systemd => "systemctl",
            Backend::Launchd => "launchctl",
            Backend::Schtasks => "schtasks",
        }
    }
}

/// `es.gachapon.ea-web`: la etiqueta del LaunchAgent.
pub fn etiqueta_launchd(nombre: &str) -> String {
    format!("es.gachapon.{nombre}")
}

/// `ecosistema-asistente\ea-web`: el nombre de la tarea (en su carpeta).
pub fn tarea_windows(nombre: &str) -> String {
    format!("ecosistema-asistente\\{nombre}")
}

fn cadenas(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_string()).collect()
}

// ------------------------------------------------------------ argumentos

/// `launchctl print gui/<uid>/<etiqueta>`.
pub fn launchd_print_args(uid: u32, nombre: &str) -> Vec<String> {
    vec!["print".into(), format!("gui/{uid}/{}", etiqueta_launchd(nombre))]
}

/// `launchctl kickstart -k gui/<uid>/<etiqueta>`: reinicia aunque corra.
pub fn launchd_reiniciar_args(uid: u32, nombre: &str) -> Vec<String> {
    vec!["kickstart".into(), "-k".into(), format!("gui/{uid}/{}", etiqueta_launchd(nombre))]
}

/// `schtasks /Query /TN <tarea> /FO CSV /NH /V`.
pub fn schtasks_query_args(nombre: &str) -> Vec<String> {
    let mut v = cadenas(&["/Query", "/TN"]);
    v.push(tarea_windows(nombre));
    v.extend(cadenas(&["/FO", "CSV", "/NH", "/V"]));
    v
}

fn schtasks_con_tarea(accion: &str, nombre: &str) -> Vec<String> {
    vec![accion.into(), "/TN".into(), tarea_windows(nombre)]
}

// ------------------------------------------------------------- lectura

/// El estado de `launchctl print`: `state = running` es activo.
pub fn parsear_launchd_print(texto: &str) -> EstadoServicio {
    let estado = texto.lines().find_map(|l| l.trim().strip_prefix("state = ")).map(str::trim);
    EstadoServicio { cargado: true, activo: if estado == Some("running") { "active" } else { "inactive" }.into() }
}

/// Divide una línea CSV de `schtasks` (todo entre comillas).
fn campos_csv(linea: &str) -> Vec<String> {
    let mut campos = Vec::new();
    let mut actual = String::new();
    let mut dentro = false;
    let mut c = linea.chars().peekable();
    while let Some(ch) = c.next() {
        match ch {
            '"' if dentro && c.peek() == Some(&'"') => {
                actual.push('"');
                c.next();
            }
            '"' => dentro = !dentro,
            ',' if !dentro => campos.push(std::mem::take(&mut actual)),
            ch => actual.push(ch),
        }
    }
    campos.push(actual);
    campos
}

/// El estado de `schtasks /Query /FO CSV /NH /V`: la cuarta columna es el
/// estado de la tarea (`Running`/`Ready`/`Disabled`…, en el idioma del
/// sistema; se reconocen inglés y español, lo demás cuenta como parada).
pub fn parsear_schtasks_csv(texto: &str) -> EstadoServicio {
    let estado = texto
        .lines()
        .map(campos_csv)
        .find(|c| c.len() >= 4)
        .map(|c| c[3].trim().to_lowercase())
        .unwrap_or_default();
    let corriendo = ["running", "en ejecución", "en ejecucion", "em execução", "en cours d'exécution", "wird ausgeführt", "in esecuzione"]
        .contains(&estado.as_str());
    EstadoServicio { cargado: true, activo: if corriendo { "active" } else { "inactive" }.into() }
}

/// Lo que enseña el panel de cada servicio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoServicio {
    pub nombre: String,
    /// `active`, `inactive`, `failed`… o `desconocido`.
    pub activo: String,
    /// Segundos que lleva activo; solo con systemd (con el reloj monotónico,
    /// no con la hora de pared: puede estar desajustada al arrancar).
    pub activo_s: Option<i64>,
}

/// `systemctl --user show` por bloques: un mapa por unidad.
pub(crate) fn bloques_show(texto: &str) -> Vec<HashMap<String, String>> {
    texto
        .split("\n\n")
        .map(|b| {
            b.lines()
                .filter_map(|l| l.split_once('='))
                .map(|(k, v)| (k.to_string(), v.trim().to_string()))
                .collect::<HashMap<_, _>>()
        })
        .filter(|m| !m.is_empty())
        .collect()
}

/// Segundos desde un `ActiveEnterTimestampMonotonic` (µs desde el arranque;
/// 0 si la unidad nunca ha estado activa).
fn segundos_activo(valor: Option<&String>, ahora_s: Option<f64>) -> Option<i64> {
    let desde: i64 = valor?.parse().ok().filter(|&us| us > 0)?;
    Some(((ahora_s? * 1e6) as i64 - desde).div_euclid(1_000_000).max(0))
}

// ------------------------------------------------------------- gestor

/// Pregunta y reinicia los servicios con el gestor del sistema.
#[derive(Debug, Clone)]
pub struct GestorServicios {
    pub backend: Backend,
    /// La orden y sus primeros argumentos (`["systemctl"]`; en las pruebas,
    /// `["sh", "guion.sh"]`).
    pub orden: Vec<String>,
    /// El uid de launchd (`gui/<uid>`).
    pub uid: Option<u32>,
}

impl GestorServicios {
    /// El gestor de un sistema dado (parametrizable para las pruebas).
    pub fn para(sistema: Sistema) -> Self {
        let backend = Backend::de(sistema);
        Self { backend, orden: vec![backend.programa().into()], uid: ea_plataforma::usuario::uid() }
    }

    /// El del sistema en el que corre este binario.
    pub fn usuario() -> Self {
        Self::para(Sistema::actual())
    }

    async fn correr(&self, args: &[String], limite: Duration) -> Result<std::process::Output, String> {
        let (prog, previos) = self.orden.split_first().ok_or("orden vacía")?;
        let mut cmd = Command::new(prog);
        cmd.args(previos).args(args).kill_on_drop(true);
        timeout(limite, cmd.output())
            .await
            .map_err(|_| format!("{prog} tardó demasiado"))?
            .map_err(|e| format!("no se pudo ejecutar {prog}: {e}"))
    }

    fn uid(&self) -> Result<u32, String> {
        self.uid.ok_or_else(|| "no se pudo saber el uid del usuario para launchd".to_string())
    }

    /// Si el servicio existe y si está activo. Un servicio que el gestor no
    /// conoce no es un error: `cargado: false`.
    pub async fn estado(&self, nombre: &str) -> Result<EstadoServicio, String> {
        match self.backend {
            Backend::Systemd => {
                let args = cadenas(&["--user", "show", "-p", "LoadState", "-p", "ActiveState"])
                    .into_iter()
                    .chain([format!("{nombre}.service")])
                    .collect::<Vec<_>>();
                let salida = self.correr(&args, LIMITE_CONSULTA).await?;
                if !salida.status.success() {
                    return Err(String::from_utf8_lossy(&salida.stderr).trim().to_string());
                }
                let texto = String::from_utf8_lossy(&salida.stdout);
                let valor = |clave: &str| {
                    texto
                        .lines()
                        .find_map(|l| l.strip_prefix(clave).and_then(|r| r.strip_prefix('=')))
                        .unwrap_or("")
                        .trim()
                        .to_string()
                };
                Ok(EstadoServicio { cargado: valor("LoadState") == "loaded", activo: valor("ActiveState") })
            }
            Backend::Launchd => {
                let salida = self.correr(&launchd_print_args(self.uid()?, nombre), LIMITE_CONSULTA).await?;
                if !salida.status.success() {
                    return Ok(EstadoServicio { cargado: false, activo: "inactive".into() });
                }
                Ok(parsear_launchd_print(&String::from_utf8_lossy(&salida.stdout)))
            }
            Backend::Schtasks => {
                let salida = self.correr(&schtasks_query_args(nombre), LIMITE_CONSULTA).await?;
                if !salida.status.success() {
                    return Ok(EstadoServicio { cargado: false, activo: "inactive".into() });
                }
                Ok(parsear_schtasks_csv(&String::from_utf8_lossy(&salida.stdout)))
            }
        }
    }

    /// Reinicia un servicio. Quien llama ya ha comprobado la lista cerrada.
    pub async fn reiniciar(&self, nombre: &str) -> Result<(), String> {
        let salida = match self.backend {
            Backend::Systemd => {
                let args = cadenas(&["--user", "restart"]).into_iter().chain([format!("{nombre}.service")]).collect::<Vec<_>>();
                self.correr(&args, LIMITE_REINICIO).await?
            }
            Backend::Launchd => self.correr(&launchd_reiniciar_args(self.uid()?, nombre), LIMITE_REINICIO).await?,
            Backend::Schtasks => {
                // Parar una tarea que no corre falla: no importa.
                let _ = self.correr(&schtasks_con_tarea("/End", nombre), LIMITE_REINICIO).await;
                self.correr(&schtasks_con_tarea("/Run", nombre), LIMITE_REINICIO).await?
            }
        };
        if salida.status.success() {
            Ok(())
        } else {
            let e = String::from_utf8_lossy(&salida.stderr).trim().to_string();
            Err(if e.is_empty() { format!("{} terminó con {}", self.backend.programa(), salida.status) } else { e })
        }
    }

    /// El estado de varios servicios para el panel. `None` si el gestor no
    /// responde (el panel dice «desconocido»). Con systemd, una sola
    /// invocación y el tiempo activo; con los demás, una consulta por
    /// servicio y sin tiempo activo.
    pub async fn panel(&self, nombres: &[&str]) -> Option<Vec<InfoServicio>> {
        if self.backend != Backend::Systemd {
            let mut v = Vec::new();
            for n in nombres {
                let activo = self.estado(n).await.ok()?.activo;
                v.push(InfoServicio { nombre: (*n).to_string(), activo, activo_s: None });
            }
            return Some(v);
        }
        let mut args = cadenas(&["--user", "--timestamp=unix", "show"]);
        args.extend(nombres.iter().map(|n| format!("{n}.service")));
        for p in ["Id", "ActiveState", "ActiveEnterTimestampMonotonic"] {
            args.push("-p".into());
            args.push(p.into());
        }
        let salida = self.correr(&args, Duration::from_secs(3)).await.ok()?;
        if !salida.status.success() {
            return None;
        }
        let bloques = bloques_show(&String::from_utf8_lossy(&salida.stdout));
        let ahora = ea_plataforma::reloj::segundos_desde_arranque();
        Some(
            nombres
                .iter()
                .map(|n| {
                    let id = format!("{n}.service");
                    let b = bloques.iter().find(|b| b.get("Id") == Some(&id));
                    InfoServicio {
                        nombre: (*n).to_string(),
                        activo: b.and_then(|b| b.get("ActiveState").cloned()).unwrap_or_else(|| "desconocido".into()),
                        activo_s: b.and_then(|b| segundos_activo(b.get("ActiveEnterTimestampMonotonic"), ahora)),
                    }
                })
                .collect(),
        )
    }
}

// ------------------------------------------------------ unidades (puras)

struct Def {
    nombre: &'static str,
    descripcion: &'static str,
    despues: &'static str,
    quiere: Option<&'static str>,
}

const DEFS: [Def; 3] = [
    Def { nombre: "ea-mcp", descripcion: "Servidor de herramientas MCP del ecosistema de asistente", despues: "network.target", quiere: None },
    Def {
        nombre: "ea-orchestrator",
        descripcion: "Orquestador de rutinas del ecosistema de asistente",
        despues: "network.target ea-mcp.service",
        quiere: Some("ea-mcp.service"),
    },
    Def {
        nombre: "ea-web",
        descripcion: "Interfaz web del ecosistema de asistente",
        despues: "network.target ea-mcp.service",
        quiere: Some("ea-mcp.service"),
    },
];

/// Un argumento de `ExecStart=` de systemd entre comillas: `\` y `"` se
/// escapan y `%` se duplica (especificadores).
pub fn cita_systemd(t: &str) -> String {
    let mut s = String::from("\"");
    for c in t.chars() {
        match c {
            '\\' => s.push_str("\\\\"),
            '"' => s.push_str("\\\""),
            '%' => s.push_str("%%"),
            c => s.push(c),
        }
    }
    s.push('"');
    s
}

/// La unidad `systemd --user` de un servicio, con la ruta real del binario.
/// Equivale a las plantillas de `crates/*/systemd/*.service`.
pub fn unidad_systemd(nombre: &str, exe: &Path) -> Option<String> {
    let d = DEFS.iter().find(|d| d.nombre == nombre)?;
    let wants = d.quiere.map(|w| format!("Wants={w}\n")).unwrap_or_default();
    Some(format!(
        "# Generado por `ea servicios instalar`; se quita con `ea servicios desinstalar`.\n\
         [Unit]\n\
         Description={}\n\
         After={}\n\
         {wants}\n\
         [Service]\n\
         Type=simple\n\
         ExecStart={}\n\
         Restart=on-failure\n\
         RestartSec=2\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        d.descripcion,
        d.despues,
        cita_systemd(&exe.display().to_string())
    ))
}

/// El LaunchAgent de un servicio: arranca al iniciar sesión y se reinicia si
/// termina con error. Los registros van a `<logs>/<nombre>.log`.
pub fn plist_servicio(nombre: &str, exe: &Path, logs: &Path) -> String {
    let log = logs.join(format!("{nombre}.log")).display().to_string();
    let mut c = String::new();
    c.push_str(&plist::cadena("  ", "Label", &etiqueta_launchd(nombre)));
    c.push_str(&plist::lista_de_cadenas("  ", "ProgramArguments", &[exe.display().to_string()]));
    c.push_str(&plist::booleano("  ", "RunAtLoad", true));
    c.push_str("  <key>KeepAlive</key>\n  <dict>\n    <key>SuccessfulExit</key>\n    <false/>\n  </dict>\n");
    c.push_str(&plist::entero("  ", "ThrottleInterval", 2));
    c.push_str(&plist::cadena("  ", "StandardOutPath", &log));
    c.push_str(&plist::cadena("  ", "StandardErrorPath", &log));
    plist::documento(&c)
}

/// `schtasks /Create` de un servicio: al iniciar sesión, sin privilegios
/// elevados. `/F` reemplaza la tarea si ya existía.
pub fn schtasks_crear_args(nombre: &str, exe: &Path) -> Vec<String> {
    let mut v = cadenas(&["/Create", "/F", "/SC", "ONLOGON", "/RL", "LIMITED", "/TN"]);
    v.push(tarea_windows(nombre));
    v.push("/TR".into());
    v.push(format!("\"{}\"", exe.display()));
    v
}

// ------------------------------------------------------------- planes

/// Un paso de instalar o desinstalar. [`aplicar`] los ejecuta en orden.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Paso {
    CrearDir(PathBuf),
    Escribir { ruta: PathBuf, contenido: String },
    Borrar(PathBuf),
    /// Una orden externa; con `tolerar`, si falla se sigue (p. ej. parar lo
    /// que ya estaba parado).
    Ejecutar { argv: Vec<String>, tolerar: bool },
}

impl Paso {
    /// Una línea para enseñarlo al usuario.
    pub fn describir(&self) -> String {
        match self {
            Paso::CrearDir(d) => format!("crear la carpeta {}", d.display()),
            Paso::Escribir { ruta, .. } => format!("escribir {}", ruta.display()),
            Paso::Borrar(r) => format!("borrar {}", r.display()),
            Paso::Ejecutar { argv, tolerar } => format!("ejecutar {}{}", argv.join(" "), if *tolerar { " (si falla, se sigue)" } else { "" }),
        }
    }
}

/// Dónde se instala y con qué binarios.
#[derive(Debug, Clone)]
pub struct Entorno {
    pub backend: Backend,
    /// Carpeta de los binarios (la de `ea`, por defecto): de aquí sale la
    /// ruta real de cada servicio.
    pub exes_dir: PathBuf,
    /// Carpeta de las unidades: `~/.config/systemd/user` o
    /// `~/Library/LaunchAgents`. Windows no escribe ficheros.
    pub unidades_dir: PathBuf,
    /// Registros de los LaunchAgents.
    pub logs_dir: PathBuf,
    pub uid: Option<u32>,
}

/// La carpeta de unidades de usuario de un sistema, sin crearla: Linux
/// `$XDG_CONFIG_HOME/systemd/user` (o `~/.config/systemd/user`), macOS
/// `~/Library/LaunchAgents`. Windows no usa ficheros: devuelve una carpeta
/// vacía.
pub fn dir_unidades(backend: Backend) -> PathBuf {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    match backend {
        Backend::Systemd => std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home.map(|h| h.join(".config")))
            .unwrap_or_else(|| PathBuf::from(".config"))
            .join("systemd/user"),
        Backend::Launchd => home.unwrap_or_default().join("Library/LaunchAgents"),
        Backend::Schtasks => PathBuf::new(),
    }
}

impl Entorno {
    /// El del sistema actual, con los binarios junto al que corre.
    pub fn usuario() -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| format!("no se pudo saber dónde está este binario: {e}"))?;
        let exes_dir = exe.parent().ok_or("el binario no tiene carpeta")?.to_path_buf();
        let backend = Backend::de(Sistema::actual());
        Ok(Self {
            backend,
            exes_dir,
            unidades_dir: dir_unidades(backend),
            logs_dir: ea_ajustes::cargar().rutas.estado.join("logs"),
            uid: ea_plataforma::usuario::uid(),
        })
    }

    /// La ruta real del binario de `base` (`ea-web` → `…/ea-web.exe` en
    /// Windows).
    pub fn exe(&self, base: &str) -> PathBuf {
        self.exes_dir.join(ea_plataforma::rutas::nombre_exe(base))
    }

    /// Los binarios que faltan (`ea` incluido), para avisar antes de
    /// instalar.
    pub fn faltan(&self) -> Vec<PathBuf> {
        SERVICIOS_INSTALABLES.iter().copied().chain(["ea"]).map(|b| self.exe(b)).filter(|p| !p.is_file()).collect()
    }

    fn ruta_unidad(&self, nombre: &str) -> PathBuf {
        match self.backend {
            Backend::Launchd => self.unidades_dir.join(format!("{}.plist", etiqueta_launchd(nombre))),
            _ => self.unidades_dir.join(format!("{nombre}.service")),
        }
    }
}

fn ejecutar(argv: Vec<String>, tolerar: bool) -> Paso {
    Paso::Ejecutar { argv, tolerar }
}

/// Los pasos para instalar y arrancar `ea-mcp`, `ea-orchestrator` y
/// `ea-web`. En Linux también la plantilla `ea-rutina@.service` de las
/// rutinas programadas, apuntando al `ea` real.
pub fn plan_instalar(env: &Entorno) -> Result<Vec<Paso>, String> {
    let mut p = Vec::new();
    match env.backend {
        Backend::Systemd => {
            for n in SERVICIOS_INSTALABLES {
                let contenido = unidad_systemd(n, &env.exe(n)).ok_or("servicio desconocido")?;
                p.push(Paso::Escribir { ruta: env.ruta_unidad(n), contenido });
            }
            p.push(Paso::Escribir {
                ruta: env.unidades_dir.join("ea-rutina@.service"),
                contenido: crate::temporizador::unidad_servicio_rutina(&env.exe("ea")),
            });
            p.push(ejecutar(cadenas(&["systemctl", "--user", "daemon-reload"]), false));
            for n in SERVICIOS_INSTALABLES {
                p.push(ejecutar(vec!["systemctl".into(), "--user".into(), "enable".into(), "--now".into(), format!("{n}.service")], false));
            }
        }
        Backend::Launchd => {
            let uid = env.uid.ok_or("no se pudo saber el uid del usuario para launchd")?;
            p.push(Paso::CrearDir(env.logs_dir.clone()));
            for n in SERVICIOS_INSTALABLES {
                let ruta = env.ruta_unidad(n);
                p.push(Paso::Escribir { ruta: ruta.clone(), contenido: plist_servicio(n, &env.exe(n), &env.logs_dir) });
                // Si ya estaba cargado, `bootstrap` falla: se descarga antes.
                p.push(ejecutar(vec!["launchctl".into(), "bootout".into(), format!("gui/{uid}/{}", etiqueta_launchd(n))], true));
                p.push(ejecutar(vec!["launchctl".into(), "bootstrap".into(), format!("gui/{uid}"), ruta.display().to_string()], false));
            }
        }
        Backend::Schtasks => {
            for n in SERVICIOS_INSTALABLES {
                let mut crear = vec!["schtasks".to_string()];
                crear.extend(schtasks_crear_args(n, &env.exe(n)));
                p.push(ejecutar(crear, false));
                let mut run = vec!["schtasks".to_string()];
                run.extend(schtasks_con_tarea("/Run", n));
                p.push(ejecutar(run, true));
            }
        }
    }
    Ok(p)
}

/// Los pasos para parar y quitar los tres servicios. No toca las rutinas
/// programadas ni la plantilla `ea-rutina@.service`.
pub fn plan_desinstalar(env: &Entorno) -> Result<Vec<Paso>, String> {
    let mut p = Vec::new();
    match env.backend {
        Backend::Systemd => {
            for n in SERVICIOS_INSTALABLES {
                p.push(ejecutar(vec!["systemctl".into(), "--user".into(), "disable".into(), "--now".into(), format!("{n}.service")], true));
                p.push(Paso::Borrar(env.ruta_unidad(n)));
            }
            p.push(ejecutar(cadenas(&["systemctl", "--user", "daemon-reload"]), false));
        }
        Backend::Launchd => {
            let uid = env.uid.ok_or("no se pudo saber el uid del usuario para launchd")?;
            for n in SERVICIOS_INSTALABLES {
                p.push(ejecutar(vec!["launchctl".into(), "bootout".into(), format!("gui/{uid}/{}", etiqueta_launchd(n))], true));
                p.push(Paso::Borrar(env.ruta_unidad(n)));
            }
        }
        Backend::Schtasks => {
            for n in SERVICIOS_INSTALABLES {
                let mut fin = vec!["schtasks".to_string()];
                fin.extend(schtasks_con_tarea("/End", n));
                p.push(ejecutar(fin, true));
                let mut del = vec!["schtasks".to_string()];
                del.extend(schtasks_con_tarea("/Delete", n));
                del.push("/F".into());
                p.push(ejecutar(del, false));
            }
        }
    }
    Ok(p)
}

/// Ejecuta los pasos en orden. `correr` lanza una orden externa (en
/// producción [`correr_orden`]; en las pruebas, un registro). Devuelve una
/// línea por paso hecho; se detiene en el primer error no tolerado.
pub fn aplicar(pasos: &[Paso], correr: &mut dyn FnMut(&[String]) -> Result<(), String>) -> Result<Vec<String>, String> {
    let mut hecho = Vec::new();
    for paso in pasos {
        match paso {
            Paso::CrearDir(d) => std::fs::create_dir_all(d).map_err(|e| format!("no se pudo crear {}: {e}", d.display()))?,
            Paso::Escribir { ruta, contenido } => {
                ea_bus::escribir_atomico(ruta, contenido.as_bytes()).map_err(|e| format!("no se pudo escribir {}: {e}", ruta.display()))?;
            }
            Paso::Borrar(r) => match std::fs::remove_file(r) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("no se pudo borrar {}: {e}", r.display())),
            },
            Paso::Ejecutar { argv, tolerar } => {
                if let Err(e) = correr(argv) {
                    if !tolerar {
                        return Err(format!("{}: {e}", argv.join(" ")));
                    }
                }
            }
        }
        hecho.push(paso.describir());
    }
    Ok(hecho)
}

/// Lanza una orden externa de verdad (sin shell) y devuelve su error.
pub fn correr_orden(argv: &[String]) -> Result<(), String> {
    let (prog, args) = argv.split_first().ok_or("orden vacía")?;
    let salida = std::process::Command::new(prog)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("no se pudo ejecutar {prog}: {e}"))?;
    if salida.status.success() {
        Ok(())
    } else {
        let e = String::from_utf8_lossy(&salida.stderr).trim().to_string();
        Err(if e.is_empty() { format!("terminó con {}", salida.status) } else { e })
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn entorno(backend: Backend, dir: &Path) -> Entorno {
        Entorno {
            backend,
            exes_dir: PathBuf::from("/opt/ea con espacio"),
            unidades_dir: dir.join("unidades"),
            logs_dir: dir.join("logs"),
            uid: Some(501),
        }
    }

    #[test]
    fn cada_sistema_tiene_su_gestor() {
        assert_eq!(GestorServicios::para(Sistema::Linux).backend, Backend::Systemd);
        assert_eq!(GestorServicios::para(Sistema::MacOs).backend, Backend::Launchd);
        assert_eq!(GestorServicios::para(Sistema::Windows).backend, Backend::Schtasks);
        assert_eq!(GestorServicios::para(Sistema::MacOs).orden, ["launchctl"]);
        assert_eq!(GestorServicios::para(Sistema::Windows).orden, ["schtasks"]);
        assert_eq!(GestorServicios::para(Sistema::Linux).orden, ["systemctl"]);
    }

    #[test]
    fn argumentos_de_launchd_y_schtasks() {
        assert_eq!(launchd_print_args(501, "ea-web"), ["print", "gui/501/es.gachapon.ea-web"]);
        assert_eq!(launchd_reiniciar_args(501, "ea-web"), ["kickstart", "-k", "gui/501/es.gachapon.ea-web"]);
        assert_eq!(
            schtasks_query_args("ea-web"),
            ["/Query", "/TN", "ecosistema-asistente\\ea-web", "/FO", "CSV", "/NH", "/V"]
        );
        assert_eq!(schtasks_con_tarea("/Run", "ea-mcp"), ["/Run", "/TN", "ecosistema-asistente\\ea-mcp"]);
    }

    #[test]
    fn lee_el_estado_de_launchd() {
        let corriendo = "gui/501/es.gachapon.ea-web = {\n\tactive count = 1\n\tstate = running\n\tpid = 99\n}\n";
        assert_eq!(parsear_launchd_print(corriendo), EstadoServicio { cargado: true, activo: "active".into() });
        let esperando = "gui/501/x = {\n\tstate = waiting\n}\n";
        assert_eq!(parsear_launchd_print(esperando).activo, "inactive");
        assert_eq!(parsear_launchd_print("").activo, "inactive");
    }

    #[test]
    fn lee_el_estado_de_schtasks() {
        let linea = |estado: &str| {
            format!("\"PC\",\"\\ecosistema-asistente\\ea-web\",\"N/A\",\"{estado}\",\"Interactive only\",\"22/10/2026 8:00:00\",\"0\"\r\n")
        };
        assert_eq!(parsear_schtasks_csv(&linea("Running")).activo, "active");
        assert_eq!(parsear_schtasks_csv(&linea("En ejecución")).activo, "active");
        assert_eq!(parsear_schtasks_csv(&linea("Ready")).activo, "inactive");
        assert_eq!(parsear_schtasks_csv(&linea("Disabled")).activo, "inactive");
        assert_eq!(parsear_schtasks_csv("").activo, "inactive");
        assert_eq!(campos_csv("\"a,b\",\"c\"\"d\",\"\""), ["a,b", "c\"d", ""]);
    }

    #[test]
    fn el_tiempo_activo_sale_del_reloj_monotonico() {
        let v = "100000000".to_string();
        assert_eq!(segundos_activo(Some(&v), Some(160.0)), Some(60));
        assert_eq!(segundos_activo(Some(&"0".to_string()), Some(160.0)), None);
        assert_eq!(segundos_activo(Some(&v), None), None);
        assert_eq!(segundos_activo(Some(&v), Some(50.0)), Some(0));
    }

    #[test]
    fn la_unidad_de_systemd_lleva_la_ruta_real() {
        let u = unidad_systemd("ea-web", Path::new("/opt/mi dir/ea-web")).unwrap();
        assert!(u.contains("ExecStart=\"/opt/mi dir/ea-web\"\n"), "{u}");
        assert!(u.contains("Description=Interfaz web del ecosistema de asistente\n"));
        assert!(u.contains("After=network.target ea-mcp.service\nWants=ea-mcp.service\n"));
        assert!(u.contains("Restart=on-failure\nRestartSec=2\n") && u.contains("WantedBy=default.target"));
        assert!(!u.contains("%h"), "sin rutas fijas");
        let mcp = unidad_systemd("ea-mcp", Path::new("/x/ea-mcp")).unwrap();
        assert!(!mcp.contains("Wants="));
        assert!(unidad_systemd("otro", Path::new("/x")).is_none());
        assert_eq!(cita_systemd("/a%b\"c\\d"), "\"/a%%b\\\"c\\\\d\"");
    }

    #[test]
    fn las_unidades_equivalen_a_las_plantillas_del_repo() {
        // Mismas líneas que `crates/*/systemd/*.service`, salvo la ruta.
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for (n, plantilla) in [("ea-web", "ea-web/systemd/ea-web.service"), ("ea-mcp", "ea-mcp/systemd/ea-mcp.service"), ("ea-orchestrator", "ea-orchestrator/systemd/ea-orchestrator.service")] {
            let esperado = std::fs::read_to_string(repo.join(plantilla)).unwrap();
            let generado = unidad_systemd(n, Path::new("/x/y")).unwrap();
            let limpio = |t: &str| t.lines().filter(|l| !l.starts_with("ExecStart=") && !l.starts_with('#')).collect::<Vec<_>>().join("\n");
            assert_eq!(limpio(&generado).trim(), limpio(&esperado).trim(), "{n}");
        }
    }

    #[test]
    fn el_plist_de_un_servicio() {
        let p = plist_servicio("ea-web", Path::new("/Users/a b/ea-web"), Path::new("/Users/a b/logs"));
        assert!(p.starts_with("<?xml"));
        assert!(p.contains("<key>Label</key>\n  <string>es.gachapon.ea-web</string>"));
        assert!(p.contains("<array>\n    <string>/Users/a b/ea-web</string>\n  </array>"));
        assert!(p.contains("<key>RunAtLoad</key>\n  <true/>"));
        assert!(p.contains("<key>KeepAlive</key>\n  <dict>\n    <key>SuccessfulExit</key>\n    <false/>"));
        assert!(p.contains("<key>StandardOutPath</key>\n  <string>/Users/a b/logs/ea-web.log</string>"));
        assert!(p.trim_end().ends_with("</plist>"));
    }

    #[test]
    fn la_tarea_de_windows_sin_privilegios_elevados() {
        let a = schtasks_crear_args("ea-web", Path::new("C:\\ea dir\\ea-web.exe"));
        assert_eq!(
            a,
            ["/Create", "/F", "/SC", "ONLOGON", "/RL", "LIMITED", "/TN", "ecosistema-asistente\\ea-web", "/TR", "\"C:\\ea dir\\ea-web.exe\""]
        );
    }

    fn argvs(pasos: &[Paso]) -> Vec<String> {
        pasos
            .iter()
            .filter_map(|p| match p {
                Paso::Ejecutar { argv, .. } => Some(argv.join(" ")),
                _ => None,
            })
            .collect()
    }

    #[test]
    #[cfg(unix)]
    fn el_plan_de_systemd() {
        let d = tempfile::tempdir().unwrap();
        let env = entorno(Backend::Systemd, d.path());
        let p = plan_instalar(&env).unwrap();
        assert!(matches!(&p[0], Paso::Escribir { ruta, contenido } if ruta.ends_with("unidades/ea-mcp.service") && contenido.contains("/opt/ea con espacio/ea-mcp")));
        let rutina = p.iter().find_map(|x| match x {
            Paso::Escribir { ruta, contenido } if ruta.ends_with("ea-rutina@.service") => Some(contenido.clone()),
            _ => None,
        });
        assert!(rutina.unwrap().contains("ExecStart=\"/opt/ea con espacio/ea\" rutina lanzar %i"));
        assert_eq!(
            argvs(&p),
            [
                "systemctl --user daemon-reload",
                "systemctl --user enable --now ea-mcp.service",
                "systemctl --user enable --now ea-orchestrator.service",
                "systemctl --user enable --now ea-web.service",
            ]
        );
        let q = plan_desinstalar(&env).unwrap();
        assert_eq!(argvs(&q).first().unwrap(), "systemctl --user disable --now ea-mcp.service");
        assert_eq!(argvs(&q).last().unwrap(), "systemctl --user daemon-reload");
        assert!(q.iter().any(|x| matches!(x, Paso::Borrar(r) if r.ends_with("ea-web.service"))));
        assert!(!q.iter().any(|x| matches!(x, Paso::Borrar(r) if r.ends_with("ea-rutina@.service"))));
    }

    #[test]
    fn el_plan_de_launchd() {
        let d = tempfile::tempdir().unwrap();
        let env = entorno(Backend::Launchd, d.path());
        let p = plan_instalar(&env).unwrap();
        assert!(matches!(&p[0], Paso::CrearDir(r) if r.ends_with("logs")));
        assert!(p.iter().any(|x| matches!(x, Paso::Escribir { ruta, .. } if ruta.ends_with("es.gachapon.ea-web.plist"))));
        let ordenes = argvs(&p);
        assert_eq!(ordenes[0], "launchctl bootout gui/501/es.gachapon.ea-mcp");
        assert!(ordenes[1].starts_with("launchctl bootstrap gui/501 ") && ordenes[1].ends_with("es.gachapon.ea-mcp.plist"));
        let q = plan_desinstalar(&env).unwrap();
        assert_eq!(argvs(&q)[0], "launchctl bootout gui/501/es.gachapon.ea-mcp");
        assert!(plan_instalar(&Entorno { uid: None, ..env }).is_err());
    }

    #[test]
    fn el_plan_de_windows() {
        let d = tempfile::tempdir().unwrap();
        let env = Entorno { exes_dir: PathBuf::from("C:\\ea"), ..entorno(Backend::Schtasks, d.path()) };
        let p = plan_instalar(&env).unwrap();
        assert!(!p.iter().any(|x| matches!(x, Paso::Escribir { .. })), "Windows no escribe unidades");
        let ordenes = argvs(&p);
        assert!(ordenes[0].starts_with("schtasks /Create /F /SC ONLOGON /RL LIMITED /TN ecosistema-asistente\\ea-mcp /TR \"C:\\ea"), "{}", ordenes[0]);
        assert_eq!(ordenes[1], "schtasks /Run /TN ecosistema-asistente\\ea-mcp");
        let q = plan_desinstalar(&env).unwrap();
        assert_eq!(argvs(&q)[1], "schtasks /Delete /TN ecosistema-asistente\\ea-mcp /F");
    }

    #[test]
    #[cfg(unix)]
    fn aplicar_escribe_borra_y_ejecuta_en_orden() {
        let d = tempfile::tempdir().unwrap();
        let env = entorno(Backend::Systemd, d.path());
        let mut vistas: Vec<String> = Vec::new();
        let hecho = aplicar(&plan_instalar(&env).unwrap(), &mut |a| {
            vistas.push(a.join(" "));
            Ok(())
        })
        .unwrap();
        assert_eq!(hecho.len(), plan_instalar(&env).unwrap().len());
        let web = d.path().join("unidades/ea-web.service");
        assert!(std::fs::read_to_string(&web).unwrap().contains("/opt/ea con espacio/ea-web"));
        assert_eq!(vistas.len(), 4);
        aplicar(&plan_desinstalar(&env).unwrap(), &mut |_| Err("no estaba cargado".into())).unwrap_err();
        // Con las órdenes toleradas fallando, sigue; la última (daemon-reload) no se tolera.
        let tolera = vec![Paso::Ejecutar { argv: vec!["x".into()], tolerar: true }, Paso::Borrar(web.clone())];
        aplicar(&tolera, &mut |_| Err("falla".into())).unwrap();
        assert!(!web.exists());
        assert!(entorno(Backend::Systemd, d.path()).faltan().len() == 4, "no hay binarios en /opt/ea con espacio");
    }
}
