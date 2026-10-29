//! Lo que pone el propio ecosistema, con agente `ecosistema`:
//!
//! - el servidor `ea-mcp` (`http://127.0.0.1:<red.puerto_mcp>/mcp`) como MCP;
//! - las skills del repo: `claude/skills/<n>/SKILL.md` y `qwen/skills/*.md`.
//!
//! Son los ORÍGENES: sus instalaciones en cada agente (enlaces en
//! `~/.claude/skills`, la copia de las skills del ejecutor) las listan los
//! lectores de cada agente. Los adaptadores de `agentes/` no se listan. No se
//! sabe si el servidor está en marcha (no se abre red): `desconocido`.

use ea_ajustes::Ajustes;

use super::comun::{crear, frontmatter, leer_texto, recortar, skills_en};
use crate::modelo::{AGENTE_ECOSISTEMA as AGENTE, Activado, Ambito, DefMcp, Elemento, Mecanismo, Tipo};
use crate::raices::Raices;

/// El ecosistema no tiene versión de formato de un agente externo.
pub const VERSION: &str = "";
/// Nombre con que los agentes registran el servidor.
pub const NOMBRE_MCP: &str = "ea-ecosistema";

pub fn leer(r: &Raices, a: &Ajustes, avisos: &mut Vec<String>) -> Vec<Elemento> {
    let mut sal = Vec::new();
    let sitio = r.repo_efectivo().unwrap_or_default();
    if let Some(mut e) = crear(avisos, AGENTE, Tipo::Mcp, Ambito::Ecosistema, NOMBRE_MCP, sitio.clone(), VERSION) {
        e.descripcion = "Servidor MCP del ecosistema (solo consulta y propuestas)".into();
        e.def_mcp = Some(DefMcp { transporte: "http".into(), url: Some(format!("http://127.0.0.1:{}", a.red.puerto_mcp)), ..DefMcp::default() });
        sal.push(e);
    }
    if let Some(repo) = r.repo_efectivo() {
        for mut e in skills_en(&repo.join("claude/skills"), AGENTE, Ambito::Ecosistema, Some(&repo), Mecanismo::No, VERSION, avisos) {
            e.activado = Activado::Desconocido;
            sal.push(e);
        }
        let mut ficheros: Vec<_> = std::fs::read_dir(repo.join("qwen/skills")).into_iter().flatten().flatten().map(|e| e.path()).collect();
        ficheros.sort();
        for ruta in ficheros {
            let Some(nombre) = ruta.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".md")).map(str::to_string) else { continue };
            if let Some(mut e) = crear(avisos, AGENTE, Tipo::Skill, Ambito::Ecosistema, &nombre, ruta.clone(), VERSION) {
                let fm = leer_texto(&ruta).map(|t| frontmatter(&t)).unwrap_or_default();
                e.descripcion = recortar(fm.get("descripcion").or_else(|| fm.get("description")).map_or("", String::as_str));
                sal.push(e);
            }
        }
    }
    sal
}
