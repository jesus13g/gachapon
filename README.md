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
Los fallos, bloqueos y paquetes en espera avisan con `notify-send`.

### Seguridad

Qwen Code corre dentro de **bubblewrap** (`bwrap`): sistema de ficheros de solo
lectura salvo el worktree, un `/tmp` propio y `~/.qwen`; la red queda abierta para
llegar a `llama-server`. Con `sandbox = false` el agente corre con aprobación
total y puede escribir donde escriba tu usuario. Además: topes por ejecución
(turnos, tiempo, llamadas a herramientas, iteraciones), exclusión de las
herramientas web y semáforo para que los dos agentes no escriban a la vez.

### Configuración (`orquestador.toml`)

`~/.config/ecosistema-asistente/orquestador.toml`, todo opcional (plantilla en
`crates/ea-bus/config/orquestador.example.toml`). Es el único sitio donde se
nombra el modelo.

```toml
base_url = "http://127.0.0.1:8080/v1"
modelo = "qwen3-coder-30b"
qwen_bin = "qwen"
avisos_escritorio = true
sandbox = true

[topes]
turnos = 30
tiempo_s = 900
llamadas_herramienta = 60
iteraciones = 5
```

### Cambiar de modelo

`llama-server` lee `~/.config/llama-cpp/activo.env`, un enlace simbólico a un
`.env` por modelo:

```bash
ln -sf qwen3-coder-30b.env ~/.config/llama-cpp/activo.env
systemctl --user restart llama-server
```

Después ajusta `modelo` (y `base_url` si cambia) en `orquestador.toml` y reinicia
`ea-orchestrator`. No hay que tocar código.

## Actualizar desde una versión anterior

El esquema de la base de datos se ha reiniciado (una sola migración,
`0001_inicial.sql`; sin tareas ni `routine_runs`). Una base antigua **no se
migra**: `ea` y los servicios se niegan a abrirla con el mensaje «base de datos
del esquema antiguo» y no tocan nada. Tras actualizar hay que empezar con una
base nueva: parar los servicios, copiar `~/.local/share/ecosistema-asistente/ecosistema.db*`
a un lugar seguro, borrarlos, vaciar `~/.local/state/ecosistema-asistente/` y
volver a arrancar los servicios.

## Estructura

- `crates/ea-store`: capa de datos compartida (SQLite embebido): diario,
  propuestas, índice de la bóveda, trabajos, fases y rutinas.
- `crates/ea-bus`: el bus de ficheros: colas, estado de los trabajos, semáforo
  del worktree, rutinas versionadas y `orquestador.toml`.
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

Las elige Claude Code al formalizar (`/mision`, `/obra`). La sección `[skills]`
de `orquestador.toml` decide qué se puede elegir y quién:

