//! Dependencias externas de `ea instalar` (spec 023, decisión 13 y D-3):
//! lo que no es de gachapon se **detecta** y, si falta, se **ofrece**
//! instalar con su propia confirmación. Aquí solo se describe: la pregunta
//! y la ejecución las hace `ea-cli`.

use std::path::Path;

use ea_acciones::servicios::Paso;
use ea_ajustes::descargas::descarga_para;
use ea_ajustes::{AgenteId, Ajustes, Rol};
use ea_plataforma::Sistema;

use super::{Contexto, Sonda};

/// Si la dependencia está.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Estado {
    /// Está; el texto dice dónde o cómo se ha visto.
    Presente(String),
    Falta,
}

/// Cómo instalarla, si se puede ofrecer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Oferta {
    /// Qué se hará, para la pregunta.
    pub explicacion: String,
    pub pasos: Vec<Paso>,
    /// Pide permisos de administrador (`sudo`, UAC).
    pub admin: bool,
}

/// Una dependencia externa con su estado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependencia {
    /// `bwrap`, `webkitgtk`, `webview2`, `sh`, `git`, `llama-server`,
    /// `modelo` o `agente:<id>`.
    pub id: &'static str,
    pub descripcion: String,
    pub estado: Estado,
    /// Sin ella algo no funciona (sandbox, webview, agentes de los roles).
    pub obligatoria: bool,
    pub oferta: Option<Oferta>,
    /// Lo que queda en manos del usuario (iniciar sesión, arrancar el
    /// modelo, instalar a mano).
    pub nota: Option<String>,
}

/// Un gestor de paquetes del sistema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestorPaquetes {
    Pacman,
    Apt,
    Dnf,
    Zypper,
    Brew,
    Winget,
}

impl GestorPaquetes {
    /// La orden que instala estos paquetes (ids en `winget`).
    pub fn instalar(&self, paquetes: &[&str]) -> Vec<String> {
        let base: &[&str] = match self {
            GestorPaquetes::Pacman => &["sudo", "pacman", "-S", "--needed"],
            GestorPaquetes::Apt => &["sudo", "apt-get", "install", "-y"],
            GestorPaquetes::Dnf => &["sudo", "dnf", "install", "-y"],
            GestorPaquetes::Zypper => &["sudo", "zypper", "install", "-y"],
            GestorPaquetes::Brew => &["brew", "install"],
            GestorPaquetes::Winget => &["winget", "install", "-e"],
        };
        let mut argv: Vec<String> = base.iter().map(|s| s.to_string()).collect();
        for p in paquetes {
            if *self == GestorPaquetes::Winget {
                argv.push("--id".into());
            }
            argv.push(p.to_string());
        }
        if *self == GestorPaquetes::Winget {
            argv.extend(["--accept-source-agreements", "--accept-package-agreements"].map(String::from));
        }
        argv
    }

    pub fn nombre(&self) -> &'static str {
        match self {
            GestorPaquetes::Pacman => "pacman",
            GestorPaquetes::Apt => "apt",
            GestorPaquetes::Dnf => "dnf",
            GestorPaquetes::Zypper => "zypper",
            GestorPaquetes::Brew => "brew",
            GestorPaquetes::Winget => "winget",
        }
    }

    /// `sudo` en Linux; `winget` puede pedir UAC.
    fn pide_admin(&self) -> bool {
        !matches!(self, GestorPaquetes::Brew)
    }
}

