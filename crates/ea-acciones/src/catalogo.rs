//! El catálogo único de herramientas (decisión 11: un catálogo, dos
//! proyecciones). Es puramente declarativo: nombre, nivel de riesgo (§7.3),
//! cómo se ejecuta y a quién se proyecta. No contiene lógica.

use std::io::IsTerminal;

use crate::error::ErrorAccion;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Riesgo {
    Lectura,
    /// Se puede deshacer o repetir sin pérdida (cambiar un estado, reiniciar
    /// un servicio, lanzar una rutina).
    Reversible,
    /// No se puede deshacer (borrar un fichero del usuario).
    Irreversible,
}

impl Riesgo {
    pub fn as_str(self) -> &'static str {
        match self {
            Riesgo::Lectura => "lectura",
            Riesgo::Reversible => "reversible",
            Riesgo::Irreversible => "irreversible",
        }
    }

    pub fn parse(s: &str) -> Option<Riesgo> {
        match s {
            "lectura" => Some(Riesgo::Lectura),
            "reversible" => Some(Riesgo::Reversible),
            "irreversible" => Some(Riesgo::Irreversible),
            _ => None,
        }
    }

    pub fn es_mutante(self) -> bool {
        self != Riesgo::Lectura
    }
}

/// Cómo se ejecuta una herramienta cuando la invoca un agente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ejecucion {
    /// Se ejecuta al invocarla (lecturas, y la escritura en la región propia
    /// de la bóveda: ver su entrada).
    Directa,
    /// El agente solo obtiene la simulación y puede encolar una propuesta;
    /// ejecutarla requiere la confirmación del usuario.
    Supervisada,
}

#[derive(Debug)]
pub struct Herramienta {
    pub nombre: &'static str,
    pub riesgo: Riesgo,
    pub ejecucion: Ejecucion,
    /// Si forma parte de la proyección reducida del modelo local.
    pub local: bool,
    /// Resumen de una línea (la descripción rica de cada agente vive en la
    /// definición MCP; la del agente local, en el cliente MCP de Qwen Code).
    pub resumen: &'static str,
}

