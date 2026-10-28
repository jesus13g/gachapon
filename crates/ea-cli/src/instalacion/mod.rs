//! `ea instalar` y `ea desinstalar` (spec 023): la instalación en un paso.
//!
//! Aquí solo se **planifica**: cada función devuelve los [`Paso`]s que se
//! enseñan al usuario y que `ea_acciones::servicios::aplicar` ejecuta tras su
//! confirmación en una terminal real (lo hace `commands::instalar`). Lo que se mira del
//! sistema pasa por una [`Sonda`], para que las pruebas no dependan de la
//! máquina.

use std::path::{Path, PathBuf};

use ea_acciones::servicios::{self, Backend, Entorno, Paso};
use ea_acciones::SERVICIOS_INSTALABLES;
use ea_plataforma::Sistema;

pub mod adaptadores;
pub mod agentes;
pub mod dependencias;
pub mod menu;
pub mod origen;
pub mod recursos;
pub mod ruta_usuario;
pub mod wsl;

/// De dónde salen los binarios del `ea` que corre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origen {
    /// Un paquete descargado (Release): se copian a una carpeta estable.
    Paquete,
    /// `target/release` (o `target/debug`) de un clon del repo: se quedan
    /// donde están (modo desarrollo).
    Clon { repo: PathBuf },
}

/// Adónde va cada cosa.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destinos {
    /// Los binarios (con origen clon, `target/release`).
    pub bin: PathBuf,
    /// Las guías `agentes/flujo/*.md` (con origen clon, las del repo).
    pub flujo: PathBuf,
    /// Base de lo demás que se instala (icono de la app…).
    pub datos: PathBuf,
}

/// Todo lo que un plan necesita saber.
#[derive(Debug, Clone)]
pub struct Contexto {
    pub sistema: Sistema,
    pub origen: Origen,
    /// Carpeta del `ea` que corre (de ahí se copian los binarios).
    pub exes_origen: PathBuf,
    pub destinos: Destinos,
    pub home: PathBuf,
    /// `agente.skills.dir` efectivo.
    pub skills_dir: PathBuf,
    /// `red.puerto_mcp` efectivo.
    pub puerto_mcp: u16,
    /// `false` con `--sin-escritorio` o dentro de WSL.
    pub con_escritorio: bool,
}

/// Lo que los planes miran del sistema. En producción, [`SondaReal`]; en
/// las pruebas, una falsa.
pub trait Sonda {
    /// La ruta de un programa del `PATH`.
    fn binario(&self, nombre: &str) -> Option<PathBuf>;
    fn existe(&self, ruta: &Path) -> bool;
    /// El contenido de un fichero de texto, si se puede leer.
    fn leer(&self, ruta: &Path) -> Option<String>;
    /// La salida estándar de una orden que termina con 0.
    fn salida(&self, argv: &[&str]) -> Option<String>;
    /// ¿Responde con 2xx un GET a esta URL local (tope corto)?
    fn http_ok(&self, url: &str) -> bool;
    /// Una variable de entorno (el `PATH`, para saber si hace falta tocarlo).
    fn var(&self, nombre: &str) -> Option<String> {
        let _ = nombre;
        None
    }
    /// Adónde apunta un enlace simbólico, si lo es.
    fn destino_enlace(&self, ruta: &Path) -> Option<PathBuf> {
        let _ = ruta;
        None
    }
    /// ¿Existe pero no se puede leer como texto (permisos, no es UTF-8)?
    /// Un fichero así no se reescribe: se perdería lo que hay.
    fn ilegible(&self, ruta: &Path) -> bool {
        let _ = ruta;
        false
    }
}

/// El contenido de un fichero que un plan va a reescribir: `Ok(None)` si no
/// existe, `Err` si existe y no se puede leer (entonces el paso se omite y
/// se avisa: reescribirlo perdería lo que hay).
pub fn leer_para_reescribir(sonda: &dyn Sonda, ruta: &Path) -> Result<Option<String>, String> {
    if sonda.ilegible(ruta) {
        return Err(format!("{} no se puede leer como texto: no se toca (hazlo a mano)", ruta.display()));
    }
    Ok(sonda.leer(ruta))
}