/// El gestor de paquetes del sistema, si hay uno conocido en el `PATH`.
pub fn gestor_paquetes(sistema: Sistema, sonda: &dyn Sonda) -> Option<GestorPaquetes> {
    let candidatos: &[(&str, GestorPaquetes)] = match sistema {
        Sistema::Linux => {
            &[("pacman", GestorPaquetes::Pacman), ("apt-get", GestorPaquetes::Apt), ("dnf", GestorPaquetes::Dnf), ("zypper", GestorPaquetes::Zypper)]
        }
        Sistema::MacOs => &[("brew", GestorPaquetes::Brew)],
        Sistema::Windows => &[("winget", GestorPaquetes::Winget)],
        Sistema::Otro => &[],
    };
    candidatos.iter().find(|(b, _)| sonda.binario(b).is_some()).map(|(_, g)| *g)
}

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// La VRAM en GB (GiB): la suma de las GPU NVIDIA o AMD; en macOS, el 75 %
/// de la memoria unificada. `None`: no se sabe.
pub fn vram_gb(sistema: Sistema, sonda: &dyn Sonda) -> Option<u32> {
    if sistema == Sistema::MacOs {
        let bytes: f64 = sonda.salida(&["sysctl", "-n", "hw.memsize"])?.trim().parse().ok()?;
        return Some((bytes * 0.75 / GIB) as u32);
    }
    if let Some(s) = sonda.salida(&["nvidia-smi", "--query-gpu=memory.total", "--format=csv,noheader,nounits"]) {
        let mib: f64 = s.lines().filter_map(|l| l.trim().parse::<f64>().ok()).sum();
        if mib > 0.0 {
            return Some((mib / 1024.0) as u32);
        }
    }
    if let Some(s) = sonda.salida(&["rocm-smi", "--showmeminfo", "vram", "--csv"]) {
        // device,VRAM Total Memory (B),VRAM Total Used Memory (B)
        let bytes: f64 = s.lines().skip(1).filter_map(|l| l.split(',').nth(1)?.trim().parse::<f64>().ok()).sum();
        if bytes > 0.0 {
            return Some((bytes / GIB) as u32);
        }
    }
    None
}

fn ejecutar(argv: Vec<String>) -> Paso {
    Paso::Ejecutar { argv, tolerar: false, reintentos: 0 }
}

fn argv(partes: &[&str]) -> Vec<String> {
    partes.iter().map(|s| s.to_string()).collect()
}

/// Oferta por el gestor de paquetes, o nota si no hay gestor o paquete.
fn por_gestor(
    gestor: Option<GestorPaquetes>,
    paquetes: impl Fn(GestorPaquetes) -> Option<&'static [&'static str]>,
    que: &str,
) -> (Option<Oferta>, Option<String>) {
    match gestor.and_then(|g| paquetes(g).map(|p| (g, p))) {
        Some((g, p)) => (
            Some(Oferta { explicacion: format!("instalar {} con {}", p.join(" "), g.nombre()), pasos: vec![ejecutar(g.instalar(p))], admin: g.pide_admin() }),
            None,
        ),
        None => (None, Some(format!("instala {que} con el gestor de paquetes de tu sistema"))),
    }
}

fn dep(id: &'static str, descripcion: &str, estado: Estado, obligatoria: bool) -> Dependencia {
    Dependencia { id, descripcion: descripcion.into(), estado, obligatoria, oferta: None, nota: None }
}

/// Un binario del `PATH` como dependencia; la oferta solo si falta.
fn binario(
    sonda: &dyn Sonda,
    id: &'static str,
    bin: &str,
    descripcion: &str,
    obligatoria: bool,
    oferta: impl FnOnce() -> (Option<Oferta>, Option<String>),
) -> Dependencia {
    match sonda.binario(bin) {
        Some(r) => dep(id, descripcion, Estado::Presente(r.display().to_string()), obligatoria),
        None => {
            let (oferta, nota) = oferta();
            Dependencia { oferta, nota, ..dep(id, descripcion, Estado::Falta, obligatoria) }
        }
    }
}

/// Las dependencias del sistema actual, con los ajustes efectivos.
pub fn detectar(ctx: &Contexto, sonda: &dyn Sonda) -> Vec<Dependencia> {
    detectar_con(ctx, sonda, &ea_ajustes::cargar())
}

