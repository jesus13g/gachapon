# ecosistema-asistente

Orquestador en Rust que reparte el trabajo entre **Claude Code** (formaliza y
revisa) y un **agente local** (Qwen Code sobre `llama-server`, que ejecuta). Ambos
trabajan sobre el mismo worktree de git y se comunican por un bus de ficheros con
colas y semáforos. Diseño y razones en `orquestador-claude-qwen.md`.

[![Cómo funciona gachapon en 15 segundos](docs/media/gachapon-explicado.gif)](docs/media/gachapon-explicado.mp4)

*Cómo funciona en 15 segundos, sobre la interfaz `ea-web` ([vídeo en MP4](docs/media/gachapon-explicado.mp4)).*

## Estado actual

Funcionan el bus, el ejecutor, las skills de Claude Code, la CLI (`ea`), el
monitor (TUI y `ea-web`), el servidor de herramientas MCP, la bóveda compartida y
las mutaciones supervisadas. El agente local es hoy Qwen3-Coder-30B-A3B.

### Roles

| | Claude Code | Agente local |
|---|---|---|
| Lanza el trabajo | Sí (y el calendario, para rutinas) | Nunca por su cuenta |
| Define qué se hace | Objetivo, plan o proceso | No |
| Ejecuta | Solo lo que exige criterio | Todo lo que se describe en pasos verificables |
| Revisa | Sí: cierra, reformula o continúa | No |

El criterio de éxito es un comando de shell que comprueba el **sistema**, no el
agente: Qwen no se da el visto bueno a sí mismo.

### Los tres modos

| | `/mision` | `/obra` | `/rutina` |
|---|---|---|---|
| Para qué | Alcanzar un objetivo iterando | Implementar un plan por fases | Repetir un proceso conocido |
| Vuelve a Claude Code | Al terminar o quedar bloqueado | Al terminar, o cada `revisar_cada` fases | Solo si falla y tú lo confirmas |
| Se guarda | Orden e informe | Plan, fases, diffs por fase | Definición TOML versionada |

Una rutina que falla deja un **paquete de corrección** en espera. Hasta que no lo
confirmas (`ea trabajo confirmar` o el botón de `ea-web`, con contexto opcional) no
llega a Claude Code, que entonces corrige la definición y sube la versión.

### El bus

Ficheros en `~/.local/state/ecosistema-asistente/` (o `$EA_ESTADO_DIR`); SQLite
guarda el historial y el diario.

```
colas/modelo-local/       órdenes por ejecutar (un .json por entrada)
colas/claude-code/        trabajos entregados que esperan revisión
trabajos/<id>/            orden.json, estado.json, informe.json,
                          historial.jsonl, paquete-correccion.md (rutinas)
semaforos/                un .lock por worktree: quién lo tiene y desde cuándo
```

Estados de un trabajo: `en_cola` → `ejecutando` → `en_revision` → `cerrado`
(o `en_espera` para el paquete de una rutina, y `cancelado`). Las transiciones las
valida una máquina de estados pura (`ea-bus`).

### El ciclo del ejecutor

`ea-orchestrator` sondea la cola `modelo-local`, toma el trabajo más antiguo,
adquiere el semáforo del worktree (`flock` exclusivo) y lanza Qwen Code en modo
headless. Después **ejecuta él mismo el criterio**. En una misión repite la
llamada con la evidencia del fallo hasta que el criterio pasa o se agotan las
iteraciones; en una obra recorre las fases, guarda una instantánea del árbol tras
cada una y se detiene cada `revisar_cada`. Entrega un informe (resultado, motivo,
evidencia, diff stat, fases, `obra_completa`) y encola el trabajo en `claude-code`.
Una rutina que pasa se cierra sola. Al arrancar recupera lo que se quedó a medias.
Los fallos, bloqueos y paquetes en espera avisan con `notify-send`; el aviso
lleva una acción **abrir** que lanza `ea-web-open '#trabajos/<id>'` (el proceso
del aviso va aparte: el ejecutor no lo espera).

Cancelar un trabajo `ejecutando` (`ea trabajo cancelar`, o «devolver» en la web)
lo detiene de verdad: el ejecutor relee el estado antes de cada iteración y de
cada fase y, mientras corre Qwen, lo sondea cada 2 s; el agente va en su propio
grupo de procesos, que recibe `SIGTERM` y, a los 5 s, `SIGKILL`. Tras cancelar no
se corre el criterio ni hay informe: se suelta el semáforo y el diario apunta
`bus_cancelado_en_curso`. **Lo que el agente ya escribió en el worktree no se
deshace.** Mientras el semáforo siga cogido, la web muestra el trabajo como
«cancelando…» (`cancelando: true` en `GET /api/trabajos`).

Reformular una obra la rehace desde una fase: la que se pida (`--fase N`, o
`fase` en la web) o, si no, la pendiente (`fase_actual`; en una obra completa,
la última). Esa fase y las siguientes vuelven a quedar pendientes (`fase_actual`,
`fases-informe.json` y las fases de SQLite); las anteriores se conservan. El
worktree no se revierte: el agente rehace sobre lo que hay, y los diffs por fase
y de revisión parten del último árbol guardado. Una fase que la obra no tiene, o
una fase en una misión o rutina, se rechaza sin escribir nada.

### Seguridad

Qwen Code corre dentro de **bubblewrap** (`bwrap`): sistema de ficheros de solo
lectura salvo el worktree, un `/tmp` propio y `~/.qwen`, con `$XDG_RUNTIME_DIR`
tapado por un tmpfs vacío (ahí están la clave de sesión del panel web y los
sockets de D-Bus y `systemd --user`); la red queda abierta para llegar a
`llama-server`. Con `sandbox = false` el agente corre con aprobación
total y puede escribir donde escriba tu usuario. Además: topes por ejecución
(turnos, tiempo, llamadas a herramientas, iteraciones; recortados al techo de los
ajustes), exclusión de las
herramientas web y semáforo para que los dos agentes no escriban a la vez.

### Configuración (`ajustes.toml`)

Un único fichero, `~/.config/ecosistema-asistente/ajustes.toml` (o
`$EA_CONFIG_DIR/ajustes.toml`), todo opcional (plantilla comentada en
`crates/ea-ajustes/config/ajustes.example.toml`). Lo lee `ea-ajustes`, la base de
todos los crates. Precedencia **por clave**: variable de entorno >
`ajustes.toml` > TOML heredado > valor por defecto. Un valor inválido (fuera de
rango, una URL que no es local, un fichero que no es TOML) deja un aviso en
`ea ajustes ver` y en ⚙ Ajustes y cae a la capa siguiente: nunca impide arrancar.

