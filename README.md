# ecosistema-asistente

Implementación en Rust de la especificación del ecosistema de asistente dual
(`~/especificacion-ecosistema-asistente.md`).

## Estado actual

Persistencia y observabilidad: base de datos embebida, diario de eventos
append-only, primitiva de tarea, interfaz de terminal (`ea`). Un servidor de
herramientas MCP (`ea-mcp`) que Claude Code consume por HTTP. Un proxy de
inferencia local (`ea-proxy`) que resuelve peticiones contra el modelo local
(Qwen2.5-Coder-7B vía `llama-server`) con generación acotada por gramática.
Un orquestador (`ea-orchestrator`) que ejecuta rutinas declarativas
disparadas por `systemd --user timers`, de solo lectura, encolando una
propuesta en cada punto donde haría falta mutar. Voz con activación manual:
el propio orquestador escucha, transcribe, pregunta al modelo local y habla
la respuesta. Sin palabra de activación ni mutaciones todavía. Un panel web
(`ea-web`) para ver y gestionar todo desde el escritorio.

## Estructura

- `crates/ea-store`: capa de datos compartida (SQLite embebido). Único punto
  de acceso a la base de datos; el resto de crates la reutilizan sin
  reimplementar esquema.
- `crates/ea-cli`: binario `ea`, interfaz de terminal.
- `crates/ea-mcp`: binario `ea-mcp`, servidor de herramientas MCP (HTTP,
  `127.0.0.1`) consumido por Claude Code y por `ea-proxy`/`ea-orchestrator`.
- `crates/ea-proxy`: binario `ea-proxy`, proxy de inferencia local (HTTP,
  `127.0.0.1`) — el único punto que habla con `llama-server`.
- `crates/ea-orchestrator`: binario `ea-orchestrator`, ejecutor de rutinas y
  voz embebida (activación manual).
- `crates/ea-web`: binario `ea-web`, API JSON y panel web local (HTTP,
  `127.0.0.1`).

## Uso

```bash
cargo build --release
# los binarios quedan en target/release/{ea,ea-mcp,ea-proxy,ea-orchestrator,ea-web}

ea status                                    # resumen no interactivo
ea tareas crear --titulo "..." --asignado-a claude-code --solicita "..."
ea tareas listar
ea tareas ver <id>
ea tareas actualizar <id> --estado en_progreso
ea diario listar
ea diario ver <id>
ea rutinas listar
ea rutinas ver <slug>
ea propuestas listar [--estado pendiente]
ea propuestas decidir <id> --aceptar|--rechazar
ea voz hablar                                # activación manual (ver más abajo)
ea tui                                       # interfaz interactiva
```

La base de datos vive en `~/.local/share/ecosistema-asistente/ecosistema.db`
(o en la ruta de `$EA_DB_PATH` si está definida).

### Servidor de herramientas MCP (`ea-mcp`)

```bash
./target/release/ea-mcp        # arranque manual, escucha en 127.0.0.1:8765
# puerto configurable con $EA_MCP_PORT
```

Herramientas expuestas: `ea_status`, `ea_listar_tareas`, `ea_ver_tarea`,
`ea_listar_diario` (consulta) y `ea_crear_tarea` (con `simular: true` para
previsualizar sin escribir). Toda invocación queda registrada en el diario,
atribuida al cliente que la hizo: `claude-code` por defecto, `modelo-local`
si el cliente se identifica como `ea-proxy`, `orquestador` si se identifica
como `ea-orchestrator`.

Instalación como servicio de sesión (`systemd --user`):

```bash
mkdir -p ~/.config/systemd/user
ln -sf ~/Proyectos/ecosistema-asistente/crates/ea-mcp/systemd/ea-mcp.service \
    ~/.config/systemd/user/ea-mcp.service
systemctl --user daemon-reload
systemctl --user enable --now ea-mcp.service
```

Registro en Claude Code (disponible en cualquier sesión, no solo en este
directorio):

```bash
claude mcp add --transport http --scope user ea-ecosistema http://127.0.0.1:8765/mcp
```

### Proxy de inferencia local (`ea-proxy`)

```bash
./target/release/ea-proxy      # arranque manual, escucha en 127.0.0.1:8090
# puerto configurable con $EA_PROXY_PORT; resto de opciones en
# ~/.config/ecosistema-asistente/proxy.toml (perfiles, endpoints)
```

Dos endpoints:

- `POST /v1/chat/completions`: forma de chat completion de OpenAI. Resuelve
  hasta 3 llamadas acotadas por gramática (clasificar → elegir herramienta →
  extraer argumentos) contra el catálogo reducido de solo lectura, y ejecuta
  contra `ea-mcp`.
- `POST /v1/rama`: punto de decisión de una rutina — `{contexto, opciones}`
  → `{rama}`, una elección acotada por gramática entre un conjunto cerrado.
  Lo usa `ea-orchestrator`; ningún otro proceso habla con `llama-server`
  directamente.

