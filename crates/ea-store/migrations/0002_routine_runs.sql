-- Fase 4: rutinas y programación (§9.2, §10.2, §11.2, §12.3 de la
-- especificación). El hueco que 0001_initial.sql dejó reservado a propósito.

CREATE TABLE routine_runs (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    routine_slug      TEXT    NOT NULL,
    task_id           INTEGER REFERENCES tasks(id),
    started_at        TEXT    NOT NULL,
    finished_at       TEXT,
    outcome           TEXT    CHECK (outcome IN ('ok', 'fallida')),
    pasos_ejecutados  INTEGER NOT NULL DEFAULT 0,
    llamadas_modelo   INTEGER NOT NULL DEFAULT 0,
    detalle           TEXT
);

CREATE INDEX idx_routine_runs_slug         ON routine_runs(routine_slug);
CREATE INDEX idx_routine_runs_started_at   ON routine_runs(started_at);
-- Resuelve §10.2 ("última ejecución"): última fila con outcome='ok' para un slug.
CREATE INDEX idx_routine_runs_slug_outcome ON routine_runs(routine_slug, outcome, started_at);

-- §11.2 / §12.3: acciones mutantes que una rutina desatendida no ejecuta,
-- sino que encola para que el usuario decida al volver.
CREATE TABLE propuestas (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    routine_run_id  INTEGER NOT NULL REFERENCES routine_runs(id),
    creado_en       TEXT    NOT NULL,
    descripcion     TEXT    NOT NULL,
    estado          TEXT    NOT NULL DEFAULT 'pendiente'
                        CHECK (estado IN ('pendiente', 'aceptada', 'rechazada')),
    decidido_en     TEXT
);

CREATE INDEX idx_propuestas_estado        ON propuestas(estado);
CREATE INDEX idx_propuestas_routine_run   ON propuestas(routine_run_id);
