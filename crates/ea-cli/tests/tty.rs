//! La frontera «sin TTY no» (ningún agente se confirma a sí mismo): cada orden
//! que confirma algo se lanza con el binario real sin terminal (stdin y stdout
//! canalizados, como la lanzaría un agente con Bash), con la confirmación ya
//! escrita en su entrada. Tiene que rechazarse con el texto de
//! `ea_acciones::terminal_interactiva()` sin cambiar el diario, las propuestas
//! ni los ficheros, y sin lanzar nada: los gestores de servicios, los avisos y
//! las terminales del PATH son dobles que dejan una marca. Todo el estado
//! (HOME, XDG, EA_*) vive en un directorio temporal.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use ea_ajustes::{Ajustes, Fuentes};
use ea_bus::{Bus, Miembro, NuevoEquipo, Presencia, RolEquipo};
use ea_pruebas::escribe;
use ea_store::{EstadoPropuesta, EventFilter, Propuesta, PropuestaFilter, Store};
use serde_json::json;

const SKILL: &str = "claude-code/skill/usuario/propia";

/// Lo que dice `terminal_interactiva()` sin TTY.
const SIN_TTY: &str = "la confirmación debe darse en una terminal interactiva (sin TTY no se confirma)";

/// Programas que una orden confirmada lanzaría (servicios, avisos,
/// terminales, sudo): en el PATH de la prueba son dobles que dejan una marca.
#[cfg(unix)]
const DOBLES: &[&str] = &[
    "systemctl",
    "systemd-analyze",
    "loginctl",
    "launchctl",
    "schtasks",
    "notify-send",
    "xdg-open",
    "xdg-terminal-exec",
    "omarchy-launch-terminal",
    "open",
    "osascript",
    "sudo",
];

struct Mundo {
    d: tempfile::TempDir,
    /// Propuesta pendiente de `ea_eliminar_rutina` (irreversible).
    irreversible: i64,
    /// Propuesta pendiente de `ea_desactivar_herramienta` (reversible).
    reversible: i64,
    /// Propuesta de solo texto.
    texto: i64,
    /// Un equipo despierto.
    equipo: String,
}