/// Como [`detectar`], con los ajustes dados (las pruebas usan los valores
/// por defecto).
pub fn detectar_con(ctx: &Contexto, sonda: &dyn Sonda, ajustes: &Ajustes) -> Vec<Dependencia> {
    let gestor = gestor_paquetes(ctx.sistema, sonda);
    let mut v = Vec::new();
    match ctx.sistema {
        Sistema::Linux => {
            v.push(binario(sonda, "bwrap", "bwrap", "bubblewrap: el sandbox del ejecutor", true, || {
                let (o, n) = por_gestor(gestor, |_| Some(&["bubblewrap"]), "bubblewrap");
                (o, n.map(|n| format!("{n}; sin él, el ejecutor con sandbox no arranca")))
            }));
            if ctx.con_escritorio {
                v.push(webkit(sonda, gestor));
            }
            v.push(binario(sonda, "git", "git", "git: los worktrees de los trabajos", true, || por_gestor(gestor, |_| Some(&["git"]), "git")));
        }
        Sistema::MacOs => {
            v.push(dep("sandbox", "Seatbelt (sandbox-exec): el sandbox del ejecutor", Estado::Presente("viene con macOS".into()), true));
            v.push(binario(sonda, "git", "git", "git: los worktrees de los trabajos", true, || {
                (
                    Some(Oferta {
                        explicacion: "instalar las herramientas de línea de órdenes de Xcode (traen git)".into(),
                        pasos: vec![ejecutar(argv(&["xcode-select", "--install"]))],
                        admin: false,
                    }),
                    None,
                )
            }));
        }
        Sistema::Windows => {
            let presente = sonda.binario("sh").zip(sonda.binario("git"));
            let d = match presente {
                Some((sh, _)) => dep("sh", "Git for Windows: git y el sh de los criterios", Estado::Presente(sh.display().to_string()), true),
                None => {
                    let (oferta, nota) = por_gestor(gestor, |_| Some(&["Git.Git"]), "Git for Windows (https://git-scm.com)");
                    Dependencia { oferta, nota, ..dep("sh", "Git for Windows: git y el sh de los criterios", Estado::Falta, true) }
                }
            };
            v.push(d);
            if ctx.con_escritorio {
                v.push(webview2(sonda, gestor));
            }
        }
        Sistema::Otro => {}
    }
    v.push(llama_server(sonda, gestor));
    v.push(modelo(ctx, sonda, ajustes));
    for rol in Rol::TODOS {
        let (id, config) = ajustes.agente_de(rol);
        v.push(agente(ctx.sistema, sonda, id, &config.bin));
    }
    // Primero las obligatorias; el resto en el orden de arriba.
    v.sort_by_key(|d| !d.obligatoria);
    v
}

fn webkit(sonda: &dyn Sonda, gestor: Option<GestorPaquetes>) -> Dependencia {
    let descripcion = "WebKitGTK 4.1 y la bandeja: la app de escritorio";
    let libs = sonda.salida(&["ldconfig", "-p"]).unwrap_or_default();
    if libs.contains("libwebkit2gtk-4.1.so") && libs.contains("libayatana-appindicator3") {
        return dep("webkitgtk", descripcion, Estado::Presente("libwebkit2gtk-4.1 y libayatana-appindicator3".into()), true);
    }
    let (oferta, nota) = por_gestor(
        gestor,
        |g| match g {
            GestorPaquetes::Pacman => Some(&["webkit2gtk-4.1", "libayatana-appindicator"]),
            GestorPaquetes::Apt => Some(&["libwebkit2gtk-4.1-0", "libayatana-appindicator3-1"]),
            GestorPaquetes::Dnf => Some(&["webkit2gtk4.1", "libayatana-appindicator-gtk3"]),
            GestorPaquetes::Zypper => Some(&["libwebkit2gtk-4_1-0", "libayatana-appindicator3-1"]),
            _ => None,
        },
        "WebKitGTK 4.1 y libayatana-appindicator3",
    );
    Dependencia { oferta, nota, ..dep("webkitgtk", descripcion, Estado::Falta, true) }
}

const CLAVE_WEBVIEW2: &str = r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
const CLAVE_WEBVIEW2_USUARIO: &str = r"Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

fn webview2(sonda: &dyn Sonda, gestor: Option<GestorPaquetes>) -> Dependencia {
    let descripcion = "WebView2: la app de escritorio";
    let version = [format!(r"HKLM\{CLAVE_WEBVIEW2}"), format!(r"HKCU\{CLAVE_WEBVIEW2_USUARIO}")].iter().find_map(|clave| {
        let s = sonda.salida(&["reg", "query", clave, "/v", "pv"])?;
        let v = s.lines().find(|l| l.contains("pv"))?.split_whitespace().last()?.to_string();
        (v != "0.0.0.0").then_some(v)
    });
    match version {
        Some(v) => dep("webview2", descripcion, Estado::Presente(format!("versión {v}")), true),
        None => {
            let (oferta, nota) =
                por_gestor(gestor, |_| Some(&["Microsoft.EdgeWebView2Runtime"]), "WebView2 Runtime (https://developer.microsoft.com/microsoft-edge/webview2/)");
            Dependencia { oferta, nota, ..dep("webview2", descripcion, Estado::Falta, true) }
        }
    }
}

const RELEASES_LLAMA: &str = "https://github.com/ggml-org/llama.cpp/releases";

