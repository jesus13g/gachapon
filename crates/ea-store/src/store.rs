use std::path::Path;

use crate::connection;
use crate::error::StoreResult;
use crate::models::{
    EntradaIndice, ResultadoBusqueda, RutaIndexada,
    EstadoEjecucion, EstadoPropuesta, Event, NewEvent, NuevaPropuesta, Propuesta,
};
use crate::repository::boveda;
use crate::repository::events::{self, EventFilter};
use crate::repository::propuestas::{self, PropuestaFilter};
use crate::repository::trabajos::{self, FilaFase, FilaRutina, FilaTrabajo};

/// Fachada pública sobre la base de datos embebida. Único punto de acceso
/// para todo consumidor (CLI hoy; proceso orquestador y servidor MCP en
/// fases futuras) — ninguno ve tipos de `rusqlite`, solo tipos de dominio.
pub struct Store {
    conn: rusqlite::Connection,
}

impl Store {
    /// Abre la base de datos en su ruta XDG estándar (o `$EA_DB_PATH` si está
    /// definida) y aplica migraciones pendientes.
    pub fn open() -> StoreResult<Self> {
        Ok(Self {
            conn: connection::open()?,
        })
    }

    /// Igual que [`Store::open`] pero sobre una ruta explícita (`":memory:"`
    /// para tests).
    pub fn open_at(path: &Path) -> StoreResult<Self> {
        Ok(Self {
            conn: connection::open_at(path)?,
        })
    }

    /// Otra conexión a la misma base. Sirve para que un trabajo largo no
    /// retenga la conexión con la que el mismo proceso atiende lecturas. No
    /// vale para una base en memoria: cada conexión sería una base distinta.
    pub fn duplicar(&self) -> StoreResult<Self> {
        match self.conn.path() {
            Some(ruta) if !ruta.is_empty() => Self::open_at(Path::new(ruta)),
            _ => Err(rusqlite::Error::InvalidPath("base en memoria".into()).into()),
        }
    }

    pub fn insertar_trabajo(&self, t: &FilaTrabajo) -> StoreResult<()> {
        trabajos::insertar_trabajo(&self.conn, t)
    }

    pub fn actualizar_estado_trabajo(&self, id: &str, estado: &str, actualizado_en: &str) -> StoreResult<()> {
        trabajos::actualizar_estado_trabajo(&self.conn, id, estado, actualizado_en)
    }

    pub fn obtener_trabajo(&self, id: &str) -> StoreResult<Option<FilaTrabajo>> {
        trabajos::obtener_trabajo(&self.conn, id)
    }

    pub fn guardar_fase(&self, f: &FilaFase) -> StoreResult<()> {
        trabajos::guardar_fase(&self.conn, f)
    }

    pub fn listar_fases(&self, trabajo_id: &str) -> StoreResult<Vec<FilaFase>> {
        trabajos::listar_fases(&self.conn, trabajo_id)
    }

    pub fn guardar_rutina(&self, r: &FilaRutina) -> StoreResult<()> {
        trabajos::guardar_rutina(&self.conn, r)
    }

    pub fn obtener_rutina(&self, slug: &str, version: i64) -> StoreResult<Option<FilaRutina>> {
        trabajos::obtener_rutina(&self.conn, slug, version)
    }

    pub fn ultima_rutina(&self, slug: &str) -> StoreResult<Option<FilaRutina>> {
        trabajos::ultima_rutina(&self.conn, slug)
    }

    pub fn listar_rutinas(&self) -> StoreResult<Vec<FilaRutina>> {
        trabajos::listar_rutinas(&self.conn)
    }

    /// Todas las versiones de una rutina, la más reciente primero.
    pub fn listar_versiones_rutina(&self, slug: &str) -> StoreResult<Vec<FilaRutina>> {
        trabajos::listar_versiones_rutina(&self.conn, slug)
    }

    pub fn append_event(&self, new_event: NewEvent) -> StoreResult<Event> {
        events::append_event(&self.conn, new_event)
    }

    pub fn get_event(&self, id: i64) -> StoreResult<Event> {
        events::get_event(&self.conn, id)
    }

    pub fn list_events(&self, filter: &EventFilter) -> StoreResult<Vec<Event>> {
        events::list_events(&self.conn, filter)
    }

    pub fn list_events_by_trabajo(&self, trabajo: &str) -> StoreResult<Vec<Event>> {
        events::list_events_by_trabajo(&self.conn, trabajo)
    }

    /// Encola una propuesta (de texto libre o con acción estructurada).
    pub fn crear_propuesta(&self, nueva: NuevaPropuesta) -> StoreResult<Propuesta> {
        propuestas::crear(&self.conn, nueva)
    }

    /// Acepta una propuesta con acción y la reclama para ejecutarla (atómico).
    pub fn reclamar_propuesta(&self, id: i64) -> StoreResult<Propuesta> {
        propuestas::reclamar_para_ejecucion(&self.conn, id)
    }

    pub fn registrar_ejecucion_propuesta(
        &self,
        id: i64,
        estado: EstadoEjecucion,
        detalle: &str,
    ) -> StoreResult<Propuesta> {
        propuestas::registrar_ejecucion(&self.conn, id, estado, detalle)
    }

    pub fn get_propuesta(&self, id: i64) -> StoreResult<Propuesta> {
        propuestas::get_propuesta(&self.conn, id)
    }

    pub fn list_propuestas(&self, filter: &PropuestaFilter) -> StoreResult<Vec<Propuesta>> {
        propuestas::list_propuestas(&self.conn, filter)
    }

    pub fn decidir_propuesta(&self, id: i64, estado: EstadoPropuesta) -> StoreResult<Propuesta> {
        propuestas::decidir_propuesta(&self.conn, id, estado)
    }

    // --- Índice de búsqueda de la bóveda (caché derivada de los ficheros) ---

    pub fn boveda_indexar(&mut self, entrada: &EntradaIndice, cuerpo: &str) -> StoreResult<()> {
        boveda::indexar(&mut self.conn, entrada, cuerpo)
    }

    pub fn boveda_olvidar(&mut self, ruta: &str) -> StoreResult<()> {
        boveda::olvidar(&mut self.conn, ruta)
    }

    pub fn boveda_rutas_indexadas(&self) -> StoreResult<Vec<RutaIndexada>> {
        boveda::rutas_indexadas(&self.conn)
    }

    /// `consulta_fts` debe venir saneada (ver `ea_boveda::consulta_fts`).
    pub fn boveda_buscar(
        &self,
        consulta_fts: &str,
        region: Option<&str>,
        limite: i64,
        max_fragmento: i64,
    ) -> StoreResult<Vec<ResultadoBusqueda>> {
        boveda::buscar(&self.conn, consulta_fts, region, limite, max_fragmento)
    }

    pub fn boveda_recientes(&self, region: Option<&str>, limite: i64) -> StoreResult<Vec<ResultadoBusqueda>> {
        boveda::recientes(&self.conn, region, limite)
    }

    /// Todas las notas indexadas, ordenadas por ruta.
    pub fn boveda_todas(&self) -> StoreResult<Vec<ResultadoBusqueda>> {
        boveda::todas(&self.conn)
    }

    pub fn boveda_contar_por_region(&self) -> StoreResult<Vec<(String, i64)>> {
        boveda::contar_por_region(&self.conn)
    }
}
