use clap::builder::{PossibleValue, PossibleValuesParser};
use clap::{Parser, Subcommand, ValueEnum};
use ea_ajustes::Rol;
use ea_bus::Modo;
use ea_store::EstadoPropuesta;

#[derive(Parser)]
#[command(name = "ea", about = "Interfaz de terminal del ecosistema de asistente", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Resumen no interactivo del estado del sistema (apto para login banner)
    Status,
    /// Diario de eventos
    Diario {
        #[command(subcommand)]
        accion: DiarioAccion,
    },
    /// Rutinas versionadas (registrar, listar, ver, lanzar, retirar, restaurar)
    #[command(alias = "rutinas")]
    Rutina {
        #[command(subcommand)]
        accion: RutinaAccion,
    },
    /// Propuestas pendientes de rutinas desatendidas
    Propuestas {
        #[command(subcommand)]
        accion: PropuestasAccion,
    },
    /// Herramientas del catálogo: listar, simular y ejecutar (con confirmación)
    Acciones {
        #[command(subcommand)]
        accion: AccionesAccion,
    },
    /// Bóveda de notas: ver, buscar, leer y promover a la región compartida
    Boveda {
        #[command(subcommand)]
        accion: BovedaAccion,
    },
    /// Trabajos del bus del orquestador (crear, ver, reformular, cerrar)
    Trabajo {
        #[command(subcommand)]
        accion: TrabajoAccion,
    },
    /// Entradas de una cola del bus (por defecto, las dos)
    Cola {
        /// El rol de la cola (también su nombre en disco: claude-code o modelo-local)
        #[arg(value_enum)]
        agente: Option<ArgRol>,
        /// Salida en JSON
        #[arg(long)]
        json: bool,
    },
    /// Semáforos de worktree ocupados ahora mismo
    Semaforos,
    /// Skills propias del agente local (Qwen): catálogo y contenido
    QwenSkills {
        #[command(subcommand)]
        accion: QwenSkillsAccion,
    },
    /// Agentes del catálogo: qué rol ocupa cada uno, su binario y su modelo
    Agentes {
        #[command(subcommand)]
        accion: Option<AgentesAccion>,
        /// Salida en JSON
        #[arg(long)]
        json: bool,
    },
    /// Ajustes del ecosistema (ajustes.toml): ver, obtener y establecer
    Ajustes {
        #[command(subcommand)]
        accion: AjustesAccion,
    },
    /// Biblioteca de herramientas de los agentes: skills, plugins, subagentes, MCP, comandos y extensiones
    Biblioteca {
        #[command(subcommand)]
        accion: BibliotecaAccion,
    },
    /// Equipos de agentes: despertar, listar, ver y dormir (despertar y dormir exigen una terminal interactiva)
    Equipo {
        #[command(subcommand)]
        accion: EquipoAccion,
    },
    /// Worktrees propios de los trabajos (spec 019, anexo A): listarlos y quitar los de trabajos terminados
    Worktree {
        #[command(subcommand)]
        accion: WorktreeAccion,
    },
    /// Servicios residentes (ea-mcp, ea-orchestrator, ea-web) en el gestor del sistema: ver, instalar y desinstalar (instalar y desinstalar exigen una terminal interactiva real)
    Servicios {
        #[command(subcommand)]
        accion: ServiciosAccion,
    },
    /// Instala todo en un paso (binarios, PATH, skills, adaptadores de los agentes, MCP, hook, servicios y app de escritorio); exige una terminal interactiva real y una confirmación
    Instalar {
        /// Solo en Windows: `wsl` (WSL2, con sandbox) o `nativa` (sin sandbox); si falta, se pregunta
        #[arg(long)]
        via: Option<String>,
        /// Distribución de WSL (vía `wsl`)
        #[arg(long, default_value = "Ubuntu")]
        distro: String,
        /// No instala la app de escritorio (automático dentro de WSL)
        #[arg(long)]
        sin_escritorio: bool,
        /// No mira ni ofrece las dependencias externas
        #[arg(long)]
        sin_dependencias: bool,
        /// Sigue una instalación de WSL tras el reinicio (lo pone la entrada RunOnce)
        #[arg(long)]
        continuar: bool,
    },
    /// Deshace `ea instalar` (servicios, app, adaptadores, hook, MCP, PATH y binarios copiados) sin borrar datos; exige una terminal interactiva real
    Desinstalar {
        /// Al terminar espera a Intro (para la terminal que abre ea-web)
        #[arg(long)]
        esperar: bool,
    },
    /// Hooks de Claude Code (leen el JSON del hook por stdin; nunca fallan)
    Hook {
        #[command(subcommand)]
        accion: HookAccion,
    },
    /// El cliente MCP del planificador (ea-mcp en 127.0.0.1)
    Mcp {
        #[command(subcommand)]
        accion: McpAccion,
    },
    /// Interfaz interactiva
    Tui,
}

