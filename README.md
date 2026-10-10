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
propuesta en cada punto donde haría falta mutar. Voz (fase 5), operativa con
los modelos descargados: el propio orquestador escucha, transcribe, pregunta
al modelo local y habla la respuesta, con activación manual (`ea voz hablar`)
o, si el usuario la activa en `voz.toml`, con palabra de activación
(«hola asistente» por defecto; desactivada por defecto porque implica el
micrófono abierto de forma continua). Ambas recorren el mismo turno.
Mutaciones supervisadas (fase 6): las herramientas que cambian el sistema
solo simulan o encolan una propuesta con acción estructurada, y la ejecuta el
usuario al aceptarla (ver «Mutaciones supervisadas»). Bóveda compartida
(fase 7): notas en regiones por autor, escritor único serializado, búsqueda
indexada con resultados acotados y promoción a la región compartida decidida
por el usuario (ver «Bóveda compartida»). Un panel web (`ea-web`) para ver y
gestionar todo desde el escritorio.

## Estructura

- `crates/ea-store`: capa de datos compartida (SQLite embebido). Único punto
  de acceso a la base de datos; el resto de crates la reutilizan sin
  reimplementar esquema.
- `crates/ea-acciones`: capa interna de herramientas. Catálogo único con
  nivel de riesgo, simulación, revalidación, ejecución y el flujo propuesta →
  aceptación → ejecución. Aquí vive la lógica de sistema operativo.
- `crates/ea-boveda`: la bóveda de notas (§8.3). Único código que escribe
  ficheros de la bóveda: escritor serializado, regiones, contrato de cabecera,
  promoción, lectura y búsqueda acotadas. El índice FTS5 vive en `ea-store`.
- `crates/ea-cli`: binario `ea`, interfaz de terminal.
- `crates/ea-mcp`: binario `ea-mcp`, servidor de herramientas MCP (HTTP,
  `127.0.0.1`) consumido por Claude Code y por `ea-proxy`/`ea-orchestrator`.
- `crates/ea-proxy`: binario `ea-proxy`, proxy de inferencia local (HTTP,
  `127.0.0.1`) — el único punto que habla con `llama-server`.
