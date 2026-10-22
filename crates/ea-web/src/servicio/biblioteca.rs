//! La biblioteca de herramientas (spec 018, F7): el inventario de lo que
//! cada agente tiene (skills, plugins, subagentes, MCP, comandos y
//! extensiones), su uso, y los gestos del usuario sobre ello: activar,
//! desactivar y abrir una sesión con herramientas acotadas.
//!
//! Lo que sale de aquí es lo que ya da `ea-biblioteca` (redactado: nunca un
//! secreto) y la simulación de las herramientas del catálogo. Las tres
//! herramientas de cambio pasan por la misma capa de acciones que el resto
//! (simular → ejecutar con revalidación, diario); lo único propio de la web es
//! de dónde salen las raíces del inventario y la terminal (`WebConfig`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use ea_acciones::{Canal, Consulta, Sistema, SistemaReal};
use ea_biblioteca::uso::{calcular_uso, FuentesUso, InformeUso, Uso};
use ea_biblioteca::{
    Activado, Ambito, Biblioteca, Cambio, Contexto, Elemento, Filtro, Hecho, Inventario, Mecanismo, OrdenesReales,
    PlanSesion, Raices, Tipo, AGENTES_CON_SESION, AGENTE_ECOSISTEMA,
};
use ea_boveda::{ErrorBoveda, PlanPromocion};
use ea_store::Store;
use serde::Serialize;
use serde_json::Value;

use super::terminal::Terminal;
use super::{sistema, ErrorWeb, Resultado, ACTOR};
use crate::vistas::{ResultadoAccion, Simulacion};
use crate::AppState;

/// Las únicas herramientas del catálogo que se simulan y ejecutan desde esta sección.
pub const HERRAMIENTAS: [&str; 3] = ["ea_activar_herramienta", "ea_desactivar_herramienta", "ea_lanzar_sesion"];

/// Cuánto vale un informe de uso ya calculado (la página sondea cada pocos segundos).
const VIGENCIA_USO: Duration = Duration::from_secs(20);

// ------------------------------------------------------------------- vistas

/// Si el elemento se puede activar o desactivar desde aquí, y si no, por qué.
#[derive(Serialize)]
pub struct CambioPosible {
    pub posible: bool,
    pub motivo: Option<String>,
}

/// Un elemento del inventario (ya redactado) con lo que la página necesita
/// saber para ofrecer sus botones.
#[derive(Serialize)]
pub struct ElementoVista {
    #[serde(flatten)]
    pub elemento: Elemento,
    pub cambio: CambioPosible,
    /// Los agentes con sesión en cuyas sesiones tiene sentido este elemento.
    pub sesion: Vec<&'static str>,
}

#[derive(Serialize)]
pub struct BibliotecaVista {
    pub elementos: Vec<ElementoVista>,
    pub avisos: Vec<String>,
    /// Las categorías presentes en TODO el inventario (no solo en lo filtrado).
    pub categorias: Vec<String>,
    /// Cuántos elementos hay sin filtrar.
    pub total: usize,
    /// Los agentes que admiten sesión (para «▶ sesión con…»).
    pub agentes_sesion: Vec<&'static str>,
}

#[derive(Serialize)]
pub struct FichaVista {
    #[serde(flatten)]
    pub elemento: ElementoVista,
    /// `None`: sin ningún uso registrado.
    pub uso: Option<Uso>,
}

/// Lo que pide `GET /api/biblioteca` (cada campo es opcional).
#[derive(Default)]
pub struct Peticion {
    pub agente: Option<String>,
    pub tipo: Option<String>,
    pub categoria: Option<String>,
    pub activado: Option<String>,
}

fn invalido(m: impl Into<String>) -> ErrorWeb {
    ErrorWeb::Invalido(m.into())
}

fn filtro(c: &Peticion) -> Resultado<Filtro> {
    Ok(Filtro {
        agente: c.agente.clone().filter(|a| !a.is_empty()),
        tipo: match c.tipo.as_deref().filter(|t| !t.is_empty()) {
            Some(t) => Some(Tipo::desde(t).ok_or_else(|| invalido(format!("tipo desconocido: «{t}»")))?),
            None => None,
        },
        categoria: c.categoria.clone().filter(|a| !a.is_empty()),
        activado: match c.activado.as_deref().filter(|t| !t.is_empty()) {
            Some(t) => Some(Activado::desde(t).ok_or_else(|| invalido("«activado» es si, no o desconocido"))?),
            None => None,
        },
    })
}

