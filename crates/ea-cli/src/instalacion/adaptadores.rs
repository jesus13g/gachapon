//! Los adaptadores de cada planificador se generan al instalar desde una sola
//! fuente por skill (U-18): la skill de Claude Code
//! (`claude/skills/<x>/SKILL.md`, que sigue siendo una skill válida tal cual)
//! y el fragmento común de las reglas del planificador
//! (`agentes/planificador.md`). Cada agente solo cambia el nombre, el título y
//! la cabecera.

use super::recursos::{self, Recurso};

/// La fuente común de las reglas del planificador (el `AGENTS.md` de Codex y
/// OpenCode y la regla de Antigravity).
pub const FUENTE_PLANIFICADOR: &str = "agentes/planificador.md";

const PREFIJO_SKILLS: &str = "claude/skills/";

/// Cómo escribe un agente cada skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Formato {
    /// Claude Code: la fuente tal cual, en `<x>/SKILL.md` (`/x`).
    Claude,
    /// Codex y Antigravity: `ea-<x>/SKILL.md`, con `name: ea-<x>` y el título
    /// `# ea-<x>`.
    Prefijada,
    /// OpenCode: el comando `ea-<x>.md`, sin `name` ni título y con la
    /// petición del usuario (`$ARGUMENTS`) al final.
    Comando,
}

/// Un fichero generado, con su ruta relativa a la carpeta del montaje (con
/// `/`). La unidad es su primer componente: la carpeta de la skill o el
/// fichero suelto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generado {
    pub ruta: String,
    pub contenido: String,
}

impl Generado {
    pub fn unidad(&self) -> &str {
        self.ruta.split('/').next().unwrap_or(&self.ruta)
    }
}

/// El nombre de la skill de una fuente (`claude/skills/<x>/SKILL.md`).
fn nombre(r: &Recurso) -> &'static str {
    let resto = &r.ruta[PREFIJO_SKILLS.len()..];
    resto.split('/').next().unwrap_or(resto)
}

/// La skill `nombre` en el formato `f`, a partir de su `fuente`.
pub fn generar(f: Formato, nombre: &str, fuente: &str) -> Generado {
    let linea_nombre = format!("name: {nombre}\n");
    let titulo = format!("\n# /{nombre}\n");
    match f {
        Formato::Claude => Generado { ruta: format!("{nombre}/SKILL.md"), contenido: fuente.to_string() },
        Formato::Prefijada => Generado {
            ruta: format!("ea-{nombre}/SKILL.md"),
            contenido: fuente.replacen(&linea_nombre, &format!("name: ea-{nombre}\n"), 1).replacen(&titulo, &format!("\n# ea-{nombre}\n"), 1),
        },
        Formato::Comando => Generado {
            ruta: format!("ea-{nombre}.md"),
            contenido: fuente.replacen(&linea_nombre, "", 1).replacen(&format!("{titulo}\n"), "\n", 1) + "\nPetición del usuario: $ARGUMENTS\n",
        },
    }
}

/// Todas las skills del planificador en el formato `f`.
pub fn skills(f: Formato) -> Vec<Generado> {
    recursos::bajo(PREFIJO_SKILLS).map(|r| generar(f, nombre(r), r.contenido)).collect()
}

fn planificador() -> &'static str {
    recursos::recurso(FUENTE_PLANIFICADOR).map_or("", |r| r.contenido)
}

/// El fragmento del `AGENTS.md` de `agente` (`Codex`, `OpenCode`).
pub fn fragmento(agente: &str) -> String {
    format!("<!-- Fragmento para el AGENTS.md de {agente}. Instalación: docs/manual.md del repo, «Adaptadores del planificador». -->\n{}", planificador())
}