pub const CATALOGO: &[Herramienta] = &[
    Herramienta {
        nombre: "ea_status",
        riesgo: Riesgo::Lectura,
        ejecucion: Ejecucion::Directa,
        local: true,
        resumen: "Resumen del estado del ecosistema",
    },
    Herramienta {
        nombre: "ea_listar_trabajos",
        riesgo: Riesgo::Lectura,
        ejecucion: Ejecucion::Directa,
        local: true,
        resumen: "Lista trabajos del bus (misiones, obras y rutinas)",
    },
    Herramienta {
        nombre: "ea_ver_trabajo",
        riesgo: Riesgo::Lectura,
        ejecucion: Ejecucion::Directa,
        local: true,
        resumen: "Detalle de un trabajo del bus: orden, estado, fases e informe",
    },
    Herramienta {
        nombre: "ea_listar_diario",
        riesgo: Riesgo::Lectura,
        ejecucion: Ejecucion::Directa,
        local: true,
        resumen: "Lista eventos del diario",
    },
    Herramienta {
        nombre: "ea_encolar_rutina",
        riesgo: Riesgo::Reversible,
        ejecucion: Ejecucion::Supervisada,
        local: false,
        resumen: "Encola la ejecución de una rutina",
    },
    // Registrar una versión nueva no borra nada: la anterior sigue en
    // SQLite y se recupera registrándola otra vez con una versión mayor.
    Herramienta {
        nombre: "ea_registrar_rutina",
        riesgo: Riesgo::Reversible,
        ejecucion: Ejecucion::Supervisada,
        local: false,
        resumen: "Registra una rutina o una versión nueva de una (TOML)",
    },
    Herramienta {
        nombre: "ea_reiniciar_servicio",
        riesgo: Riesgo::Reversible,
        ejecucion: Ejecucion::Supervisada,
        local: false,
        resumen: "Reinicia un servicio del ecosistema (lista cerrada)",
    },
    // El timer de una rutina (spec 004, H5): se deshace desprogramándolo
    // (o programándolo otra vez con el horario anterior). Solo crea trabajos
    // a su hora, nunca los ejecuta (art. V).
    Herramienta {
        nombre: "ea_programar_rutina",
        riesgo: Riesgo::Reversible,
        ejecucion: Ejecucion::Supervisada,
        local: false,
        resumen: "Programa una rutina registrada con un timer de systemd (OnCalendar)",
    },
    Herramienta {
        nombre: "ea_desprogramar_rutina",
        riesgo: Riesgo::Reversible,
        ejecucion: Ejecucion::Supervisada,
        local: false,
        resumen: "Desactiva y borra el timer de systemd de una rutina",
    },
    // Retirar y restaurar una rutina (spec 004, RF-011): su TOML va a la
    // papelera de `rutinas_dir` y vuelve, sin tocar SQLite. Se deshacen la
    // una a la otra, como la papelera de la bóveda (art. IV.2, aclaración
    // 1.2.0). Retirar también desprograma el timer.
    Herramienta {
        nombre: "ea_retirar_rutina",
        riesgo: Riesgo::Reversible,
        ejecucion: Ejecucion::Supervisada,
        local: false,
        resumen: "Retira una rutina: su TOML va a la papelera y se desprograma su timer",
    },
    Herramienta {
        nombre: "ea_restaurar_rutina",
        riesgo: Riesgo::Reversible,
        ejecucion: Ejecucion::Supervisada,
        local: false,
        resumen: "Devuelve una rutina retirada a rutinas_dir (sin volver a programar su timer)",
    },
    // Spec 018, F5: abre en una terminal un agente con SOLO las herramientas
    // elegidas de la biblioteca. Reversible: se cierra la ventana y los
    // ficheros efímeros (0600, en el runtime) caducan solos a las 24 h; no
    // cambia la configuración de ningún agente. Nunca para el ejecutor
    // (`local: false`): abrir una ventana del escritorio es del usuario.
    Herramienta {
        nombre: "ea_lanzar_sesion",
        riesgo: Riesgo::Reversible,
        ejecucion: Ejecucion::Supervisada,
        local: false,
        resumen: "Abre en una terminal un agente (Claude Code, Qwen Code, Codex u OpenCode) con solo las herramientas elegidas de la biblioteca",
    },
    // Vaciar la papelera: solo una rutina ya retirada, y solo desde la terminal.
    Herramienta {
        nombre: "ea_eliminar_rutina",
        riesgo: Riesgo::Irreversible,
        ejecucion: Ejecucion::Supervisada,
        local: false,
        resumen: "Borra para siempre la copia retirada de una rutina (la de la papelera)",
    },
    // Bóveda (fase 7). Leer y buscar son lecturas con resultados acotados
    // por actor (el modelo local, con 8192 tokens de contexto, recibe cotas
    // más estrechas).
    Herramienta {
        nombre: "ea_buscar_notas",
        riesgo: Riesgo::Lectura,
        ejecucion: Ejecucion::Directa,
        local: true,
        resumen: "Busca notas en la bóveda (resultados acotados)",
    },
    Herramienta {
        nombre: "ea_leer_nota",
        riesgo: Riesgo::Lectura,
        ejecucion: Ejecucion::Directa,
        local: true,
        resumen: "Lee una nota de la bóveda (por trozos)",
    },
    // Excepción razonada a §11.1: la región
    // propia de cada agente es de libertad total (decisión 8), así que
    // escribir ahí no pide confirmación. No puede nombrar otra región, no
    // borra nada y deja un evento en el diario. El orquestador (lo
    // desatendido) no la ve: `autorizar_mcp` solo le deja las lecturas.
    Herramienta {
        nombre: "ea_escribir_nota",
        riesgo: Riesgo::Reversible,
        ejecucion: Ejecucion::Directa,
        local: true,
        resumen: "Escribe una nota en la región propia del agente",
    },
    // Promover a la región compartida lo decide el usuario (decisión 9): un
    // agente solo propone, y aceptar la propuesta la ejecuta.
    Herramienta {
        nombre: "ea_promover_nota",
        riesgo: Riesgo::Reversible,
        ejecucion: Ejecucion::Supervisada,
        local: true,
        resumen: "Propone copiar una nota a la región compartida (la decide el usuario)",
    },
    // Biblioteca de herramientas (spec 018, F4): activar o desactivar una skill,
    // plugin, subagente, MCP, comando o extensión de un agente. Reversible (la
    // una deshace la otra, con copia previa de lo que se edita); lo decide el
    // usuario. Fuera del modelo local: el ejecutor no se amplía las herramientas.
    Herramienta {
        nombre: "ea_activar_herramienta",
        riesgo: Riesgo::Reversible,
        ejecucion: Ejecucion::Supervisada,
        local: false,
        resumen: "Activa un elemento de la biblioteca (skill, plugin, subagente, MCP, comando o extensión)",
    },
    Herramienta {
        nombre: "ea_desactivar_herramienta",
        riesgo: Riesgo::Reversible,
        ejecucion: Ejecucion::Supervisada,
        local: false,
        resumen: "Desactiva un elemento de la biblioteca (skill, plugin, subagente, MCP, comando o extensión)",
    },
];

