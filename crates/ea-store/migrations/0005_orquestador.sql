-- Fase 2 del orquestador: historial de trabajos. La fuente de verdad del
-- bus son los ficheros de estado; estas tablas son el historial consultable.

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