Los ficheros antiguos se siguen leyendo, como fuentes heredadas de solo lectura
(el sistema nunca los escribe): `orquestador.toml` (`modelo` → `modelo.nombre`,
`base_url`, `qwen_bin`, `sandbox`, `avisos_escritorio` y las secciones `[topes]`
→ `agente.techo_topes` y `[skills]` → `agente.skills`), `orchestrator.toml`
(`intervalo_sondeo_s`, `intervalo_heartbeat_s`, `rutinas_dir` → `orquestador.*`) y
`boveda.toml` (`raiz` → `rutas.boveda`). El sistema solo escribe `ajustes.toml`
(de forma atómica y conservando comentarios y claves ajenas).

| Clave | Defecto | Rango o forma | Entorno | Quién la cambia | Aplica |
|---|---|---|---|---|---|
| `modelo.nombre` | `qwen3-coder-30b` | texto | | web y terminal | en caliente |
| `modelo.base_url` | `http://127.0.0.1:8080/v1` | solo `127.0.0.1`, `localhost` o `[::1]` | | web y terminal | en caliente |
| `modelo.qwen_bin` | `qwen` | texto | | solo terminal | en caliente |
| `agente.sandbox` | `true` | sí/no | | solo terminal | en caliente |
| `agente.avisos_escritorio` | `true` | sí/no | | web y terminal | en caliente |
| `agente.techo_topes.turnos` | 100 | 1–100 | | web solo baja; terminal | en caliente |
| `agente.techo_topes.tiempo_s` | 3600 | 60–3600 | | web solo baja; terminal | en caliente |
| `agente.techo_topes.llamadas_herramienta` | 200 | 1–200 | | web solo baja; terminal | en caliente |
| `agente.techo_topes.iteraciones` | 10 | 1–10 | | web solo baja; terminal | en caliente |
| `agente.skills.dir` | `<config>/qwen-skills` | ruta (`~`) | | solo terminal | en caliente |
| `agente.skills.habilitadas` | `[]` (todas) | lista | | solo terminal | en caliente |
| `agente.skills.max_por_trabajo` | 3 | 0–5 | | web solo baja; terminal | en caliente |
| `agente.skills.max_caracteres` | 6000 | 0–12000 | | web solo baja; terminal | en caliente |
| `agente.skills.pueden_elegir` | `["claude-code"]` | ⊆ `["claude-code"]` | | solo terminal | en caliente |
| `orquestador.intervalo_sondeo_s` | 5 | 1–300 | | web y terminal | reiniciar `ea-orchestrator` |
| `orquestador.intervalo_heartbeat_s` | 30 | 5–600 | | web y terminal | reiniciar los tres servicios |
| `orquestador.rutinas_dir` | `<config>/rutinas` | ruta | | solo terminal | reiniciar `ea-web`, `ea-mcp` |
| `red.puerto_web` | 8092 | 1024–65535 | `EA_WEB_PORT` | web y terminal | reiniciar `ea-web` |
| `red.puerto_mcp` | 8765 | 1024–65535 | `EA_MCP_PORT` | web y terminal | reiniciar `ea-mcp` (y cambiar la URL del MCP en Claude Code) |
| `rutas.db` | `~/.local/share/ecosistema-asistente/ecosistema.db` | ruta | `EA_DB_PATH` | solo terminal | reiniciar todo |
| `rutas.estado` | `~/.local/state/ecosistema-asistente` | ruta | `EA_ESTADO_DIR` | solo terminal | reiniciar todo (el hook de subagentes solo lee la variable) |
| `rutas.boveda` | `~/.local/share/ecosistema-asistente/boveda` | ruta | `EA_BOVEDA_DIR` | solo terminal | reiniciar `ea-web`, `ea-mcp` |
| `web.sondeo_ms` | 3000 | 1000–60000 | | web y terminal | en caliente (la página) |
| `web.diario_limite` | 300 | 10–1000 | | web y terminal | en caliente (la página) |

`<config>` es `~/.config/ecosistema-asistente` (o `$EA_CONFIG_DIR`, pensado sobre
todo para aislar pruebas). `red.puerto_web` y `red.puerto_mcp` no pueden
coincidir; el host no se configura (siempre `127.0.0.1`). Una clave que fija su
variable de entorno no se cambia desde ningún canal mientras esté definida.

**Techo de topes.** Los topes de cada orden (`turnos`, `tiempo_s`,
`llamadas_herramienta`, `iteraciones`; por defecto 30/900/60/5) los pone quien la
escribe. El ejecutor los recorta a `agente.techo_topes` antes de lanzar a Qwen
(mínimo por campo, al menos una iteración) y lo deja en el log. El techo nunca
pasa de 100/3600/200/10. Un `[topes]` en un `orquestador.toml` antiguo pasa a ser
ese techo.

**Quién cambia qué.** `ea ajustes establecer` exige una terminal interactiva real
(sin TTY se rechaza, como las acciones irreversibles). ⚙ Ajustes en `ea-web` es un
gesto directo del usuario, como el editor de Memoria: solo llega a las claves
marcadas «web», los techos y topes de skills solo bajan desde ahí, pide un
segundo clic y comprueba la versión del fichero (409 si cambió). Lo que toca la
seguridad del agente local (sandbox, binario de Qwen, quién elige skills, rutas)
es solo de terminal. Cada cambio queda en el diario como `ajustes_cambiados`
(actor `usuario`, con el canal y los valores antes y después). Ni `ea-mcp` ni el
orquestador tienen forma de leer o escribir los ajustes. Claude Code sí podría
editar `ajustes.toml` con su herramienta de ficheros, igual que antes
`orquestador.toml`.

```bash
ea ajustes ver [--json]                      # valor, origen, quién lo cambia y cuándo se aplica; avisos debajo
ea ajustes obtener red.puerto_web            # solo el valor
ea ajustes establecer agente.techo_topes.turnos=60 web.sondeo_ms=5000
ea ajustes establecer --quitar modelo.nombre # vuelve a mandar la capa siguiente
ea ajustes ruta                              # ajustes.toml y los heredados, con si existen
```

### Cambiar de modelo

`llama-server` lee `~/.config/llama-cpp/activo.env`, un enlace simbólico a un
`.env` por modelo:

```bash
ln -sf qwen3-coder-30b.env ~/.config/llama-cpp/activo.env
systemctl --user restart llama-server
```

Después ajusta el nombre (y la URL si cambia) con
`ea ajustes establecer modelo.nombre=<modelo>` (o desde ⚙ Ajustes). Se aplica en
caliente: el ejecutor relee los ajustes en cada ciclo. No hay que tocar código.

## Actualizar desde una versión anterior

