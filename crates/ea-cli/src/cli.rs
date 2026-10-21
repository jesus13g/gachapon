use clap::{Parser, Subcommand};
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
        /// modelo-local o claude-code
        agente: Option<String>,
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
    /// Ajustes del ecosistema (ajustes.toml): ver, obtener y establecer
    Ajustes {
        #[command(subcommand)]
        accion: AjustesAccion,
    },
    /// Interfaz interactiva
    Tui,
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
        /// Skills de Qwen para el trabajo, separadas por comas (ver `ea qwen-skills listar`)
        #[arg(long, value_delimiter = ',')]
        skills: Vec<String>,
        /// Quién crea el trabajo; elegir skills exige que esté en `[skills].pueden_elegir`
        #[arg(long, default_value = "ea-cli")]
        por: String,
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
    /// Valida un fichero de rutina y lo registra (la versión debe subir)
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
        /// Al terminar, espera a Intro antes de salir (la usa «⌨ abrir en la
        /// terminal» de ea-web, para que la ventana no se cierre sin leer)
        #[arg(long)]
        esperar: bool,
    },
}

/// Un prefijo de acción del diario: letras minúsculas y `_`, como los nombres
/// de las acciones.
fn prefijo_de_accion(s: &str) -> Result<String, String> {
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
        Ok(s.to_string())
    } else {
        Err("solo letras minúsculas y `_` (p. ej. `bus_` o `propuesta_`)".into())
    }
}