// ------------------------------------------------------------------ entorno

fn raices(s: &AppState) -> Raices {
    s.config.biblioteca_raices.clone().unwrap_or_else(Raices::desde_entorno)
}

fn contexto(s: &AppState) -> Contexto {
    // El canal de ajustes es Web: las skills del ejecutor solo se cambian desde una terminal.
    Contexto::nuevo(raices(s), s.config.ajustes.clone(), ea_ajustes::Canal::Web)
}

/// Lee el disco (muchos ficheros): fuera del hilo asíncrono. El inventario ya
/// trae lo que la biblioteca desactivó moviéndolo, para poder reactivarlo.
async fn inventario(s: &AppState) -> Resultado<Inventario> {
    let r = raices(s);
    let a = ea_ajustes::cargar_de(&s.config.ajustes).ajustes;
    tokio::task::spawn_blocking(move || Biblioteca::cargar(&r, &a))
    .await
    .map_err(|e| ErrorWeb::Interno(format!("la lectura de la biblioteca falló: {e}")))
}

/// ¿Tiene sentido el elemento en una sesión de `agente`? (la misma regla que
/// `ea-biblioteca::sesion`, para no ofrecer lo que el plan va a rechazar).
fn compatible(agente: &str, e: &Elemento) -> bool {
    let propio = e.agente == agente || e.agente == AGENTE_ECOSISTEMA;
    if !propio && e.tipo != Tipo::Mcp {
        return false;
    }
    let tipos: &[Tipo] = match agente {
        "claude-code" => &[Tipo::Mcp, Tipo::Plugin, Tipo::Skill, Tipo::Subagente, Tipo::Comando],
        "qwen-code" => &[Tipo::Mcp, Tipo::Extension],
        _ => &[Tipo::Mcp],
    };
    tipos.contains(&e.tipo) && (e.tipo == Tipo::Mcp || !matches!(e.ambito, Ambito::Plugin(_)))
}

fn cambio_posible(e: &Elemento) -> CambioPosible {
    let no = |m: &str| CambioPosible { posible: false, motivo: Some(m.to_string()) };
    match e.mecanismo {
        Mecanismo::SoloSesion => no("solo se puede acotar en una sesión (su configuración no se edita): usa «▶ sesión con…»"),
        Mecanismo::No => no(e.nota.as_deref().unwrap_or("este elemento no se puede activar ni desactivar")),
        Mecanismo::Ajustes => no(
            "solo desde una terminal real: lo que cambia es lo que puede hacer el ejecutor (agente.skills.habilitadas)",
        ),
        _ if e.activado == Activado::Desconocido => no("no se sabe si está activado, así que no hay qué cambiar"),
        _ => CambioPosible { posible: true, motivo: None },
    }
}

fn vista(e: &Elemento) -> ElementoVista {
    ElementoVista {
        cambio: cambio_posible(e),
        sesion: AGENTES_CON_SESION.iter().copied().filter(|a| compatible(a, e)).collect(),
        elemento: e.clone(),
    }
}

// ------------------------------------------------------------------ lectura

pub async fn listar(s: &AppState, peticion: &Peticion) -> Resultado<BibliotecaVista> {
    let f = filtro(peticion)?;
    let inv = inventario(s).await?;
    Ok(BibliotecaVista {
        total: inv.elementos.len(),
        elementos: inv.filtrar(&f).into_iter().map(vista).collect(),
        categorias: inv.categorias(),
        avisos: inv.avisos.clone(),
        agentes_sesion: AGENTES_CON_SESION.to_vec(),
    })
}

/// El informe de uso: caro la primera vez (transcripciones), con la caché
/// incremental en disco de `calcular_uso`; además se reutiliza unos segundos.
/// Se calcula fuera del hilo asíncrono y de uno en uno.
pub async fn uso(s: &AppState) -> Resultado<Arc<InformeUso>> {
    let mut guarda = s.biblioteca_uso.lock().await;
    if let Some((cuando, informe)) = guarda.as_ref()
        && cuando.elapsed() < VIGENCIA_USO
    {
        return Ok(informe.clone());
    }
    let inv = inventario(s).await?;
    let r = raices(s);
    let aislado = s.config.biblioteca_raices.is_some();
    let informe = tokio::task::spawn_blocking(move || {
        let mut fuentes = FuentesUso::desde_raices(&r);
        if aislado {
            // Con raíces de prueba, ni el diario real ni otro fuera de ellas.
            fuentes.diario = r.home.join(".local/share/ecosistema-asistente/ecosistema.db");
        }
        calcular_uso(&r, &inv, &fuentes)
    })
    .await
    .map_err(|e| ErrorWeb::Interno(format!("el cálculo del uso falló: {e}")))?;
    let informe = Arc::new(informe);
    *guarda = Some((Instant::now(), informe.clone()));
    Ok(informe)
}