#[derive(Subcommand)]
pub enum McpAccion {
    /// Escribe en JSON la cabecera con la credencial del planificador (el headersHelper de Claude Code que pone `ea instalar`)
    Cabeceras,
}

#[derive(Subcommand)]
pub enum HookAccion {
    /// SubagentStart / SubagentStop: apunta los subagentes en marcha para ea-web
    Subagentes {
        /// `start` o `stop`
        accion: String,
    },
}

#[derive(Subcommand)]
pub enum ServiciosAccion {
    /// Qué instalaría (unidades, rutas reales de los binarios y órdenes) y cómo está cada servicio; no cambia nada
    Ver,
    /// Escribe las unidades con la ruta real de los binarios y arranca los servicios (systemd --user, launchd o el Programador de tareas)
    Instalar {
        /// Al terminar espera a Intro (para la terminal que abre ea-web)
        #[arg(long)]
        esperar: bool,
    },
    /// Para y quita los servicios instalados con `ea servicios instalar`
    Desinstalar {
        /// Al terminar espera a Intro (para la terminal que abre ea-web)
        #[arg(long)]
        esperar: bool,
    },
}

#[derive(Subcommand)]
pub enum WorktreeAccion {
    /// Los worktrees propios vivos: trabajo, rama ea/<id>, estado del trabajo y si están limpios
    Listar {
        #[arg(long)]
        json: bool,
    },
    /// Quita el worktree de un trabajo cerrado o cancelado (`git worktree remove`, sin --force; la rama se conserva); simula y pide confirmación en una terminal
    Limpiar { id: String },
}

