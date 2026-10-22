# CLAUDE.md

Orquestador en Rust (repo público `jesus13g/gachapon`) que reparte el trabajo
entre dos roles: el **planificador** (formaliza y revisa; por defecto Claude
Code) y el **ejecutor** (ejecuta; por defecto Qwen Code sobre `llama-server`).
Desde la constitución 1.3.0 (1.3.1 con las sesiones interactivas de la biblioteca) el agente de cada rol es intercambiable (Claude
Code, Qwen Code, Codex, OpenCode, Antigravity; spec 017). El usuario puede
despertar **equipos** de varios agentes que trabajan a la vez, con colas por
agente, worktree propio y presencia (spec 019, constitución 1.4.0). Se comunican
por un bus de ficheros con colas y semáforos. Visión general en `README.md`; diseño original en
`orquestador-claude-qwen.md`.

## Fuente de verdad: la documentación SDD

La documentación vigente está fuera del repo, en
`~/Obsidian/Ecosistema/claude-code/ecosistema/`:

- `estado-actual.md`: backlog priorizado, discrepancias y preguntas abiertas.
  **Léelo primero al retomar.**
- `constitucion.md`: principios no negociables (artículos I-XII).
- `specs/NNN-*/{spec,plan,tareas}.md`: una spec por área (bus, ejecutor,
  misión/obra, rutinas, skills, herramientas, bóveda, diario, monitor, banco…).

Un cambio de comportamiento empieza editando la spec (y su `tareas.md`), no el
código. Esas notas son de la bóveda: se escriben con `ea_escribir_nota` (escritor
único, `ea-boveda`), no editando el fichero a mano.

## Comandos

```bash
cargo build --release      # binarios en target/release/{ea,ea-mcp,ea-orchestrator,ea-web}
cargo test --workspace     # toda la suite (unas 380 pruebas)
cargo test -p ea-bus       # un solo crate
systemctl --user restart ea-web ea-orchestrator ea-mcp   # tras compilar en release (Linux)
./target/release/ea servicios ver|instalar|desinstalar   # unidades con la ruta real: systemd --user, launchd (macOS) o schtasks (Windows)
```

`ea` no está en el `PATH`: usa `./target/release/ea`. Los servicios `systemd
--user` ejecutan directamente los binarios de `target/release/`, así que un
`cargo build --release` cambia lo que corre en cuanto se reinician.

## Mapa del workspace

| Crate | Qué es |
|---|---|
| `ea-ajustes` | Ajustes: `ajustes.toml` (más los TOML heredados), registro de claves con quién las cambia, validación y escritura atómica; base del grafo, no depende de ningún `ea-*` |
| `ea-biblioteca` | Biblioteca de herramientas (spec 018): inventario de solo lectura de skills, plugins, subagentes, MCP, comandos y extensiones de los cinco agentes y del ecosistema, uso por actor, planes de activar/desactivar y de sesión efímera; solo depende de `ea-ajustes` |
| `ea-plataforma` | Todas las diferencias entre Linux, macOS y Windows (rutas, procesos, ficheros privados, terminal, servicios); sin dependencias `ea-*`; ningún otro crate usa `libc`, `std::os::unix` ni `cfg(target_os)` |
| `ea-store` | SQLite: diario, propuestas, índice de la bóveda, trabajos, fases, rutinas. Una sola migración (`0001_inicial.sql`) |
| `ea-bus` | Bus de ficheros: colas, estado de trabajos (máquina de estados pura), semáforo `flock`, rutinas TOML, `ConfigOrquestador` (la parte de los ajustes que usa el ejecutor) |
| `ea-orchestrator` | Ejecutor: toma de la cola `modelo-local`, lanza Qwen en el sandbox del sistema, corre el criterio y entrega el informe |
| `ea-acciones` | Catálogo único de herramientas con riesgo, simulación, revalidación y flujo propuesta → aceptación → ejecución |
| `ea-boveda` | La bóveda de notas; **único código que escribe en ella** |
| `ea-cli` | Binario `ea`: CLI y TUI (`ratatui`) |
| `ea-mcp` | Servidor MCP HTTP en `127.0.0.1:8765` |
| `ea-web` | API JSON y panel en `127.0.0.1:8092`; `static/` va embebido con `include_str!` (recompilar para ver cambios) |