fn llama_server(sonda: &dyn Sonda, gestor: Option<GestorPaquetes>) -> Dependencia {
    binario(sonda, "llama-server", "llama-server", "llama-server (llama.cpp): sirve el modelo local", false, || match gestor {
        Some(GestorPaquetes::Pacman) => (None, Some("llama.cpp está en AUR (llama.cpp, o llama.cpp-cuda / llama.cpp-vulkan según tu GPU)".into())),
        Some(g @ GestorPaquetes::Brew) => por_gestor(Some(g), |_| Some(&["llama.cpp"]), "llama.cpp"),
        Some(g @ GestorPaquetes::Winget) => por_gestor(Some(g), |_| Some(&["ggml.llamacpp"]), "llama.cpp"),
        _ => (None, Some(format!("descarga llama.cpp para tu GPU de {RELEASES_LLAMA} y deja llama-server en el PATH"))),
    })
}

fn modelo(ctx: &Contexto, sonda: &dyn Sonda, ajustes: &Ajustes) -> Dependencia {
    let (id_agente, config) = ajustes.agente_de(Rol::Ejecutor);
    let perfil = ajustes.modelo_ejecutor();
    let id_perfil = if ajustes.modelos.contains_key(&config.modelo) { config.modelo.as_str() } else { "local" };
    let descripcion = format!("el modelo local del ejecutor ({}, {})", perfil.nombre, id_agente);
    let base = perfil.base_url.trim_end_matches('/');
    if sonda.http_ok(&format!("{base}/models")) {
        return Dependencia { descripcion, ..dep("modelo", "", Estado::Presente(format!("responde en {base}")), false) };
    }
    let falta = Dependencia { descripcion: descripcion.clone(), ..dep("modelo", "", Estado::Falta, false) };
    let Some(d) = descarga_para(id_perfil) else {
        return Dependencia { nota: Some(format!("arranca un servidor compatible con OpenAI en {base} que sirva el modelo {}", perfil.nombre)), ..falta };
    };
    let dir = ctx.destinos.datos.join("modelos");
    let gguf = dir.join(d.fichero());
    let parcial = dir.join(format!("{}.parcial", d.fichero()));
    // La ruta va citada: en macOS lleva espacios (`Application Support`).
    let citada = if ctx.sistema == Sistema::Windows { format!("\"{}\"", texto(&gguf)) } else { format!("'{}'", texto(&gguf).replace('\'', "'\\''")) };
    let arrancar = format!("arráncalo con: {}", d.orden_con(&citada));
    if sonda.existe(&gguf) {
        return Dependencia {
            descripcion,
            nota: Some(format!("descargado pero no responde en {base}; {arrancar}")),
            ..dep("modelo", "", Estado::Presente(format!("descargado en {}", gguf.display())), false)
        };
    }
    let vram = vram_gb(ctx.sistema, sonda);
    match vram {
        Some(v) if v >= d.vram_min_gb => {
            let curl = if ctx.sistema == Sistema::Windows { "curl.exe" } else { "curl" };
            Dependencia {
                oferta: Some(Oferta {
                    explicacion: format!("descargar {} (~{} GB) en {}", d.fichero(), d.gb(), dir.display()),
                    // A un `.parcial` que se reanuda (`-C -`) y solo al
                    // terminar al nombre final: un corte no deja un modelo a
                    // medias que pase por descargado.
                    pasos: vec![
                        Paso::CrearDir(dir.clone()),
                        ejecutar(vec![curl.into(), "-L".into(), "--fail".into(), "-C".into(), "-".into(), "-o".into(), texto(&parcial), d.url.into()]),
                        Paso::Renombrar { origen: parcial.clone(), destino: gguf.clone() },
                    ],
                    admin: false,
                }),
                nota: Some(arrancar),
                ..falta
            }
        }
        Some(v) => Dependencia {
            nota: Some(format!(
                "el modelo por defecto pide unos {} GB de VRAM y hay {v} GB; elige un modelo menor con `ea ajustes establecer modelos.local.nombre=…` y sírvelo en {base}",
                d.vram_min_gb
            )),
            ..falta
        },
        None => Dependencia {
            nota: Some(format!(
                "no se ha podido saber la VRAM; el modelo por defecto ({}, ~{} GB) pide unos {} GB. Descárgalo de {} si te cabe y sírvelo en {base}",
                d.fichero(),
                d.gb(),
                d.vram_min_gb,
                d.url
            )),
            ..falta
        },
    }
}

fn texto(ruta: &Path) -> String {
    ruta.to_string_lossy().into_owned()
}

