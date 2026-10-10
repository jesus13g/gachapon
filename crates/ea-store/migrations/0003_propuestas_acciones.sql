-- Fase 6: mutaciones supervisadas. Una propuesta puede llevar una acción
-- estructurada (herramienta + argumentos + riesgo + simulación) que se
-- ejecuta cuando el usuario la acepta, y puede venir de un agente (MCP) y no
-- solo de una rutina, así que `routine_run_id` deja de ser obligatorio.
-- SQLite no permite quitar un NOT NULL con ALTER: se reconstruye la tabla
-- copiando todas las filas existentes (las propuestas antiguas de texto
-- libre quedan intactas, con las columnas nuevas a NULL).

CREATE TABLE propuestas_nueva (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    routine_run_id       INTEGER REFERENCES routine_runs(id),
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

INSERT INTO propuestas_nueva (id, routine_run_id, creado_en, descripcion, estado, decidido_en)
    SELECT id, routine_run_id, creado_en, descripcion, estado, decidido_en FROM propuestas;

DROP TABLE propuestas;
ALTER TABLE propuestas_nueva RENAME TO propuestas;

CREATE INDEX idx_propuestas_estado        ON propuestas(estado);
CREATE INDEX idx_propuestas_routine_run   ON propuestas(routine_run_id);
