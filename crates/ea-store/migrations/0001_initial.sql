-- Fase 1: persistencia y observabilidad.
-- Reservado conceptualmente para fases futuras (no crear todavía):
--   0003_service_state.sql  -- estado de servicios residentes

PRAGMA foreign_keys = ON;

CREATE TABLE tasks (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at      TEXT    NOT NULL,
    updated_at      TEXT    NOT NULL,
    created_by      TEXT    NOT NULL,
    assigned_to     TEXT    NOT NULL,
    origin          TEXT    NOT NULL CHECK (origin IN ('manual', 'boton', 'voz', 'programado', 'agente')),
    parent_task_id  INTEGER REFERENCES tasks(id),
    title           TEXT    NOT NULL,
    request          TEXT    NOT NULL,
    status          TEXT    NOT NULL DEFAULT 'pendiente'
                        CHECK (status IN ('pendiente', 'en_progreso', 'completada', 'fallida', 'cancelada')),
    result          TEXT
);

CREATE INDEX idx_tasks_status      ON tasks(status);
CREATE INDEX idx_tasks_assigned_to ON tasks(assigned_to);
CREATE INDEX idx_tasks_parent      ON tasks(parent_task_id);
CREATE INDEX idx_tasks_created_at  ON tasks(created_at);

CREATE TABLE events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    ts          TEXT    NOT NULL,
    actor       TEXT    NOT NULL,
    action      TEXT    NOT NULL,
    task_id     INTEGER REFERENCES tasks(id),
    outcome     TEXT    CHECK (outcome IN ('ok', 'error')),
    duration_ms INTEGER,
    detail      TEXT
);

CREATE INDEX idx_events_ts      ON events(ts);
CREATE INDEX idx_events_task_id ON events(task_id);
CREATE INDEX idx_events_actor   ON events(actor);
CREATE INDEX idx_events_task_ts ON events(task_id, ts);

-- Diario de eventos append-only reforzado a nivel de motor (§8.2 de la especificación):
-- un evento erróneo se corrige emitiendo otro evento, nunca editando ni borrando.
CREATE TRIGGER events_no_update BEFORE UPDATE ON events
BEGIN
    SELECT RAISE(ABORT, 'events es append-only: corrige emitiendo un evento nuevo');
END;

CREATE TRIGGER events_no_delete BEFORE DELETE ON events
BEGIN
    SELECT RAISE(ABORT, 'events es append-only: corrige emitiendo un evento nuevo');
END;