El esquema de la base de datos se ha reiniciado (una sola migración,
`0001_inicial.sql`; sin tareas ni `routine_runs`). Una base antigua **no se
migra**: `ea` y los servicios se niegan a abrirla con el mensaje «base de datos
del esquema antiguo» y no tocan nada. Tras actualizar hay que empezar con una
base nueva: parar los servicios, copiar `~/.local/share/ecosistema-asistente/ecosistema.db*`
a un lugar seguro, borrarlos, vaciar `~/.local/state/ecosistema-asistente/` y
volver a arrancar los servicios.

## Estructura

- `crates/ea-ajustes`: los ajustes (`ajustes.toml` y los TOML heredados), con
  su registro de claves, validación y escritura atómica; no depende de ningún
  otro crate.
- `crates/ea-store`: capa de datos compartida (SQLite embebido): diario,
  propuestas, índice de la bóveda, trabajos, fases y rutinas.
- `crates/ea-bus`: el bus de ficheros: colas, estado de los trabajos, semáforo
  del worktree, rutinas versionadas y la configuración del ejecutor.
- `crates/ea-orchestrator`: binario `ea-orchestrator`, el ejecutor del agente
  local y el heartbeat.
- `crates/ea-acciones`: catálogo único de herramientas con riesgo, simulación,
  revalidación y el flujo propuesta → aceptación → ejecución.
- `crates/ea-boveda`: la bóveda de notas; único código que escribe en ella.
- `crates/ea-cli`: binario `ea`, CLI y TUI.
- `crates/ea-mcp`: binario `ea-mcp`, servidor MCP (HTTP, `127.0.0.1:8765`).
- `crates/ea-web`: binario `ea-web`, API JSON y panel web (`127.0.0.1:8092`).
- `claude/skills/`: las skills de Claude Code.
- `claude/hooks/`: los hooks de Claude Code que usa `ea-web`.
- `qwen/skills/`: las skills propias del agente local.

## Skills de Claude Code

Versionadas en `claude/skills/<nombre>/SKILL.md` y enlazadas en `~/.claude/skills/`.
Claude Code solo formaliza y revisa; el trabajo lo hace el agente local.

| Skill | Qué hace |
| --- | --- |
| `/mision` | Formaliza objetivo y criterio de éxito y crea el trabajo con `ea trabajo crear --orden` |
| `/obra` | Plan por fases pequeñas (instrucciones, entregable, criterio por fase) y `revisar_cada` |
| `/rutina` | Proceso repetible como TOML versionado: registrar, lanzar, timer y corrección |
| `/revisar` | Consume la cola `claude-code`: `ea trabajo ver`, `diff` si hace falta, y cierra, reformula o continúa |

```bash
for s in mision obra rutina revisar; do
  ln -sfn "$PWD/claude/skills/$s" ~/.claude/skills/$s
done
```

## Hook de subagentes

`ea-web` pinta en la portada cada sesión abierta de Claude Code con sus
subagentes. Las sesiones las lee de `~/.claude/sessions/`; los subagentes los
apunta `claude/hooks/ea-subagentes.sh` (`SubagentStart` / `SubagentStop`) en
`~/.local/state/ecosistema-asistente/subagentes/<sesión>/<agente>.json`.
Necesita `jq`.

```bash
ln -sfn "$PWD/claude/hooks/ea-subagentes.sh" ~/.claude/hooks/ea-subagentes.sh
# y en ~/.claude/settings.json, dentro de "hooks":
#   "SubagentStart": [{"matcher": "*", "hooks": [{"type": "command",
#     "command": "sh \"$HOME/.claude/hooks/ea-subagentes.sh\" start", "timeout": 5}]}],
#   "SubagentStop":  [{"matcher": "*", "hooks": [{"type": "command",
#     "command": "sh \"$HOME/.claude/hooks/ea-subagentes.sh\" stop", "timeout": 5}]}]
```

## Skills de Qwen

Qwen corre con `--bare`, así que no carga nada por su cuenta. Tiene una batería
propia de skills cortas (~150-250 tokens cada una) en `qwen/skills/<nombre>.md`:
`explorar`, `rust`, `python`, `tests`, `depurar`, `refactor`, `shell`,
`documentar` y `resumir`. Ninguna va activa por defecto. Cada trabajo lleva en su
orden el campo `skills` (y cada fase de una obra puede llevar las suyas), y el
orquestador inyecta solo esas en el system prompt de Qwen con
`--append-system-prompt`. El texto inyectado queda en
`trabajos/<id>/skills-qwen.md`.

Las elige Claude Code al formalizar (`/mision`, `/obra`). `[agente.skills]` de
`ajustes.toml` (o el heredado `[skills]` de `orquestador.toml`) decide qué se
puede elegir y quién:

- `habilitadas`: lista blanca del catálogo (vacía = todas).
- `max_por_trabajo` y `max_caracteres`: topes de contexto.
- `pueden_elegir`: la llave, los valores de `creada_por` autorizados (por defecto
  `claude-code`). Un trabajo con skills de otro origen se rechaza al crearlo y
  vuelve a validarse al ejecutarse: si no pasa, se bloquea sin llamar a Qwen.

La validación al crear (llave, existencia, habilitación y topes, en la orden y
en cada fase) la hace `Bus::crear_trabajo`, así que la heredan todos los puntos
de entrada (CLI, ea-web, rutinas) y una orden rechazada no deja nada en el bus.

```bash
ln -sfn "$PWD/qwen/skills" ~/.config/ecosistema-asistente/qwen-skills
ea qwen-skills listar              # catálogo, tamaños y descripciones
ea qwen-skills ver tests           # lo que recibe Qwen
ea qwen-skills probar rust,tests   # valida una selección (llave y topes)
ea trabajo crear --modo mision … --skills rust,tests --por claude-code
```

`.claude/settings.json` da permiso a Claude Code para `ea qwen-skills` y
`ea trabajo crear` sin preguntar. Una skill nueva es un `.md` más en
`qwen/skills/` con una línea `descripcion:` entre dos `---` al principio. Mantenla corta.

## Uso

