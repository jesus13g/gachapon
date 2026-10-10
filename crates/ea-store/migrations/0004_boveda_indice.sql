-- Fase 7: índice de búsqueda de la bóveda compartida. Es una caché derivada
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