/// La regla `model_decision` de Antigravity (`ea-planificador.md`).
pub fn regla_antigravity() -> Generado {
    Generado {
        ruta: "ea-planificador.md".into(),
        contenido: format!(
            "---\ntrigger: model_decision\ndescription: \"Úsala cuando el usuario hable de misiones, obras, rutinas o de revisar trabajos del ejecutor del ecosistema asistente (ea, gachapon).\"\n---\n\n{}",
            planificador()
        ),
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// Una fuente mínima con todo lo que cambia entre agentes.
    const FUENTE: &str = "---\nname: x\ndescription: \"Hace x.\"\n---\n\n# /x\n\nlee `{{flujo}}/x.md`.\n";

    #[test]
    fn cada_formato_con_una_fuente_minima() {
        assert_eq!(generar(Formato::Claude, "x", FUENTE), Generado { ruta: "x/SKILL.md".into(), contenido: FUENTE.into() });
        assert_eq!(
            generar(Formato::Prefijada, "x", FUENTE),
            Generado { ruta: "ea-x/SKILL.md".into(), contenido: "---\nname: ea-x\ndescription: \"Hace x.\"\n---\n\n# ea-x\n\nlee `{{flujo}}/x.md`.\n".into() }
        );
        assert_eq!(
            generar(Formato::Comando, "x", FUENTE),
            Generado {
                ruta: "ea-x.md".into(),
                contenido: "---\ndescription: \"Hace x.\"\n---\n\nlee `{{flujo}}/x.md`.\n\nPetición del usuario: $ARGUMENTS\n".into()
            }
        );
        assert_eq!(generar(Formato::Comando, "x", FUENTE).unidad(), "ea-x.md");
        assert_eq!(generar(Formato::Prefijada, "x", FUENTE).unidad(), "ea-x");
    }

    /// El frontmatter (sin las rayas) y el resto.
    fn partir(t: &str) -> (&str, &str) {
        let resto = t.strip_prefix("---\n").expect("empieza con frontmatter");
        resto.split_once("\n---\n").expect("frontmatter cerrado")
    }

    #[test]
    fn cada_skill_cambia_solo_nombre_titulo_y_peticion() {
        // Las cinco fuentes, cada una con las líneas que el generador cambia;
        // si una no las tuviera, su variante saldría igual que la fuente.
        let fuentes: Vec<&Recurso> = recursos::bajo(PREFIJO_SKILLS).collect();
        assert_eq!(fuentes.iter().map(|r| nombre(r)).collect::<Vec<_>>(), ["equipo", "mision", "obra", "revisar", "rutina"]);
        for r in fuentes {
            let x = nombre(r);
            let (cabecera, cuerpo) = partir(r.contenido);
            assert!(cabecera.starts_with(&format!("name: {x}\n")), "{x}");
            assert!(cuerpo.starts_with(&format!("\n# /{x}\n\n")), "{x}");
            let prefijada = generar(Formato::Prefijada, x, r.contenido).contenido;
            let (c, b) = partir(&prefijada);
            assert_eq!(c, cabecera.replacen(&format!("name: {x}"), &format!("name: ea-{x}"), 1));
            assert_eq!(b, cuerpo.replacen(&format!("# /{x}"), &format!("# ea-{x}"), 1));
            let comando = generar(Formato::Comando, x, r.contenido).contenido;
            let (c, b) = partir(&comando);
            assert_eq!(c, &cabecera[format!("name: {x}\n").len()..]);
            assert_eq!(b, format!("{}\nPetición del usuario: $ARGUMENTS\n", &cuerpo[format!("\n# /{x}\n").len()..]));
            // Lo que todas comparten: la descripción entre comillas y la guía de su flujo.
            for t in [r.contenido, &prefijada, &comando] {
                let (c, b) = partir(t);
                let desc = c.lines().find_map(|l| l.strip_prefix("description: ")).expect("description");
                assert!(desc.starts_with('"') && desc.ends_with('"'), "{x}: description sin comillas");
                assert!(b.contains(&format!("{}{x}.md", recursos::MARCA_FLUJO)), "{x}: no remite a su guía");
            }
        }
    }

    #[test]
    fn las_reglas_del_planificador_comparten_el_cuerpo() {
        let cuerpo = planificador();
        assert!(cuerpo.starts_with("## Ecosistema asistente: tú eres el planificador\n") && cuerpo.contains(recursos::MARCA_FLUJO));
        for agente in ["Codex", "OpenCode"] {
            let f = fragmento(agente);
            let (comentario, resto) = f.split_once('\n').unwrap();
            assert!(comentario.starts_with(&format!("<!-- Fragmento para el AGENTS.md de {agente}.")) && comentario.ends_with("-->"));
            assert_eq!(resto, cuerpo);
        }
        let regla = regla_antigravity();
        assert_eq!(regla.ruta, "ea-planificador.md");
        let (cabecera, resto) = partir(&regla.contenido);
        assert!(cabecera.starts_with("trigger: model_decision\ndescription: \""));
        assert_eq!(resto, format!("\n{cuerpo}"));
    }
}
