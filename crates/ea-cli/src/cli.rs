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
    /// Rutinas programadas
    Rutinas {
        #[command(subcommand)]
        accion: RutinasAccion,
    },
    /// Propuestas pendientes de rutinas desatendidas
    Propuestas {
        #[command(subcommand)]
        accion: PropuestasAccion,
    },
    /// Voz (activación manual y palabra de activación)
    Voz {
        #[command(subcommand)]
        accion: VozAccion,
    },
    /// Interfaz interactiva
    Tui,
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
pub enum RutinasAccion {
    /// Lista las rutinas dadas de alta, con su última ejecución
    Listar,
    /// Muestra la definición de una rutina y su historial de ejecuciones
    Ver { slug: String },
    /// Encola una ejecución de la rutina (lo que hace el disparador `systemd`)
    Encolar { slug: String },
}

#[derive(Subcommand)]
pub enum VozAccion {
    /// Escucha una frase (activación manual), la transcribe, pregunta al
    /// modelo local y habla la respuesta. La captura y la reproducción de
    /// audio ocurren en el proceso `ea-orchestrator` (dueño del
    /// dispositivo), no en esta CLI.
    Hablar,
    /// Estado del motor de voz y de la palabra de activación.
    Estado,
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
    /// Registra la decisión del usuario sobre una propuesta pendiente
    Decidir {
        id: i64,
        #[arg(long)]
        aceptar: bool,
        #[arg(long)]
        rechazar: bool,
    },
}