pub fn buscar(nombre: &str) -> Option<&'static Herramienta> {
    CATALOGO.iter().find(|h| h.nombre == nombre)
}

/// Herramientas de solo lectura: las únicas que una rutina puede invocar en
/// un paso `accion`.
pub fn lecturas() -> impl Iterator<Item = &'static Herramienta> {
    CATALOGO.iter().filter(|h| h.riesgo == Riesgo::Lectura)
}

/// Proyección reducida para el modelo local.
pub fn proyeccion_local() -> impl Iterator<Item = &'static Herramienta> {
    CATALOGO.iter().filter(|h| h.local)
}

/// Por dónde da el usuario su confirmación (§11.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Canal {
    Terminal,
    Web,
}

impl Canal {
    /// Terminal interactiva real: stdin y stdout deben ser un TTY. Es lo que
    /// distingue a una persona delante de un teclado de un agente que lanza
    /// `ea` como subproceso sin terminal.
    pub fn terminal_interactiva() -> Result<Canal, ErrorAccion> {
        if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
            Ok(Canal::Terminal)
        } else {
            Err(ErrorAccion::Prohibida(
                "la confirmación debe darse en una terminal interactiva (sin TTY no se confirma)"
                    .into(),
            ))
        }
    }

    pub fn nombre(self) -> &'static str {
        match self {
            Canal::Terminal => "terminal",
            Canal::Web => "web",
        }
    }

    /// §11.1: reversible requiere confirmación (terminal o web);
    /// irreversible, solo terminal. Nunca por voz ni por MCP: esos canales
    /// ni siquiera existen como variantes.
    pub fn puede_confirmar(self, riesgo: Riesgo) -> Result<(), ErrorAccion> {
        match (self, riesgo) {
            (Canal::Web, Riesgo::Irreversible) => Err(ErrorAccion::Prohibida(
                "una acción irreversible solo se confirma desde la terminal (ea propuestas decidir)"
                    .into(),
            )),
            _ => Ok(()),
        }
    }
}

/// Autorización de una invocación MCP según el actor (derivado del cliente
/// que se identificó y del rol que ocupa, §7.3 y spec 017: no es un
/// argumento del modelo). Es una lista blanca:
///
/// - `claude-code` (el rol planificador, lo ocupe el agente que lo ocupe): el
///   catálogo completo (las mutantes solo proponen).
/// - `modelo-local` (el rol ejecutor): la proyección reducida y, de ella,
///   nada irreversible.
/// - Cualquier otro (`orquestador`, lo desatendido, §11.2; un cliente
///   desconocido; un agente que no ocupa ningún rol): solo lectura.
pub fn autorizar_mcp(actor: &str, h: &Herramienta) -> Result<(), String> {
    let permitido = match actor {
        "claude-code" => true,
        "modelo-local" => h.local && h.riesgo != Riesgo::Irreversible,
        _ => h.riesgo == Riesgo::Lectura,
    };
    if permitido {
        Ok(())
    } else {
        Err(format!(
            "'{}' ({}) no está disponible para el actor '{actor}'",
            h.nombre,
            h.riesgo.as_str()
        ))
    }
}