```bash
cargo build --release
# los binarios quedan en target/release/{ea,ea-mcp,ea-orchestrator,ea-web}

# Trabajos (el bus)
ea trabajo crear --modo mision --titulo "..." --worktree <dir> --objetivo "..." --criterio "<comando>"
ea trabajo crear --orden orden.json          # obra completa, con fases
ea trabajo listar
ea trabajo ver <id> [--json]                 # orden, estado e informe
ea trabajo diff <id> [--fase N]              # diff desde commit_inicio
ea trabajo cerrar|cancelar <id>
ea trabajo reformular <id> --nota "..." [--fase N]  # vuelve al agente local (obra: desde la fase N)
ea trabajo continuar <id>                    # aprueba una revisión intermedia de una obra
ea trabajo confirmar <id> [--contexto "..."] # paquete de corrección -> claude-code
ea cola [modelo-local|claude-code] [--json]  # colas
ea semaforos                                 # quién tiene el worktree

# Rutinas versionadas
ea rutina registrar <fichero.toml>           # valida y registra (la versión debe subir)
ea rutina listar|ver <slug>
ea rutina lanzar <slug> [--entrada k=v]...   # crea el trabajo en el bus

# Resto del ecosistema
ea status                                    # orquestador, trabajos por estado, colas y semáforos
ea diario listar|ver <id>                    # listar: --trabajo, --actor, --desde, --hasta,
                                             #   --resultado ok|error, --accion <prefijo> (bus_, mcp_…)
ea propuestas listar|ver|decidir <id> --aceptar|--rechazar
ea acciones listar|simular|ejecutar          # catálogo de herramientas
ea boveda estado|listar|buscar|leer|promover|reindexar
ea ajustes ver|obtener|establecer|ruta       # ajustes.toml (establecer: solo con TTY)
ea tui                                       # interfaz interactiva
```

La base de datos vive en `~/.local/share/ecosistema-asistente/ecosistema.db`
(o en `rutas.db` de los ajustes, o en `$EA_DB_PATH`). Servicios de sesión (`systemd --user`): `llama-server`,
`ea-orchestrator`, `ea-mcp` y `ea-web`; las rutinas se programan con
`ea-rutina@<slug>.timer`, que solo crea el trabajo en el bus.

Las entradas de una rutina (`--entrada`, la ficha de la web o `entradas` en
`ea_encolar_rutina`) siguen una sola política, la de `Rutina::a_orden`: solo
las declaradas, como mucho 32, de 1024 bytes cada una y sin caracteres de
control (saltos de línea incluidos). Se sustituyen **tal cual**, también en
`criterio_exito`, que el sistema ejecuta con `sh -c`: una entrada es código de
shell si la rutina la pone en el criterio. Por eso solo las da el usuario (o
una propuesta que acepta tras ver el criterio resultante en la simulación).

Registrar sigue también una sola política (`ea_bus::comprobar_registro`), la
misma para `ea rutina registrar`, el editor de la web y `ea_registrar_rutina`:
el TOML no pasa de 64 KiB, es una rutina válida y su versión es mayor que la
registrada. Registrar guarda la versión en SQLite y copia el TOML a
`rutinas_dir/<slug>.toml`; no lanza nada. Tras un paquete de corrección,
Claude Code no pide la terminal: propone la vN+1 con `ea_registrar_rutina` (la
simulación enseña el diff y el criterio resultante) y el usuario la acepta en
◇ Propuestas, o la pega en «✎ nueva versión» de la ficha de la rutina.

### Servidor de herramientas MCP (`ea-mcp`)

```bash
./target/release/ea-mcp        # arranque manual, escucha en 127.0.0.1:8765
# puerto: red.puerto_mcp de los ajustes o $EA_MCP_PORT
```

Herramientas expuestas (un solo catálogo, `ea-acciones`), con su nivel de
riesgo (§7.3):

| Herramienta | Riesgo | Desde un agente |
|---|---|---|
| `ea_status` (orquestador, trabajos por estado, colas, semáforos), `ea_listar_trabajos`, `ea_ver_trabajo`, `ea_listar_diario` | lectura (el bus es solo de lectura desde MCP: crear o decidir trabajo es de la CLI y las skills) | se ejecuta |
| `ea_buscar_notas`, `ea_leer_nota` | lectura (resultados acotados por actor) | se ejecuta |
| `ea_escribir_nota` | mutación reversible | se ejecuta, solo en la región propia del agente |
| `ea_promover_nota` | mutación reversible | solo simula o propone |
| `ea_encolar_rutina` (crea el trabajo en el bus; `entradas` opcionales, y la simulación muestra el criterio resultante) | mutación reversible | solo simula o propone |
| `ea_registrar_rutina` (`toml`: la definición entera; la versión debe subir; la simulación muestra el diff frente a la registrada y el criterio resultante; no lanza nada) | mutación reversible | solo simula o propone |
| `ea_reiniciar_servicio` (lista cerrada: `llama-server`, `ea-mcp`, `ea-orchestrator`, `ea-web`) | mutación reversible | solo simula o propone |
| `ea_eliminar_rutina` | **mutación irreversible** | solo simula o propone; solo se confirma desde la terminal |

Toda invocación queda registrada en el diario, atribuida al cliente que la hizo
(`claude-code` por defecto, `modelo-local` si se identifica como Qwen Code —`qwen-code-mcp-client`—, `orquestador` si se identifica como `ea-orchestrator`).
La autorización es del servidor, no del prompt: lo desatendido solo puede leer.

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

### Mutaciones supervisadas (fase 6)

**La confirmación la da siempre el usuario, nunca un agente.** Una herramienta
mutante invocada por un agente (Claude Code por MCP) devuelve la **simulación** (`simular: true`: describe lo que haría sin
hacerlo) o, sin simular, **encola una propuesta con acción estructurada**
(herramienta, argumentos, riesgo, texto de la simulación y la precondición del
mundo en ese momento). No existe ninguna herramienta, argumento ni token con el
que un agente se confirme a sí mismo, y el código de `ea-mcp` y del orquestador
solo recibe una capa de *consulta* (`SoloConsulta`), que ni siquiera implementa
las operaciones que mutan el sistema.

Flujo propuesta → aceptación → ejecución:

1. Un agente propone (`ea_encolar_rutina`, `ea_promover_nota` …). Una propuesta idéntica
   pendiente no se duplica y hay un tope de 30 con acción.
2. El usuario la ve en `ea propuestas ver <id>`, en la TUI o en la sección ◇ de
   `ea-web`, con la simulación.
3. **Aceptar la ejecuta**: `ea propuestas decidir <id> --aceptar` (pide una
   terminal interactiva, muestra la simulación y exige confirmación; una
   irreversible exige escribir `confirmo <id>`) o el ✓ de `ea-web` (segundo clic
   para confirmar). Se reclama de forma atómica (aceptar dos veces no ejecuta
   dos veces), se **revalida** el estado contra la precondición (si cambió desde
   la simulación falla con un mensaje claro y no hace otra cosa) y se ejecuta
   por la capa interna. El resultado queda en la propuesta (`ejecutada` /
   `fallida`, con detalle) y en el diario (`propuesta_decidida` y
   `accion_ejecutada` / `accion_fallida`, con argumentos, resultado, proponente
   y canal).
4. Las propuestas de solo texto (sin acción) siguen funcionando: aceptar = constancia.