pub async fn ficha(s: &AppState, id: &str) -> Resultado<FichaVista> {
    let inv = inventario(s).await?;
    let e = inv.buscar(id).ok_or_else(|| ErrorWeb::NoEncontrado(format!("no existe el elemento «{id}» en la biblioteca")))?;
    let elemento = vista(e);
    let uso = uso(s).await?.buscar(id).cloned();
    Ok(FichaVista { elemento, uso })
}

// ------------------------------------------------- el sistema de la sección

/// El sistema real, con las raíces, los ajustes y la terminal de `WebConfig`
/// para lo de la biblioteca; lo demás, lo de siempre.
struct SistemaBiblioteca {
    base: SistemaReal,
    ctx: Contexto,
    terminal: Terminal,
}

/// La terminal de `WebConfig` como la de `ea-acciones`.
struct TerminalWeb<'a>(&'a Terminal);

impl ea_acciones::Terminal for TerminalWeb<'_> {
    fn abrir(&self, argv: &[String]) -> Result<(), String> {
        match self.0 {
            Terminal::Sistema { .. } => ea_acciones::TerminalSistema.abrir(argv),
            Terminal::Simulada { lanzadas, .. } => {
                lanzadas.lock().map_err(|_| "registro envenenado".to_string())?.push(argv.to_vec());
                Ok(())
            }
            Terminal::Ninguna => Err("no hay terminal configurada".into()),
        }
    }

    /// Con directorio y entorno: la del sistema los traduce a su lanzador; la
    /// simulada apunta el argv de Linux (`env -C …`).
    fn abrir_orden(&self, orden: &ea_acciones::OrdenTerminal) -> Result<(), String> {
        match self.0 {
            Terminal::Sistema { .. } => ea_acciones::TerminalSistema.abrir_orden(orden),
            _ => self.abrir(&ea_acciones::terminal::argv_linux(orden)),
        }
    }
}

impl Consulta for SistemaBiblioteca {
    async fn servicio_estado(&self, nombre: &str) -> Result<ea_acciones::EstadoServicio, String> {
        self.base.servicio_estado(nombre).await
    }
    fn rutina_contenido(&self, slug: &str) -> Result<Option<String>, String> {
        self.base.rutina_contenido(slug)
    }
    fn rutina_retirada(&self, slug: &str) -> Result<Option<String>, String> {
        self.base.rutina_retirada(slug)
    }
    fn plan_promocion(&self, ruta: &str, destino: Option<&str>) -> Result<PlanPromocion, ErrorBoveda> {
        self.base.plan_promocion(ruta, destino)
    }
    fn sesion_plan(&self, agente: &str, ids: &[String], directorio: Option<&str>) -> Result<PlanSesion, String> {
        let a = ea_ajustes::cargar_de(&self.ctx.fuentes).ajustes;
        let inv = Biblioteca::cargar(&self.ctx.raices, &a);
        ea_acciones::sesion::plan_en(agente, ids, directorio, &inv, &self.ctx.raices)
    }
    async fn timer_estado(&self, slug: &str) -> Result<ea_acciones::EstadoTimer, String> {
        self.base.timer_estado(slug).await
    }
    async fn calendario_validar(&self, c: &str) -> Result<Result<ea_acciones::Calendario, String>, String> {
        self.base.calendario_validar(c).await
    }
    fn biblioteca_cambio(&self, elemento: &str, activar: bool) -> Result<Cambio, String> {
        let c = ea_biblioteca::planear(elemento, activar, &self.ctx).map_err(|e| e.to_string())?;
        ea_biblioteca::cambio::comprobar_disponible(&c, &OrdenesReales).map_err(|e| e.to_string())?;
        Ok(c)
    }
}