/// El id de dependencia de un agente: `agente:<id>`, o `agente` si no es del
/// catálogo.
fn id_agente(agente: Option<AgenteId>) -> &'static str {
    match agente {
        Some(AgenteId::ClaudeCode) => "agente:claude-code",
        Some(AgenteId::QwenCode) => "agente:qwen-code",
        Some(AgenteId::Codex) => "agente:codex",
        Some(AgenteId::OpenCode) => "agente:opencode",
        Some(AgenteId::Antigravity) => "agente:antigravity",
        None => "agente",
    }
}

fn npm(sonda: &dyn Sonda, paquete: &str) -> (Option<Oferta>, Option<String>) {
    match sonda.binario("npm") {
        Some(_) => (
            Some(Oferta { explicacion: format!("instalar {paquete} con npm"), pasos: vec![ejecutar(argv(&["npm", "install", "-g", paquete]))], admin: false }),
            None,
        ),
        None => (None, Some(format!("instala Node.js (https://nodejs.org) y después `npm install -g {paquete}`"))),
    }
}

fn agente(sistema: Sistema, sonda: &dyn Sonda, id: &str, bin: &str) -> Dependencia {
    let agente = AgenteId::desde(id);
    let descripcion = format!("{id}: agente de un rol");
    match sonda.binario(bin) {
        Some(r) => Dependencia {
            nota: Some(format!("inicia sesión con `{bin}` si no lo has hecho")),
            ..Dependencia { descripcion, ..dep(id_agente(agente), "", Estado::Presente(r.display().to_string()), true) }
        },
        None => {
            let (oferta, nota) = match agente {
                Some(AgenteId::ClaudeCode) if sistema == Sistema::Windows => (
                    Some(Oferta {
                        explicacion: "instalar Claude Code con su instalador oficial".into(),
                        pasos: vec![ejecutar(argv(&["powershell", "-NoProfile", "-Command", "irm https://claude.ai/install.ps1 | iex"]))],
                        admin: false,
                    }),
                    None,
                ),
                Some(AgenteId::ClaudeCode) => (
                    Some(Oferta {
                        explicacion: "instalar Claude Code con su instalador oficial".into(),
                        pasos: vec![ejecutar(argv(&["sh", "-c", "curl -fsSL https://claude.ai/install.sh | bash"]))],
                        admin: false,
                    }),
                    None,
                ),
                Some(AgenteId::QwenCode) => npm(sonda, "@qwen-code/qwen-code"),
                Some(AgenteId::Codex) => npm(sonda, "@openai/codex"),
                Some(AgenteId::OpenCode) => npm(sonda, "opencode-ai"),
                Some(AgenteId::Antigravity) | None => (None, Some(format!("instala {id} y deja `{bin}` en el PATH"))),
            };
            let nota = Some(match nota {
                Some(n) => format!("{n}; después inicia sesión con `{bin}`"),
                None => format!("después inicia sesión con `{bin}`"),
            });
            Dependencia { oferta, nota, ..Dependencia { descripcion, ..dep(id_agente(agente), "", Estado::Falta, true) } }
        }
    }
}

#[cfg(test)]
mod pruebas {
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    use ea_ajustes::Fuentes;

    use super::*;
    use crate::instalacion::{Destinos, Origen};

    #[derive(Default)]
    struct Falsa {
        binarios: HashSet<&'static str>,
        ficheros: HashSet<PathBuf>,
        salidas: HashMap<String, String>,
        http: HashSet<String>,
    }

    impl Falsa {
        fn con(binarios: &[&'static str]) -> Self {
            Self { binarios: binarios.iter().copied().collect(), ..Default::default() }
        }
        fn salida_de(mut self, argv: &str, s: &str) -> Self {
            self.salidas.insert(argv.into(), s.into());
            self
        }
    }

    impl Sonda for Falsa {
        fn binario(&self, nombre: &str) -> Option<PathBuf> {
            self.binarios.contains(nombre).then(|| PathBuf::from("/usr/bin").join(nombre))
        }
        fn existe(&self, ruta: &Path) -> bool {
            self.ficheros.contains(ruta)
        }
        fn leer(&self, _: &Path) -> Option<String> {
            None
        }
        fn salida(&self, argv: &[&str]) -> Option<String> {
            self.salidas.get(&argv.join(" ")).cloned()
        }
        fn http_ok(&self, url: &str) -> bool {
            self.http.contains(url)
        }
    }