Política por canal (§11.1): lectura, automática; reversible, la confirma el
usuario en la terminal o en `ea-web`; irreversible, **solo desde la terminal**
(`ea-web` responde 403 y la propuesta sigue pendiente; MCP no tiene
camino de confirmación). La terminal exige TTY real en stdin y stdout: un
agente que lance `ea` como subproceso sin terminal no puede confirmar. Los
botones de `ea-web` (reiniciar servicios, decidir propuestas) pasan por la
misma capa y dejan el mismo `accion_ejecutada` en el diario. `ea acciones listar|simular|ejecutar` es la
vía directa del usuario en terminal.

Límite honesto: el modelo de seguridad delega en el sistema operativo la
frontera de *quién escribe en la terminal*; un proceso con la misma cuenta de
usuario y capacidad de simular un TTY podría saltarse la confirmación
reversible. La defensa es no darle a los agentes ningún camino por las
herramientas, los permisos de su propio entorno y que lo irreversible exija
escribir una frase en una terminal real. Las escrituras de `127.0.0.1:8092`
exigen la clave de sesión del panel, que el agente local no puede leer desde
`bwrap`; **eso no protege frente a Claude Code** ni frente a otro proceso sin
sandbox de tu usuario, que leen el fichero de la clave igual que `ea-web-open`.
Y la clave pasa por el perfil del navegador (historial, `sessionStorage`), que
está en `$HOME` y el agente local sí puede leer: es una fuga conocida.

### Bóveda compartida (fase 7)

Implementa §8.3: el sustrato de notas que se leen meses después, compartido
por el modelo local, Claude Code y el usuario.

**Dónde está.** La raíz es `rutas.boveda` de los ajustes y se resuelve en este
orden: la variable `EA_BOVEDA_DIR`, `ajustes.toml`, el heredado
`~/.config/ecosistema-asistente/boveda.toml` y, si no hay ninguno,
`~/.local/share/ecosistema-asistente/boveda/`. En esta instalación
`boveda.toml` apunta a la bóveda de Obsidian `Ecosistema` (se puede pasar a
`ajustes.toml` con `ea ajustes establecer rutas.boveda=~/Obsidian/Ecosistema`):

```toml
raiz = "~/Obsidian/Ecosistema"
```

Ahí conviven las regiones con notas del usuario fuera de ellas (como
`Inicio.md`) y la carpeta `.obsidian/`. El ecosistema solo ve, indexa y escribe
las tres subcarpetas que son regiones; todo lo demás le es invisible, pero crea
en la raíz las regiones, el fichero oculto `.ea-escritor.lock` y la papelera
`.papelera/`. Desde la spec 015 el punto de edición es la vista ▥ Bóveda de
`ea-web`, no Obsidian: la carpeta se puede seguir abriendo en Obsidian, pero
lo que se edite ahí no pasa por el escritor único ni deja rastro en el diario.

**Regiones** (partición por autor, no por tema; decisiones 8 y 9):

| Carpeta | Quién escribe | Contenido |
|---|---|---|
| `modelo-local/` | el modelo local, con libertad total | sus notas |
| `claude-code/` | Claude Code, con libertad total | sus notas |
| `compartida/` | ningún agente directamente: lo suyo solo llega por **promoción** que decide el usuario; el usuario crea y edita sus notas (`autor: usuario`) desde `ea-web` | lo de confianza |

Cada agente escribe únicamente en su carpeta: las herramientas no tienen
argumento de región y la ruta que dan es relativa a la suya. El orquestador
desatendido no escribe (§11.2: solo lectura). Cualquiera puede leer y buscar en
las tres regiones.

**Escritor único serializado** (decisión 10). Todo fichero de la bóveda se crea
o cambia en `ea-boveda` (`Boveda::escribir_nota` y `Boveda::promover`); ningún
otro componente toca los ficheros. Escriben procesos distintos (`ea-mcp` las
notas de los agentes; `ea` y `ea-web` al promover), así que la exclusión es un
`flock` exclusivo sobre `<raíz>/.ea-escritor.lock`: dos escritores, sean hilos
o procesos, nunca se solapan (el segundo espera hasta 10 s y, si no puede,
falla con un mensaje claro). Cada escritura es atómica (temporal oculto en el
mismo directorio, `fsync` y renombrado) y deja su evento en el diario
(`boveda_escritura`, `boveda_promocion`) *dentro* del candado, de modo que el
orden de los eventos es el de las escrituras. *Por qué así y no un servicio
dueño de una cola:* la CLI y la web tendrían que depender de que ese servicio
esté levantado, y la promoción del usuario pasaría por el proceso que sirve a
los agentes. El candado da exclusión mutua, no un orden FIFO estricto entre los
que esperan.

**Contrato de cabecera.** Toda nota que escribe el ecosistema empieza por un
bloque YAML (subconjunto plano, con analizador propio) que se valida al escribir:

```yaml
---
contrato: 1                         # versión del contrato
titulo: "Cómo se reinicia ea-mcp"   # 1-200 caracteres, una línea
autor: modelo-local                 # modelo-local | claude-code | usuario
region: modelo-local                # modelo-local | claude-code | compartida
creada: 2026-10-10T12:00:00Z        # RFC 3339, UTC
actualizada: 2026-10-10T12:00:00Z
origen: "trabajo:m-12"              # trabajo:<id> | rutina:<slug> | manual
promocion: ninguna                  # ninguna | promovida
tags: [servicios, mcp]              # opcional, hasta 10
# solo tras promover:
promovida_de: "modelo-local/x.md"   # en la copia compartida
promovida_a: "compartida/x.md"      # en el original
promovida_por: usuario
promovida_en: 2026-10-10T13:00:00Z
---
```

Reglas: en la región de un agente solo firma ese agente (`region` = `autor`);
una nota de agente en `compartida/` exige `promocion: promovida`; las claves
desconocidas se conservan al reescribir; `tags` es la clave de etiquetas de
Obsidian. Las notas ajenas (sin cabecera, o con otra) se leen y se indexan con
tolerancia (título = primer `# ` o nombre del fichero, autor desconocido); solo
se exige el contrato a lo que escribe el ecosistema. El estado `propuesta` /
`rechazada` de una promoción no se copia a la cabecera: vive en la propuesta
(`ea propuestas`), que es su única fuente.

**Rutas.** Relativas a la raíz, `región/…/nota.md`. Se rechazan las absolutas,
`..`, `.`, componentes vacíos u ocultos, barras invertidas, caracteres de
control y lo que no acabe en `.md`; además, resueltos los enlaces simbólicos
la ruta debe seguir dentro de la carpeta de su región (un enlace hacia fuera de
la bóveda, o hacia otra región, se rechaza al escribir y al leer). Límite:
la comprobación y la escritura no son una sola operación del sistema operativo,
así que un enlace creado en el instante entre ambas por alguien con acceso a
la carpeta no queda cubierto.

