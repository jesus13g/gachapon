//! Biblioteca de herramientas (spec 018): inventario de solo lectura de las
//! skills, plugins, subagentes, servidores MCP, comandos y extensiones de
//! cada agente del catálogo y del ecosistema.
//!
//! Depende solo de `ea-ajustes`. Nada de lo que sale de aquí lleva secretos
//! (ver [`redactar`]). Lo que no existe en disco es vacío, nunca error; lo que
//! no se puede interpretar queda en [`Inventario::avisos`].

pub mod agentes;
pub mod cambio;
mod error;
mod modelo;
pub mod raices;
pub mod redactar;
pub mod uso;
pub mod sesion;

use std::collections::HashSet;

use ea_ajustes::{Ajustes, IDS_AGENTES};
use serde::Serialize;

pub use cambio::{aplicar, aplicar_con, desactivadas, plan_cambio, plan_cambio_con, planear, Cambio, Contexto, Hecho, Objetivo, Ordenes, OrdenesDoble, OrdenesReales};
pub use error::ErrorBiblioteca;
pub use modelo::{Activado, Ambito, DefMcp, Elemento, Id, Mecanismo, Tipo, AGENTE_ECOSISTEMA};
pub use raices::Raices;
pub use sesion::{limpiar_sesiones, materializar, plan_sesion, EnlaceEfimero, FicheroEfimero, FuenteMcp, PlanSesion, AGENTES_CON_SESION};

/// Orden de los agentes en el inventario.
fn orden_agente(a: &str) -> usize {
    IDS_AGENTES.iter().position(|x| *x == a).unwrap_or(IDS_AGENTES.len())
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Inventario {
    pub elementos: Vec<Elemento>,
    /// Lo que no se pudo leer o se omitió (para mostrarlo, nunca un error).
    pub avisos: Vec<String>,
}

/// Filtros de `ea biblioteca listar`; los campos en `None` no filtran.
#[derive(Debug, Clone, Default)]
pub struct Filtro {
    pub agente: Option<String>,
    pub tipo: Option<Tipo>,
    pub categoria: Option<String>,
    pub activado: Option<Activado>,
}

impl Filtro {
    pub fn admite(&self, e: &Elemento) -> bool {
        self.agente.as_deref().is_none_or(|a| e.agente == a)
            && self.tipo.is_none_or(|t| e.tipo == t)
            && self.categoria.as_deref().is_none_or(|c| e.categoria == c)
            && self.activado.is_none_or(|a| e.activado == a)
    }
}

impl Inventario {
    pub fn filtrar(&self, f: &Filtro) -> Vec<&Elemento> {
        self.elementos.iter().filter(|e| f.admite(e)).collect()
    }

    /// El elemento con ese id exacto.
    pub fn buscar(&self, id: &str) -> Option<&Elemento> {
        self.elementos.iter().find(|e| e.id == id)
    }

    /// Las categorías presentes, ordenadas y sin repetir.
    pub fn categorias(&self) -> Vec<String> {
        let mut c: Vec<String> = self.elementos.iter().map(|e| e.categoria.clone()).collect();
        c.sort();
        c.dedup();
        c
    }
}

pub struct Biblioteca;

impl Biblioteca {
    /// Lee todo lo que hay y aplica las categorías: por defecto el origen
    /// (`ecosistema`, `usuario`, `proyecto`, `plugin:<id>`, `integrada`); un
    /// servidor MCP que apunta al `ea-mcp` local (`red.puerto_mcp`) es
    /// `ecosistema`; y manda `biblioteca.categorias` de los ajustes.
    pub fn cargar(r: &Raices, a: &Ajustes) -> Inventario {
        let mut avisos = Vec::new();
        let mut todos: Vec<Elemento> = Vec::new();
        todos.extend(agentes::claude::leer(r, &mut avisos));
        todos.extend(agentes::qwen::leer(r, a, &mut avisos));
        todos.extend(agentes::codex::leer(r, &mut avisos));
        todos.extend(agentes::opencode::leer(r, &mut avisos));
        todos.extend(agentes::antigravity::leer(r, &mut avisos));
        todos.extend(agentes::ecosistema::leer(r, a, &mut avisos));

        // Lo que la biblioteca movió al almacén ya no está en su sitio: se
        // añade con `activado: no`, salvo que haya otro con su id a la vista.
        let a_la_vista: HashSet<String> = todos.iter().map(|e| e.id.clone()).collect();
        todos.extend(desactivadas(r).into_iter().filter(|e| !a_la_vista.contains(&e.id)).map(|mut e| {
            e.nota = Some("desactivada por la biblioteca: está en su almacén, no en su sitio".into());
            e
        }));

        // Un id es único: el primero manda y el repetido deja un aviso.
        let mut vistos = HashSet::new();
        let mut elementos = Vec::with_capacity(todos.len());
        for mut e in todos {
            if !vistos.insert(e.id.clone()) {
                avisos.push(format!("{}: id repetido ({}); se conserva el primero", e.id, e.ruta.display()));
                continue;
            }
            if e.tipo == Tipo::Mcp && apunta_a_ea_mcp(&e, a.red.puerto_mcp) {
                e.categoria = "ecosistema".into();
            }
            if let Some(c) = a.biblioteca.categorias.get(&e.id) {
                e.categoria = c.clone();
            }
            elementos.push(e);
        }
        elementos.sort_by(|x, y| {
            (orden_agente(&x.agente), x.tipo, x.ambito.texto(), &x.nombre).cmp(&(orden_agente(&y.agente), y.tipo, y.ambito.texto(), &y.nombre))
        });
        Inventario { elementos, avisos }
    }
}

fn apunta_a_ea_mcp(e: &Elemento, puerto: u16) -> bool {
    let Some(url) = e.def_mcp.as_ref().and_then(|d| d.url.as_deref()) else { return false };
    [format!("http://127.0.0.1:{puerto}"), format!("http://localhost:{puerto}"), format!("http://[::1]:{puerto}")].iter().any(|u| url == u)
}