impl Sistema for SistemaBiblioteca {
    async fn servicio_reiniciar(&self, nombre: &str) -> Result<(), String> {
        self.base.servicio_reiniciar(nombre).await
    }
    fn rutina_eliminar(&self, slug: &str) -> Result<(), String> {
        self.base.rutina_eliminar(slug)
    }
    fn rutina_retirar(&self, slug: &str) -> Result<(), String> {
        self.base.rutina_retirar(slug)
    }
    fn rutina_restaurar(&self, slug: &str) -> Result<(), String> {
        self.base.rutina_restaurar(slug)
    }
    fn sesion_lanzar(&self, plan: &PlanSesion) -> Result<(), String> {
        // El proyecto de los ficheros efímeros es el del plan.
        let raices = Raices { proyecto: plan.cwd.clone(), ..self.ctx.raices.clone() };
        ea_acciones::sesion::lanzar_con(plan, raices, &TerminalWeb(&self.terminal))
    }
    fn rutina_registrar(&self, store: &Store, texto: &str, actor: &str) -> Result<(String, u32), String> {
        self.base.rutina_registrar(store, texto, actor)
    }
    fn nota_promover(
        &self,
        store: &mut Store,
        ruta: &str,
        destino: Option<&str>,
        huella: &str,
        actor: &str,
        canal: &str,
    ) -> Result<PlanPromocion, ErrorBoveda> {
        self.base.nota_promover(store, ruta, destino, huella, actor, canal)
    }
    async fn timer_programar(&self, slug: &str, calendario: &str) -> Result<(), String> {
        self.base.timer_programar(slug, calendario).await
    }
    async fn timer_desprogramar(&self, slug: &str) -> Result<bool, String> {
        self.base.timer_desprogramar(slug).await
    }
    async fn biblioteca_aplicar(&self, cambio: &Cambio, canal: ea_acciones::Canal) -> Result<Hecho, String> {
        let (c, mut ctx) = (cambio.clone(), self.ctx.clone());
        ctx.canal = ea_acciones::canal_de_ajustes(canal);
        // Puede lanzar una orden de hasta 30 s: en un hilo aparte.
        tokio::task::spawn_blocking(move || ea_biblioteca::aplicar_con(&c, &ctx, &OrdenesReales))
            .await
            .map_err(|e| format!("la tarea del cambio falló: {e}"))?
            .map_err(|e| e.to_string())
    }
}

fn sistema_biblioteca(s: &AppState) -> SistemaBiblioteca {
    SistemaBiblioteca { base: sistema(s), ctx: contexto(s), terminal: s.config.terminal.clone() }
}

fn comprobar(herramienta: &str, argumentos: &Value) -> Resultado<()> {
    if !HERRAMIENTAS.contains(&herramienta) {
        return Err(invalido(format!("desde la biblioteca solo se simula {}", HERRAMIENTAS.join(", "))));
    }
    // El plan de sesión usa el proyecto de ea-web: nada de directorios a pedido.
    if argumentos.get("directorio").is_some() {
        return Err(invalido("la sesión se abre en el proyecto de ea-web: no admite «directorio»"));
    }
    Ok(())
}

/// Describe lo que haría la herramienta ahora, sin hacer nada (la simulación
/// de una sesión lleva el comando sin secretos).
pub async fn simular(s: &AppState, herramienta: &str, argumentos: &Value) -> Resultado<Simulacion> {
    comprobar(herramienta, argumentos)?;
    let sis = sistema_biblioteca(s);
    let hechos = ea_acciones::observar(&sis, herramienta, argumentos).await?;
    let store = s.store.lock().await;
    let sim = ea_acciones::simular_con(&store, &sis, &hechos, herramienta, argumentos)?;
    Ok(Simulacion {
        herramienta: sim.herramienta,
        riesgo: sim.riesgo.as_str(),
        argumentos: sim.argumentos,
        texto: sim.texto,
        precondicion: sim.precondicion,
        confirmable: Canal::Web.puede_confirmar(sim.riesgo).is_ok(),
    })
}

/// Ejecuta lo simulado: el gesto del usuario es la confirmación; la
/// revalidación y el diario son los de siempre. Tras un cambio, el informe de
/// uso en caché se descarta (el inventario ya no es el mismo).
pub async fn ejecutar(
    s: &AppState,
    herramienta: &str,
    argumentos: &Value,
    precondicion: &Value,
) -> Resultado<ResultadoAccion> {
    comprobar(herramienta, argumentos)?;
    let sis = sistema_biblioteca(s);
    let mut store = s.acciones.lock().await;
    let r = ea_acciones::ejecutar(&mut store, &sis, ACTOR, Canal::Web, herramienta, argumentos, precondicion, None).await?;
    *s.biblioteca_uso.lock().await = None;
    Ok(ResultadoAccion { texto: r.texto, trabajo: r.trabajo, datos: r.datos, evento: r.evento })
}
