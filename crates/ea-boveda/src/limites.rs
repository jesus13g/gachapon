//! Cotas de lectura y escritura. El contexto del modelo local es de 8192
//! tokens: cualquier cosa que se le devuelva tiene que caber con holgura
//! junto a la petición y a la redacción de la respuesta (el proxy ya recorta
//! a 6000 caracteres el resultado de una herramienta). Claude Code, con
//! mucho más contexto, recibe cotas más amplias, pero acotadas igualmente.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limites {
    /// Máximo de resultados por búsqueda.
    pub max_resultados: usize,
    /// Máximo de caracteres del fragmento de cada resultado.
    pub max_fragmento: usize,
    /// Máximo de caracteres del cuerpo que devuelve una lectura (el resto se
    /// pide con `desde`).
    pub max_lectura: usize,
    /// Máximo de bytes de cuerpo de una nota que se escribe.
    pub max_escritura: usize,
}

impl Limites {
    pub const LOCAL: Limites =
        Limites { max_resultados: 5, max_fragmento: 200, max_lectura: 3000, max_escritura: 8 * 1024 };
    pub const AMPLIOS: Limites =
        Limites { max_resultados: 20, max_fragmento: 400, max_lectura: 20_000, max_escritura: 64 * 1024 };

    /// Las cotas del actor: el modelo local las estrechas; los demás, las amplias.
    pub fn para_actor(actor: &str) -> Limites {
        if actor == "modelo-local" { Self::LOCAL } else { Self::AMPLIOS }
    }
}

/// Tamaño máximo de un fichero que el índice y las lecturas aceptan.
pub const MAX_FICHERO_BYTES: u64 = 512 * 1024;
/// Tope de ficheros que recorre una sincronización.
pub const MAX_FICHEROS: usize = 20_000;