**Búsqueda y lectura acotadas.** El agente local no recibe el sistema de
ficheros de la bóveda: solo `ea_buscar_notas` y `ea_leer_nota`. La búsqueda es SQLite FTS5
(la versión enlazada de `rusqlite` lo trae; migración `0004`), sin acentos ni
mayúsculas, con todos los términos exigidos y el texto del usuario nunca
interpretado como sintaxis de consulta. Cotas por actor (la `region` `modelo-local` es la del agente local):

| | Modelo local | Claude Code / usuario |
|---|---|---|
| Resultados por búsqueda | 5 | 20 (50 en la web) |
| Fragmento por resultado | 200 caracteres | 400 |
| Caracteres por lectura (el resto, por trozos con `desde`) | 3000 | 20 000 |
| Cuerpo de una nota escrita | 8 KiB | 64 KiB |

El índice es una caché derivada de los ficheros: se pone al día solo (compara
fecha y tamaño) al buscar o listar, de modo que las ediciones hechas en
Obsidian se ven; `ea boveda reindexar` lo reconstruye entero. Se ignoran
ficheros y carpetas ocultos, enlaces simbólicos, lo que no sea `.md` y lo que
pese más de 512 KiB.

**Promoción** (decisión 9). Apoyada en el mismo mecanismo que las mutaciones
de la fase 6: `ea_promover_nota` es una herramienta supervisada y reversible.
Un agente solo la simula o la propone (`ea propuestas`, TUI o ◇ de la web);
**aceptar la propuesta la ejecuta**, desde la terminal o la web. El usuario
también puede promover directamente: `ea boveda promover <ruta> [--destino
x/y.md]` (pide terminal interactiva, muestra la simulación y confirma) o ⇧
en la sección ▥ de la web (⋯ → «Promover a compartida», con confirmación). Efecto: **copia** la nota a
`compartida/<misma ruta sin la región>` (o al destino indicado) con la
cabecera `promocion: promovida`, `promovida_de`, `promovida_por`,
`promovida_en`; el original se conserva y solo su cabecera pasa a
`promovida` con `promovida_a`. Se copia y no se mueve porque la acción es así
reversible y no quita nada a la región del autor. No sobrescribe nunca: si el
destino existe, falla. La propuesta guarda la huella del contenido que el
usuario vio: si la nota cambia entre proponer y aceptar, la ejecución falla
con un mensaje claro y no copia nada. `Boveda::promover` rechaza además a
cualquier agente o al orquestador como autor de la promoción.

Lo que ve el usuario: `ea boveda estado|listar|buscar|leer|promover|reindexar`,
la pestaña «Bóveda» de `ea tui` (solo lectura, fotografía) y la sección ▥ del
panel web (tecla `8`), que es el editor de la bóveda.

**El editor de la bóveda en `ea-web`** (spec 015). Es el punto único de edición
de la bóveda para el usuario, en lugar de Obsidian. Como el resto, escribe a
través de `ea-boveda` (`Boveda::guardar_usuario`, `crear_usuario`, `renombrar`,
`a_papelera`, `restaurar`, `crear_carpeta`): bajo el candado, con escritura
atómica, el índice al día y un evento en el diario dentro del candado. Estas
operaciones **no** están en `ea-mcp` ni en `ea-acciones` (ningún agente llega a
ellas) y rechazan como `quien` a un agente o al orquestador. Reglas:

- **Editar** notas de las tres regiones, con control de versión (huella del
  contenido: 409 si cambió en disco). La **firma** de una cabecera válida
  (`autor`, `region`, `creada`, `promocion`, `promovida_*`) no se cambia a mano
  (400) ni se puede quitar la cabecera; `actualizada` se pone al día sola. A una
  nota sin cabecera no se le puede poner una firmada por un agente (403).
  Título, etiquetas, origen y cuerpo sí se editan.
- **Crear notas** solo en `compartida/` (cabecera con `autor: usuario`, título =
  nombre del fichero; 403 en otra región). **Carpetas**, en cualquier región.
- **Renombrar o mover** solo dentro de la misma región (a otra se llega
  promoviendo; 403); nunca pisa otra nota (409). Reescribe los enlaces que
  apuntaban a ella en todas las notas indexadas, al estilo de Obsidian:
  `[[x]]`, `[[ruta/x|alias]]`, `[[x#ancla]]` (con o sin `.md`) y los markdown
  `](../x.md)`, conservando alias y anclas. Un `[[x]]` se resuelve como en
  Obsidian (ruta exacta o que acaba en `/x.md`; si hay varias, la de la misma
  carpeta); si sigue siendo ambiguo no se toca y se avisa.
- **Borrar** es mandar a la **papelera**: `<raíz>/.papelera/<marca>-<n>/<ruta>`
  (fuera de las regiones: no se indexa). Se restaura a su ruta original si allí
  no hay otra nota (409). Vaciar la papelera no está en la web.

Eventos nuevos en el diario (actor `usuario`): `boveda_edicion`,
`boveda_creacion`, `boveda_renombrado` (con las notas actualizadas y las
ambiguas), `boveda_papelera`, `boveda_restauracion` y `boveda_carpeta`.

**Limitaciones conocidas.** El orden entre escritores que esperan no es FIFO.
Las ediciones externas (fuera de `ea-web`) no dejan evento en el diario (solo se
reflejan en el índice). Los agentes no borran ni renombran notas. Los agentes
solo reemplazan notas propias, no hacen *append*. La TUI es una fotografía: no
promueve desde dentro. El editor no toca lo que está fuera de las regiones (p.
ej. un `Inicio.md` en la raíz) ni renombra o borra carpetas.

### Monitor: TUI y `ea-web`

`ea tui` es una fotografía de solo lectura. Su primera pestaña, **Orquestador**,
muestra las dos colas, el semáforo del worktree y los trabajos con su estado
(↑/↓ para moverse, Enter para ver el resumen del informe); **Rutinas** lista las
registradas con su versión, como `ea rutina listar`.

`ea-web` es un panel local en `http://localhost:8092` (solo `127.0.0.1`; puerto
en `red.puerto_web` de los ajustes o `$EA_WEB_PORT`): una página sin
dependencias, a base de símbolos, con los colores del tema activo de Omarchy. Se
actualiza sola cada 3 s (`web.sondeo_ms`). Nunca habla con
Claude Code ni con el modelo; lee del bus y de SQLite, y sus acciones pasan por
el mismo `Bus` que la CLI. La cabecera ocupa una sola línea: la bola de gashapon y
el estado (servicios, orquestador y salud) a la izquierda, el nombre de la sección
en el centro y los iconos de tema (☾/☀) y ayuda (?) a la derecha.