/// La sonda de verdad.
pub struct SondaReal;

impl Sonda for SondaReal {
    fn binario(&self, nombre: &str) -> Option<PathBuf> {
        ea_plataforma::rutas::buscar_en_path(nombre)
    }

    fn existe(&self, ruta: &Path) -> bool {
        // Un enlace roto también «existe»: hay algo que sustituir.
        std::fs::symlink_metadata(ruta).is_ok()
    }

    fn leer(&self, ruta: &Path) -> Option<String> {
        std::fs::read_to_string(ruta).ok()
    }

    fn salida(&self, argv: &[&str]) -> Option<String> {
        let (prog, args) = argv.split_first()?;
        let s = std::process::Command::new(prog).args(args).stdin(std::process::Stdio::null()).output().ok()?;
        s.status.success().then(|| String::from_utf8_lossy(&s.stdout).into_owned())
    }

    fn http_ok(&self, url: &str) -> bool {
        http_ok(url, std::time::Duration::from_secs(2))
    }

    fn var(&self, nombre: &str) -> Option<String> {
        std::env::var(nombre).ok()
    }

    fn destino_enlace(&self, ruta: &Path) -> Option<PathBuf> {
        std::fs::read_link(ruta).ok()
    }

    fn ilegible(&self, ruta: &Path) -> bool {
        std::fs::metadata(ruta).is_ok_and(|m| m.is_file()) && std::fs::read_to_string(ruta).is_err()
    }
}

