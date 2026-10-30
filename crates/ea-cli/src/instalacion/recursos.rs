//! Lo que `ea instalar` lleva dentro (spec 023, decisión 3): las skills de
//! Claude Code y las reglas del planificador (de ellas se generan los
//! adaptadores de los otros planificadores, ver [`super::adaptadores`]), las
//! guías `agentes/flujo/*.md`, las skills del ejecutor y el icono de la app.
//! Así un paquete descargado no necesita el repo.

use std::path::Path;

/// Un fichero del repo embebido en el binario.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recurso {
    /// Ruta relativa a la raíz del repo, con `/`.
    pub ruta: &'static str,
    pub contenido: &'static str,
}

/// Todos los ficheros de `claude/skills/**`, `agentes/**` y
/// `qwen/skills/*.md`. Una prueba comprueba que coinciden con el repo.
pub const RECURSOS: &[Recurso] = &[
    Recurso { ruta: "agentes/flujo/equipo.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../agentes/flujo/equipo.md")) },
    Recurso { ruta: "agentes/flujo/mision.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../agentes/flujo/mision.md")) },
    Recurso { ruta: "agentes/flujo/obra.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../agentes/flujo/obra.md")) },
    Recurso { ruta: "agentes/flujo/revisar.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../agentes/flujo/revisar.md")) },
    Recurso { ruta: "agentes/flujo/rutina.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../agentes/flujo/rutina.md")) },
    Recurso { ruta: "agentes/planificador.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../agentes/planificador.md")) },
    Recurso { ruta: "claude/skills/equipo/SKILL.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../claude/skills/equipo/SKILL.md")) },
    Recurso { ruta: "claude/skills/mision/SKILL.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../claude/skills/mision/SKILL.md")) },
    Recurso { ruta: "claude/skills/obra/SKILL.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../claude/skills/obra/SKILL.md")) },
    Recurso { ruta: "claude/skills/revisar/SKILL.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../claude/skills/revisar/SKILL.md")) },
    Recurso { ruta: "claude/skills/rutina/SKILL.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../claude/skills/rutina/SKILL.md")) },
    Recurso { ruta: "qwen/skills/depurar.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qwen/skills/depurar.md")) },
    Recurso { ruta: "qwen/skills/documentar.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qwen/skills/documentar.md")) },
    Recurso { ruta: "qwen/skills/explorar.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qwen/skills/explorar.md")) },
    Recurso { ruta: "qwen/skills/python.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qwen/skills/python.md")) },
    Recurso { ruta: "qwen/skills/refactor.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qwen/skills/refactor.md")) },
    Recurso { ruta: "qwen/skills/resumir.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qwen/skills/resumir.md")) },
    Recurso { ruta: "qwen/skills/rust.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qwen/skills/rust.md")) },
    Recurso { ruta: "qwen/skills/shell.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qwen/skills/shell.md")) },
    Recurso { ruta: "qwen/skills/tests.md", contenido: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qwen/skills/tests.md")) },
];

/// El icono de la app de escritorio (entrada de menú).
pub const ICONO_PNG: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../escritorio/iconos/icono.png"));

/// El marcador que llevan los adaptadores donde va la carpeta de las guías;
/// al instalar se cambia siempre por la carpeta real (la del repo o la
/// copiada desde el paquete). Sin sustituir, el adaptador sigue siendo
/// válido: dice además dónde está la guía dentro del repo.
pub const MARCA_FLUJO: &str = "{{flujo}}/";

/// Cambia [`MARCA_FLUJO`] por `flujo`, con `/` final y barras `/`
/// también en Windows (los agentes las entienden y el Markdown no las
/// escapa).
pub fn reescribir_ruta_flujo(texto: &str, flujo: &Path) -> String {
    let mut real = flujo.to_string_lossy().replace('\\', "/");
    if !real.ends_with('/') {
        real.push('/');
    }
    texto.replace(MARCA_FLUJO, &real)
}

/// El recurso de esa ruta (relativa a la raíz del repo).
pub fn recurso(ruta: &str) -> Option<&'static Recurso> {
    RECURSOS.iter().find(|r| r.ruta == ruta)
}

/// Los recursos bajo un prefijo (`"claude/skills/"`, `"qwen/skills/"`…).
pub fn bajo(prefijo: &str) -> impl Iterator<Item = &'static Recurso> + '_ {
    RECURSOS.iter().filter(move |r| r.ruta.starts_with(prefijo))
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use std::path::PathBuf;

    fn raiz() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn recorrer(dir: &Path, base: &Path, salida: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                recorrer(&p, base, salida);
            } else {
                salida.push(p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }

    #[test]
    fn los_recursos_coinciden_con_el_repo() {
        let base = raiz();
        let mut en_repo = Vec::new();
        recorrer(&base.join("claude/skills"), &base, &mut en_repo);
        recorrer(&base.join("agentes"), &base, &mut en_repo);
        for e in std::fs::read_dir(base.join("qwen/skills")).unwrap().flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.ends_with(".md") {
                en_repo.push(format!("qwen/skills/{n}"));
            }
        }
        en_repo.sort();
        let mut embebidos: Vec<String> = RECURSOS.iter().map(|r| r.ruta.to_string()).collect();
        embebidos.sort();
        assert_eq!(en_repo, embebidos, "añade el fichero nuevo a RECURSOS (recursos.rs)");
        for r in RECURSOS {
            let disco = std::fs::read_to_string(base.join(r.ruta)).unwrap();
            assert_eq!(disco, r.contenido, "{}", r.ruta);
        }
    }

    #[test]
    fn reescribe_la_ruta_de_las_guias() {
        let t = "lee `{{flujo}}/mision.md` y síguelo";
        assert_eq!(reescribir_ruta_flujo(t, Path::new("/opt/g/flujo")), "lee `/opt/g/flujo/mision.md` y síguelo");
        assert_eq!(reescribir_ruta_flujo(t, Path::new("C:\\g\\flujo\\")), "lee `C:/g/flujo/mision.md` y síguelo");
        assert!(recurso("claude/skills/mision/SKILL.md").unwrap().contenido.contains(MARCA_FLUJO));
    }

    #[test]
    fn lo_embebido_no_lleva_rutas_personales() {
        // Cada adaptador nombra las guías con el marcador, que el instalador
        // sustituye; ninguno con la carpeta de una persona ni de un HOME.
        for r in RECURSOS {
            for personal in ["Proyectos/", "/home/", "/Users/", "C:\\Users"] {
                assert!(!r.contenido.contains(personal), "{}: {personal}", r.ruta);
            }
            let adaptador = !r.ruta.starts_with("agentes/flujo/") && !r.ruta.starts_with("qwen/");
            if adaptador {
                assert!(r.contenido.contains(MARCA_FLUJO), "{} sin el marcador de las guías", r.ruta);
                let flujo = Path::new("/srv/clon/agentes/flujo");
                assert!(!reescribir_ruta_flujo(r.contenido, flujo).contains("{{"), "{}", r.ruta);
            }
        }
    }

    #[test]
    fn ayudantes() {
        assert_eq!(bajo("agentes/flujo/").count(), 5);
        assert!(bajo("qwen/skills/").all(|r| r.ruta.ends_with(".md")));
        assert!(ICONO_PNG.starts_with(b"\x89PNG"));
    }
}