| Símbolo | Sección | Tecla |
|---|---|---|
| ⌂ | Orquestador: cola `modelo-local` → cola `claude-code`, semáforo del worktree, avisos de paquetes de corrección en espera (con contexto y botón de confirmar), propuestas por decidir y tablero en cola → ejecutando → en revisión → en espera → cerrado | `1` |
| ▤ | Trabajos: lista con filtro por estado y ficha con informe, fases, diff plegable e historial; cerrar, reformular (nota amplia con las notas anteriores encima y, en una obra, «rehacer desde la fase N» con la pendiente preseleccionada), continuar, confirmar y cancelar según el estado. **＋ misión** (en la barra) abre en línea un formulario para encargar una misión al modelo local: título, worktree (sugiere los de las sesiones de Claude Code y los de trabajos anteriores), objetivo, criterio (obligatorio) e iteraciones y minutos, acotados al techo de `ajustes.toml`; al encolarla se abre su ficha. El criterio lo ejecuta el sistema, nunca la página | `2` |
| ≡ | Diario. La barra filtra en el servidor (sobre todo el diario, no solo lo cargado): quién (actor), ✓/✕ (resultado), qué (familia: `bus_`, `mcp_`, `propuesta_`, `accion_`, `boveda_`, `memoria_`, `ajustes_`), cuándo (hoy, ayer, 24 h, 7 días o fechas) y el trabajo, si se llega desde la ficha (→ diario en su historial; ✕ lo quita); ♡/♥ muestra u oculta los latidos. El cuadro de filtrar busca texto en lo devuelto y el sondeo respeta los filtros | `3` |
| ↻ | Rutinas registradas, con versión y próxima ejecución. La ficha tiene un campo por entrada (con su valor por defecto) y ◌ vista previa, que enseña el objetivo y el criterio ya sustituidos sin lanzar nada; «echar ficha» lanza con lo escrito en la ficha. «✎ nueva versión» abre el TOML registrado con la versión ya subida; «◌ validar» lo comprueba como al registrar y enseña el diff y la orden resultante sin escribir nada, y «✓ registrar» (segundo clic) lo registra como `ea rutina registrar`. «＋ rutina», en la barra, abre el mismo editor con una plantilla comentada | `4` |
| ◇ | Propuestas: aceptar ✓ (ejecuta; ⌨ = solo terminal) o rechazar ✕ | `5` |
| ▶ | Acciones: simular y ejecutar las herramientas supervisadas | `6` |
| ▦ | Servicios (reiniciar ↻, pide confirmar), orquestador y agentes | `7` |
| ▥ | Bóveda: editor tipo Obsidian de la bóveda, con el mismo componente que Memoria. A la izquierda, el árbol con las tres regiones como raíces, sus carpetas (también vacías) y notas (`j`/`k`); encima, ＋ nota (en `compartida/`), ＋ carpeta y la papelera (lista y ↺ restaurar). El cuadro filtra por nombre y busca también en el contenido (FTS). A la derecha, la nota renderizada con autor, región, origen, promoción y etiquetas; los `[[enlaces]]` y `(x.md)` abren la nota destino. ✎ o `Ctrl+E` edita (la firma de la cabecera no cambia), `Ctrl+S` guarda. ⋯ renombra o mueve dentro de la región (con los enlaces al día), promueve a la compartida, copia la ruta, la muestra en el explorador o la manda a la papelera | `8` |
| ✎ | Memoria: editor tipo Obsidian de la memoria de Claude Code (`~/.claude/projects/*/memory/*.md`). A la izquierda, un árbol con una carpeta por proyecto y sus notas (`j`/`k`); a la derecha, la nota renderizada. ✎ o `Ctrl+E` alterna lectura y edición sin perder el borrador (`Ctrl+S` guarda; avisa si cambió en disco). ⋯ renombra (y reescribe los `[[enlaces]]` y `(x.md)` de las demás memorias del proyecto), copia la ruta absoluta, la muestra en el explorador de archivos o la elimina para siempre tras confirmar (el contenido queda en el diario). `MEMORY.md` no se renombra ni se borra. Apunta `memoria_editada`, `memoria_renombrada` y `memoria_eliminada` en el diario | `9` |
| ⚙ | Ajustes: los de `ajustes.toml` por secciones, con valor, origen (por defecto, `ajustes.toml`, heredado o entorno) y si se aplican en caliente o piden reinicio; avisos arriba. ◐ se cambia aquí (✓ guardar y ↺ por defecto piden un segundo clic; los techos solo bajan), ⌨ solo en una terminal (la fila da el comando) y ⊘ lo fija una variable de entorno. Apunta `ajustes_cambiados` en el diario | `0` |

`/` filtra, `j`/`k` mueve, `Enter` abre o cierra la ficha, `Esc` cierra, `?`
abre la ayuda y `t` abre el recorrido guiado, que va de vista en vista
iluminando cada zona (solo mira, nunca lanza nada; arranca solo la primera vez).

Enlaces profundos: el hash dice la sección, la fila desplegada y los filtros,
`#<seccion>[/<fila>][?<param>=<valor>&…]`. Por ejemplo
`localhost:8092/#trabajos/<id>` abre la ficha de un trabajo,
`#rutinas/<slug>`, `#acciones/<herramienta>` o `#propuestas/<n>` despliegan
esa fila, y `#diario?latidos=1&q=texto`,
`#diario?actor=modelo-local&resultado=error&accion=bus_&desde=24h` (también
`hasta`, `trabajo`; `desde` es `hoy`, `ayer`, `24h`, `7d` o `AAAA-MM-DD`),
`#trabajos?estado=en_revision` o
`#propuestas?todas=1` abren la sección con esos filtros (`q` es el cuadro de
filtrar y vale en todas). Navegar reescribe el hash: cambiar de sección o abrir
una fila deja una entrada en el historial (el botón atrás vuelve), y recargar
conserva la vista. La fila y los valores van con `encodeURIComponent`; la
gramática y sus pruebas están en `crates/ea-web/static/enlaces.js` y
`crates/ea-web/tests/js/` (`node --test crates/ea-web/tests/js/`; `cargo test`
las lanza si hay `node`). `ea-web-open '#trabajos/<id>'` abre una ventana en esa
vista con la clave (`#trabajos/<id>&clave=…`), y los avisos de escritorio del
ejecutor llevan una acción **abrir** que hace justo eso.