#[cfg(unix)]
fn dobles(bin: &Path, marcas: &Path) {
    use std::os::unix::fs::PermissionsExt;
    for n in DOBLES {
        let ruta = bin.join(n);
        escribe(&ruta, &format!("#!/bin/sh\n: > '{}'\nexit 1\n", marcas.join(n).display()));
        std::fs::set_permissions(&ruta, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// En Windows `Command` solo busca `.exe`: no hay dobles (ni se escribe la
/// confirmación en la entrada, por si algún día una orden no la rechazara).
#[cfg(not(unix))]
fn dobles(_bin: &Path, _marcas: &Path) {}

fn mundo() -> Mundo {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    for dir in ["home/proyecto", "rt", "marcas", "xdg/config", "xdg/datos", "xdg/estado", "xdg/cache"] {
        std::fs::create_dir_all(r.join(dir)).unwrap();
    }
    dobles(&r.join("bin"), &r.join("marcas"));
    escribe(r.join("home/.claude/skills/propia/SKILL.md"), "---\ndescription: Hace cosas\n---\n");
    // Una rutina en uso y otra retirada (en la papelera).
    let rutina = |slug: &str| format!("[rutina]\nslug = \"{slug}\"\nversion = 1\nworktree = \"/tmp\"\ncriterio_exito = \"true\"\n[[pasos]]\ntexto = \"x\"\n");
    escribe(r.join("config/rutinas/activa.toml"), &rutina("activa"));
    escribe(r.join("config/rutinas/.papelera/vieja.toml"), &rutina("vieja"));
    // Una nota de un agente y algo en la papelera de la bóveda.
    escribe(r.join("boveda/claude-code/nota.md"), "# Nota\n");
    escribe(r.join("boveda/.papelera/borrada.md"), "# Borrada\n");

    let bus = Bus::abrir_en(&r.join("estado")).unwrap();
    let ajustes = Ajustes::por_defecto(&Fuentes::aislada(&r.join("config")));
    let m = |agente: &str, rol| Miembro { agente: agente.into(), rol };
    let equipo = bus
        .crear_equipo(
            NuevoEquipo {
                titulo: "equipo TTY".into(),
                objetivo: "probar la frontera".into(),
                worktree_base: r.to_string_lossy().into(),
                miembros: vec![m("claude-code", RolEquipo::Planificador), m("qwen-code", RolEquipo::Ejecutor)],
                creado_por: "test".into(),
            },
            &ajustes,
        )
        .unwrap();
    bus.anotar_presencia(&Presencia::nueva("claude-code", RolEquipo::Planificador, Some(&equipo.id), std::process::id())).unwrap();

    // Una propuesta irreversible como la dejaría un agente por MCP.
    let store = Store::open_at(&r.join("ea.db")).unwrap();
    let consulta = ea_acciones::SoloConsulta::usuario(r.join("config/rutinas"), ea_boveda::Boveda::en(r.join("boveda")), r.join("estado"));
    let args = json!({ "slug": "vieja" });
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let hechos = rt.block_on(ea_acciones::observar(&consulta, "ea_eliminar_rutina", &args)).unwrap();
    let (p, _) = ea_acciones::proponer(&store, &consulta, &hechos, "claude-code", None, "ea_eliminar_rutina", &args).unwrap();
    assert_eq!(p.accion.as_ref().unwrap().riesgo, "irreversible");
    let irreversible = p.id;
    let texto = store
        .crear_propuesta(ea_store::NuevaPropuesta { proponente: "claude-code".into(), descripcion: "solo texto".into(), accion: None })
        .unwrap()
        .id;
    drop(store);

    let mut m = Mundo { d, irreversible, reversible: 0, texto, equipo: equipo.id };
    // Una reversible, la que deja la propia CLI sin terminal.
    let o = m.ea(&["biblioteca", "desactivar", SKILL], "");
    assert!(o.status.success(), "{}", salida(&o));
    m.reversible = m.propuestas().iter().find(|p| p.accion.as_ref().is_some_and(|a| a.herramienta == "ea_desactivar_herramienta")).unwrap().id;
    m
}

fn salida(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

impl Mundo {
    fn ruta(&self, p: &str) -> PathBuf {
        self.d.path().join(p)
    }

    /// `ea` sin terminal, con `respuesta` en la entrada y nada del entorno
    /// real: ni HOME, ni XDG, ni EA_*.
    fn ea(&self, args: &[&str], respuesta: &str) -> Output {
        let r = self.d.path();
        let mut c = Command::new(env!("CARGO_BIN_EXE_ea"));
        for (k, _) in std::env::vars_os() {
            if k.to_string_lossy().starts_with("EA_") || k.to_string_lossy().starts_with("XDG_") {
                c.env_remove(k);
            }
        }
        let path = std::env::join_paths(std::iter::once(r.join("bin")).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))).unwrap();
        let mut hijo = c
            .args(args)
            .current_dir(r.join("home/proyecto"))
            .env("PATH", path)
            .env("HOME", r.join("home"))
            .env("USERPROFILE", r.join("home"))
            .env("APPDATA", r.join("home/AppData/Roaming"))
            .env("LOCALAPPDATA", r.join("rt"))
            .env("XDG_CONFIG_HOME", r.join("xdg/config"))
            .env("XDG_DATA_HOME", r.join("xdg/datos"))
            .env("XDG_STATE_HOME", r.join("xdg/estado"))
            .env("XDG_CACHE_HOME", r.join("xdg/cache"))
            .env("XDG_RUNTIME_DIR", r.join("rt"))
            .env("EA_CONFIG_DIR", r.join("config"))
            .env("EA_DB_PATH", r.join("ea.db"))
            .env("EA_ESTADO_DIR", r.join("estado"))
            .env("EA_BOVEDA_DIR", r.join("boveda"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("lanzar ea");
        let mut entrada = hijo.stdin.take().unwrap();
        if cfg!(unix) {
            // Si ea sale antes de leerla, la tubería está cerrada: da igual.
            let _ = entrada.write_all(respuesta.as_bytes());
        }
        drop(entrada);
        hijo.wait_with_output().expect("esperar a ea")
    }

    fn propuestas(&self) -> Vec<Propuesta> {
        Store::open_at(&self.ruta("ea.db")).unwrap().list_propuestas(&PropuestaFilter::default()).unwrap()
    }

    fn eventos(&self) -> usize {
        Store::open_at(&self.ruta("ea.db")).unwrap().list_events(&EventFilter { limit: Some(100_000), ..Default::default() }).unwrap().len()
    }

    /// Todo lo que hay bajo el directorio de la prueba salvo la base de datos
    /// (que se mira por el diario y las propuestas): ruta → (tamaño, fecha) de
    /// los ficheros, y los directorios.
    fn ficheros(&self) -> BTreeMap<String, Option<(u64, std::time::SystemTime)>> {
        fn recorrer(raiz: &Path, dir: &Path, v: &mut BTreeMap<String, Option<(u64, std::time::SystemTime)>>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                let rel = p.strip_prefix(raiz).unwrap().to_string_lossy().into_owned();
                if rel.starts_with("ea.db") {
                    continue;
                }
                let meta = std::fs::symlink_metadata(&p).unwrap();
                if meta.is_dir() {
                    v.insert(rel, None);
                    recorrer(raiz, &p, v);
                } else {
                    v.insert(rel, Some((meta.len(), meta.modified().unwrap())));
                }
            }
        }
        let mut v = BTreeMap::new();
        recorrer(self.d.path(), self.d.path(), &mut v);
        v
    }

    fn marcas(&self) -> Vec<String> {
        std::fs::read_dir(self.ruta("marcas")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()
    }
}

/// Lo que se espera sin terminal.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Sin {
    /// Se rechaza con el texto de `terminal_interactiva()`.
    Rechaza,
    /// Deja (o encuentra) la propuesta pendiente y no acepta nada.
    Propone,
}

#[test]
fn sin_terminal_ninguna_orden_confirma_nada() {
    let m = mundo();
    let (irr, rev) = (m.irreversible.to_string(), m.reversible.to_string());
    let eq = m.equipo.as_str();
    let confirma_irr = format!("confirmo {irr}\n");
    let tabla: Vec<(Vec<&str>, &str, Sin)> = vec![
        (vec!["propuestas", "decidir", &irr, "--aceptar"], &confirma_irr, Sin::Rechaza),
        (vec!["propuestas", "decidir", &rev, "--aceptar"], "s\n", Sin::Rechaza),
        (vec!["acciones", "ejecutar", "ea_eliminar_rutina", "--args", r#"{"slug":"vieja"}"#], "confirmo ea_eliminar_rutina\n", Sin::Rechaza),
        (vec!["acciones", "ejecutar", "ea_retirar_rutina", "--args", r#"{"slug":"activa"}"#], "s\n", Sin::Rechaza),
        (vec!["boveda", "vaciar-papelera"], "confirmo vaciar-papelera\n", Sin::Rechaza),
        (vec!["boveda", "promover", "claude-code/nota.md"], "s\n", Sin::Rechaza),
        (vec!["worktree", "limpiar", "20260101T000000-00000001"], "s\n", Sin::Rechaza),
        (vec!["rutina", "retirar", "activa"], "s\n", Sin::Rechaza),
        (vec!["rutina", "restaurar", "vieja"], "s\n", Sin::Rechaza),
        (vec!["servicios", "instalar"], "s\n", Sin::Rechaza),
        (vec!["servicios", "desinstalar"], "s\n", Sin::Rechaza),
        (vec!["instalar", "--sin-dependencias", "--sin-escritorio"], "s\n", Sin::Rechaza),
        (vec!["desinstalar"], "s\n", Sin::Rechaza),
        (vec!["agentes", "rol", "ejecutor", "codex"], "", Sin::Rechaza),
        (vec!["ajustes", "establecer", "web.sondeo_ms=4000"], "", Sin::Rechaza),
        (vec!["biblioteca", "categoria", SKILL, "mia"], "", Sin::Rechaza),
        (vec!["equipo", "despertar", "--miembro", "claude-code:planificador", "--titulo", "t", "--objetivo", "o", "--worktree", "/tmp"], "s\n", Sin::Rechaza),
        (vec!["equipo", "dormir", eq], "", Sin::Rechaza),
        (vec!["equipo", "sesion", eq, "claude-code"], "", Sin::Rechaza),
        (vec!["biblioteca", "desactivar", SKILL, "--esperar"], "s\n\n", Sin::Propone),
        (vec!["biblioteca", "sesion", "claude-code", SKILL, "--esperar"], "s\n\n", Sin::Propone),
    ];
    for (args, respuesta, esperado) in tabla {
        let (ficheros, eventos, propuestas) = (m.ficheros(), m.eventos(), m.propuestas());
        let o = m.ea(&args, respuesta);
        let t = salida(&o);
        match esperado {
            Sin::Rechaza => {
                assert!(!o.status.success(), "{args:?} no se rechazó: {t}");
                assert!(t.contains(SIN_TTY), "{args:?}: {t}");
                assert_eq!(m.eventos(), eventos, "{args:?} escribió en el diario: {t}");
                let despues = m.propuestas();
                assert_eq!(
                    despues.iter().map(|p| (p.id, p.estado.as_str())).collect::<Vec<_>>(),
                    propuestas.iter().map(|p| (p.id, p.estado.as_str())).collect::<Vec<_>>(),
                    "{args:?} cambió las propuestas: {t}"
                );
            }
            Sin::Propone => {
                assert!(o.status.success(), "{args:?}: {t}");
                assert!(t.contains("Sin terminal interactiva no se acepta"), "{args:?}: {t}");
                // Ninguna se acepta: todas siguen pendientes y sin ejecutar.
                for p in m.propuestas() {
                    assert!(p.estado == EstadoPropuesta::Pendiente && p.ejecucion.is_none(), "{args:?} aceptó #{}: {t}", p.id);
                }
            }
        }
        assert_eq!(m.ficheros(), ficheros, "{args:?} cambió ficheros: {t}");
        assert!(m.marcas().is_empty(), "{args:?} lanzó {:?}: {t}", m.marcas());
    }
}

/// Lo que no exige terminal, a propósito: rechazar una propuesta y aceptar una
/// de solo texto (deja constancia, no ejecuta nada).
#[test]
fn sin_terminal_se_rechaza_y_se_acepta_lo_que_no_ejecuta_nada() {
    let m = mundo();
    let o = m.ea(&["propuestas", "decidir", &m.texto.to_string(), "--aceptar"], "");
    assert!(o.status.success(), "{}", salida(&o));
    let o = m.ea(&["propuestas", "decidir", &m.irreversible.to_string(), "--rechazar"], "");
    assert!(o.status.success(), "{}", salida(&o));
    let estado = |id: i64| m.propuestas().into_iter().find(|p| p.id == id).unwrap().estado;
    assert_eq!((estado(m.texto), estado(m.irreversible)), (EstadoPropuesta::Aceptada, EstadoPropuesta::Rechazada));
    assert!(m.ruta("config/rutinas/.papelera/vieja.toml").exists(), "rechazar no borra nada");
    assert!(m.marcas().is_empty(), "{:?}", m.marcas());
}

/// `ea trabajo confirmar` (devolver a la cola una rutina en espera con el
/// contexto del usuario) hoy NO exige terminal: es la decisión U-04, pendiente
/// del usuario. Si se decide exigirla, esta prueba cambia con ella.
#[test]
fn trabajo_confirmar_hoy_no_exige_terminal_u04() {
    let m = mundo();
    escribe(m.ruta("informe.toml"), "[rutina]\nslug = \"informe\"\nversion = 1\nworktree = \"/tmp\"\ncriterio_exito = \"true\"\n[[pasos]]\ntexto = \"x\"\n");
    let o = m.ea(&["rutina", "registrar", m.ruta("informe.toml").to_str().unwrap()], "");
    assert!(o.status.success(), "{}", salida(&o));
    std::fs::copy(m.ruta("informe.toml"), m.ruta("config/rutinas/informe.toml")).unwrap();
    let o = m.ea(&["rutina", "lanzar", "informe"], "");
    assert!(o.status.success(), "{}", salida(&o));
    let id = String::from_utf8_lossy(&o.stdout).trim().to_string();
    let bus = Bus::abrir_en(&m.ruta("estado")).unwrap().con_store(Store::open_at(&m.ruta("ea.db")).unwrap());
    bus.retirar_de_colas(&id).unwrap();
    bus.aplicar(&id, ea_bus::Evento::Tomar).unwrap();
    let informe = serde_json::from_value(json!({ "resultado": "bloqueado", "motivo": "no pasa", "duracion_s": 1 })).unwrap();
    assert_eq!(bus.entregar(&id, informe).unwrap(), ea_bus::Entrega::EnEspera);

    let o = m.ea(&["trabajo", "confirmar", &id], "");
    assert!(o.status.success(), "{}", salida(&o));
    assert_ne!(bus.leer_estado(&id).unwrap().estado, ea_bus::EstadoTrabajo::EnEspera);
}