#[derive(Subcommand)]
pub enum EquipoAccion {
    /// Despierta un equipo: ejecutores para el orquestador y sesiones para planificadores y revisores; exige una terminal interactiva real
    Despertar {
        /// Todos los agentes del catálogo disponibles en el PATH, repartidos según lo que admiten
        #[arg(long, conflicts_with = "miembro", required_unless_present = "miembro")]
        todos: bool,
        /// Un miembro `agente:rol` (rol: planificador, ejecutor o revisor); repetible
        #[arg(long)]
        miembro: Vec<String>,
        #[arg(long)]
        titulo: String,
        #[arg(long)]
        objetivo: String,
        /// Repo sobre el que trabaja el equipo
        #[arg(long)]
        worktree: String,
        /// No abre terminales para planificadores y revisores: enseña el comando de cada sesión
        #[arg(long)]
        sin_terminal: bool,
    },
    /// Los equipos con sus miembros
    Listar {
        #[arg(long)]
        json: bool,
    },
    /// Miembros con su presencia, trabajos del equipo y su último informe
    Ver {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Duerme un equipo: lo pendiente de sus ejecutores vuelve a la cola de modelo-local; exige una terminal interactiva real
    Dormir { id: String },
    /// Abre la sesión de un planificador o revisor del equipo con presencia (la lanza `despertar` en una terminal)
    #[command(hide = true)]
    Sesion { id: String, agente: String },
}

#[derive(Subcommand)]
pub enum BibliotecaAccion {
    /// Inventario agrupado por categoría y tipo, con su estado y su uso
    Listar {
        /// Solo este agente (claude-code, qwen-code, codex, opencode, antigravity o ecosistema)
        #[arg(long)]
        agente: Option<String>,
        /// skill, plugin, subagente, mcp, comando o extension
        #[arg(long)]
        tipo: Option<String>,
        #[arg(long)]
        categoria: Option<String>,
        /// Solo los activados
        #[arg(long, conflicts_with = "desactivados")]
        activados: bool,
        /// Solo los desactivados
        #[arg(long)]
        desactivados: bool,
        /// Muestra el id completo en vez del nombre (el id es lo que piden `ver`, `activar`…)
        #[arg(long)]
        ids: bool,
        /// Salida en JSON
        #[arg(long)]
        json: bool,
    },
    /// Ficha de un elemento: origen, estado, cómo se cambia, definición MCP (sin secretos) y uso
    Ver {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Informe de uso: por elemento y actor, lo usado que no está en el inventario y la actividad por agente
    Uso {
        /// Solo este elemento
        id: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Propone activar un elemento (se acepta con `ea propuestas decidir`)
    Activar {
        id: String,
        /// Con una terminal interactiva, pide confirmación y lo aplica en el acto; espera a Intro al terminar
        #[arg(long)]
        esperar: bool,
    },
    /// Propone desactivar un elemento (se acepta con `ea propuestas decidir`)
    Desactivar {
        id: String,
        /// Con una terminal interactiva, pide confirmación y lo aplica en el acto; espera a Intro al terminar
        #[arg(long)]
        esperar: bool,
    },
    /// Propone abrir un agente con solo las herramientas elegidas (sin cambiar su configuración)
    Sesion {
        /// claude-code, qwen-code, codex u opencode
        agente: String,
        /// Ids de la biblioteca
        #[arg(required = true)]
        ids: Vec<String>,
        /// Carpeta del proyecto en que se abre (por defecto, la actual)
        #[arg(long)]
        directorio: Option<String>,
        /// Con una terminal interactiva, pide confirmación y abre la sesión en el acto; espera a Intro al terminar
        #[arg(long)]
        esperar: bool,
    },
    /// Pone o quita la categoría de un elemento (atajo de `biblioteca.categorias`); exige una terminal interactiva real
    Categoria {
        id: String,
        /// Nombre de la categoría
        #[arg(required_unless_present = "quitar", conflicts_with = "quitar")]
        categoria: Option<String>,
        /// Vuelve a la categoría por defecto (el origen)
        #[arg(long)]
        quitar: bool,
    },
}

#[derive(Subcommand)]
pub enum AgentesAccion {
    /// Pone un agente en un rol (atajo de `ea ajustes establecer roles.<rol>=<agente>`);
    /// exige una terminal interactiva real
    Rol {
        /// El rol (también claude-code o modelo-local)
        #[arg(value_enum)]
        rol: ArgRol,
        /// Id del catálogo
        #[arg(value_parser = PossibleValuesParser::new(ea_ajustes::IDS_AGENTES.iter().copied()))]
        agente: String,
    },
}

#[derive(Subcommand)]
pub enum AjustesAccion {
    /// Todos los ajustes efectivos con su origen, quién los cambia y cuándo se aplican
    Ver {
        #[arg(long)]
        json: bool,
    },
    /// Solo el valor efectivo de una clave (p. ej. red.puerto_web)
    Obtener { clave: String },
    /// Cambia claves en ajustes.toml; exige una terminal interactiva real
    Establecer {
        /// Cambios `clave=valor` (listas separadas por comas)
        cambios: Vec<String>,
        /// Quita la clave de ajustes.toml (vuelve a mandar la capa siguiente)
        #[arg(long)]
        quitar: Vec<String>,
    },
    /// Dónde están ajustes.toml y los ficheros heredados
    Ruta,
}

// Se lee una vez de la línea de órdenes: no compensa meter `Crear` en una
// caja.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
pub enum TrabajoAccion {
    /// Crea un trabajo y lo encola en modelo-local
    Crear {
        /// Orden completa en JSON (el resto de opciones se ignora)
        #[arg(long)]
        orden: Option<std::path::PathBuf>,
        #[arg(long, value_enum, default_value = "mision")]
        modo: ArgModo,
        #[arg(long)]
        titulo: Option<String>,
        #[arg(long)]
        worktree: Option<String>,
        #[arg(long)]
        objetivo: Option<String>,
        /// Comando de shell que decide si está hecho
        #[arg(long)]
        criterio: Option<String>,
        /// Skills de Qwen para el trabajo, separadas por comas (ver `ea qwen-skills listar`)
        #[arg(long, value_delimiter = ',')]
        skills: Vec<String>,
        /// Quién crea el trabajo; elegir skills exige que esté en `[skills].pueden_elegir`
        #[arg(long, default_value = "ea-cli")]
        por: String,
        /// Equipo despierto para el que se crea (ver `ea equipo listar`)
        #[arg(long)]
        equipo: Option<String>,
        /// Ejecutor del equipo que lo hará; sin él, lo toma cualquier ejecutor despierto
        #[arg(long, requires = "equipo")]
        asignar: Option<String>,
        /// Revisor o planificador del equipo al que llega la entrega (distinto del ejecutor)
        #[arg(long, requires = "equipo")]
        revisor: Option<String>,
        /// El sistema crea un git worktree propio para el trabajo (rama ea/<id>), para correr en paralelo sobre el mismo repo
        #[arg(long)]
        worktree_propio: bool,
    },
    /// Lista los trabajos con su estado
    Listar,
    /// Orden, estado e informe de un trabajo
    Ver {
        id: String,
        /// Salida completa en JSON (orden, estado e informe)
        #[arg(long)]
        json: bool,
    },
    /// Diff completo del trabajo desde commit_inicio, más los ficheros nuevos sin seguimiento
    Diff {
        id: String,
        /// Solo el diff de esa fase de una obra (usa las instantáneas de árbol)
        #[arg(long)]
        fase: Option<u32>,
    },
    /// Da por bueno un trabajo en revisión
    Cerrar { id: String },
    /// Aprueba una revisión intermedia: la obra sigue en la fase actual
    Continuar { id: String },
    /// Confirma el paquete de corrección de una rutina en espera y lo encola en claude-code
    Confirmar {
        id: String,
        /// Contexto que se añade al paquete antes de encolarlo
        #[arg(long)]
        contexto: Option<String>,
    },
    /// Devuelve un trabajo en revisión al modelo local, con una nota
    Reformular {
        id: String,
        #[arg(long)]
        nota: String,
        /// Obra: rehace desde esta fase (y las siguientes); por defecto, la
        /// pendiente (en una obra completa, la última)
        #[arg(long)]
        fase: Option<u32>,
    },
    /// Cancela un trabajo
    Cancelar { id: String },
}

#[derive(Subcommand)]
pub enum QwenSkillsAccion {
    /// Catálogo: nombre, tamaño, si está habilitada y descripción
    Listar {
        #[arg(long)]
        json: bool,
    },
    /// Muestra el cuerpo de una skill tal como lo recibe Qwen
    Ver { nombre: String },
    /// Comprueba una selección (llave, existencia, topes) sin crear nada
    Probar {
        /// Skills separadas por comas
        #[arg(value_delimiter = ',')]
        skills: Vec<String>,
        #[arg(long, default_value = "claude-code")]
        por: String,
    },
}

#[derive(Subcommand)]
pub enum DiarioAccion {
    /// Lista eventos del diario, con filtros opcionales
    Listar {
        /// Solo los eventos asociados a este trabajo
        #[arg(long)]
        trabajo: Option<String>,
        #[arg(long)]
        actor: Option<String>,
        /// Fecha/hora mínima (RFC3339 o AAAA-MM-DD)
        #[arg(long)]
        desde: Option<String>,
        /// Fecha/hora máxima (RFC3339 o AAAA-MM-DD)
        #[arg(long)]
        hasta: Option<String>,
        /// Solo los eventos con este resultado
        #[arg(long, value_parser = ["ok", "error"])]
        resultado: Option<String>,
        /// Solo las acciones que empiezan por este prefijo, la familia de
        /// eventos (`bus_`, `mcp_`, `propuesta_`…); solo `[a-z_]`
        #[arg(long, value_parser = prefijo_de_accion)]
        accion: Option<String>,
        /// Página siguiente: solo los eventos anteriores al de este id (el
        /// último de la página ya vista), con los mismos filtros
        #[arg(long, value_parser = clap::value_parser!(i64).range(1..))]
        antes_de: Option<i64>,
        #[arg(long, default_value_t = 20)]
        limite: i64,
    },
    /// Muestra el detalle de un evento
    Ver { id: i64 },
    /// Añade una nota del usuario (`usuario_nota`): deja constancia de algo
    /// o corrige un evento (el diario no se edita: se añade otro evento)
    Anotar {
        /// Texto de la nota (no vacío; como mucho 10.000 caracteres)
        #[arg(long)]
        texto: String,
        /// Id del evento que esta nota corrige
        #[arg(long)]
        corrige: Option<i64>,
        /// Trabajo del bus al que se asocia la nota
        #[arg(long)]
        trabajo: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum RutinaAccion {
    /// Valida un fichero de rutina y lo registra (la versión debe subir; una versión nueva se confirma en una terminal)
    Registrar { fichero: std::path::PathBuf },
    /// Lista las rutinas registradas en uso, con su última ejecución
    Listar {
        /// Lista en su lugar las retiradas (en la papelera o sin fichero)
        #[arg(long)]
        retiradas: bool,
    },
    /// Muestra la definición registrada de una rutina y su historial
    Ver {
        slug: String,
        /// Muestra esa versión registrada en vez de la última
        #[arg(long)]
        version: Option<i64>,
    },
    /// Crea un trabajo de la rutina en el bus (lo que hace el disparador `systemd`)
    #[command(alias = "encolar")]
    Lanzar {
        slug: String,
        /// Entrada `clave=valor` que sustituye al valor por defecto
        #[arg(long = "entrada")]
        entrada: Vec<String>,
    },
    /// Retira una rutina: su TOML va a la papelera y se desprograma su timer (se restaura)
    Retirar { slug: String },
    /// Devuelve una rutina retirada a su sitio (no vuelve a programar su timer)
    Restaurar { slug: String },
}

#[derive(Subcommand)]
pub enum AccionesAccion {
    /// Lista el catálogo de herramientas con su nivel de riesgo
    Listar,
    /// Describe lo que haría una herramienta mutante, sin hacerlo
    Simular {
        herramienta: String,
        /// Argumentos como objeto JSON
        #[arg(long, default_value = "{}")]
        args: String,
    },
    /// Ejecuta una herramienta mutante: muestra la simulación y pide
    /// confirmación en la terminal (los irreversibles, confirmación expresa)
    Ejecutar {
        herramienta: String,
        /// Argumentos como objeto JSON
        #[arg(long, default_value = "{}")]
        args: String,
        /// Al terminar, espera a Intro antes de salir (la usa «⌨ abrir en la
        /// terminal» de ea-web, para que la ventana no se cierre sin leer)
        #[arg(long)]
        esperar: bool,
    },
}

#[derive(Subcommand)]
pub enum BovedaAccion {
    /// Raíz de la bóveda y número de notas por región
    Estado,
    /// Lista las notas más recientes
    Listar {
        #[arg(long, value_parser = ea_boveda::ruta::REGIONES)]
        region: Option<String>,
        #[arg(long, default_value_t = 20)]
        limite: usize,
    },
    /// Busca notas por palabras (todas deben aparecer)
    Buscar {
        consulta: String,
        #[arg(long, value_parser = ea_boveda::ruta::REGIONES)]
        region: Option<String>,
        #[arg(long, default_value_t = 10)]
        limite: usize,
    },
    /// Lee una nota por su ruta (p. ej. compartida/servicios/mcp.md)
    Leer {
        ruta: String,
        /// Carácter del cuerpo desde el que leer
        #[arg(long, default_value_t = 0)]
        desde: usize,
    },
    /// Promueve una nota de la región de un agente a la compartida: copia la
    /// nota tras mostrar la simulación y pedir confirmación en la terminal
    Promover {
        ruta: String,
        /// Ruta de destino dentro de «compartida» (por defecto, la misma)
        #[arg(long)]
        destino: Option<String>,
    },
    /// Reconstruye el índice de búsqueda desde los ficheros
    Reindexar,
    /// Borra PARA SIEMPRE lo que hay en la papelera de la bóveda. Es
    /// irreversible: exige una terminal real, enseña lo que borraría y pide
    /// escribir `confirmo vaciar-papelera`
    VaciarPapelera {
        /// Al terminar, espera a Intro antes de salir (la usa «⌨» de ea-web)
        #[arg(long)]
        esperar: bool,
    },
}

#[derive(Subcommand)]
pub enum PropuestasAccion {
    /// Lista propuestas, con filtro opcional por estado
    Listar {
        #[arg(long)]
        estado: Option<EstadoPropuesta>,
    },
    /// Muestra el detalle de una propuesta
    Ver { id: i64 },
    /// Decide una propuesta pendiente. Aceptar una propuesta con acción la
    /// ejecuta, tras mostrar la simulación y pedir confirmación en la terminal
    Decidir {
        id: i64,
        #[arg(long)]
        aceptar: bool,
        #[arg(long)]
        rechazar: bool,
        /// Al terminar, espera a Intro antes de salir (la usa «⌨ abrir en la
        /// terminal» de ea-web, para que la ventana no se cierre sin leer)
        #[arg(long)]
        esperar: bool,
    },
}

/// Un prefijo de acción del diario: letras minúsculas y `_`, como los nombres
/// de las acciones.
fn prefijo_de_accion(s: &str) -> Result<String, String> {
    if ea_store::prefijo_accion_valido(s) {
        Ok(s.to_string())
    } else {
        Err("solo letras minúsculas y `_` (p. ej. `bus_` o `propuesta_`)".into())
    }
}

// ------------------------------------------------------ dominios cerrados
//
// `clap` valida al leer la línea de órdenes los valores de una lista cerrada
// y la ayuda los enumera. Las listas son las de las bibliotecas (que no
// dependen de `clap`): aquí solo se envuelven sus tipos o se citan sus
// constantes (`IDS_AGENTES`, `REGIONES`).

/// Un rol: `planificador` o `ejecutor` (también su nombre en disco,
/// `claude-code` o `modelo-local`).
#[derive(Clone, Copy)]
pub struct ArgRol(pub Rol);

impl ValueEnum for ArgRol {
    fn value_variants<'a>() -> &'a [Self] {
        const V: [ArgRol; Rol::TODOS.len()] = [ArgRol(Rol::TODOS[0]), ArgRol(Rol::TODOS[1])];
        &V
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(self.0.nombre()).alias(self.0.as_str()))
    }
}

/// El modo de una orden: `mision`, `obra` o `rutina`.
#[derive(Clone, Copy)]
pub struct ArgModo(pub Modo);

impl ValueEnum for ArgModo {
    fn value_variants<'a>() -> &'a [Self] {
        &[ArgModo(Modo::Mision), ArgModo(Modo::Obra), ArgModo(Modo::Rutina)]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(self.0.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn la_linea_de_ordenes_es_coherente() {
        Cli::command().debug_assert();
    }

    /// Las listas de la ayuda son las de las bibliotecas, sin perder ninguna.
    #[test]
    fn los_dominios_cerrados_son_los_de_las_bibliotecas() {
        let nombres = |v: &[PossibleValue]| v.iter().map(|p| p.get_name().to_string()).collect::<Vec<_>>();
        let roles: Vec<_> = ArgRol::value_variants().iter().filter_map(ValueEnum::to_possible_value).collect();
        assert_eq!(nombres(&roles), Rol::TODOS.map(|r| r.nombre()));
        for r in Rol::TODOS {
            assert!(roles.iter().any(|p| p.matches(r.as_str(), false)), "{}", r.as_str());
        }
        let modos: Vec<_> = ArgModo::value_variants().iter().filter_map(ValueEnum::to_possible_value).collect();
        assert_eq!(nombres(&modos), ["mision", "obra", "rutina"]);
        for (m, p) in ArgModo::value_variants().iter().zip(&modos) {
            let vuelta: Modo = serde_json::from_value(serde_json::json!(p.get_name())).unwrap();
            assert_eq!(vuelta, m.0);
        }
    }
}
