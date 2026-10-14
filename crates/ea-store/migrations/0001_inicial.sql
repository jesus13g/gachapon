-- Esquema inicial único del ecosistema (orquestador). Todo lo anterior
-- (tareas, ejecuciones de rutina) se eliminó: se arranca desde una base nueva.

PRAGMA foreign_keys = ON;

-- Diario de eventos. `trabajo` asocia opcionalmente un evento a un trabajo
-- del bus (id de texto).
CREATE TABLE events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    ts          TEXT    NOT NULL,
    actor       TEXT    NOT NULL,
    action      TEXT    NOT NULL,
    trabajo     TEXT,
    outcome     TEXT    CHECK (outcome IN ('ok', 'error')),
    duration_ms INTEGER,
    detail      TEXT
);

CREATE INDEX idx_events_ts          ON events(ts);
CREATE INDEX idx_events_actor       ON events(actor);
CREATE INDEX idx_events_trabajo     ON events(trabajo);
CREATE INDEX idx_events_trabajo_ts  ON events(trabajo, ts);

-- Append-only reforzado a nivel de motor: un evento erróneo se corrige
-- emitiendo otro evento, nunca editando ni borrando.
CREATE TRIGGER events_no_update BEFORE UPDATE ON events
BEGIN
    SELECT RAISE(ABORT, 'events es append-only: corrige emitiendo un evento nuevo');
END;

CREATE TRIGGER events_no_delete BEFORE DELETE ON events
BEGIN
    SELECT RAISE(ABORT, 'events es append-only: corrige emitiendo un evento nuevo');
END;

-- Mutaciones supervisadas: acciones que un agente propone y solo el usuario
-- acepta. Puede llevar una acción estructurada que se ejecuta al aceptarla.
CREATE TABLE propuestas (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    creado_en            TEXT    NOT NULL,
    descripcion          TEXT    NOT NULL,
    estado               TEXT    NOT NULL DEFAULT 'pendiente'
                             CHECK (estado IN ('pendiente', 'aceptada', 'rechazada')),
    decidido_en          TEXT,
    proponente           TEXT,
    accion_herramienta   TEXT,
    accion_argumentos    TEXT,
    accion_riesgo        TEXT CHECK (accion_riesgo IN ('reversible', 'irreversible')),
    accion_simulacion    TEXT,
    accion_precondicion  TEXT,
    ejecucion_estado     TEXT CHECK (ejecucion_estado IN ('ejecutando', 'ejecutada', 'fallida')),
    ejecucion_detalle    TEXT,
    ejecutada_en         TEXT,
    CHECK ((accion_herramienta IS NULL) = (accion_riesgo IS NULL))
);

CREATE INDEX idx_propuestas_estado ON propuestas(estado);

-- Índice de búsqueda de la bóveda compartida. Es una caché derivada
-- de los ficheros (la fuente de verdad son las notas, que se leen meses
-- después): se puede borrar y reconstruir entero con `ea boveda reindexar`.
-- Vive en esta base y no en la bóveda para no dejar ficheros ajenos al
-- usuario dentro de su carpeta de notas.

CREATE TABLE boveda_notas (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    ruta        TEXT    NOT NULL UNIQUE,   -- relativa a la raíz, con '/'
    region      TEXT    NOT NULL,
    autor       TEXT,
    titulo      TEXT    NOT NULL,
    origen      TEXT,
    promocion   TEXT,
    creada      TEXT,
    actualizada TEXT,
    mtime_ns    INTEGER NOT NULL,          -- para detectar ediciones externas
    bytes       INTEGER NOT NULL
);

CREATE INDEX idx_boveda_notas_region ON boveda_notas(region);

-- rowid de la tabla FTS = id de boveda_notas. Texto sin acentos ni
-- mayúsculas, para que «cafe» encuentre «café».
CREATE VIRTUAL TABLE boveda_fts USING fts5(
    titulo, tags, cuerpo,
    tokenize = 'unicode61 remove_diacritics 2'
);

-- Historial de trabajos. La fuente de verdad del bus son los ficheros de estado;
-- estas tablas son el historial consultable.

CREATE TABLE trabajos (
    id               TEXT PRIMARY KEY,
    modo             TEXT NOT NULL,          -- mision | obra | rutina
    titulo           TEXT NOT NULL,
    worktree         TEXT NOT NULL,
    estado           TEXT NOT NULL,
    creado_en        TEXT NOT NULL,
    actualizado_en   TEXT NOT NULL,
    creado_por       TEXT NOT NULL,
    rutina_slug      TEXT,
    rutina_version   INTEGER
);
CREATE INDEX idx_trabajos_estado ON trabajos(estado);

CREATE TABLE trabajo_fases (
    trabajo_id  TEXT    NOT NULL REFERENCES trabajos(id),
    numero      INTEGER NOT NULL,
    titulo      TEXT    NOT NULL,
    estado      TEXT    NOT NULL,
    informe     TEXT,                        -- JSON del informe
    cerrado_en  TEXT,
    PRIMARY KEY (trabajo_id, numero)
);

CREATE TABLE rutinas (
    slug        TEXT    NOT NULL,
    version     INTEGER NOT NULL,
    definicion  TEXT    NOT NULL,            -- TOML o JSON
    creada_en   TEXT    NOT NULL,
    creada_por  TEXT    NOT NULL,
    PRIMARY KEY (slug, version)
);
