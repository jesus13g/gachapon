# ecosistema-asistente

Orquestador en Rust que reparte el trabajo entre dos roles: el **planificador**
(formaliza y revisa; por defecto Claude Code) y el **ejecutor** (ejecuta; por
defecto Qwen Code sobre `llama-server`). El agente de cada rol es intercambiable
([Agentes y roles](#agentes-y-roles)). Ambos trabajan sobre el mismo worktree de git y se comunican por un bus de ficheros con
colas y semáforos. Diseño y razones en `orquestador-claude-qwen.md`.

[![Cómo funciona gachapon en 15 segundos](docs/media/gachapon-explicado.gif)](docs/media/gachapon-explicado.mp4)

*Cómo funciona en 15 segundos, sobre la interfaz `ea-web` ([vídeo en MP4](docs/media/gachapon-explicado.mp4)).*

## Estado actual

Funcionan el bus, el ejecutor, las skills del planificador, la CLI (`ea`), el
monitor (TUI y `ea-web`), el servidor de herramientas MCP, la bóveda compartida y
las mutaciones supervisadas. Por defecto el ejecutor es Qwen Code con
Qwen3-Coder-30B-A3B (`ea agentes` dice qué ocupa cada rol ahora).

### Roles

| | Planificador | Ejecutor |
|---|---|---|
| Lanza el trabajo | Sí (y el calendario, para rutinas) | Nunca por su cuenta |
| Define qué se hace | Objetivo, plan o proceso | No |
| Ejecuta | Solo lo que exige criterio | Todo lo que se describe en pasos verificables |
| Revisa | Sí: cierra, reformula o continúa | No |

El criterio de éxito es un comando de shell que comprueba el **sistema**, no el
agente: ningún agente se da el visto bueno a sí mismo.

### Agentes y roles

El agente de cada rol se elige entre un catálogo cerrado (cada uno necesita su
adaptador en el código). La nube solo se usa para planificar: el ejecutor corre
desatendido en `bwrap` y su modelo es siempre un endpoint local.

| Agente | Id | Planificador | Ejecutor | Notas |
|---|---|:-:|:-:|---|
| Claude Code | `claude-code` | sí (por defecto) | no | modelo remoto de Anthropic |
| Qwen Code | `qwen-code` | no | sí (por defecto) | contra `llama-server` |
| Codex | `codex` | sí | sí | como ejecutor, contra `llama-server` (`/v1/responses`) |
| OpenCode | `opencode` | sí | sí | como ejecutor, contra `llama-server` |
| Antigravity (`agy`) | `antigravity` | sí | no | solo modelos de Google; sin modo desatendido local |

Un agente no ocupa los dos roles a la vez (planificador ≠ ejecutor). Cómo se
configuran los modelos (`modelos.*`), los agentes (`agentes.*`) y los roles
(`roles.*`), con sus validaciones y el cambio en caliente, está en
[Configuración](#configuración-ajustestoml) (apartado «Roles, agentes y
modelos») y en la tabla de claves; `ea agentes` lo muestra en vivo y `ea agentes
rol <rol> <agente>` lo cambia desde una terminal real. El ejecutor se cambia en
caliente (vale desde el siguiente trabajo; el que está en curso conserva el suyo);
el planificador, cuando se quiera, sin reconectar su MCP. Cómo trabaja cada
planificador, en [Adaptadores del planificador](#adaptadores-del-planificador).

### Los tres modos

| | `/mision` | `/obra` | `/rutina` |
|---|---|---|---|
| Para qué | Alcanzar un objetivo iterando | Implementar un plan por fases | Repetir un proceso conocido |
| Vuelve al planificador | Al terminar o quedar bloqueado | Al terminar, o cada `revisar_cada` fases | Solo si falla y tú lo confirmas |
| Se guarda | Orden e informe | Plan, fases, diffs por fase | Definición TOML versionada |

Una rutina que falla deja un **paquete de corrección** en espera. Hasta que no lo
confirmas (`ea trabajo confirmar` o el botón de `ea-web`, con contexto opcional) no
llega al planificador, que entonces corrige la definición y sube la versión.

### El bus

Ficheros en `~/.local/state/ecosistema-asistente/` (o `$EA_ESTADO_DIR`); SQLite
guarda el historial y el diario.

```
colas/modelo-local/       órdenes por ejecutar (un .json por entrada)
colas/claude-code/        trabajos entregados que esperan revisión
trabajos/<id>/            orden.json, estado.json, informe.json,
                          historial.jsonl, paquete-correccion.md (rutinas),
                          salida-agente.json, skills-agente.md
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

El agente ejecutor (Qwen Code, Codex u OpenCode) corre dentro de
**bubblewrap** (`bwrap`): sistema de ficheros de solo lectura salvo el
worktree, un `/tmp` propio y sus directorios de estado (`~/.qwen`; `~/.codex`;
`~/.local/share/opencode`, `~/.local/state/opencode` y `~/.cache/opencode`), con
`$XDG_RUNTIME_DIR` tapado por un tmpfs vacío (ahí están la clave de sesión del
panel web y los sockets de D-Bus y `systemd --user`); la red queda abierta para
llegar a `llama-server`. Con `sandbox = false` Qwen y OpenCode corren con
aprobación total y pueden escribir donde escriba tu usuario; Codex usa entonces
su propio sandbox (`workspace-write`). Además: topes por ejecución (recortados
al techo de los ajustes; Codex y OpenCode solo admiten el de tiempo), exclusión
de las herramientas web y semáforo para que dos agentes no escriban a la vez en
el mismo worktree.

Qué se le quita a cada agente para que no salga a la web:

| Agente | Cómo | Herramientas que le quedan (comprobado) |
|---|---|---|
| Qwen Code | `--exclude-tools web_fetch web_search …` | las de ficheros y `run_shell_command` |
| Codex | `--ignore-user-config`, `-c web_search="disabled"` y `--disable` de `apps`, `browser_use`, `computer_use`, `plugins`, `multi_agent`, `goals`, `view_image`… | `exec_command`, `write_stdin`, `request_user_input` |
| OpenCode | configuración en línea (`OPENCODE_CONFIG_CONTENT`, sin la del usuario ni la del proyecto) que deniega `webfetch`, `websearch`, `codesearch`, `execute` y `question` | `edit`, `glob`, `grep`, `read`, `shell`, `skill`, `subagent`, `write` |

### Configuración (`ajustes.toml`)

Un único fichero, `~/.config/ecosistema-asistente/ajustes.toml` (o
`$EA_CONFIG_DIR/ajustes.toml`), todo opcional (plantilla comentada en
`crates/ea-ajustes/config/ajustes.example.toml`). Lo lee `ea-ajustes`, la base de
todos los crates. Precedencia **por clave**: variable de entorno >
`ajustes.toml` > TOML heredado > valor por defecto. Un valor inválido (fuera de
rango, una URL que no es local, un fichero que no es TOML) deja un aviso en
`ea ajustes ver` y en ⚙ Ajustes y cae a la capa siguiente: nunca impide arrancar.

Los ficheros antiguos se siguen leyendo, como fuentes heredadas de solo lectura
(el sistema nunca los escribe): `orquestador.toml` (`modelo` →
`modelos.local.nombre`, `base_url` → `modelos.local.base_url`, `qwen_bin` →
`agentes.qwen-code.bin`, `sandbox`, `avisos_escritorio` y las secciones `[topes]`
→ `agente.techo_topes` y `[skills]` → `agente.skills`), `orchestrator.toml`
(`intervalo_sondeo_s`, `intervalo_heartbeat_s`, `rutinas_dir` → `orquestador.*`) y
`boveda.toml` (`raiz` → `rutas.boveda`). La sección `[modelo]` que tenía
`ajustes.toml` antes de los roles también se lee así, y los nombres
`modelo.nombre`, `modelo.base_url` y `modelo.qwen_bin` siguen valiendo en `ea
ajustes`: al escribir uno se guarda con su nombre nuevo y se quita el antiguo.
El sistema solo escribe `ajustes.toml` (de forma atómica y conservando
comentarios y claves ajenas).

| Clave | Defecto | Rango o forma | Entorno | Quién la cambia | Aplica |
|---|---|---|---|---|---|
| `roles.planificador` | `claude-code` | un agente del catálogo que admita el rol | | web y terminal | en caliente |
| `roles.ejecutor` | `qwen-code` | `qwen-code`, `codex` u `opencode`; ≠ planificador | | web y terminal | en caliente (próximo trabajo) |
| `agentes.<id>.bin` | `claude`, `qwen`, `codex`, `opencode`, `agy` | texto | | solo terminal | en caliente |
| `agentes.<id>.modelo` | `local` (`""` en `claude-code` y `antigravity`) | id de un perfil o vacío | | web y terminal | en caliente |
| `modelos.<perfil>.nombre` | `local`: `qwen3-coder-30b` | texto | | web y terminal | en caliente |
| `modelos.<perfil>.base_url` | `local`: `http://127.0.0.1:8080/v1` | perfil no remoto: solo `127.0.0.1`, `localhost` o `[::1]` | | web y terminal | en caliente |
| `modelos.<perfil>.remoto` | `false` | sí/no; uno remoto solo vale para el planificador | | web y terminal | en caliente |
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
| `agente.skills.pueden_elegir` | `["claude-code"]` | ⊆ `["claude-code", "planificador"]` | | solo terminal | en caliente |
| `orquestador.intervalo_sondeo_s` | 5 | 1–300 | | web y terminal | reiniciar `ea-orchestrator` |
| `orquestador.intervalo_heartbeat_s` | 30 | 5–600 | | web y terminal | reiniciar los tres servicios |
| `orquestador.paralelo` | 1 | 1–8 | | solo terminal | reiniciar `ea-orchestrator` |
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

**Roles, agentes y modelos** (spec 017). Cada rol lo ocupa un agente
del catálogo; el planificador y el ejecutor son agentes distintos, y el modelo
del ejecutor tiene que ser un perfil local. Un cambio que rompa alguna de estas
reglas no se escribe; si están rotas en un `ajustes.toml` editado a mano, `ea
ajustes ver` lo avisa y se usan los valores por defecto de lo afectado. Un
perfil nuevo se crea en un solo cambio: `ea ajustes establecer
modelos.rapido.nombre=qwen3-8b modelos.rapido.base_url=http://127.0.0.1:8081/v1`.
Cada trabajo anota en `estado.json`, en `historial.jsonl` y en su informe (y en
cada fase de una obra) con qué agente y modelo se ejecutó (`ejecutado_por`); el
que está en curso conserva el suyo aunque cambie `roles.ejecutor`. El
orquestador elige el adaptador en cada ciclo, así que `ea ajustes establecer
roles.ejecutor=codex` (o el selector de la web) vale desde el siguiente trabajo,
sin reiniciar nada. Si el binario del ejecutor no está (`agentes.<id>.bin`, en
el `PATH` del servicio), el trabajo se entrega bloqueado con ese motivo.

**Ver y cambiar los agentes.** `ea agentes` lista el catálogo: el rol que ocupa
cada agente, los roles que admite, su binario (y versión), su perfil de modelo
y si el endpoint local responde (`GET <base_url>/models`, 2 s; a uno remoto no se
le pregunta). `ea agentes rol ejecutor codex` es un atajo de `ea ajustes
establecer roles.ejecutor=codex`, con la misma exigencia de terminal. En la web,
▦ Sistema tiene una fila por rol con su agente, modelo y última actividad; su
ficha lista los agentes que admiten el rol con un botón «⇄ poner de …» (segundo
clic para confirmar) que pasa por `POST /api/ajustes` y deja `ajustes_cambiados`
en el diario.

**Techo de topes.** Los topes de cada orden (`turnos`, `tiempo_s`,
`llamadas_herramienta`, `iteraciones`; por defecto 30/900/60/5) los pone quien la
escribe. El ejecutor los recorta a `agente.techo_topes` antes de lanzar al agente
(mínimo por campo, al menos una iteración) y lo deja en el log. El techo nunca
pasa de 100/3600/200/10. Un `[topes]` en un `orquestador.toml` antiguo pasa a ser
ese techo.

**Quién cambia qué.** `ea ajustes establecer` exige una terminal interactiva real
(sin TTY se rechaza, como las acciones irreversibles). ⚙ Ajustes en `ea-web` es un
gesto directo del usuario, como el editor de Memoria: solo llega a las claves
marcadas «web», los techos y topes de skills solo bajan desde ahí, pide un
segundo clic y comprueba la versión del fichero (409 si cambió). Lo que toca la
seguridad del ejecutor (sandbox, binarios de los agentes, quién elige skills,
rutas) es solo de terminal. Cada cambio queda en el diario como `ajustes_cambiados`
(actor `usuario`, con el canal y los valores antes y después). Ni `ea-mcp` ni el
orquestador tienen forma de leer o escribir los ajustes. Claude Code sí podría
editar `ajustes.toml` con su herramienta de ficheros, igual que antes
`orquestador.toml`.

```bash
ea ajustes ver [--json]                      # valor, origen, quién lo cambia y cuándo se aplica; avisos debajo
ea ajustes obtener red.puerto_web            # solo el valor
ea ajustes establecer agente.techo_topes.turnos=60 web.sondeo_ms=5000
ea ajustes establecer --quitar modelos.local.nombre # vuelve a mandar la capa siguiente
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
`ea ajustes establecer modelos.local.nombre=<modelo>` (o desde ⚙ Ajustes). Se aplica en
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
- `crates/ea-biblioteca`: la biblioteca de herramientas (spec 018): inventario de
  solo lectura de skills, plugins, subagentes, MCP, comandos y extensiones de
  los agentes, su uso, los planes de activar y desactivar y de sesión efímera;
  solo depende de `ea-ajustes`.
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
- `agentes/flujo/`: el procedimiento neutral del planificador (misión, obra,
  rutina, revisar); `agentes/{codex,opencode,antigravity}/`: sus adaptadores.
- `claude/skills/`: las skills vivas de Claude Code (adaptador del flujo).
- `claude/hooks/`: los hooks de Claude Code que usa `ea-web`.
- `qwen/skills/`: las skills propias del agente local.

## Adaptadores del planificador

Las instrucciones del planificador son una **guía neutral común** más un
**adaptador fino por agente**, sin copias del procedimiento:

- `agentes/flujo/{mision,obra,rutina,revisar}.md`: el procedimiento sin nombrar al
  agente ni al modelo (comandos reales de `ea` y herramientas MCP de solo lectura;
  el ejecutor y su modelo se consultan con `ea agentes`).
- Cada adaptador solo trae el frontmatter del agente, las reglas que no se rompen
  y un resumen, y remite a su `agentes/flujo/<x>.md`. Una prueba
  (`crates/ea-cli/tests/adaptadores.rs`) comprueba que ese fichero existe.

| Flujo | Qué hace |
| --- | --- |
| misión | Formaliza objetivo y criterio de éxito y crea el trabajo con `ea trabajo crear --orden` (o `--por planificador`) |
| obra | Plan por fases pequeñas (instrucciones, entregable, criterio por fase) y `revisar_cada` |
| rutina | Proceso repetible como TOML versionado: registrar, lanzar, timer y corrección |
| revisar | Consume la cola del planificador (`ea cola planificador`): `ea trabajo ver`, `diff` si hace falta, y cierra, reformula o continúa |

Los adaptadores se instalan con enlaces simbólicos (nada se instala solo en tu
HOME; los comandos se ejecutan desde la raíz del repo). Un agente solo necesita
el suyo, y solo si va a ser planificador.

| Agente | Adaptador | Formato | Destino |
|---|---|---|---|
| Claude Code | `claude/skills/<x>/` | skills (`/mision`, `/obra`, `/rutina`, `/revisar`) | `~/.claude/skills/` |
| Codex | `agentes/codex/skills/ea-<x>/` y `agentes/codex/AGENTS.md` | skills (`$ea-mision`…) y un fragmento de `AGENTS.md` | `~/.codex/skills/`, `~/.codex/AGENTS.md` |
| OpenCode | `agentes/opencode/commands/ea-<x>.md` y `agentes/opencode/AGENTS.md` | comandos (`/ea-mision`…, con `$ARGUMENTS`) y un fragmento de `AGENTS.md` | `~/.config/opencode/commands/`, `~/.config/opencode/AGENTS.md` |
| Antigravity | `agentes/antigravity/skills/ea-<x>/` y `agentes/antigravity/rules/ea-planificador.md` | skills (los workflows están obsoletos en favor de ellas) y una regla `model_decision` | `~/.gemini/config/skills/`, `~/.gemini/config/rules/` |

```bash
# Claude Code (las skills vivas: editarlas cambia las skills en uso)
for s in mision obra rutina revisar; do
  ln -sfn "$PWD/claude/skills/$s" ~/.claude/skills/$s
done

# Codex: skills y reglas (el fragmento se añade una sola vez)
mkdir -p ~/.codex/skills
for s in mision obra rutina revisar; do
  ln -sfn "$PWD/agentes/codex/skills/ea-$s" ~/.codex/skills/ea-$s
done
cat agentes/codex/AGENTS.md >> ~/.codex/AGENTS.md

# OpenCode: comandos y reglas
mkdir -p ~/.config/opencode/commands
for s in mision obra rutina revisar; do
  ln -sfn "$PWD/agentes/opencode/commands/ea-$s.md" ~/.config/opencode/commands/ea-$s.md
done
cat agentes/opencode/AGENTS.md >> ~/.config/opencode/AGENTS.md

# Antigravity: skills y regla
mkdir -p ~/.gemini/config/skills ~/.gemini/config/rules
for s in mision obra rutina revisar; do
  ln -sfn "$PWD/agentes/antigravity/skills/ea-$s" ~/.gemini/config/skills/ea-$s
done
ln -sfn "$PWD/agentes/antigravity/rules/ea-planificador.md" ~/.gemini/config/rules/ea-planificador.md
```

Los adaptadores nombran la ruta `~/Proyectos/ecosistema-asistente/agentes/flujo/`;
si el repo está en otro sitio, el resumen de cada adaptador basta para arrancar y
`agentes/flujo/` en la raíz del repo es la referencia. Los formatos salen de lo
instalado (`codex` 0.159, `opencode` 2.1, `agy` 1.4: sus propias skills de
personalización y su carga de `{command,commands}/**/*.md`), pero el de
Antigravity no se ha probado en una sesión real (necesita iniciar sesión con Google; sin ella `agy` no lista sus skills). OpenCode sí se ha comprobado: con los enlaces puestos, `opencode api GET /api/command` lista `ea-mision`, `ea-obra`, `ea-revisar` y `ea-rutina`, y `GET /api/mcp` da `ea-ecosistema` conectado.

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
`trabajos/<id>/skills-agente.md` (`skills-qwen.md` en los trabajos de antes).
Son las skills del ejecutor, tenga el agente que tenga (Codex y OpenCode las
reciben como prefijo del prompt); el comando conserva el nombre `ea qwen-skills`.

Las elige el planificador al formalizar (`/mision`, `/obra`). `[agente.skills]` de
`ajustes.toml` (o el heredado `[skills]` de `orquestador.toml`) decide qué se
puede elegir y quién:

- `habilitadas`: lista blanca del catálogo (vacía = todas).
- `max_por_trabajo` y `max_caracteres`: topes de contexto.
- `pueden_elegir`: la llave. Por defecto la tiene el rol planificador
  (`claude-code`, su nombre en disco, o `planificador`): vale un `creada_por`
  igual a `planificador`, a `claude-code` o al agente que ocupa
  `roles.planificador` al crear el trabajo. Un trabajo con skills de otro origen
  se rechaza al crearlo y vuelve a validarse al ejecutarse: si no pasa, se
  bloquea sin llamar al agente.

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
ea trabajo crear ... --equipo <id> [--asignar <ejecutor>] [--revisor <agente>] [--worktree-propio]
                                             # trabajo de un equipo despierto (spec 019): --asignar lo
                                             #   deja en la cola de ese ejecutor; --revisor recibe la entrega
                                             #   (distinto del ejecutor); --worktree-propio crea un git
                                             #   worktree (rama ea/<id>) para correr en paralelo sobre el repo
ea trabajo listar
ea trabajo ver <id> [--json]                 # orden, estado e informe (con el agente y modelo que lo ejecutó)
ea trabajo diff <id> [--fase N]              # diff desde commit_inicio
ea trabajo cerrar|cancelar <id>
ea trabajo reformular <id> --nota "..." [--fase N]  # vuelve al agente local (obra: desde la fase N)
ea trabajo continuar <id>                    # aprueba una revisión intermedia de una obra
ea trabajo confirmar <id> [--contexto "..."] # paquete de corrección -> claude-code
ea cola [planificador|ejecutor] [--json]     # colas (valen claude-code y modelo-local)
ea semaforos                                 # quién tiene el worktree

# Equipos de agentes (spec 019; el flujo completo, en la documentación de F6)
ea equipo despertar (--todos | --miembro agente:rol ...) --titulo T --objetivo O --worktree <repo> [--sin-terminal]
                                             # TTY. Roles: planificador, ejecutor, revisor. --todos: los agentes
                                             #   del PATH según lo que admiten. Planificadores y revisores abren una
                                             #   sesión en una terminal real (`ea equipo sesion`, con presencia);
                                             #   --sin-terminal solo enseña el comando de cada sesión
ea equipo listar [--json]
ea equipo ver <id> [--json]                  # miembros con presencia y colas, trabajos con su último informe
ea equipo dormir <id>                        # TTY; lo pendiente de sus ejecutores vuelve a la cola de modelo-local

# Rutinas versionadas
ea rutina registrar <fichero.toml>           # valida y registra (la versión debe subir)
ea rutina listar [--retiradas]|ver <slug>     # --retiradas: las de la papelera (o sin fichero)
ea rutina ver <slug> --version N             # esa versión registrada y la lista de todas
ea rutina lanzar <slug> [--entrada k=v]...   # crea el trabajo en el bus (no si está retirada)
ea rutina retirar|restaurar <slug>           # a la papelera y vuelta (simula y confirma con TTY)

# Resto del ecosistema
ea status                                    # orquestador, roles (agente y modelo), trabajos por estado, colas, semáforos y presencia de los equipos
ea agentes [--json]                          # catálogo: rol, binario, versión, si responde el modelo y su presencia
ea agentes rol <planificador|ejecutor> <agente>  # = ea ajustes establecer roles.<rol>=<agente> (TTY)
ea diario listar|ver <id>                    # listar: --trabajo, --actor, --desde, --hasta,
                                             #   --resultado ok|error, --accion <prefijo> (bus_, mcp_…),
                                             #   --antes-de <id> (página siguiente, más antiguos)
ea diario anotar --texto "…" [--corrige <id>] [--trabajo <id>]
                                             # nota del usuario (usuario_nota): constancia o corrección
ea propuestas listar|ver|decidir <id> --aceptar|--rechazar
ea acciones listar|simular|ejecutar          # catálogo de herramientas
ea boveda estado|listar|buscar|leer|promover|reindexar
ea ajustes ver|obtener|establecer|ruta       # ajustes.toml (establecer: solo con TTY)
ea biblioteca listar|ver|uso|activar|desactivar|sesion|categoria  # herramientas de los agentes (ver «Biblioteca»)
ea tui                                       # interfaz interactiva
```

La base de datos vive en `~/.local/share/ecosistema-asistente/ecosistema.db`
(o en `rutas.db` de los ajustes, o en `$EA_DB_PATH`). Servicios de sesión (`systemd --user`): `llama-server`,
`ea-orchestrator`, `ea-mcp` y `ea-web`; las rutinas se programan con
`ea-rutina@<slug>.timer`, que solo crea el trabajo en el bus.

Programar una rutina no exige copiar la plantilla a mano: `ea_programar_rutina`
(`{slug, calendario}`) y `ea_desprogramar_rutina` (`{slug}`) son herramientas
supervisadas y reversibles. La simulación valida el `OnCalendar` con
`systemd-analyze calendar` y enseña las próximas 3 ejecuciones; aplicarla
escribe `~/.config/systemd/user/ea-rutina@<slug>.timer` (escritura atómica, el
mismo contenido que la plantilla con otro `OnCalendar`), hace `daemon-reload`,
`enable` y `restart` (o `disable --now`, borra el fichero y `daemon-reload`).
Solo se programan rutinas registradas; el calendario va como argumento, nunca
por shell, y se rechazan los saltos de línea y demás caracteres de control,
`%` y `\`. Se hace desde la ficha de ↻ Rutinas, desde ▶ Acciones o aceptando
la propuesta que Claude Code encola por MCP (`/rutina`). Comprobación manual:
`systemctl --user list-timers 'ea-rutina@*'`.

Una rutina se **retira** de forma reversible: `ea_retirar_rutina` (`{slug}`)
mueve su TOML de `rutinas_dir/<slug>.toml` a `rutinas_dir/.papelera/<slug>.toml`
y antes desprograma su timer (la simulación lo dice). La base de datos no
cambia: sus versiones siguen en SQLite y el fichero decide. Una rutina
retirada (o sin fichero en `rutinas_dir`) no se lista salvo pidiendo las
retiradas, y no se lanza: la rechazan `ea rutina lanzar` (y con él su timer),
`POST /api/rutinas/<slug>/lanzar` (409), `ea_programar_rutina` y la simulación
de `ea_encolar_rutina`, con el mismo texto. `ea_restaurar_rutina` la devuelve
a su sitio sin volver a programar el timer; registrar una versión nueva de una
retirada también la vuelve a poner en uso. Las dos son reversibles (se
confirman en la terminal, en ▶ Acciones o en la ficha de ↻ Rutinas). Borrar
para siempre la copia retirada es `ea_eliminar_rutina`, irreversible y solo
desde la terminal; antes hay que retirarla.

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
| `ea_listar_equipos`, `ea_ver_equipo` (miembros con presencia, colas y últimos trabajos con su informe; `ea_status` también trae equipos y presencias) | lectura (despertar y dormir equipos es de la CLI con TTY: ningún agente puede) | se ejecuta |
| `ea_biblioteca` (sin `id`: lista con filtros `agente`, `tipo`, `categoria`, `activado` y `limite`, por defecto 50 y hasta 200, y 15 y 40 para el agente local; con `id`: la ficha con la definición MCP sin secretos y el uso por actor) | lectura | se ejecuta |
| `ea_buscar_notas`, `ea_leer_nota` | lectura (resultados acotados por actor) | se ejecuta |
| `ea_escribir_nota` | mutación reversible | se ejecuta, solo en la región propia del agente |
| `ea_promover_nota` | mutación reversible | solo simula o propone |
| `ea_encolar_rutina` (crea el trabajo en el bus; `entradas` opcionales, y la simulación muestra el criterio resultante) | mutación reversible | solo simula o propone |
| `ea_registrar_rutina` (`toml`: la definición entera; la versión debe subir; la simulación muestra el diff frente a la registrada y el criterio resultante; no lanza nada) | mutación reversible | solo simula o propone |
| `ea_reiniciar_servicio` (lista cerrada: `llama-server`, `ea-mcp`, `ea-orchestrator`, `ea-web`) | mutación reversible | solo simula o propone |
| `ea_programar_rutina` (`slug` de una rutina registrada y `calendario`, un `OnCalendar`; la simulación da las próximas 3 ejecuciones) y `ea_desprogramar_rutina` (`slug`) | mutación reversible | solo simula o propone |
| `ea_lanzar_sesion` (`agente`: `claude-code`, `qwen-code`, `codex` u `opencode`; `herramientas`: ids de la biblioteca; `directorio` opcional; abre una terminal con el agente y solo esas herramientas, con los secretos de los MCP en ficheros 0600 del runtime y nunca en argumentos; la simulación muestra el comando y los ficheros sin secretos; no está disponible para el agente local) | mutación reversible | solo simula o propone |
| `ea_retirar_rutina` (su TOML a la papelera y su timer desprogramado) y `ea_restaurar_rutina` (vuelve, sin timer), con `slug` | mutación reversible | solo simula o propone |
| `ea_activar_herramienta` y `ea_desactivar_herramienta` (`elemento`: id `agente/tipo/ámbito/nombre` de la biblioteca; la simulación da el comando o fichero y el valor antes y después; comando oficial, campo `enabled` con copia previa, mover a un almacén o `agente.skills.habilitadas`; nunca `~/.claude.json`) | mutación reversible | solo simula o propone |
| `ea_eliminar_rutina` (borra para siempre la copia retirada; antes hay que retirarla) | **mutación irreversible** | solo simula o propone; solo se confirma desde la terminal |

Toda invocación queda registrada en el diario. El actor sale del nombre con
que el cliente se presenta al conectar (`clientInfo.name`, nunca de un
argumento) y del rol que ocupa ese agente **en ese momento** según los ajustes
(spec 017):

| Cliente MCP | Agente |
|---|---|
| `claude-code` | Claude Code |
| `qwen-code-mcp-client`, `qwen-code` | Qwen Code |
| `codex-mcp-client` | Codex |
| `opencode` | OpenCode |
| `antigravity-client` | Antigravity (`agy`) |
| `ea-orchestrator` | el propio orquestador (actor `orquestador`) |

- El agente que ocupa el planificador actúa como `claude-code` (el nombre en
  disco del rol): catálogo completo, aunque las mutantes solo simulan o
  proponen. El del ejecutor actúa como `modelo-local`: la proyección reducida
  y nada irreversible. El diario guarda en el detalle qué agente fue
  (`"agente": "codex"`).
- Un agente del catálogo sin rol (actor `sin-rol`), un cliente desconocido
  (actor `desconocido`) y lo desatendido (`orquestador`) **solo leen**. Del
  desconocido, el diario guarda el nombre que declaró (`"cliente": …`): así se
  ve cuál añadir. Antigravity aún no tiene nombre comprobado, así que por ahora
  solo lee.

La autorización es del servidor, no del prompt. Cambiar de planificador no exige
reconectar: los roles se releen en cada llamada.

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

Registro en los demás agentes (misma URL, `http://127.0.0.1:8765/mcp`):

| Agente | Registro | Nombre de cliente MCP |
|---|---|---|
| Claude Code | `claude mcp add --transport http --scope user ea-ecosistema <url>` | `claude-code` |
| Qwen Code | `qwen mcp add --transport http ea-ecosistema <url>` | `qwen-code-mcp-client` |
| Codex | `codex mcp add ea-ecosistema --url <url>` | `codex-mcp-client` |
| OpenCode | `opencode mcp add --global --url <url> ea-ecosistema` (escribe `"mcp": {"servers": {"ea-ecosistema": {"type": "remote", "url": "<url>"}}}` en `~/.config/opencode/opencode.json`) | `opencode` |
| Antigravity | `agy mcp add -t http ea-ecosistema <url>` | `antigravity-client` |

`agy` 1.4 prueba antes con una petición `server/discover` (`ea-mcp` la rechaza con un error de parámetros; `agy` manda después `initialize` con su `clientInfo`, sin comprobar aún que conecta con sesión iniciada).

El nombre de cliente sale de lo que cada agente manda en `initialize`
(`clientInfo.name`); es lo que decide sus permisos, junto con el rol que ocupa.

### Biblioteca

Inventario de solo lectura de lo que tienen los agentes: skills, plugins,
subagentes, servidores MCP, comandos y extensiones de Claude Code, Qwen Code,
Codex, OpenCode y Antigravity, más lo del ecosistema (spec 018). Cada elemento
tiene un id estable `agente/tipo/ámbito/nombre` (p. ej.
`claude-code/skill/usuario/revisar`) y se agrupa por categoría, que por defecto
es su origen (`usuario`, `proyecto`, `ecosistema`, `plugin:<id>`, `integrada`).
Lo que la propia biblioteca desactiva moviéndolo a su almacén sigue en el
inventario con `activado: no`. Nada de lo que sale lleva secretos: de un MCP
solo se ven los nombres de las variables y las cabeceras, y de la URL el
esquema, el host y el puerto.

```bash
ea biblioteca listar [--agente A] [--tipo T] [--categoria C] [--activados|--desactivados] [--ids] [--json]
                                             # tabla por categoría y tipo: agente, ámbito, estado, cómo se cambia y usos; avisos al final
ea biblioteca ver <id> [--json]              # ficha: origen, descripción, estado, mecanismo, nota, MCP redactado y uso por actor
ea biblioteca uso [<id>] [--json]            # usos por elemento y actor, lo usado sin elemento en el inventario y la actividad por agente
ea biblioteca activar|desactivar <id> [--esperar]
                                             # crea la propuesta (como «usuario») y muestra la simulación
ea biblioteca sesion <agente> <id>… [--directorio D] [--esperar]
                                             # propone abrir el agente solo con esas herramientas; muestra el comando y los ficheros, sin secretos
ea biblioteca categoria <id> <categoria>|--quitar
                                             # atajo de `biblioteca.categorias` (solo con TTY)
```

`activar`, `desactivar` y `sesion` solo proponen: se aceptan con `ea propuestas
decidir <n> --aceptar` o en ◇ Propuestas. Con `--esperar` y una terminal
interactiva real piden confirmación y aplican en el acto (y esperan a Intro al
terminar); sin terminal, `--esperar` deja la propuesta pendiente. El uso sale
de los contadores y transcripciones de cada agente, el diario y los trabajos del
ejecutor, con una caché incremental en `~/.local/state/ecosistema-asistente/biblioteca/`;
los agentes llegan a él por `ea_biblioteca` (MCP).

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
registradas con su versión, como `ea rutina listar`; **Agentes**, cada rol con
el agente que lo ocupa, su modelo y la última actividad.

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
| ▤ | Trabajos: lista con filtro por estado y ficha con informe, resumen del agente, metadatos (skills, topes, `creada_por`, rutina vN), el plan de la obra (estado de cada fase, con su plan y su diff plegables), diff plegable e historial; cerrar, reformular (nota amplia con las notas anteriores encima y, en una obra, «rehacer desde la fase N» con la pendiente preseleccionada), continuar, confirmar y cancelar según el estado. **＋ misión** (en la barra) abre en línea un formulario para encargar una misión al modelo local: título, worktree (sugiere los de las sesiones de Claude Code y los de trabajos anteriores), objetivo, criterio (obligatorio) e iteraciones y minutos, acotados al techo de `ajustes.toml`; al encolarla se abre su ficha. En modo **obra**, el mismo formulario lleva un objetivo general y una lista de fases (título, instrucciones, entregable y criterio cada una) que se añaden, quitan y reordenan, y obliga a elegir cada cuántas fases revisa Claude Code (o solo al final). El worktree tiene que ser un repo con al menos un commit. El criterio lo ejecuta el sistema, nunca la página. En la ficha de un trabajo cerrado o cancelado, **⧉ lanzar de nuevo** abre ese formulario relleno con su orden (objetivo, criterio, worktree, topes y, en una obra, fases y revisión) para editarla y encolarla con un id nuevo; no copia las notas, el informe ni las skills, y una rutina vuelve como misión normal | `2` |
| ≡ | Diario. La barra filtra en el servidor (sobre todo el diario, no solo lo cargado): quién (actor), ✓/✕ (resultado), qué (familia: `bus_`, `mcp_`, `propuesta_`, `accion_`, `boveda_`, `memoria_`, `ajustes_`, `usuario_`), cuándo (hoy, ayer, 24 h, 7 días o fechas) y el trabajo, si se llega desde la ficha (→ diario en su historial; ✕ lo quita); ♡/♥ muestra u oculta los latidos. ✎ anotar abre en línea una nota del usuario (texto y, si quieres, trabajo) que queda como `usuario_nota`; en el detalle de un evento, ✎ corregir abre lo mismo apuntando a ese evento, que no se toca: el corregido muestra «corregido por #n» y la nota «corrige #m», enlazados. El cuadro de filtrar busca texto en lo devuelto y el sondeo respeta los filtros. Al pie, ↓ más antiguos añade la página siguiente con los mismos filtros (desaparece cuando no queda nada); el sondeo refresca solo la primera sin perder las cargadas, y cambiar un filtro vuelve a la primera | `3` |
| ↻ | Rutinas registradas, con versión y próxima ejecución. La ficha tiene un campo por entrada (con su valor por defecto) y ◌ vista previa, que enseña el objetivo y el criterio ya sustituidos sin lanzar nada; «echar ficha» lanza con lo escrito en la ficha. «✎ nueva versión» abre el TOML registrado con la versión ya subida; «◌ validar» lo comprueba como al registrar y enseña el diff y la orden resultante sin escribir nada, y «✓ registrar» (segundo clic) lo registra como `ea rutina registrar`. «＋ rutina», en la barra, abre el mismo editor con una plantilla comentada. «▸ versiones» lista las registradas (número, fecha y quién) con «ver» (su TOML) y «diff» (qué cambió frente a la anterior), solo lectura. «horario» enseña el `OnCalendar` del timer y si está activo; «◷ guardar» simula el horario escrito (próximas 3 ejecuciones) y el segundo clic lo programa; «✕ desprogramar» quita el timer. «▣ retirar» (segundo clic) manda la rutina a la papelera y quita su timer; «▣ retiradas», en la barra, las lista, y su ficha tiene «↺ restaurar» | `4` |
| ◇ | Propuestas: aceptar ✓ (ejecuta; ⌨ = solo terminal) o rechazar ✕ | `5` |
| ▶ | Acciones: simular y ejecutar las herramientas supervisadas. Los formularios guían: la ruta de `ea_promover_nota` sugiere las notas de `claude-code` y `modelo-local` aún sin promover (se puede escribir otra), el servicio de `ea_reiniciar_servicio` dice el estado de cada uno y el slug elige entre las rutinas registradas (`GET /api/acciones` trae por campo `opciones`, `sugerencias` y `estados`). La simulación dice en un renglón qué se revalidará al ejecutar («se comprobará: huella …») y avisa cuando tiene más de 60 s. En la ficha de cada herramienta (también de las directas), sus últimos 10 eventos del diario | `6` |
| ▦ | Sistema: servicios (reiniciar ↻, pide confirmar), orquestador y una fila por rol (planificador y ejecutor) con su agente, modelo y última actividad. La ficha de un rol lista los agentes que lo admiten (binario ●/✕, modelo, si responde, topes) y cambia el agente con «⇄ poner de …» (segundo clic) | `7` |
| ▥ | Bóveda: editor tipo Obsidian de la bóveda, con el mismo componente que Memoria. A la izquierda, el árbol con las tres regiones como raíces, sus carpetas (también vacías) y notas (`j`/`k`); encima, ＋ nota (en `compartida/`), ＋ carpeta y la papelera (lista y ↺ restaurar). El cuadro filtra por nombre y busca también en el contenido (FTS). A la derecha, la nota renderizada con autor, región, origen, promoción y etiquetas; los `[[enlaces]]` y `(x.md)` abren la nota destino. ✎ o `Ctrl+E` edita (la firma de la cabecera no cambia), `Ctrl+S` guarda. ⋯ renombra o mueve dentro de la región (con los enlaces al día), promueve a la compartida, copia la ruta, la muestra en el explorador o la manda a la papelera | `8` |
| ✎ | Memoria: editor tipo Obsidian de la memoria de Claude Code (`~/.claude/projects/*/memory/*.md`). A la izquierda, un árbol con una carpeta por proyecto y sus notas (`j`/`k`); a la derecha, la nota renderizada. ✎ o `Ctrl+E` alterna lectura y edición sin perder el borrador (`Ctrl+S` guarda; avisa si cambió en disco). ⋯ renombra (y reescribe los `[[enlaces]]` y `(x.md)` de las demás memorias del proyecto), copia la ruta absoluta, la muestra en el explorador de archivos o la elimina para siempre tras confirmar (el contenido queda en el diario). `MEMORY.md` no se renombra ni se borra. Apunta `memoria_editada`, `memoria_renombrada` y `memoria_eliminada` en el diario | `9` |
| ◫ | Biblioteca (spec 018): las skills, plugins, subagentes, servidores MCP, comandos y extensiones de cada agente, agrupados por categoría y tipo (●/○ activado, usos), con filtros de agente, tipo, categoría y estado y el cuadro de buscar. La ficha da origen, descripción, cómo se cambia, la definición de un MCP sin secretos (solo los nombres de sus variables y cabeceras) y el uso por actor. ⏻ desactivar/activar simula (antes/después) y pide aceptar, ▶ sesión con… abre una terminal con solo las herramientas elegidas (la simulación lleva el comando sin secretos) y ✎ categoría escribe `biblioteca.categorias` por los ajustes. Enlace `#biblioteca/<id>` | `b` |
| ◎ | Equipos (spec 019): una fila por equipo con su estado, los miembros (● despierto, ○ dormido) y el nº de trabajos. La ficha da el objetivo, cada miembro con su rol, presencia, trabajo actual y cola pendiente, y los trabajos del equipo (estado, asignado, revisor, quién los ejecutó y la última línea del informe, con «→ trabajo» a `#trabajos/<id>`). «＋ despertar» abre el formulario (título, objetivo, worktree, miembros agente:rol según lo que admite cada agente o «todos», y «abrir terminales» para planificadores y revisores) y «◌ dormir» apaga el equipo; los dos piden la clave de sesión y dormir, un segundo clic. En ＋ misión, «◎ equipo…» elige equipo, ejecutor asignado, revisor y worktree propio. Sin clave el panel solo lee. Enlace `#equipos/<id>` | `e` |
| ⚙ | Ajustes: los de `ajustes.toml` por secciones, con valor, origen (por defecto, `ajustes.toml`, heredado o entorno) y si se aplican en caliente o piden reinicio; avisos arriba. ◐ se cambia aquí (✓ guardar y ↺ por defecto piden un segundo clic; los techos solo bajan), ⌨ solo en una terminal (la fila da el comando) y ⊘ lo fija una variable de entorno. Apunta `ajustes_cambiados` en el diario | `0` |

`/` filtra, `j`/`k` mueve, `Enter` abre o cierra la ficha, `Esc` cierra, `?`
abre la ayuda y `t` abre el recorrido guiado, que va de vista en vista
iluminando cada zona (solo mira, nunca lanza nada; arranca solo la primera vez).

Enlaces profundos: el hash dice la sección, la fila desplegada y los filtros,
`#<seccion>[/<fila>][?<param>=<valor>&…]`. Por ejemplo
`localhost:8092/#trabajos/<id>` abre la ficha de un trabajo,
`#rutinas/<slug>`, `#acciones/<herramienta>` o `#propuestas/<n>` despliegan
esa fila (`#rutinas/<slug>?version=N`, con `&diff=1` si se quiere su diff,
abre además esa versión de la rutina), y `#diario?latidos=1&q=texto`,
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

Enlaces cruzados del diario: el detalle de un evento lleva, además de
«→ trabajo», un botón por cada cosa que nombre su `detail`: «→ propuesta #n»
(`propuesta`), la herramienta en ▶ Acciones (`herramienta`, o la `ea_<x>` de un
`mcp_<x>`), la rutina (`rutina` o `slug`), la nota en ▥ Bóveda (`ruta`; al
renombrar, `a`; al promover, `destino`; no la que fue a la papelera), la memoria
(`proyecto` y `fichero`) y cada ajuste cambiado (`cambios[].clave`). A la
inversa, «→ diario» en el historial de la ficha de un trabajo abre el diario
filtrado por él.

Lo que produjo una acción: tras «▶ ejecutar» en ▶ Acciones, o al aceptar una
propuesta con acción en ◇ Propuestas (o en ⌂ Inicio), el resultado enlaza con
lo que produjo: «→ trabajo» (`ea_encolar_rutina`), «→ nota en compartida»
(`ea_promover_nota`), el estado nuevo del servicio y «→ Sistema»
(`ea_reiniciar_servicio`), «→ rutina» (`ea_registrar_rutina`) y siempre
«≡ en el diario», que abre su evento `accion_ejecutada`. La propuesta recién
aceptada sigue a la vista con esos enlaces hasta que se cambia de sección.
`POST /api/acciones/ejecutar` devuelve `{texto, trabajo, datos, evento}`
(`evento`: id de ese evento) y `POST /api/propuestas/<id>/decidir`, al aceptar
una que se ejecuta bien, añade lo mismo en `resultado`.

API (JSON): `GET /api/estado` (servicios, orquestador y una entrada por rol con
`rol`, `agente`, `agente_nombre`, `modelo` y `ultimo_agente`), `GET /api/agentes`
(`{version, fijados, agentes}`: cada agente del catálogo con `id`, `nombre`,
`admite`, `rol`, `bin`, `ruta` (null si no está en el `PATH`), `perfil`, `modelo`,
`topes`, `modelo_accesible` y `presencia` (`{viva, rol, equipo, trabajo, desde, latido}` o `null`, como `ea agentes --json`); `fijados`, los roles que fija una variable de entorno),
`GET /api/trabajos`, `/api/trabajos/<id>`, `/api/trabajos/<id>/diff` (`?fase=N`: el de esa fase de una obra, entre sus instantáneas de árbol, como `ea trabajo diff --fase N`; 404 si no hay instantánea),
`/api/colas`, `/api/semaforos`, `/api/rutinas` (las activas; `?retiradas=1`, las
retiradas; cada fila y la ficha llevan `ubicacion`: `activa`, `retirada` o `sin_fichero`),
`/api/rutinas/<slug>/versiones` (`[{version, creada_en, creada_por}]`, la más
reciente primero, sin la definición), `/api/rutinas/<slug>/versiones/<n>`
(`{slug, version, creada_en, creada_por, contenido}`; 404 si no existe, 400 si
`<n>` no es un entero), `/api/diario` (`?trabajo=`,
`actor`, `desde`, `hasta`, `resultado=ok|error`, `accion=<prefijo>` —solo
`[a-z_]`, si no 400—, `latidos=1`, `limite`, `antes_de=<id>` —cursor: los
eventos anteriores a ese id con los mismos filtros, estable aunque lleguen
nuevos; no numérico, 400—; cada evento corregido trae
`corregido_por: [ids]`); y, con `Content-Type:
application/json`, `POST /api/diario/anotar` (`{"texto", "corrige"?: id,
"trabajo"?}` → 201 con el evento `usuario_nota`, actor `usuario`; 400 sin escribir
nada si el texto está vacío o pasa de 10.000 caracteres, si `corrige` no existe,
si el trabajo no tiene formato de id o si sobra algún campo), `POST /api/trabajos/<id>/{cerrar,cancelar,continuar,reformular,confirmar}`
(`reformular` lleva `{"nota": "...", "fase"?: N}` —`fase` solo en obras; fuera
de rango o en una misión, 400 sin escribir nada—, `confirmar` `{"contexto": "..."}`),
`POST /api/trabajos` (crea una misión o una obra; misión: `{"modo"?: "mision", "titulo", "worktree",
"objetivo", "criterio", "topes"?: {"iteraciones"?, "tiempo_s"?}}` → 201 `{id, estado}`;
`creada_por` es `usuario` y no admite skills ni otros campos; 400 si falta el
criterio o el worktree no es una ruta absoluta existente dentro de un repo git
con al menos un commit; los topes se recortan al techo; una obra es `{"modo":
"obra", "titulo", "worktree", "objetivo", "fases": [{"titulo", "instrucciones",
"entregable", "criterio"}], "revisar_cada": n|null, "topes"?}`, sin `criterio`
general, con 1-20 fases numeradas por su orden y `revisar_cada` obligatorio
—`null` es «solo al final», si no entre 1 y el número de fases—; 400 si falta
o una fase no tiene criterio), `GET /api/trabajos/nuevo` (modos, techo, topes por
defecto y worktrees sugeridos para el formulario) y
equipos de agentes (spec 019): `GET /api/equipos` (una fila por equipo con sus miembros y si están vivos), `GET /api/equipos/<id>` (miembros con presencia y cola, y los últimos trabajos con su informe; 404 si no existe), `POST /api/equipos` (despertar; exige la clave; `{"titulo", "objetivo", "worktree", "miembros": [{"agente", "rol"}] | "todos": true, "terminal"?: bool}` → 201 `{equipo, omitidos, sesiones}`; mismas reglas que `ea equipo despertar` —roles que admite cada agente, sin repetir, ejecutor con modelo local, tope `orquestador.paralelo`, un agente en un solo equipo despierto, binarios presentes—, 400 con el motivo y nada escrito; `creado_por` es `usuario`; con `terminal` abre una terminal por cada planificador y revisor con `ea equipo sesion <id> <agente>` y lo apunta como `equipo_sesion_abierta`) y `POST /api/equipos/<id>/dormir` (exige la clave; lo pendiente de sus ejecutores vuelve a la cola de `modelo-local`); `POST /api/trabajos` admite además `equipo`, `asignado_a`, `revisor` y `worktree_propio` (400 con las validaciones del bus); `POST /api/rutinas/<slug>/lanzar` (`{"entradas": {"clave": "valor"}}` opcional; sin cuerpo, las de por defecto; 400 si una entrada no está declarada o se sale de los límites, sin crear trabajo) y `POST /api/rutinas/<slug>/previa` (mismo cuerpo; devuelve `{rutina, version, objetivo, criterio}` sin escribir nada ni ejecutar el criterio); `POST /api/rutinas/validar` (`{"toml": "..."}`; devuelve `{rutina, version, anterior: {version, definicion}|null, previa}` sin escribir nada) y `POST /api/rutinas/registrar` (mismo cuerpo; 201 `{rutina, version, anterior}`; 400 con el mensaje del bus y nada escrito si el TOML no vale o la versión no sube; deja `rutina_registrada` en el diario); la memoria de Claude Code va por
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
versión vieja o fijada por el entorno); la biblioteca, por `GET /api/biblioteca` (`?agente=&tipo=&categoria=&activado=`; `{elementos, avisos, categorias, total, agentes_sesion}`, cada elemento ya redactado y con `cambio: {posible, motivo}` y `sesion`), `GET /api/biblioteca/<id>` (el id con `/` codificado como `%2F`; ficha y `uso`), `GET /api/biblioteca/uso` (informe, con caché) y `POST /api/biblioteca/{simular,ejecutar}` (solo `ea_activar_herramienta`, `ea_desactivar_herramienta` y `ea_lanzar_sesion`, con el cuerpo de `/api/acciones/*`). Solo atiende `Host: localhost|127.0.0.1`.

Ejecuciones de las rutinas (solo lectura, leídas del bus): en ↻ Rutinas, la
columna «última» de cada fila dice el estado de su trabajo más reciente y hace
cuánto, y la ficha empieza por las últimas 10 ejecuciones (estado, versión de
la rutina, duración y detalle), cada una con un botón a la ficha de su trabajo
(`#trabajos/<id>`). En la API, cada fila de `GET /api/rutinas` lleva `ultima` (el trabajo más reciente de la rutina, o
`null`) y `GET /api/rutinas/<slug>` lleva `ejecuciones` (los 10 más recientes,
el más nuevo primero), con `{trabajo, estado, version, resultado, duracion_s,
detalle, creado, actualizado}`; `resultado` y `duracion_s` salen del informe y
son `null` mientras no lo haya.

Historial de una herramienta (solo lectura, leído del diario): la ficha de
cada herramienta de ▶ Acciones, también de las directas, termina con sus
últimos 10 eventos: `accion_ejecutada`/`accion_fallida`, `propuesta_encolada`
y `propuesta_decidida` (también el rechazo, que solo nombra la propuesta) y las
invocaciones por MCP (`mcp_<x>`, `mcp_<x>_simulada`, `mcp_<x>_denegada`), con
hora, resultado, actor y canal; «#n» lleva al evento en el diario y «◇ n», a su
propuesta. En la API, `GET /api/acciones/<nombre>/historial` devuelve
`[{id, ts, actor, accion, resultado, canal, proponente, propuesta, trabajo,
error, duracion_ms}]`, el más reciente primero (`canal` es el del evento o
`mcp`; 404 si la herramienta no está en el catálogo).

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

Para ellas está **⌨ terminal** (en ◇ Propuestas, y en ▶ Acciones tras
simular): `POST /api/terminal/abrir` con `{"propuesta": <id>}` o
`{"herramienta": …, "argumentos": {…}}` abre una terminal real
(`xdg-terminal-exec` u `omarchy-launch-terminal`) con
`ea propuestas decidir <id> --aceptar` o `ea acciones ejecutar <h> --args '<json>'`
ya escrito, y `--esperar` para que la ventana no se cierre sin leer el
resultado. **La web solo abre la ventana**: la simulación y la frase
`confirmo <id>` se escriben en esa TTY. El comando sale de una lista cerrada
(una propuesta pendiente irreversible, o una herramienta supervisada que la
web no puede confirmar con argumentos que validan contra sus campos), va como
argv sin shell, el `ea` es el de la carpeta de `ea-web` (no el del `PATH`) y
cualquier otra cosa, texto libre incluido, da 400. Queda en el diario como
`accion_terminal_abierta`.

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
