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
micrófono abierto de forma continua). Ambas recorren el mismo turno. Sin
mutaciones todavía. Un panel web (`ea-web`) para ver y gestionar todo desde
el escritorio.

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
ea propuestas decidir <id> --aceptar|--rechazar
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