Instalación como servicio de sesión:

```bash
ln -sf ~/Proyectos/ecosistema-asistente/crates/ea-proxy/systemd/ea-proxy.service \
    ~/.config/systemd/user/ea-proxy.service
systemctl --user daemon-reload
systemctl --user enable --now ea-proxy.service
```

### Orquestador de rutinas (`ea-orchestrator`)

Sondea la cola de tareas por encargos de rutina (`assigned_to=orquestador`),
ejecuta el grafo de pasos declarativo correspondiente, y emite un heartbeat
periódico en el diario (`actor=orquestador, action=heartbeat` — es lo que
`ea status` usa para reportar si está vivo).

Una **rutina** es un fichero TOML de usuario en
`~/.config/ecosistema-asistente/rutinas/<slug>.toml` (ruta configurable en
`orchestrator.toml`). Grafo de pasos lineal con ramificación, sin bucles
(el ejecutor los detecta y aborta si aparecen): `accion` (invoca una de las
cuatro herramientas de solo lectura), `decision` (el modelo local elige una
rama de un conjunto cerrado, vía `ea-proxy`), `propuesta` (encola una
propuesta en vez de actuar — el único mecanismo desatendido para "esto
necesita al usuario"), `fin`. Presupuestos duros por ejecución
(`max_duracion_s`, `max_pasos`, `max_llamadas_modelo`) hacen el bucle
infinito imposible, no improbable. Ver
`crates/ea-orchestrator/rutinas-ejemplo/revision-diaria.toml` para un
ejemplo completo y comentado.

```bash
mkdir -p ~/.config/ecosistema-asistente/rutinas
cp crates/ea-orchestrator/rutinas-ejemplo/revision-diaria.toml \
    ~/.config/ecosistema-asistente/rutinas/

./target/release/ea-orchestrator   # arranque manual
ea rutinas encolar revision-diaria # lo mismo que hace el disparador temporal
```

Instalación como servicio de sesión:

```bash
ln -sf ~/Proyectos/ecosistema-asistente/crates/ea-orchestrator/systemd/ea-orchestrator.service \
    ~/.config/systemd/user/ea-orchestrator.service
systemctl --user daemon-reload
systemctl --user enable --now ea-orchestrator.service
```

**Disparador temporal** (`systemd --user timer`): el disparador nunca
ejecuta la rutina, solo encola una tarea que el orquestador recoge cuando
puede. Cada rutina programada necesita su propio horario, así que
`ea-rutina@.timer` es una plantilla que se copia como fichero de instancia
concreta:

```bash
cp crates/ea-orchestrator/systemd/ea-rutina@.service ~/.config/systemd/user/
cp crates/ea-orchestrator/systemd/ea-rutina@.timer \
    ~/.config/systemd/user/ea-rutina@revision-diaria.timer
# editar OnCalendar= en ese fichero de instancia, luego:
systemctl --user daemon-reload
systemctl --user enable --now ea-rutina@revision-diaria.timer
```

Sin recuperación de ejecuciones perdidas (a propósito: la máquina arranca en
Windows buena parte del tiempo, y recuperar produciría avalanchas de
ejecuciones acumuladas compitiendo con el arranque del escritorio). Cada
rutina recibe como `{ultima_ejecucion}` la fecha de su última ejecución con
éxito, para que revise la ventana real transcurrida.

Como el equipo rara vez está en Linux a la hora fijada, el timer añade
`OnStartupSec=10min`: una única ejecución por arranque de sesión, retrasada
para no competir con el escritorio. No recupera nada (`Persistent` sigue en
`false`): si ya hubo ejecución a las 08:00, la del arranque simplemente se
suma, y la rutina ajusta su ventana con `{ultima_ejecucion}`.

### Voz (activación manual)

Corre embebida dentro de `ea-orchestrator` (§6.3 de la especificación: el
proceso que posee el dispositivo de audio es el que decide cuándo hay voz),
sobre `sherpa-onnx` — un único runtime para reconocimiento, síntesis y
detección de actividad vocal, en CPU (la GPU queda reservada al modelo de
lenguaje). `ea voz hablar` no captura ni reproduce nada por sí misma: le
pide al orquestador, por una superficie HTTP local nueva
(`127.0.0.1:8091` por defecto, solo `POST /v1/voz/hablar`), que escuche.

**Ficheros de modelo** — se descargan a mano (sin script de instalación en
esta fase, mismo criterio que el modelo de `llama-server`) bajo
`~/.local/share/ecosistema-asistente/voz/`:

| Fichero (nombre por defecto) | Qué es | De dónde |
|---|---|---|
| `whisper-small-encoder.onnx`, `whisper-small-decoder.onnx`, `whisper-small-tokens.txt` | ASR, Whisper-small multilingüe | `csukuangfj/sherpa-onnx-whisper-small` (Hugging Face) |
| `vits-coqui-es-css10.onnx`, `vits-coqui-es-css10-tokens.txt` | TTS en español, VITS | `csukuangfj/vits-coqui-es-css10` (Hugging Face) |
| `silero_vad.onnx` | Detección de actividad vocal | releases de modelos VAD de `k2-fsa/sherpa-onnx` (GitHub) |

Si los nombres reales de los ficheros que descargues no coinciden con los
de la tabla (varía según el build concreto), ajústalos en
`~/.config/ecosistema-asistente/voz.toml` — mismo mecanismo de
configuración-con-defaults que `orchestrator.toml`/`proxy.toml`. Si algún
fichero obligatorio falta, `ea-orchestrator` arranca igual (rutinas y
heartbeat siguen funcionando) pero `/v1/voz/hablar` responde 503 hasta que
se resuelva.

```bash
ea voz hablar
# "Escuchando…" — habla; el VAD corta sola la grabación tras el silencio
# (tope duro de 15s). Crea una tarea (origin=voz, asignada a modelo-local),
# la responde vía ea-proxy (POST /v1/chat/completions, el mismo endpoint de
# "tareas más abiertas" de la fase 3) y habla la respuesta.
```

Si no se detecta voz (o la transcripción sale vacía) no se crea tarea: se
habla un aviso breve y queda un evento `voz_transcripcion_descartada` en el
diario — la mitigación de §6.6 contra texto inventado en silencios.

### Interfaz web (`ea-web`)

Panel local en `http://localhost:8092` (solo `127.0.0.1`; puerto con
`$EA_WEB_PORT`) para ver y gestionar tareas, diario, rutinas, propuestas,
servicios y agentes. Una sola página sin dependencias, minimalista y a base
de símbolos, que toma los colores del tema activo de Omarchy
(`~/.local/state/omarchy/current/theme/colors.toml`; si falta, City 783).

Respeta el invariante de §12.2 de la especificación: lee del diario y de las
tareas, y actúa creando tareas o decidiendo propuestas; nunca llama a Claude
Code ni al modelo local. La voz se reenvía al orquestador, dueño del audio. Es
una desviación consciente de §12.1 y §12.4 ("sin interfaz web", "no se levanta
al arrancar"). Aceptar una propuesta solo deja constancia de la decisión, como
`ea propuestas decidir`: aún no existen herramientas mutantes que la ejecuten.

| Símbolo | Sección | Tecla |
|---|---|---|
| ▤ | Tareas: crear, iniciar, completar, fallar, cancelar | `1` |
| ≡ | Diario (♡/♥ muestra u oculta los latidos) | `2` |
| ↻ | Rutinas: pasos, ejecuciones, encolar | `3` |
| ◇ | Propuestas: aceptar ✓ o rechazar ✕ | `4` |
| ▦ | Servicios (reiniciar ↻, pide confirmar), orquestador y agentes | `5` |

`n` tarea nueva, `/` filtra, `j`/`k` mueve, `Enter` abre o cierra el detalle,
`Esc` cierra. `◉` pide al orquestador que escuche.

Instalación como servicio de sesión:

```bash
ln -sf ~/Proyectos/ecosistema-asistente/crates/ea-web/systemd/ea-web.service \
    ~/.config/systemd/user/ea-web.service
systemctl --user daemon-reload
systemctl --user enable --now ea-web.service
```

Abierta en el escritorio 4 desde el inicio de sesión (Hyprland). Se abre como
`localhost`, no `127.0.0.1`, porque esa clase de ventana ya la usa el tutor:

```lua
-- ~/.config/hypr/hyprland.lua
o.window("^chrome-localhost__-Default$", { workspace = "4 silent" })
-- ~/.config/hypr/autostart.lua
o.exec_on_start(os.getenv("HOME") .. "/Proyectos/ecosistema-asistente/crates/ea-web/bin/ea-web-open")
```

La máquina arranca con la hora desajustada tras Windows hasta que NTP la
corrige, así que el panel mide el tiempo activo de los servicios con el reloj
monotónico e ignora latidos del orquestador fechados en el futuro.

## Verificación

```bash
cargo test          # incluye ea-store, ea-mcp, ea-proxy, ea-orchestrator, ea-web
cargo test -- --ignored   # además, los tests de extremo a extremo contra
                           # el llama-server real (requiere el servicio arriba)

ea tareas crear --titulo "Probar mecanismo" --asignado-a claude-code \
    --solicita "Verificar que el diario registra el ciclo de vida" \
    --origen manual --creado-por cli-test
ea diario listar --tarea 1
ea tareas actualizar 1 --estado en_progreso
ea tareas actualizar 1 --estado completada --resultado "Ciclo verificado"
ea tareas ver 1
ea status

# El diario es append-only a nivel de motor, no solo por convención:
sqlite3 ~/.local/share/ecosistema-asistente/ecosistema.db \
    "DELETE FROM events WHERE id = 1;"
# -> error del trigger ABORT
```