Fuera de `crates/`: `claude/skills/` (skills `/mision`, `/obra`, `/rutina`,
`/revisar`, `/equipo`, enlazadas desde `~/.claude/skills/`: editarlas cambia las skills
vivas), `claude/hooks/` (envoltorio del hook de subagentes; el trabajo lo hace `ea hook subagentes`), `qwen/skills/`
(skills cortas de Qwen) y `bench/` (banco de pruebas en Python, ajeno a los
binarios).

Estado en ejecución: ajustes en `~/.config/ecosistema-asistente/ajustes.toml`
(`$EA_CONFIG_DIR`; `ea ajustes ver` dice el origen de cada valor), base de datos
en `~/.local/share/ecosistema-asistente/` (`$EA_DB_PATH`), bus en
`~/.local/state/ecosistema-asistente/` (`$EA_ESTADO_DIR`), bóveda según
`$EA_BOVEDA_DIR`, `ajustes.toml` o el heredado `boveda.toml`.

## Reglas que el código hace cumplir (no las rompas)

- **Ningún agente se confirma a sí mismo.** Lo mutante se simula o se propone;
  solo el usuario acepta. Lo irreversible exige una terminal real (TTY). `ea-mcp`
  y el orquestador solo reciben la capa `SoloConsulta`. No añadas atajos.
- **El criterio de éxito lo ejecuta el sistema**, nunca el agente; un criterio
  vacío no es éxito.
- **Los modelos se configuran como perfiles en `ajustes.toml`** (o los
  heredados `modelo.*` y `orquestador.toml`); en el código, solo en los valores
  por defecto de `ea-ajustes`. El modelo del ejecutor tiene que ser local; uno
  remoto solo vale para el planificador.
- **Un agente no ocupa los dos roles**: planificador ≠ ejecutor.
- **El diario es append-only a nivel de motor** (triggers): un error se corrige
  con otro evento. El estado del bus se escribe con `fs_atomico`.
- **Solo local**: todo escucha en `127.0.0.1`; `ea-web` filtra el `Host`.
- **El ejecutor**, sea cual sea el agente: el sandbox del sistema (`bwrap` en Linux, Seatbelt en
  macOS; Windows nativo se niega con `sandbox = true`; esto depende de la
  enmienda 1.5.0, propuesta y aún sin aprobar), topes duros, sin
  herramientas web, aprobación automática solo dentro del sandbox y semáforo
  exclusivo del worktree.
- Si un cambio choca con un artículo de la constitución, se enmienda la
  constitución primero; no se hace una excepción callada.

## Convenciones

- **Todo en español**: identificadores, mensajes, CLI, commits y documentación
  (p. ej. `trabajo`, `orden`, `informe`, `semaforo`). Commits con frase
  descriptiva en español, sin prefijos tipo `feat:`.
- Rust edition 2024, toolchain stable. Las bibliotecas definen errores con
  `thiserror` (`error.rs` por crate); los binarios (`ea-cli`, `ea-mcp`,
  `ea-orchestrator`, `ea-web`) usan `anyhow`.
- Las pruebas de integración aíslan el estado en directorios de `tempfile`
  (bus vía `EA_ESTADO_DIR`, base de datos y bóveda temporales); nunca toques el
  estado real del usuario desde una prueba.
- Esquema de base de datos: no hay migración desde bases antiguas; una base del
  esquema anterior se rechaza. Pregunta antes de añadir migraciones.
- **`ea-web`**: app minimalista (raíl lateral, cabecera, filas con detalle en
  línea) con el estilo de la escena gashapon. Los cambios visuales parten de
  `app.css`/`gashapon.css`. **La escena de Inicio (`gashapon.js`) no se toca.**
- Al cambiar comandos, API o comportamiento visible, actualiza el `README.md`.

## Git

- `main` estable (no se hacen commits directos), `desarrollo` de integración;
  cada mejora sale de `desarrollo` en su rama y vuelve a ella.
  `banco-de-pruebas` es la rama del banco de modelos (`bench/`).
- El repo es público: no cambies `user.email` (es el noreply de GitHub) y revisa
  que no haya secretos antes de cada push.
- Pregunta antes de fusionar ramas o retirar piezas del sistema.

## Trabajar dentro del ecosistema

El propio repo se puede trabajar con el flujo que implementa: si una tarea se
puede escribir como pasos con un criterio comprobable, se lanza al agente local
con `/mision` u `/obra` y se revisa con `/revisar`. Úsalo cuando el usuario lo
pida; por defecto, en este repo trabaja directamente.