/// La cabecera de la respuesta a un GET a una URL local en claro
/// (`http://127.0.0.1:<puerto>/<ruta>`), sin dependencias; `None` si no hay
/// respuesta.
fn cabeza_http(url: &str, tope: std::time::Duration) -> Option<String> {
    use std::io::{Read, Write};
    let resto = url.strip_prefix("http://")?;
    let (hostport, ruta) = resto.split_once('/').map(|(h, r)| (h, format!("/{r}"))).unwrap_or((resto, "/".into()));
    let dir = std::net::ToSocketAddrs::to_socket_addrs(hostport).ok()?.next()?;
    let mut c = std::net::TcpStream::connect_timeout(&dir, tope).ok()?;
    let _ = c.set_read_timeout(Some(tope));
    let _ = c.set_write_timeout(Some(tope));
    let host = hostport.split(':').next().unwrap_or("localhost");
    // De una vez: `write!` sobre el socket haría varios envíos.
    c.write_all(format!("GET {ruta} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes()).ok()?;
    let mut buf = [0u8; 16];
    let n = c.read(&mut buf).ok()?;
    let cabeza = String::from_utf8_lossy(&buf[..n]).into_owned();
    cabeza.starts_with("HTTP/").then_some(cabeza)
}

/// ¿Responde con 2xx?
pub fn http_ok(url: &str, tope: std::time::Duration) -> bool {
    cabeza_http(url, tope).is_some_and(|c| c.starts_with("HTTP/1.1 2") || c.starts_with("HTTP/1.0 2"))
}

/// ¿Responde algo HTTP, con el código que sea? (`ea-mcp` contesta 4xx a un
/// GET a su raíz, y eso basta para saber que está vivo.)
pub fn http_responde(url: &str, tope: std::time::Duration) -> bool {
    cabeza_http(url, tope).is_some()
}

/// El contenido de un `ajustes.toml` recién creado: mandan los valores por
/// defecto.
pub const AJUSTES_INICIALES: &str = "# Ajustes de gachapon (spec 016). Vacío: mandan los valores por defecto.\n\
# `ea ajustes ver` enseña cada clave, su valor y su origen; `ea ajustes establecer` las cambia.\n";

/// El fichero de unidad de un servicio, si el gestor usa ficheros.
fn unidad_instalada(entorno: &Entorno, nombre: &str) -> Option<PathBuf> {
    match entorno.backend {
        Backend::Systemd => Some(entorno.unidades_dir.join(format!("{nombre}.service"))),
        Backend::Launchd => Some(entorno.unidades_dir.join(format!("{}.plist", servicios::etiqueta_launchd(nombre)))),
        Backend::Schtasks => None,
    }
}

/// Parar los servicios que ya estén instalados (antes de sustituir sus
/// binarios). Todo tolerante: lo que no corre, no se para. En Windows no
/// hay ficheros que mirar: se intenta siempre.
fn plan_parar(entorno: &Entorno, sonda: &dyn Sonda) -> Vec<Paso> {
    let mut p = Vec::new();
    for n in SERVICIOS_INSTALABLES {
        let argv: Vec<String> = match entorno.backend {
            Backend::Systemd if unidad_instalada(entorno, n).is_some_and(|u| sonda.existe(&u)) => {
                vec!["systemctl".into(), "--user".into(), "stop".into(), format!("{n}.service")]
            }
            Backend::Launchd if unidad_instalada(entorno, n).is_some_and(|u| sonda.existe(&u)) => match entorno.uid {
                Some(uid) => vec!["launchctl".into(), "bootout".into(), format!("gui/{uid}/{}", servicios::etiqueta_launchd(n))],
                None => continue,
            },
            Backend::Schtasks => vec!["schtasks".into(), "/End".into(), "/TN".into(), servicios::tarea_windows(n)],
            _ => continue,
        };
        p.push(Paso::Ejecutar { argv, tolerar: true, reintentos: 0 });
    }
    p
}

/// Los binarios que se copian: los del paquete que estén junto al `ea` que
/// corre (`ea-escritorio` solo con app de escritorio).
fn binarios_a_copiar(ctx: &Contexto, sonda: &dyn Sonda) -> Vec<String> {
    let mut v: Vec<String> = origen::BINARIOS.iter().map(|b| origen::nombre_binario(ctx.sistema, b)).collect();
    let escritorio = origen::nombre_binario(ctx.sistema, origen::BINARIO_ESCRITORIO);
    if ctx.con_escritorio && sonda.existe(&ctx.exes_origen.join(&escritorio)) {
        v.push(escritorio);
    }
    v
}

/// El plan completo de `ea instalar` (sin la vía WSL ni las ofertas de
/// dependencias, que van aparte), en el orden del plan de la spec 023.
/// `servicios` es el entorno del gestor de servicios del usuario; su
/// `exes_dir` se sustituye por `destinos.bin`. `ajustes_toml` es el fichero
/// de ajustes, que se crea si no existe.
pub fn plan_instalar_todo(ctx: &Contexto, servicios: &Entorno, ajustes_toml: &Path, sonda: &dyn Sonda) -> Result<Vec<Paso>, String> {
    let mut p = plan_parar(servicios, sonda);
    if ctx.con_escritorio {
        // La app también tiene su binario abierto.
        p.extend(plan_parar_escritorio(ctx));
    }
    // Binarios.
    match &ctx.origen {
        Origen::Paquete => {
            if ctx.exes_origen != ctx.destinos.bin {
                for b in binarios_a_copiar(ctx, sonda) {
                    p.push(Paso::Copiar { origen: ctx.exes_origen.join(&b), destino: ctx.destinos.bin.join(&b) });
                }
            }
        }
        Origen::Clon { repo } => {
            // `ea-escritorio` se compila aparte, en `escritorio/target`: se
            // enlaza junto a los demás para que lo encuentren `ea` y el
            // orquestador.
            let nombre = origen::nombre_binario(ctx.sistema, origen::BINARIO_ESCRITORIO);
            let compilado = repo.join("escritorio").join("target").join("release").join(&nombre);
            let junto = ctx.destinos.bin.join(&nombre);
            if ctx.con_escritorio && sonda.existe(&compilado) && compilado != junto {
                p.push(Paso::Enlazar { origen: compilado, destino: junto });
            }
        }
    }
    p.extend(ruta_usuario::plan_ruta(ctx, sonda));
    if !sonda.existe(ajustes_toml) {
        p.push(Paso::Escribir { ruta: ajustes_toml.to_path_buf(), contenido: AJUSTES_INICIALES.into() });
    }
    if ctx.origen == Origen::Paquete {
        for r in recursos::RECURSOS.iter().filter(|r| r.ruta.starts_with("agentes/flujo/")) {
            let nombre = r.ruta.trim_start_matches("agentes/flujo/");
            p.push(Paso::Escribir { ruta: ctx.destinos.flujo.join(nombre), contenido: r.contenido.into() });
        }
    }
    p.extend(agentes::plan_agentes(ctx, sonda)?);
    let entorno = Entorno { exes_dir: ctx.destinos.bin.clone(), ..servicios.clone() };
    p.extend(servicios::plan_instalar(&entorno)?);
    p.extend(menu::plan_menu(ctx));
    Ok(p)
}

/// Cerrar `ea-escritorio` si está abierto (tolerante).
pub fn plan_parar_escritorio(ctx: &Contexto) -> Vec<Paso> {
    let argv: Vec<String> = match ctx.sistema {
        Sistema::Windows => vec!["taskkill".into(), "/IM".into(), "ea-escritorio.exe".into(), "/F".into()],
        _ => vec!["pkill".into(), "-x".into(), "ea-escritorio".into()],
    };
    vec![Paso::Ejecutar { argv, tolerar: true, reintentos: 0 }]
}

/// El plan de `ea desinstalar`: lo inverso, sin tocar datos (base de datos,
/// bóveda, ajustes y bus se quedan).
pub fn plan_desinstalar_todo(ctx: &Contexto, servicios: &Entorno, sonda: &dyn Sonda) -> Result<Vec<Paso>, String> {
    let entorno = Entorno { exes_dir: ctx.destinos.bin.clone(), ..servicios.clone() };
    let mut p = servicios::plan_desinstalar(&entorno)?;
    p.extend(plan_parar_escritorio(ctx));
    p.extend(menu::plan_quitar_menu(ctx));
    p.extend(agentes::plan_quitar_agentes(ctx, sonda)?);
    p.extend(ruta_usuario::plan_quitar_ruta(ctx, sonda));
    if ctx.origen == Origen::Paquete {
        for r in recursos::RECURSOS.iter().filter(|r| r.ruta.starts_with("agentes/flujo/")) {
            p.push(Paso::Borrar(ctx.destinos.flujo.join(r.ruta.trim_start_matches("agentes/flujo/"))));
        }
        let mut binarios: Vec<String> = origen::BINARIOS.iter().map(|b| origen::nombre_binario(ctx.sistema, b)).collect();
        binarios.push(origen::nombre_binario(ctx.sistema, origen::BINARIO_ESCRITORIO));
        for b in binarios {
            // En Windows, el `ea.exe` que corre no se puede borrar: se dice
            // al final (lo hace `ea-cli`).
            if ctx.sistema == Sistema::Windows && b == "ea.exe" {
                continue;
            }
            p.push(Paso::Borrar(ctx.destinos.bin.join(b)));
        }
    }
    Ok(p)
}

#[cfg(test)]
mod pruebas {
    use std::collections::{HashMap, HashSet};

    use super::*;

    /// Una sonda con lo que diga cada prueba.
    #[derive(Default)]
    struct SondaFalsa {
        existen: HashSet<PathBuf>,
        ficheros: HashMap<PathBuf, String>,
        enlaces: HashMap<PathBuf, PathBuf>,
        path: Option<String>,
    }

    impl Sonda for SondaFalsa {
        fn binario(&self, _: &str) -> Option<PathBuf> {
            None
        }
        fn existe(&self, ruta: &Path) -> bool {
            self.existen.contains(ruta) || self.ficheros.contains_key(ruta)
        }
        fn leer(&self, ruta: &Path) -> Option<String> {
            self.ficheros.get(ruta).cloned()
        }
        fn salida(&self, _: &[&str]) -> Option<String> {
            None
        }
        fn http_ok(&self, _: &str) -> bool {
            false
        }
        fn var(&self, nombre: &str) -> Option<String> {
            (nombre == "PATH").then(|| self.path.clone()).flatten()
        }
        fn destino_enlace(&self, ruta: &Path) -> Option<PathBuf> {
            self.enlaces.get(ruta).cloned()
        }
    }

    #[test]
    fn un_ea_ajeno_en_local_bin_no_se_pisa() {
        let ctx = contexto(Sistema::Linux, Origen::Paquete);
        let enlace = ctx.home.join(".local/bin/ea");
        let tiene_enlace = |s: &SondaFalsa| ruta_usuario::plan_ruta(&ctx, s).iter().any(|p| matches!(p, Paso::Enlazar { destino, .. } if *destino == enlace));
        let path = Some(format!("/usr/bin:{}", ctx.home.join(".local/bin").display()));
        // Un fichero normal ajeno.
        let s = SondaFalsa { existen: HashSet::from([enlace.clone()]), path: path.clone(), ..Default::default() };
        assert!(!tiene_enlace(&s));
        assert!(!ruta_usuario::avisos_ruta(&ctx, &s).is_empty());
        // Un enlace a otro programa.
        let s = SondaFalsa {
            existen: HashSet::from([enlace.clone()]),
            enlaces: HashMap::from([(enlace.clone(), PathBuf::from("/opt/otro/ea"))]),
            path: path.clone(),
            ..Default::default()
        };
        assert!(!tiene_enlace(&s));
        // El nuestro (o nada) sí.
        let s = SondaFalsa {
            existen: HashSet::from([enlace.clone()]),
            enlaces: HashMap::from([(enlace.clone(), ctx.destinos.bin.join("ea"))]),
            path: path.clone(),
            ..Default::default()
        };
        assert!(tiene_enlace(&s));
        assert!(tiene_enlace(&SondaFalsa { path, ..Default::default() }));
    }

    fn contexto(sistema: Sistema, origen: Origen) -> Contexto {
        let home = PathBuf::from(if sistema == Sistema::Windows { "C:/Users/ana" } else { "/home/ana" });
        let destinos = origen::destinos(sistema, &origen, &home, |_| None);
        Contexto {
            sistema,
            origen,
            exes_origen: PathBuf::from("/tmp/paquete"),
            destinos,
            home: home.clone(),
            skills_dir: home.join(".config/ecosistema-asistente/qwen-skills"),
            puerto_mcp: 8765,
            con_escritorio: true,
        }
    }

    fn entorno(sistema: Sistema) -> Entorno {
        Entorno {
            backend: Backend::de(sistema),
            exes_dir: PathBuf::from("/tmp/paquete"),
            unidades_dir: PathBuf::from("/home/ana/.config/systemd/user"),
            logs_dir: PathBuf::from("/home/ana/.local/state/ecosistema-asistente/logs"),
            uid: Some(501),
            path: None,
            usuario: Some("PC\\ana".into()),
        }
    }

    fn ajustes() -> PathBuf {
        PathBuf::from("/home/ana/.config/ecosistema-asistente/ajustes.toml")
    }

    fn posicion(p: &[Paso], f: impl Fn(&Paso) -> bool) -> usize {
        p.iter().position(f).expect("paso esperado")
    }

    fn es_orden(x: &Paso, prog: &str) -> bool {
        matches!(x, Paso::Ejecutar { argv, .. } if argv.first().map(String::as_str) == Some(prog))
    }

    fn lleva(x: &Paso, arg: &str) -> bool {
        matches!(x, Paso::Ejecutar { argv, .. } if argv.iter().any(|a| a.contains(arg)))
    }

    /// Una ruta o un texto con `/` (en Windows `join` pone `\`, y las
    /// unidades citan la barra invertida).
    fn n(t: impl AsRef<std::ffi::OsStr>) -> String {
        Path::new(t.as_ref()).to_string_lossy().replace("\\\\", "/").replace('\\', "/")
    }

    fn escribe(x: &Paso, fin: &str) -> bool {
        matches!(x, Paso::Escribir { ruta, .. } | Paso::EditarPersonal { ruta, .. } if ruta.ends_with(fin))
    }

    #[test]
    fn el_plan_de_un_paquete_en_linux_sigue_el_orden() {
        let ctx = contexto(Sistema::Linux, Origen::Paquete);
        let mut sonda = SondaFalsa::default();
        sonda.existen.insert(PathBuf::from("/tmp/paquete/ea-escritorio"));
        sonda.existen.insert(PathBuf::from("/home/ana/.config/systemd/user/ea-web.service"));
        sonda.path = Some("/usr/bin".into());
        let p = plan_instalar_todo(&ctx, &entorno(Sistema::Linux), &ajustes(), &sonda).unwrap();

        let parar = posicion(&p, |x| lleva(x, "stop"));
        let copia = posicion(&p, |x| matches!(x, Paso::Copiar { destino, .. } if destino.ends_with("bin/ea-web")));
        let enlace = posicion(&p, |x| matches!(x, Paso::Enlazar { destino, .. } if n(destino) == "/home/ana/.local/bin/ea"));
        let toml = posicion(&p, |x| matches!(x, Paso::Escribir { ruta, .. } | Paso::EditarPersonal { ruta, .. } if ruta == &ajustes()));
        let unidad = posicion(&p, |x| {
            matches!(x, Paso::Escribir { ruta, contenido } | Paso::EditarPersonal { ruta, contenido } if ruta.ends_with("ea-web.service") && n(contenido).contains("/home/ana/.local/share/ecosistema-asistente/bin/ea-web"))
        });
        let desktop = posicion(&p, |x| escribe(x, "ea-escritorio.desktop"));
        assert!(parar < copia && copia < enlace && enlace < toml && toml < unidad && unidad < desktop);
        // Solo se para lo instalado.
        assert_eq!(p.iter().filter(|x| lleva(x, "stop")).count(), 1);
        // El escritorio se copia porque está en el paquete.
        assert!(p.iter().any(|x| matches!(x, Paso::Copiar { destino, .. } if destino.ends_with("bin/ea-escritorio"))));
        // Sin ~/.local/bin en el PATH, el bloque en ~/.profile.
        assert!(p.iter().any(|x| matches!(x, Paso::Escribir { ruta, contenido } | Paso::EditarPersonal { ruta, contenido } if ruta.ends_with(".profile") && contenido.contains(ruta_usuario::MARCA_INICIO))));
    }

    #[test]
    fn repetir_no_duplica_ni_pisa_los_ajustes() {
        let ctx = contexto(Sistema::Linux, Origen::Paquete);
        let mut sonda = SondaFalsa { path: Some("/usr/bin".into()), ..Default::default() };
        let primero = plan_instalar_todo(&ctx, &entorno(Sistema::Linux), &ajustes(), &sonda).unwrap();
        let perfil = primero.iter().find_map(|x| match x {
            Paso::Escribir { ruta, contenido } | Paso::EditarPersonal { ruta, contenido } if ruta.ends_with(".profile") => Some(contenido.clone()),
            _ => None,
        });
        // Segunda vez: el perfil ya tiene el bloque y los ajustes existen.
        sonda.ficheros.insert(PathBuf::from("/home/ana/.profile"), perfil.unwrap());
        sonda.existen.insert(ajustes());
        let segundo = plan_instalar_todo(&ctx, &entorno(Sistema::Linux), &ajustes(), &sonda).unwrap();
        assert!(!segundo.iter().any(|x| escribe(x, ".profile")));
        assert!(!segundo.iter().any(|x| matches!(x, Paso::Escribir { ruta, .. } | Paso::EditarPersonal { ruta, .. } if ruta == &ajustes())));
        // Y con ~/.local/bin ya en el PATH, ni se mira el perfil.
        sonda.ficheros.clear();
        sonda.path = Some("/home/ana/.local/bin:/usr/bin".into());
        let tercero = plan_instalar_todo(&ctx, &entorno(Sistema::Linux), &ajustes(), &sonda).unwrap();
        assert!(!tercero.iter().any(|x| escribe(x, ".profile")));
    }

    #[test]
    fn desde_un_clon_no_se_copia_nada() {
        let repo = PathBuf::from("/src/gachapon");
        let mut ctx = contexto(Sistema::Linux, Origen::Clon { repo: repo.clone() });
        ctx.exes_origen = repo.join("target/release");
        let mut sonda = SondaFalsa::default();
        sonda.existen.insert(repo.join("escritorio/target/release/ea-escritorio"));
        let p = plan_instalar_todo(&ctx, &entorno(Sistema::Linux), &ajustes(), &sonda).unwrap();
        assert!(!p.iter().any(|x| matches!(x, Paso::Copiar { .. })));
        assert!(p.iter().any(|x| {
            matches!(x, Paso::Enlazar { origen, destino } if origen.ends_with("escritorio/target/release/ea-escritorio") && destino == &repo.join("target/release/ea-escritorio"))
        }));
        assert!(p.iter().any(|x| matches!(x, Paso::Escribir { ruta, contenido } | Paso::EditarPersonal { ruta, contenido } if ruta.ends_with("ea-web.service") && n(contenido).contains("/src/gachapon/target/release/ea-web"))));
    }

    #[test]
    fn en_macos_hay_app_y_launchd() {
        let ctx = contexto(Sistema::MacOs, Origen::Paquete);
        let mut sonda = SondaFalsa::default();
        sonda.existen.insert(PathBuf::from("/tmp/paquete/ea-escritorio"));
        let mut env = entorno(Sistema::MacOs);
        env.unidades_dir = PathBuf::from("/home/ana/Library/LaunchAgents");
        let p = plan_instalar_todo(&ctx, &env, &ajustes(), &sonda).unwrap();
        assert!(p.iter().any(|x| escribe(x, "gachapon.app/Contents/Info.plist")));
        assert!(p.iter().any(|x| matches!(x, Paso::Copiar { destino, .. } if destino.ends_with("gachapon.app/Contents/MacOS/ea-escritorio"))));
        assert!(p.iter().any(|x| escribe(x, ".zprofile")));
        assert!(p.iter().any(|x| es_orden(x, "launchctl")));
    }

    #[test]
    fn en_windows_para_antes_de_copiar_y_usa_el_path_del_usuario() {
        let ctx = contexto(Sistema::Windows, Origen::Paquete);
        let mut sonda = SondaFalsa::default();
        sonda.existen.insert(PathBuf::from("/tmp/paquete/ea-escritorio.exe"));
        let p = plan_instalar_todo(&ctx, &entorno(Sistema::Windows), &ajustes(), &sonda).unwrap();
        let fin = posicion(&p, |x| lleva(x, "/End"));
        let mata = posicion(&p, |x| es_orden(x, "taskkill"));
        let copia = posicion(&p, |x| matches!(x, Paso::Copiar { destino, .. } if destino.ends_with("ea-web.exe")));
        assert!(fin < copia && mata < copia);
        assert!(p.iter().any(|x| lleva(x, "SetEnvironmentVariable")));
        assert!(p.iter().any(|x| lleva(x, "CreateShortcut")));
        assert!(!p.iter().any(|x| matches!(x, Paso::Enlazar { .. })), "en Windows no hay ~/.local/bin");
    }

    #[test]
    fn sin_escritorio_no_hay_menu_ni_app() {
        let mut ctx = contexto(Sistema::Linux, Origen::Paquete);
        ctx.con_escritorio = false;
        let mut sonda = SondaFalsa::default();
        sonda.existen.insert(PathBuf::from("/tmp/paquete/ea-escritorio"));
        let p = plan_instalar_todo(&ctx, &entorno(Sistema::Linux), &ajustes(), &sonda).unwrap();
        assert!(!p.iter().any(|x| matches!(x, Paso::Copiar { destino, .. } if destino.ends_with("ea-escritorio"))));
        assert!(!p.iter().any(|x| escribe(x, ".desktop")));
        assert!(!p.iter().any(|x| es_orden(x, "pkill")));
    }

    #[test]
    fn desinstalar_no_toca_datos() {
        let ctx = contexto(Sistema::Linux, Origen::Paquete);
        let mut sonda = SondaFalsa::default();
        let enlace = PathBuf::from("/home/ana/.local/bin/ea");
        sonda.enlaces.insert(enlace.clone(), ctx.destinos.bin.join("ea"));
        let p = plan_desinstalar_todo(&ctx, &entorno(Sistema::Linux), &sonda).unwrap();
        let borrados: Vec<&PathBuf> = p.iter().filter_map(|x| if let Paso::Borrar(r) = x { Some(r) } else { None }).collect();
        assert!(borrados.contains(&&enlace));
        assert!(borrados.contains(&&ctx.destinos.bin.join("ea-web")));
        for r in &borrados {
            let s = r.to_string_lossy();
            assert!(!s.ends_with(".db") && !s.contains("ajustes.toml") && !s.contains("boveda"), "no se borran datos: {s}");
            assert!(!s.ends_with("ecosistema-asistente"), "la carpeta base no se borra");
        }
        // Un ~/.local/bin/ea que no es nuestro no se toca.
        sonda.enlaces.insert(enlace.clone(), PathBuf::from("/otro/ea"));
        let p = plan_desinstalar_todo(&ctx, &entorno(Sistema::Linux), &sonda).unwrap();
        assert!(!p.iter().any(|x| matches!(x, Paso::Borrar(r) if r == &enlace)));
    }

    #[test]
    fn desinstalar_un_clon_no_borra_sus_binarios() {
        let repo = PathBuf::from("/src/gachapon");
        let ctx = contexto(Sistema::Linux, Origen::Clon { repo: repo.clone() });
        let p = plan_desinstalar_todo(&ctx, &entorno(Sistema::Linux), &SondaFalsa::default()).unwrap();
        assert!(!p.iter().any(|x| matches!(x, Paso::Borrar(r) if r.starts_with(&repo))));
    }

    #[test]
    fn la_sonda_http_contesta_no_sin_servidor() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let puerto = l.local_addr().unwrap().port();
        drop(l);
        assert!(!http_ok(&format!("http://127.0.0.1:{puerto}/"), std::time::Duration::from_millis(200)));
        assert!(!http_responde("https://127.0.0.1/", std::time::Duration::from_millis(200)));
    }

    #[test]
    fn la_sonda_http_distingue_codigos() {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let puerto = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            for respuesta in ["HTTP/1.1 200 OK\r\n\r\n", "HTTP/1.1 405 Method Not Allowed\r\n\r\n", "HTTP/1.1 405 Method Not Allowed\r\n\r\n"] {
                let (mut c, _) = l.accept().unwrap();
                let mut buf = [0u8; 256];
                let _ = c.read(&mut buf);
                c.write_all(respuesta.as_bytes()).unwrap();
            }
        });
        let base = format!("http://127.0.0.1:{puerto}");
        assert!(http_ok(&format!("{base}/api/estado"), std::time::Duration::from_secs(2)));
        assert!(!http_ok(&format!("{base}/"), std::time::Duration::from_secs(2)));
        assert!(http_responde(&format!("{base}/"), std::time::Duration::from_secs(2)));
        h.join().unwrap();
    }
}