- `crates/ea-orchestrator`: binario `ea-orchestrator`, ejecutor de rutinas y
  voz embebida (activación manual y palabra de activación).
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
ea propuestas ver <id>                       # acción, riesgo, simulación, ejecución
ea propuestas decidir <id> --aceptar|--rechazar   # aceptar una con acción la ejecuta
ea acciones listar|simular|ejecutar          # catálogo de herramientas y ejecución directa
ea boveda estado|listar|buscar|leer          # ver la bóveda (ver «Bóveda compartida»)
ea boveda promover <ruta>                    # copiar una nota a la región compartida
ea voz hablar                                # activación manual (ver más abajo)
ea voz estado                                # motor de voz y palabra de activación
ea tui                                       # interfaz interactiva
```

La base de datos vive en `~/.local/share/ecosistema-asistente/ecosistema.db`
(o en la ruta de `$EA_DB_PATH` si está definida).

### Servidor de herramientas MCP (`ea-mcp`)

```bash
./target/release/ea-mcp        # arranque manual, escucha en 127.0.0.1:8765
# puerto configurable con $EA_MCP_PORT
```

Herramientas expuestas (un solo catálogo, `ea-acciones`), con su nivel de
riesgo (§7.3):

| Herramienta | Riesgo | Desde un agente | Modelo local |
|---|---|---|---|
| `ea_status`, `ea_listar_tareas`, `ea_ver_tarea`, `ea_listar_diario` | lectura | se ejecuta | sí |
| `ea_buscar_notas`, `ea_leer_nota` | lectura (resultados acotados por actor) | se ejecuta | sí |
| `ea_escribir_nota` | mutación reversible | se ejecuta, pero solo en la región propia del agente (excepción razonada: decisión 8) | sí |
| `ea_promover_nota` | mutación reversible | solo simula o propone | sí (solo propone) |
| `ea_crear_tarea` (con `simular: true`) | mutación reversible | se ejecuta (excepción razonada: es la primitiva de comunicación entre actores, §9.1; solo escribe una fila) | no |
| `ea_cambiar_estado_tarea` | mutación reversible | solo simula o propone | sí (solo propone) |
| `ea_encolar_rutina` | mutación reversible | solo simula o propone | no |
| `ea_reiniciar_servicio` (lista cerrada: `llama-server`, `ea-mcp`, `ea-proxy`, `ea-orchestrator`, `ea-web`) | mutación reversible | solo simula o propone | no |
| `ea_eliminar_rutina` (borra el fichero de la rutina) | **mutación irreversible** | solo simula o propone; solo se confirma desde la terminal | no |

Toda invocación queda registrada en el diario, atribuida al cliente que la
hizo: `claude-code` por defecto, `modelo-local` si el cliente se identifica
como `ea-proxy`, `orquestador` si se identifica como `ea-orchestrator`.
La autorización es del servidor, no del prompt: el `orquestador` (lo
desatendido) solo puede leer, sin excepciones; el `modelo-local` solo ve su
proyección reducida y nunca una irreversible.

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
mutante invocada por un agente (Claude Code por MCP, el modelo local, una
rutina) devuelve la **simulación** (`simular: true`: describe lo que haría sin
hacerlo) o, sin simular, **encola una propuesta con acción estructurada**
(herramienta, argumentos, riesgo, texto de la simulación y la precondición del
mundo en ese momento). No existe ninguna herramienta, argumento ni token con el
que un agente se confirme a sí mismo, y el código de `ea-mcp` y del orquestador
solo recibe una capa de *consulta* (`SoloConsulta`), que ni siquiera implementa
las operaciones que mutan el sistema.

Flujo propuesta → aceptación → ejecución:

1. Un agente propone (`ea_cambiar_estado_tarea` …) o una rutina lo hace en un
   paso `propuesta` con `accion = { herramienta = "...", argumentos = {...} }`,
   validada al cargar la rutina contra el catálogo (una herramienta de lectura,
   una inexistente o unos argumentos fuera de dominio hacen fallar la carga).
   Una propuesta idéntica pendiente no se duplica y hay un tope de 30 con acción.
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
4. Las propuestas antiguas de solo texto siguen funcionando: aceptar = constancia.

Política por canal (§11.1): lectura, automática; reversible, la confirma el
usuario en la terminal o en `ea-web`; irreversible, **solo desde la terminal**
(`ea-web` responde 403 y la propuesta sigue pendiente; MCP y voz no tienen
camino de confirmación). La terminal exige TTY real en stdin y stdout: un
agente que lance `ea` como subproceso sin terminal no puede confirmar. Los
botones de `ea-web` (estado de tareas, reiniciar servicios, encolar rutinas) y
`ea tareas actualizar` pasan por la misma capa y dejan el mismo
`accion_ejecutada` en el diario. `ea acciones listar|simular|ejecutar` es la
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
origen: "tarea:12"                  # tarea:<id> | rutina:<slug> | manual
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

**Búsqueda y lectura acotadas.** El modelo local nunca ve el sistema de
ficheros: solo `ea_buscar_notas` y `ea_leer_nota`. La búsqueda es SQLite FTS5
(la versión enlazada de `rusqlite` lo trae; migración `0004`), sin acentos ni
mayúsculas, con todos los términos exigidos y el texto del usuario nunca
interpretado como sintaxis de consulta. Cotas por actor (el contexto del
modelo local es de 8192 tokens):

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

**Rutinas (§8.1).** La especificación dice que el documento de cada rutina
vive en la bóveda. **No se ha movido:** siguen en
`~/.config/ecosistema-asistente/rutinas/` (TOML, cargados por el orquestador),
y el escritor único no gestiona ficheros `.toml`. Es aditivo y sin cambios de
código: `rutinas_dir` de `orchestrator.toml` puede apuntar a una carpeta de la
bóveda (fuera de las tres regiones, porque no son notas de agente) si el
usuario quiere verlas en Obsidian. No se ha hecho por defecto porque mezclaría
un fichero ejecutable con notas que un agente puede reescribir.

**Limitaciones conocidas.** El orden entre escritores que esperan no es FIFO.
Las ediciones externas no dejan evento en el diario (solo se reflejan en el
índice). No hay borrado ni renombrado de notas por herramienta (los agentes
sobrescriben; borrar es cosa del usuario en el sistema de ficheros). Los
agentes solo "reemplazan" notas propias, no hacen *append*. El catálogo local
pasa de 5 a 9 herramientas, lo que puede restar fiabilidad al modelo de 7B al
elegir; está sin medir con el `llama-server` real más allá de los tests
existentes. La TUI es una fotografía: no promueve desde dentro.

### Proxy de inferencia local (`ea-proxy`)

```bash
./target/release/ea-proxy      # arranque manual, escucha en 127.0.0.1:8090
# puerto configurable con $EA_PROXY_PORT; resto de opciones en
# ~/.config/ecosistema-asistente/proxy.toml (perfiles, endpoints)
```

Dos endpoints:

- `POST /v1/chat/completions`: forma de chat completion de OpenAI. Resuelve
  hasta 3 llamadas acotadas por gramática (clasificar → elegir herramienta →
  extraer argumentos) contra el catálogo reducido de solo lectura, ejecuta
  contra `ea-mcp` y, con una última llamada sin acotar, redacta el resultado
  en una o dos frases de texto llano — la voz lee en alto lo que llegue en
  `content`. El JSON crudo de la herramienta va aparte, en
  `resultado_herramienta`.
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
(el ejecutor los detecta y aborta si aparecen): `accion` (invoca una
herramienta de solo lectura del catálogo), `decision` (el modelo local elige una
rama de un conjunto cerrado, vía `ea-proxy`), `propuesta` (encola una
propuesta en vez de actuar — el único mecanismo desatendido para "esto
necesita al usuario"; puede llevar una `accion` estructurada, ver «Mutaciones
supervisadas»), `fin`. Presupuestos duros por ejecución
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

### Voz (activación manual y palabra de activación)

Corre embebida dentro de `ea-orchestrator` (§6.3 de la especificación: el
proceso que posee el dispositivo de audio es el que decide cuándo hay voz),
sobre `sherpa-onnx` — un único runtime para reconocimiento, síntesis,
detección de actividad vocal y palabra de activación, en CPU (la GPU queda
reservada al modelo de lenguaje). `ea voz hablar` no captura ni reproduce
nada por sí misma: le pide al orquestador, por una superficie HTTP local
(`127.0.0.1:8091` por defecto: `POST /v1/voz/hablar` y `GET /v1/voz/estado`),
que escuche.

**Ficheros de modelo** — se descargan a mano (sin script de instalación en
esta fase, mismo criterio que el modelo de `llama-server`) bajo
`~/.local/share/ecosistema-asistente/voz/`. Los nombres son los reales de
cada descarga y los valores por defecto del código:

| Fichero (bajo `voz/`) | Qué es | De dónde |
|---|---|---|
| `whisper-small/small-encoder.int8.onnx`, `small-decoder.int8.onnx`, `small-tokens.txt` | ASR, Whisper-small multilingüe (int8: 1,6x más rápido que fp32 con el mismo resultado) | Hugging Face `csukuangfj/sherpa-onnx-whisper-small` |
| `vits-coqui-es-css10/model.onnx`, `tokens.txt` | TTS en español, VITS (a nivel de carácter: no necesita espeak-ng) | `vits-coqui-es-css10.tar.bz2` en la release `tts-models` de `k2-fsa/sherpa-onnx` |
| `silero_vad.onnx` | Detección de actividad vocal | release `asr-models` de `k2-fsa/sherpa-onnx` |
| `kws-zipformer-es-kroko/encoder.onnx`, `decoder.onnx`, `joiner.onnx`, `tokens.txt` | Detector de la palabra de activación (opcional, solo si se activa) | Hugging Face `csukuangfj/sherpa-onnx-streaming-zipformer-es-kroko-2025-08-06` |

```bash
D=~/.local/share/ecosistema-asistente/voz; mkdir -p $D/whisper-small $D/vits-coqui-es-css10 $D/kws-zipformer-es-kroko
HF=https://huggingface.co/csukuangfj; GH=https://github.com/k2-fsa/sherpa-onnx/releases/download
for f in small-encoder.int8.onnx small-decoder.int8.onnx small-tokens.txt; do
  curl -L -o $D/whisper-small/$f $HF/sherpa-onnx-whisper-small/resolve/main/$f; done