API (JSON): `GET /api/trabajos`, `/api/trabajos/<id>`, `/api/trabajos/<id>/diff`,
`/api/colas`, `/api/semaforos`, `/api/rutinas`, `/api/diario` (`?trabajo=`,
`actor`, `desde`, `hasta`, `resultado=ok|error`, `accion=<prefijo>` —solo
`[a-z_]`, si no 400—, `latidos=1`, `limite`); y, con `Content-Type:
application/json`, `POST /api/trabajos/<id>/{cerrar,cancelar,continuar,reformular,confirmar}`
(`reformular` lleva `{"nota": "...", "fase"?: N}` —`fase` solo en obras; fuera
de rango o en una misión, 400 sin escribir nada—, `confirmar` `{"contexto": "..."}`),
`POST /api/trabajos` (crea una misión: `{"modo"?: "mision", "titulo", "worktree",
"objetivo", "criterio", "topes"?: {"iteraciones"?, "tiempo_s"?}}` → 201 `{id, estado}`;
`creada_por` es `usuario` y no admite skills ni otros campos; 400 si falta el
criterio o el worktree no es una ruta absoluta existente dentro de un repo git;
los topes se recortan al techo), `GET /api/trabajos/nuevo` (techo, topes por
defecto y worktrees sugeridos para el formulario) y
`POST /api/rutinas/<slug>/lanzar` (`{"entradas": {"clave": "valor"}}` opcional; sin cuerpo, las de por defecto; 400 si una entrada no está declarada o se sale de los límites, sin crear trabajo) y `POST /api/rutinas/<slug>/previa` (mismo cuerpo; devuelve `{rutina, version, objetivo, criterio}` sin escribir nada ni ejecutar el criterio); `POST /api/rutinas/validar` (`{"toml": "..."}`; devuelve `{rutina, version, anterior: {version, definicion}|null, previa}` sin escribir nada) y `POST /api/rutinas/registrar` (mismo cuerpo; 201 `{rutina, version, anterior}`; 400 con el mensaje del bus y nada escrito si el TOML no vale o la versión no sube; deja `rutina_registrada` en el diario); la memoria de Claude Code va por
`GET /api/memoria`, `GET /api/memoria/<proyecto>/<fichero>` y
`POST /api/memoria/<proyecto>/<fichero>` (`{"contenido": "...", "version": "..."}`;
409 si la versión ya no es la del disco), `POST …/<fichero>/eliminar` (`{"version": "..."}`),
`POST …/<fichero>/renombrar` (`{"nuevo": "x.md", "version": "..."}`; 409 si ya existe) y
`POST …/<fichero>/mostrar` (`{}`; abre el gestor de ficheros por D-Bus
`FileManager1` o, si no, `xdg-open` sobre la carpeta); la bóveda, por
`GET /api/boveda` (`?q=`, `region`, `limite`: lista o busca), `GET /api/boveda/nota?ruta=`
(lectura paginada), `POST /api/boveda/promover` (`{"ruta": "..."}`) y el editor:
`GET /api/boveda/arbol` (regiones, carpetas, notas y cuántas hay en la papelera),
`GET /api/boveda/fichero?ruta=` (la nota entera con `contenido`, `version` y
`absoluta`), `POST /api/boveda/guardar` (`{"ruta", "contenido", "version"}`; 409 si
cambió en disco, 400 si cambia la firma), `POST /api/boveda/crear` (`{"ruta",
"contenido"?}`; 201; solo `compartida/`), `POST /api/boveda/renombrar` (`{"ruta",
"nuevo", "version"}`; devuelve `{nota, actualizadas, ambiguas}`),
`POST /api/boveda/eliminar` (`{"ruta", "version"}`: a la papelera),
`POST /api/boveda/carpeta` (`{"ruta"}`; 201), `POST /api/boveda/mostrar`
(`{"ruta"}`), `GET /api/boveda/papelera` y `POST /api/boveda/papelera/restaurar`
(`{"id"}`); los ajustes, por `GET /api/ajustes` (secciones con cada clave: valor,
defecto, rango, origen, edición, quién la fija y si pide reinicio; `version` y
`cliente`) y `POST /api/ajustes` (`{"version": "...", "cambios": {"clave": valor}}`,
`null` quita la clave; 400 inválido, 403 solo terminal o aflojar un techo, 409
versión vieja o fijada por el entorno). Solo atiende `Host: localhost|127.0.0.1`.

Clave de sesión: al arrancar, `ea-web` genera una clave aleatoria y la deja en
`$XDG_RUNTIME_DIR/ea-web/clave` (0600; cambia en cada arranque y se borra al
parar). Todo lo que no sea GET/HEAD tiene que llevarla en la cabecera
`X-EA-Clave`; sin ella o con otra, 403. Los GET no la piden, y
`GET /api/sesion` dice si la que se manda vale (`{"escritura": true|false}`).
`ea-web-open` abre `http://localhost:8092/#clave=<clave>`; la página la guarda
en `sessionStorage` y la quita de la barra de direcciones. Abierta a mano, o
tras reiniciar `ea-web`, la cabecera marca **⊘ solo lectura** y cada botón que
cambia algo avisa de que hay que cerrarla y abrirla con `ea-web-open`. Desde
la terminal:

```bash
curl -X POST -H 'Content-Type: application/json' \
     -H "X-EA-Clave: $(cat "$XDG_RUNTIME_DIR/ea-web/clave")" \
     -d '{}' http://localhost:8092/api/rutinas/<slug>/lanzar
```

Aceptar una propuesta con acción la ejecuta por la capa de `ea-acciones`; las
irreversibles no se confirman desde aquí (403).

Instalación como servicio de sesión:

```bash
ln -sf ~/Proyectos/ecosistema-asistente/crates/ea-web/systemd/ea-web.service \
    ~/.config/systemd/user/ea-web.service
systemctl --user daemon-reload
systemctl --user enable --now ea-web.service
```

Se abre en el escritorio 4 al iniciar sesión (Hyprland) como `localhost`, no
`127.0.0.1`, porque esa clase de ventana ya la usa el tutor:

```lua
-- ~/.config/hypr/hyprland.lua
o.window("^chrome-localhost__-Default$", { workspace = "4 silent" })
-- ~/.config/hypr/autostart.lua
o.exec_on_start(os.getenv("HOME") .. "/Proyectos/ecosistema-asistente/crates/ea-web/bin/ea-web-open")
```

La máquina arranca con la hora desajustada tras Windows hasta que NTP la
corrige, así que el panel mide el tiempo activo de los servicios con el reloj
monotónico e ignora latidos fechados en el futuro.

## Verificación

```bash
cargo test --workspace
cargo build --release
systemctl --user restart ea-web ea-orchestrator ea-mcp

# El diario es append-only a nivel de motor, no solo por convención:
sqlite3 ~/.local/share/ecosistema-asistente/ecosistema.db \
    "DELETE FROM events WHERE id = 1;"
# -> error del trigger ABORT
```