- `habilitadas`: lista blanca del catálogo (vacía = todas).
- `max_por_trabajo` y `max_caracteres`: topes de contexto.
- `pueden_elegir`: la llave, los valores de `creada_por` autorizados (por defecto
  `claude-code`). Un trabajo con skills de otro origen se rechaza al crearlo y
  vuelve a validarse al ejecutarse: si no pasa, se bloquea sin llamar a Qwen.

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
ea trabajo reformular <id> --nota "..."      # vuelve al agente local
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
ea diario listar|ver <id>                    # listar acepta --trabajo <id>
ea propuestas listar|ver|decidir <id> --aceptar|--rechazar
ea acciones listar|simular|ejecutar          # catálogo de herramientas
ea boveda estado|listar|buscar|leer|promover|reindexar
ea tui                                       # interfaz interactiva
```

La base de datos vive en `~/.local/share/ecosistema-asistente/ecosistema.db`
(o en `$EA_DB_PATH`). Servicios de sesión (`systemd --user`): `llama-server`,
`ea-orchestrator`, `ea-mcp` y `ea-web`; las rutinas se programan con
`ea-rutina@<slug>.timer`, que solo crea el trabajo en el bus.

### Servidor de herramientas MCP (`ea-mcp`)

```bash
./target/release/ea-mcp        # arranque manual, escucha en 127.0.0.1:8765
# puerto configurable con $EA_MCP_PORT
```

Herramientas expuestas (un solo catálogo, `ea-acciones`), con su nivel de
riesgo (§7.3):

| Herramienta | Riesgo | Desde un agente |
|---|---|---|
| `ea_status` (orquestador, trabajos por estado, colas, semáforos), `ea_listar_trabajos`, `ea_ver_trabajo`, `ea_listar_diario` | lectura (el bus es solo de lectura desde MCP: crear o decidir trabajo es de la CLI y las skills) | se ejecuta |
| `ea_buscar_notas`, `ea_leer_nota` | lectura (resultados acotados por actor) | se ejecuta |
| `ea_escribir_nota` | mutación reversible | se ejecuta, solo en la región propia del agente |
| `ea_promover_nota` | mutación reversible | solo simula o propone |
| `ea_encolar_rutina` (crea el trabajo en el bus) | mutación reversible | solo simula o propone |
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
usuario y capacidad de simular un TTY o de hablar con `127.0.0.1:8092` podría
saltarse la confirmación reversible. La defensa es no darle a los agentes
ningún camino por las herramientas, los permisos de su propio entorno y que lo
irreversible exija escribir una frase en una terminal real.

### Bóveda compartida (fase 7)

Implementa §8.3: el sustrato de notas que se leen meses después, compartido
por el modelo local, Claude Code y el usuario.

**Dónde está.** Por defecto en `~/.local/share/ecosistema-asistente/boveda/`,
un directorio propio del ecosistema (no la bóveda personal de Obsidian). La raíz
se cambia con la variable `EA_BOVEDA_DIR` o con
`~/.config/ecosistema-asistente/boveda.toml`:

```toml
raiz = "~/ruta/a/una/carpeta"
```

Orden de resolución: variable de entorno, fichero, valor por defecto. Para
que Obsidian la abra, basta con abrir esa carpeta como bóveda, o apuntar la
configuración a una carpeta de una bóveda existente. **Eso es decisión del
usuario y no se ha hecho.** Conviene apuntar a una carpeta dedicada (p. ej.
`~/Obsidian/SegundoCerebro/asistente`) y no a la raíz de la bóveda: el
ecosistema solo ve, indexa y escribe tres subcarpetas de la raíz (las
regiones); todo lo demás que haya en ella le es invisible, pero crea en ella
las regiones y un fichero oculto `.ea-escritor.lock`.

**Regiones** (partición por autor, no por tema; decisiones 8 y 9):

| Carpeta | Quién escribe | Contenido |
|---|---|---|
| `modelo-local/` | el modelo local, con libertad total | sus notas |
| `claude-code/` | Claude Code, con libertad total | sus notas |
| `compartida/` | nadie directamente: solo recibe notas por **promoción** que decide el usuario | lo de confianza |

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
en la sección ▥ de la web (segundo clic). Efecto: **copia** la nota a
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
panel web (tecla `8`; el cuadro de filtro hace una búsqueda de texto completo).

**Limitaciones conocidas.** El orden entre escritores que esperan no es FIFO.
Las ediciones externas no dejan evento en el diario (solo se reflejan en el
índice). No hay borrado ni renombrado de notas por herramienta. Los agentes
solo reemplazan notas propias, no hacen *append*. La TUI es una fotografía: no
promueve desde dentro.

### Monitor: TUI y `ea-web`

`ea tui` es una fotografía de solo lectura. Su primera pestaña, **Orquestador**,
muestra las dos colas, el semáforo del worktree y los trabajos con su estado
(↑/↓ para moverse, Enter para ver el resumen del informe); **Rutinas** lista las
registradas con su versión, como `ea rutina listar`.

`ea-web` es un panel local en `http://localhost:8092` (solo `127.0.0.1`; puerto
con `$EA_WEB_PORT`): una página sin dependencias, a base de símbolos, con los
colores del tema activo de Omarchy. Se actualiza sola cada 3 s. Nunca habla con
Claude Code ni con el modelo; lee del bus y de SQLite, y sus acciones pasan por
el mismo `Bus` que la CLI.

| Símbolo | Sección | Tecla |
|---|---|---|
| ⌂ | Orquestador: cola `modelo-local` → cola `claude-code`, semáforo del worktree, avisos de paquetes de corrección en espera (con contexto y botón de confirmar), propuestas por decidir y tablero en cola → ejecutando → en revisión → en espera → cerrado | `1` |
| ▤ | Trabajos: lista con filtro por estado y ficha con informe, fases, diff plegable e historial; cerrar, reformular (con nota), continuar, confirmar y cancelar según el estado | `2` |
| ≡ | Diario (♡/♥ muestra u oculta los latidos) | `3` |
| ↻ | Rutinas registradas, con versión y próxima ejecución; ▶ lanzar | `4` |
| ◇ | Propuestas: aceptar ✓ (ejecuta; ⌨ = solo terminal) o rechazar ✕ | `5` |
| ▶ | Acciones: simular y ejecutar las herramientas supervisadas | `6` |
| ▦ | Servicios (reiniciar ↻, pide confirmar), orquestador y agentes | `7` |
| ▥ | Bóveda: listar, buscar, leer y promover | `8` |
| ✎ | Memoria: la memoria de Claude Code de cada proyecto (`~/.claude/projects/*/memory/*.md`); se lee renderizada como markdown y se edita en un editor de texto (✓ guardar o `Ctrl+S`, ◐ vista previa, ✕ descartar). No crea ni borra memorias, solo reescribe las existentes, avisa si cambiaron en disco y apunta `memoria_editada` en el diario | `9` |

`/` filtra, `j`/`k` mueve, `Enter` abre o cierra la ficha, `Esc` cierra, `?`
abre la ayuda y `t` abre el recorrido guiado, que va de vista en vista
iluminando cada zona (solo mira, nunca lanza nada; arranca solo la primera vez).

API (JSON): `GET /api/trabajos`, `/api/trabajos/<id>`, `/api/trabajos/<id>/diff`,
`/api/colas`, `/api/semaforos`, `/api/rutinas`; y, con `Content-Type:
application/json`, `POST /api/trabajos/<id>/{cerrar,cancelar,continuar,reformular,confirmar}`
(`reformular` lleva `{"nota": "..."}`, `confirmar` `{"contexto": "..."}`) y
`POST /api/rutinas/<slug>/lanzar`; la memoria de Claude Code va por
`GET /api/memoria`, `GET /api/memoria/<proyecto>/<fichero>` y
`POST /api/memoria/<proyecto>/<fichero>` (`{"contenido": "...", "version": "..."}`;
409 si la versión ya no es la del disco). Solo atiende `Host: localhost|127.0.0.1`.
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