    fn ctx(sistema: Sistema, con_escritorio: bool) -> Contexto {
        Contexto {
            sistema,
            origen: Origen::Paquete,
            exes_origen: "/tmp/paquete".into(),
            destinos: Destinos { bin: "/d/bin".into(), flujo: "/d/flujo".into(), datos: "/d".into() },
            home: "/home/ana".into(),
            skills_dir: "/home/ana/.config/ecosistema-asistente/qwen-skills".into(),
            puerto_mcp: 8765,
            con_escritorio,
        }
    }

    fn ajustes() -> Ajustes {
        Ajustes::por_defecto(&Fuentes::aislada(Path::new("/nada")))
    }

    fn detectar(sistema: Sistema, escritorio: bool, sonda: &Falsa) -> Vec<Dependencia> {
        detectar_con(&ctx(sistema, escritorio), sonda, &ajustes())
    }

    fn buscar<'a>(v: &'a [Dependencia], id: &str) -> &'a Dependencia {
        v.iter().find(|d| d.id == id).unwrap_or_else(|| panic!("falta {id}"))
    }

    fn argv_de(o: &Oferta) -> Vec<Vec<String>> {
        o.pasos
            .iter()
            .filter_map(|p| match p {
                Paso::Ejecutar { argv, .. } => Some(argv.clone()),
                _ => None,
            })
            .collect()
    }

    const VRAM_24: &str = "nvidia-smi --query-gpu=memory.total --format=csv,noheader,nounits";

    #[test]
    fn gestor_por_binario() {
        assert_eq!(gestor_paquetes(Sistema::Linux, &Falsa::con(&["apt-get"])), Some(GestorPaquetes::Apt));
        assert_eq!(gestor_paquetes(Sistema::Linux, &Falsa::con(&["pacman", "apt-get"])), Some(GestorPaquetes::Pacman));
        assert_eq!(gestor_paquetes(Sistema::MacOs, &Falsa::con(&["brew"])), Some(GestorPaquetes::Brew));
        assert_eq!(gestor_paquetes(Sistema::Windows, &Falsa::con(&["winget"])), Some(GestorPaquetes::Winget));
        assert_eq!(gestor_paquetes(Sistema::Linux, &Falsa::con(&["brew"])), None);
    }

    #[test]
    fn ordenes_de_cada_gestor() {
        assert_eq!(GestorPaquetes::Apt.instalar(&["a", "b"]), argv(&["sudo", "apt-get", "install", "-y", "a", "b"]));
        assert_eq!(
            GestorPaquetes::Winget.instalar(&["Git.Git"]),
            argv(&["winget", "install", "-e", "--id", "Git.Git", "--accept-source-agreements", "--accept-package-agreements"])
        );
    }

    #[test]
    fn linux_sin_bwrap_con_pacman_ofrece_bubblewrap_con_sudo() {
        let v = detectar(Sistema::Linux, false, &Falsa::con(&["pacman", "git"]));
        let b = buscar(&v, "bwrap");
        assert_eq!(b.estado, Estado::Falta);
        assert!(b.obligatoria);
        let o = b.oferta.as_ref().unwrap();
        assert!(o.admin);
        assert_eq!(argv_de(o), vec![argv(&["sudo", "pacman", "-S", "--needed", "bubblewrap"])]);
        assert!(v.iter().take_while(|d| d.obligatoria).count() >= 3, "las obligatorias van primero");
        assert!(v.iter().skip_while(|d| d.obligatoria).all(|d| !d.obligatoria));
    }

    #[test]
    fn sin_gestor_no_hay_oferta_y_si_nota() {
        let v = detectar(Sistema::Linux, false, &Falsa::con(&["git"]));
        let b = buscar(&v, "bwrap");
        assert!(b.oferta.is_none());
        assert!(b.nota.as_deref().unwrap().contains("bubblewrap"));
    }

    #[test]
    fn webkit_solo_con_escritorio() {
        let s = Falsa::con(&["apt-get", "bwrap", "git"]);
        assert!(!detectar(Sistema::Linux, false, &s).iter().any(|d| d.id == "webkitgtk"));
        let v = detectar(Sistema::Linux, true, &s);
        let w = buscar(&v, "webkitgtk");
        assert_eq!(argv_de(w.oferta.as_ref().unwrap())[0], argv(&["sudo", "apt-get", "install", "-y", "libwebkit2gtk-4.1-0", "libayatana-appindicator3-1"]));
        let s = s.salida_de("ldconfig -p", "libwebkit2gtk-4.1.so.0 (libc6,x86-64)\nlibayatana-appindicator3.so.1\n");
        assert!(matches!(buscar(&detectar(Sistema::Linux, true, &s), "webkitgtk").estado, Estado::Presente(_)));
    }

    #[test]
    fn modelo_presente_si_responde() {
        let mut s = Falsa::con(&[]);
        s.http.insert("http://127.0.0.1:8080/v1/models".into());
        let v = detectar(Sistema::Linux, false, &s);
        assert!(matches!(buscar(&v, "modelo").estado, Estado::Presente(_)));
    }

    #[test]
    fn modelo_ausente_con_vram_ofrece_curl() {
        let s = Falsa::con(&[]).salida_de(VRAM_24, "24576\n");
        let v = detectar(Sistema::Linux, false, &s);
        let m = buscar(&v, "modelo");
        assert_eq!(m.estado, Estado::Falta);
        let o = m.oferta.as_ref().unwrap();
        let a = &argv_de(o)[0];
        assert_eq!(a[0], "curl");
        assert!(a.contains(&"-C".to_string()) && a.last().unwrap().starts_with("https://"));
        assert!(a.iter().map(|x| x.replace('\\', "/")).any(|x| x.starts_with("/d/modelos/") && x.ends_with(".gguf.parcial")));
        assert!(o.pasos.iter().any(|p| matches!(p, Paso::Renombrar { destino, .. } if destino.to_string_lossy().ends_with(".gguf"))));
        assert!(o.explicacion.contains("GB"));
        assert!(m.nota.as_deref().unwrap().replace('\\', "/").contains("llama-server -m '/d/modelos/"));
    }

    #[test]
    fn modelo_descargado_esta_presente_con_la_orden() {
        let mut s = Falsa::con(&[]);
        let d = descarga_para("local").unwrap();
        s.ficheros.insert(PathBuf::from("/d/modelos").join(d.fichero()));
        let v = detectar(Sistema::Linux, false, &s);
        let m = buscar(&v, "modelo");
        assert!(matches!(m.estado, Estado::Presente(_)));
        assert!(m.oferta.is_none() && m.nota.as_deref().unwrap().contains("llama-server"));
    }

    #[test]
    fn vram_insuficiente_o_desconocida_sin_oferta() {
        let v = detectar(Sistema::Linux, false, &Falsa::con(&[]).salida_de(VRAM_24, "8192\n"));
        let m = buscar(&v, "modelo");
        assert!(m.oferta.is_none() && m.nota.as_deref().unwrap().contains("8 GB"));
        let v = detectar(Sistema::Linux, false, &Falsa::con(&[]));
        assert!(buscar(&v, "modelo").oferta.is_none());
    }

    #[test]
    fn vram_de_varias_fuentes() {
        assert_eq!(vram_gb(Sistema::Linux, &Falsa::con(&[]).salida_de(VRAM_24, "12288\n12288\n")), Some(24));
        let rocm = "device,VRAM Total Memory (B),VRAM Total Used Memory (B)\ncard0,25753026560,1000\n";
        assert_eq!(vram_gb(Sistema::Linux, &Falsa::con(&[]).salida_de("rocm-smi --showmeminfo vram --csv", rocm)), Some(23));
        // 32 GiB de memoria unificada: el 75 %.
        let mac = Falsa::con(&[]).salida_de("sysctl -n hw.memsize", "34359738368\n");
        assert_eq!(vram_gb(Sistema::MacOs, &mac), Some(24));
        assert_eq!(vram_gb(Sistema::Linux, &Falsa::con(&[])), None);
    }

    #[test]
    fn macos_ofrece_brew_y_xcode() {
        let v = detectar(Sistema::MacOs, true, &Falsa::con(&["brew"]));
        assert!(matches!(buscar(&v, "sandbox").estado, Estado::Presente(_)));
        assert_eq!(argv_de(buscar(&v, "git").oferta.as_ref().unwrap())[0], argv(&["xcode-select", "--install"]));
        assert_eq!(argv_de(buscar(&v, "llama-server").oferta.as_ref().unwrap())[0], argv(&["brew", "install", "llama.cpp"]));
        assert!(!v.iter().any(|d| d.id == "webkitgtk" || d.id == "bwrap"));
    }

    #[test]
    fn windows_webview2_y_sh() {
        let s = Falsa::con(&["winget"]);
        let v = detectar(Sistema::Windows, true, &s);
        let sh = buscar(&v, "sh");
        assert!(sh.obligatoria && sh.estado == Estado::Falta);
        assert!(argv_de(sh.oferta.as_ref().unwrap())[0].contains(&"Git.Git".to_string()));
        assert!(argv_de(buscar(&v, "webview2").oferta.as_ref().unwrap())[0].contains(&"Microsoft.EdgeWebView2Runtime".to_string()));
        assert!(argv_de(buscar(&v, "llama-server").oferta.as_ref().unwrap())[0].contains(&"ggml.llamacpp".to_string()));

        let clave = format!(r"reg query HKLM\{CLAVE_WEBVIEW2} /v pv");
        let s = Falsa::con(&["winget", "sh", "git"]).salida_de(&clave, "\r\nHKEY_LOCAL_MACHINE\\...\r\n    pv    REG_SZ    130.0.2849.80\r\n");
        let v = detectar(Sistema::Windows, true, &s);
        assert_eq!(buscar(&v, "webview2").estado, Estado::Presente("versión 130.0.2849.80".into()));
        assert!(matches!(buscar(&v, "sh").estado, Estado::Presente(_)));
        assert!(!detectar(Sistema::Windows, false, &s).iter().any(|d| d.id == "webview2"));
    }

    #[test]
    fn llama_en_pacman_es_nota_de_aur() {
        let v = detectar(Sistema::Linux, false, &Falsa::con(&["pacman"]));
        let l = buscar(&v, "llama-server");
        assert!(l.oferta.is_none() && l.nota.as_deref().unwrap().contains("AUR"));
        let v = detectar(Sistema::Linux, false, &Falsa::con(&["apt-get"]));
        assert!(buscar(&v, "llama-server").nota.as_deref().unwrap().contains(RELEASES_LLAMA));
    }

    #[test]
    fn agentes_con_y_sin_npm() {
        let v = detectar(Sistema::Linux, false, &Falsa::con(&["npm"]));
        let c = buscar(&v, "agente:claude-code");
        assert!(c.obligatoria);
        assert_eq!(argv_de(c.oferta.as_ref().unwrap())[0], argv(&["sh", "-c", "curl -fsSL https://claude.ai/install.sh | bash"]));
        let q = buscar(&v, "agente:qwen-code");
        assert_eq!(argv_de(q.oferta.as_ref().unwrap())[0], argv(&["npm", "install", "-g", "@qwen-code/qwen-code"]));
        assert!(q.nota.as_deref().unwrap().contains("inicia sesión con `qwen`"));

        let v = detectar(Sistema::Linux, false, &Falsa::con(&[]));
        let q = buscar(&v, "agente:qwen-code");
        assert!(q.oferta.is_none() && q.nota.as_deref().unwrap().contains("Node.js"));

        let v = detectar(Sistema::Windows, false, &Falsa::con(&["claude"]));
        let c = buscar(&v, "agente:claude-code");
        assert!(matches!(c.estado, Estado::Presente(_)) && c.oferta.is_none());
        assert!(c.nota.as_deref().unwrap().contains("inicia sesión con `claude`"));
        let v = detectar(Sistema::Windows, false, &Falsa::con(&[]));
        assert!(argv_de(buscar(&v, "agente:claude-code").oferta.as_ref().unwrap())[0].contains(&"powershell".to_string()));
    }

    #[test]
    fn cada_agente_con_su_id_de_dependencia_y_su_oferta() {
        let s = Falsa::con(&["npm"]);
        let de = |id: &str, bin: &str| agente(Sistema::Linux, &s, id, bin);
        let ids = ["claude-code", "qwen-code", "codex", "opencode", "antigravity", "gpt"].map(|id| de(id, "x").id);
        assert_eq!(ids, ["agente:claude-code", "agente:qwen-code", "agente:codex", "agente:opencode", "agente:antigravity", "agente"]);
        let orden = |id: &str| argv_de(de(id, "x").oferta.as_ref().unwrap())[0].clone();
        assert_eq!(orden("codex"), argv(&["npm", "install", "-g", "@openai/codex"]));
        assert_eq!(orden("opencode"), argv(&["npm", "install", "-g", "opencode-ai"]));
        for id in ["antigravity", "gpt"] {
            let d = de(id, "agy");
            assert!(d.oferta.is_none(), "{id}");
            assert_eq!(d.descripcion, format!("{id}: agente de un rol"));
            assert_eq!(d.nota.unwrap(), format!("instala {id} y deja `agy` en el PATH; después inicia sesión con `agy`"));
        }
    }
}
