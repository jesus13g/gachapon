use clap::{Parser, Subcommand};
use ea_store::{EstadoPropuesta, TaskOrigin, TaskStatus};

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
    /// Gestión de tareas
    Tareas {
        #[command(subcommand)]
        accion: TareasAccion,
    },
    /// Diario de eventos
    Diario {
        #[command(subcommand)]
        accion: DiarioAccion,
    },
    /// Rutinas versionadas (registrar, listar, ver, lanzar)
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
        /// modelo-local o claude-code
        agente: Option<String>,
    },
    /// Semáforos de worktree ocupados ahora mismo
    Semaforos,
    /// Interfaz interactiva
    Tui,
}

#[derive(Subcommand)]
pub enum TrabajoAccion {
    /// Crea un trabajo y lo encola en modelo-local
    Crear {
        /// Orden completa en JSON (el resto de opciones se ignora)
        #[arg(long)]
        orden: Option<std::path::PathBuf>,
        /// mision, obra o rutina
        #[arg(long, default_value = "mision")]
        modo: String,
        #[arg(long)]
        titulo: Option<String>,
        #[arg(long)]
        worktree: Option<String>,
        #[arg(long)]
        objetivo: Option<String>,
        /// Comando de shell que decide si está hecho
        #[arg(long)]
        criterio: Option<String>,
    },
    /// Lista los trabajos con su estado
    Listar,
    /// Orden, estado e informe de un trabajo
    Ver { id: String },
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
    },
    /// Cancela un trabajo
    Cancelar { id: String },
}

#[derive(Subcommand)]
pub enum TareasAccion {
    /// Crea una tarea nueva
    Crear {
        #[arg(long)]
        titulo: String,
        #[arg(long = "asignado-a")]
        asignado_a: String,
        #[arg(long)]
        solicita: String,
        #[arg(long, default_value = "manual")]
        origen: TaskOrigin,
        #[arg(long = "creado-por", default_value = "usuario")]
        creado_por: String,
        #[arg(long = "tarea-padre")]
        tarea_padre: Option<i64>,
    },
    /// Lista tareas, con filtros opcionales
    Listar {
        #[arg(long)]
        estado: Option<TaskStatus>,
        #[arg(long = "asignado-a")]
        asignado_a: Option<String>,
        #[arg(long, default_value_t = 20)]
        limite: i64,
    },
    /// Muestra el detalle de una tarea y su historial de eventos
    Ver { id: i64 },
    /// Actualiza el estado de una tarea (registra el evento correspondiente)
    Actualizar {
        id: i64,
        #[arg(long)]
        estado: TaskStatus,
        #[arg(long)]
        resultado: Option<String>,
        #[arg(long, default_value = "usuario")]
        actor: String,
    },
}

#[derive(Subcommand)]
pub enum DiarioAccion {
    /// Lista eventos del diario, con filtros opcionales
    Listar {
        #[arg(long)]
        tarea: Option<i64>,
        #[arg(long)]
        actor: Option<String>,
        /// Fecha/hora mínima (RFC3339 o AAAA-MM-DD)
        #[arg(long)]
        desde: Option<String>,
        /// Fecha/hora máxima (RFC3339 o AAAA-MM-DD)
        #[arg(long)]
        hasta: Option<String>,
        #[arg(long, default_value_t = 20)]
        limite: i64,
    },
    /// Muestra el detalle de un evento
    Ver { id: i64 },
}

#[derive(Subcommand)]
pub enum RutinaAccion {
    /// Valida un fichero de rutina y lo registra (la versión debe subir)
    Registrar { fichero: std::path::PathBuf },
    /// Lista las rutinas registradas, con su última ejecución
    Listar,
    /// Muestra la definición registrada de una rutina y su historial
    Ver { slug: String },
    /// Crea un trabajo de la rutina en el bus (lo que hace el disparador `systemd`)
    #[command(alias = "encolar")]
    Lanzar {
        slug: String,
        /// Entrada `clave=valor` que sustituye al valor por defecto
        #[arg(long = "entrada")]
        entrada: Vec<String>,
    },
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
    },
}

#[derive(Subcommand)]
pub enum BovedaAccion {
    /// Raíz de la bóveda y número de notas por región
    Estado,
    /// Lista las notas más recientes
    Listar {
        /// modelo-local, claude-code o compartida
        #[arg(long)]
        region: Option<String>,
        #[arg(long, default_value_t = 20)]
        limite: usize,
    },
    /// Busca notas por palabras (todas deben aparecer)
    Buscar {
        consulta: String,
        #[arg(long)]
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
    },
}