curl -L $GH/tts-models/vits-coqui-es-css10.tar.bz2 | tar xj -C /tmp
cp /tmp/vits-coqui-es-css10/{model.onnx,tokens.txt} $D/vits-coqui-es-css10/
curl -L -o $D/silero_vad.onnx $GH/asr-models/silero_vad.onnx
for f in encoder.onnx decoder.onnx joiner.onnx tokens.txt; do   # solo para la palabra de activación
  curl -L -o $D/kws-zipformer-es-kroko/$f $HF/sherpa-onnx-streaming-zipformer-es-kroko-2025-08-06/resolve/main/$f; done
```

Si usas otro build, ajusta las rutas en `~/.config/ecosistema-asistente/voz.toml`
(mismo mecanismo de configuración-con-defaults que `orchestrator.toml` y
`proxy.toml`; claves `whisper_encoder`, `whisper_decoder`, `whisper_tokens`,
`vits_model`, `vits_tokens`, `vad_model`). Si algún fichero obligatorio falta,
`ea-orchestrator` arranca igual (rutinas y heartbeat siguen funcionando) pero
`/v1/voz/hablar` responde 503 hasta que se resuelva. `ea voz estado` muestra
qué está cargado.

**Activación manual**

```bash
ea voz hablar
# "Escuchando…" — habla; el VAD corta sola la grabación tras el silencio
# (tope duro de 15s; si en 20s no hay voz, desiste). Crea una tarea
# (origin=voz, asignada a modelo-local), la responde vía ea-proxy
# (POST /v1/chat/completions, el mismo endpoint de "tareas más abiertas" de
# la fase 3) y habla la respuesta.
```

Si no se detecta voz (o la transcripción sale vacía) no se crea tarea: se
habla un aviso breve y queda un evento `voz_transcripcion_descartada` en el
diario — la mitigación de §6.6 contra texto inventado en silencios. Solo
puede haber un turno de voz a la vez (el micrófono es de uno solo): una
segunda petición recibe `409`.

**Palabra de activación**

Desactivada por defecto: implica que el micrófono esté abierto de forma
continua (el audio se procesa en memoria y se descarta; solo se guarda en el
diario que hubo una detección, nunca el audio). Se activa a conciencia en
`~/.config/ecosistema-asistente/voz.toml` y se reinicia el servicio:

```toml
[palabra_activacion]
activada = true
palabra = "hola asistente"   # cualquier palabra o frase en español
# alias = ["ola asistente"]  # otras formas en que el reconocedor puede oírla
# umbral = 0.25              # 0-1; más alto = menos falsos positivos y más fallos
# puntuacion = 1.0           # refuerzo de la palabra en la búsqueda
# enfriamiento_s = 3.0       # tras activarse, ignora otras activaciones
# exigir_voz = true          # exige que el VAD vea voz justo antes
```

Al oír la palabra suena un pitido breve y, desde ahí, sigue exactamente el
mismo turno que la activación manual (`crates/ea-orchestrator/src/turno.rs`):
escuchar, transcribir, tarea `origin=voz`, `ea-proxy`, hablar. Hay que hacer
una pausa tras la palabra y esperar el pitido; lo que se diga antes de que
suene no se captura. Se desactiva quitando `activada` (o poniéndolo a
`false`) y reiniciando `ea-orchestrator.service`.

*Modelo.* `sherpa-onnx` solo publica detectores de palabra clave
preentrenados para chino e inglés. Pero su detector es de vocabulario
abierto: acepta cualquier transductor *streaming* más una lista de palabras
expresadas en los tokens del propio modelo. Se usa un transductor streaming
entrenado en español (Kroko, zipformer, 150 MB) y la palabra se tokeniza
contra su vocabulario al arrancar (`voz/palabra.rs`), también con la primera
letra en mayúscula porque el modelo distingue mayúsculas. Así sirve cualquier
palabra en español sin reentrenar. La licencia de Kroko (`license: other`,
ver `Banafo/Kroko-ASR`) no es una licencia libre estándar: vale para uso
personal, hay que revisarla antes de cualquier otro uso. Alternativa
descartada, dentro del mismo runtime: VAD + Whisper sobre todo lo que se oiga
buscando la palabra en el texto — transcribiría de forma continua cualquier
conversación cercana y reintroduce el texto inventado de §6.6.

*Falsos positivos* (§6.6): umbral y refuerzo configurables; el VAD debe haber
visto voz justo antes de la detección (`exigir_voz`); enfriamiento tras cada
activación; el micrófono se libera durante el turno, así que el asistente no
se activa con su propia voz; y el turno posterior conserva su puerta de VAD y
el descarte de transcripciones vacías. Si el modelo de la palabra falta, el
orquestador arranca igual y `ea voz estado` lo dice.

*Diario:* `voz_palabra_escucha_iniciada` (el micrófono queda abierto),
`voz_palabra_detectada`, `voz_palabra_descartada` (con motivo: `sin_voz` o
`enfriamiento`), `voz_escucha_iniciada` (con `disparador`: `manual` o
`palabra_activacion`), `voz_transcripcion`, `voz_respuesta`, `voz_habla`,
`voz_transcripcion_descartada`.

**Latencias medidas** (CPU de 16 hilos, modelos cargados): carga de los tres
modelos de voz ~1,2 s al arrancar (+1,8 s con la palabra de activación);
síntesis ~0,17-0,19 s para 3 s de audio (factor tiempo real 0,06);
reconocimiento Whisper-small int8 ~1,2 s para 3 s de audio (fp32: 1,9 s);
detección de la palabra ~45-75 ms por cada 1,3-2,5 s de audio, es decir
~5 % de un núcleo escuchando de forma continua. Cadena completa medida en
la máquina (palabra → pitido → pregunta → respuesta hablada): ~12 s, de los
que ~1,2 s son el modelo de lenguaje y el resto la espera de silencio, el
reconocimiento y la reproducción de la respuesta.

**Ajuste del micrófono.** En el portátil actual la entrada interna tiene
60 dB de ganancia (`Capture` 30 dB + `Internal Mic Boost` 30 dB) y se satura
con el ruido ambiente (RMS ~6500 sobre 32768 en reposo); con el refuerzo a 0
queda en ~700. Con la saturación, el VAD toma el ruido por voz y Whisper
inventa texto. Si ocurre, baja `Internal Mic Boost` (`amixer -c 2 sset
'Internal Mic Boost' 0`) antes de activar la palabra de activación.

**Pruebas.** Las de lógica (tokenización, filtro de falsos positivos, guarda
de turno único) corren con `cargo test`. Las que usan los modelos reales,
sin micrófono ni altavoces (síntesis, reconocimiento de la propia síntesis,
VAD sobre silencio/ruido/voz, palabra de activación sobre audio sintético),
van con `#[ignore]`:

```bash
cargo test -p ea-orchestrator --release --test voz_modelos -- --ignored --nocapture --test-threads=1
```

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
al arrancar"). Aceptar una propuesta con acción la ejecuta por la capa interna
(`ea-acciones`), igual que `ea propuestas decidir`; las irreversibles no se
confirman desde aquí (403): solo desde la terminal. El panel muestra la
simulación, el riesgo, quién propuso y el resultado de la ejecución, y aceptar
una propuesta con acción pide un segundo clic.

| Símbolo | Sección | Tecla |
|---|---|---|
| ⌂ | Inicio: propuestas por decidir, problemas del sistema, tareas en curso, rutinas y lo último que terminó | `1` |
| ▤ | Tareas: crear, iniciar, completar, fallar, cancelar, repetir ↺ | `2` |
| ≡ | Diario (♡/♥ muestra u oculta los latidos) | `3` |
| ↻ | Rutinas: pasos, ejecuciones, encolar | `4` |
| ◇ | Propuestas: aceptar ✓ (ejecuta; ⌨ = solo terminal) o rechazar ✕ | `5` |
| ▶ | Acciones: el catálogo de herramientas; las supervisadas se simulan y se ejecutan | `6` |
| ▦ | Servicios (reiniciar ↻, pide confirmar), orquestador y agentes | `7` |
| ▥ | Bóveda: listar, buscar, leer y promover | `8` |

La ficha de una tarea (abrirla en ⌂ o ▤) es donde se comprueba un resultado:
duración, petición, resultado (el error, destacado), su línea temporal con lo
que cada evento dice de sí mismo y, si es la ejecución de una rutina, los pasos
dados y las propuestas que dejó. `↺` la vuelve a lanzar como tarea nueva
enlazada a la original (la de una rutina se encola con `ea_encolar_rutina`) y
`✎` abre el formulario de tarea nueva relleno con sus datos.

La consola de acciones (▶) hace lo mismo que `ea acciones simular|ejecutar`:
se rellenan los argumentos, `◌ simular` describe lo que pasaría sin hacer nada
y `▶ ejecutar` lo aplica revalidando contra lo simulado (si el estado ha
cambiado entre medias, responde 409 y no hace nada). Las irreversibles se
pueden simular, pero el panel solo enseña el comando de terminal. Las
herramientas no supervisadas se listan a título informativo.

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
cargo test          # incluye ea-store, ea-boveda, ea-acciones, ea-mcp, ea-proxy, ea-orchestrator, ea-web
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
