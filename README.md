# gachapon

**Un agente en la nube piensa; un modelo en tu equipo hace el trabajo; el sistema comprueba que está hecho.**

[![Licencia MIT](https://img.shields.io/badge/licencia-MIT-blue.svg)](LICENSE)
![Rust stable](https://img.shields.io/badge/rust-stable-orange.svg)
![Linux · macOS · Windows](https://img.shields.io/badge/sistemas-Linux%20%C2%B7%20macOS%20%C2%B7%20Windows-lightgrey.svg)

gachapon (`ecosistema-asistente`) es un orquestador local, escrito en Rust, que
reparte el trabajo de programación entre dos agentes de IA:

- el **planificador** (por defecto, Claude Code) entiende lo que pides, lo
  convierte en una orden concreta con un criterio de éxito y revisa el
  resultado;
- el **ejecutor** (por defecto, Qwen Code sobre un modelo local con
  `llama-server`) hace el trabajo mecánico dentro de un sandbox, iterando hasta
  que el criterio pasa.

Así el modelo caro solo gasta tokens en pensar y revisar, el código se escribe
en tu máquina y **ningún agente se da el visto bueno a sí mismo**: el criterio
es un comando de shell que ejecuta el sistema, no el agente.

[![Cómo funciona gachapon en 20 segundos](docs/media/gachapon-explicado.gif)](docs/media/gachapon-explicado.mp4)

*Cómo funciona en 20 segundos, sobre el panel web ([vídeo en MP4](docs/media/gachapon-explicado.mp4)).*

## Índice

- [Cómo funciona](#cómo-funciona)
- [Qué incluye](#qué-incluye)
- [Requisitos](#requisitos)
- [Inicio rápido](#inicio-rápido)
- [Uso diario](#uso-diario)
- [Configuración](#configuración)
- [Seguridad](#seguridad)
- [Problemas frecuentes](#problemas-frecuentes)
- [Documentación](#documentación)
- [Desarrollo](#desarrollo)
- [Licencia](#licencia)

## Cómo funciona

```
  tú ──▶ planificador ──orden + criterio──▶ bus ──▶ ejecutor (sandbox, modelo local)
           ▲                                            │  escribe en el worktree
           │                                            ▼
           └──── informe + diff ◀── criterio OK/KO ◀── el sistema ejecuta el criterio
```

1. Le pides algo al planificador (por ejemplo, `/mision añade pruebas al parser`).
2. El planificador escribe una **orden**: objetivo, worktree de git y un
   **criterio de éxito** (`cargo test -p parser`, `pytest`, `grep -q …`).
3. La orden entra en el **bus**, una cola de ficheros en tu disco.
4. El **orquestador** (`ea-orchestrator`) la toma, bloquea el worktree y lanza
   al ejecutor en el sandbox del sistema, con topes de tiempo, turnos e
   iteraciones.
5. Al acabar cada intento, el orquestador **ejecuta él mismo el criterio**. Si
   falla, vuelve a lanzar al ejecutor con la evidencia del fallo.
6. El trabajo vuelve al planificador con un informe y el diff. Tú (o el
   planificador con `/revisar`) lo cierras, lo reformulas o lo continúas.

Hay tres formas de encargar trabajo:

| Modo | Para qué | Vuelve al planificador |
|---|---|---|
| **Misión** (`/mision`) | Alcanzar un objetivo iterando hasta que el criterio pasa | Al terminar o al quedar bloqueado |
| **Obra** (`/obra`) | Implementar un plan largo por fases, cada una con su criterio | Al terminar, o cada N fases para revisar |
| **Rutina** (`/rutina`) | Repetir un proceso conocido (TOML versionado, programable) | Solo si falla y tú lo confirmas |

## Qué incluye

- **CLI y TUI** (`ea`): crear y revisar trabajos, colas, rutinas, ajustes, diario.
- **Panel web** (`ea-web`, `http://localhost:8092`): escena en vivo con una
  máquina de gashapon por agente, bandeja «Te espera», trabajos, rutinas,
  propuestas, bóveda de notas, memoria de Claude Code, biblioteca de
  herramientas y ajustes. Solo escucha en `127.0.0.1`.
- **App de escritorio** (`ea-escritorio`): el mismo panel en una ventana
  nativa, con icono en la bandeja y avisos del sistema.
- **Servidor MCP** (`ea-mcp`, `127.0.0.1:8765`): el planificador consulta el
  estado y propone cambios; lo que muta el sistema siempre lo aceptas tú.
- **Agentes intercambiables**: Claude Code, Qwen Code, Codex, OpenCode y
  Antigravity, cada uno en los roles que admite.
- **Equipos**: varios agentes trabajando a la vez, cada uno con su cola y, si
  hace falta, su propio `git worktree`.
- **Diario** de todo lo que pasa, append-only a nivel de base de datos.

| Agente | Planificador | Ejecutor |
|---|:-:|:-:|
| Claude Code | ✓ (por defecto) | |
| Qwen Code | | ✓ (por defecto) |
| Codex | ✓ | ✓ |
| OpenCode | ✓ | ✓ |
| Antigravity (`agy`) | ✓ | |

El ejecutor siempre usa un modelo **local**; solo el planificador puede usar
uno en la nube. Un mismo agente no ocupa los dos roles a la vez.

## Requisitos

| | Linux | macOS | Windows |
|---|---|---|---|
| Rust ≥ 1.89 (1.90 para la app) | solo para compilar desde el código | ídem | ídem |
| Git | sí | sí | sí |
| Sandbox del ejecutor | `bubblewrap` (`bwrap`) | incluido (`sandbox-exec`) | no hay: usa **WSL2** (el instalador lo prepara) |
| Servicios | `systemd --user` | `launchd` | Programador de tareas |

Además:

- **Un modelo local** con API compatible con OpenAI, por ejemplo
  [`llama-server`](https://github.com/ggml-org/llama.cpp) de llama.cpp. Por
  defecto se espera en `http://127.0.0.1:8080/v1` con el nombre
  `qwen3-coder-30b` (Qwen3-Coder-30B-A3B rinde bien en una GPU de 24 GB).
- **Los agentes** que vayas a usar en su `PATH`: por defecto
  [Claude Code](https://docs.anthropic.com/claude-code) y
  [Qwen Code](https://github.com/QwenLM/qwen-code).
- Para la app de escritorio en Linux: WebKitGTK 4.1 (`libwebkit2gtk-4.1`) y
  `libayatana-appindicator3`.

El instalador detecta todo esto y ofrece instalar lo que falte con tu gestor de
paquetes (`pacman`, `apt`, `dnf`, `zypper`, `brew` o `winget`).

## Inicio rápido

### 1. Instala en un paso

**Linux y macOS:**

```bash
curl -fsSL https://jesus13g.github.io/gachapon/instalar.sh | sh
```

**Windows** (PowerShell; o descarga y ejecuta `gachapon-instalar.exe` de la
[última Release](https://github.com/jesus13g/gachapon/releases/latest)):

```powershell
irm https://jesus13g.github.io/gachapon/instalar.ps1 | iex
```

El instalador (`ea instalar`) hace esto:

1. Comprueba lo que hace falta: sandbox, WebView, git, `llama-server`, el modelo
   local y los agentes.
2. Ofrece instalar lo que falte. Cada oferta pide su propia confirmación.
3. Enseña el plan completo y pide **una** confirmación escrita.
4. Deja instalados:
   - los binarios, con `ea` en el `PATH`;
   - los servicios (`systemd --user`, `launchd` o el Programador de tareas) en
     marcha;
   - la app de escritorio en el menú del sistema, arrancando con la sesión;
   - las skills del ejecutor;
   - el adaptador de cada agente que tengas instalado;
   - el MCP (con la credencial del planificador) y el hook de subagentes en
     Claude Code.
5. Comprueba que el panel y el MCP responden y ofrece abrir la app.

En **Windows** pregunta la vía:

- **WSL2 (recomendada).** Instala WSL si falta; pide permiso de administrador y
  un reinicio, y después sigue solo. Pone gachapon dentro de WSL con el mismo
  aislamiento que en Linux y la app de escritorio nativa en Windows.
- **Nativa.** Sin aislamiento del ejecutor. Solo se instala si escribes
  `sin sandbox`.

Los binarios no están firmados: SmartScreen avisará («Más información →
Ejecutar de todos modos»). Cada paquete de la Release lleva una atestación de
procedencia que puedes comprobar con `gh attestation verify <fichero> --repo
jesus13g/gachapon` (ver el [manual](docs/manual.md)).

Hay dos cosas que el instalador no puede hacer por ti, y te las recuerda al
final con la orden exacta:

- **Iniciar sesión** en cada agente (por ejemplo, `claude`).
- **Arrancar el modelo local.** Si tu GPU tiene memoria suficiente, ofrece
  descargarlo, y te da la orden de `llama-server` (por defecto se espera en
  `http://127.0.0.1:8080/v1` con el nombre `qwen3-coder-30b`). ¿Otro modelo u
  otro puerto? `ea ajustes establecer modelos.local.nombre=mi-modelo
  modelos.local.base_url=http://127.0.0.1:8081/v1` (se aplica en caliente).

Repetir el comando **actualiza** sin duplicar nada. `ea desinstalar` lo quita
todo, salvo tus datos (base de datos, bóveda y ajustes).

### 2. Desde el código (desarrollo)

```bash
git clone https://github.com/jesus13g/gachapon.git
cd gachapon
./instalar.sh
```

Compila en release (también la app, si están sus dependencias) y ejecuta
`target/release/ea instalar`. En este modo los servicios usan `target/release`:
tras un `cargo build --release` basta reiniciarlos. Las skills de Claude Code
y los adaptadores de los otros planificadores se copian con la ruta real de las
guías del clon (`agentes/flujo/`); si cambias un adaptador, vuelve a ejecutar
`ea instalar`. La instalación paso a paso está en el manual
([Instalación por sistema](docs/manual.md#instalación-por-sistema)).

### 3. Comprueba

```bash
ea status     # orquestador, roles, trabajos, colas y semáforos
ea agentes    # qué agente ocupa cada rol y si su modelo responde
```

¿Usas Codex, OpenCode o Antigravity como planificador? Si están en el `PATH`
al instalar, sus adaptadores se instalan solos; los detalles están en el manual
([Adaptadores del planificador](docs/manual.md#adaptadores-del-planificador)).

El panel se abre con la app de escritorio, que le pasa la clave de sesión y
así puedes aceptar propuestas y lanzar trabajos. En el navegador
(`http://localhost:8092`) funciona en **solo lectura**.

### 4. Lanza tu primera misión

Desde Claude Code, en cualquier repositorio:

```
/mision crea un script hola.py que imprima "Hola, gachapon"
```

O directamente desde la terminal, sobre un repo de prueba:

```bash
mkdir ~/prueba-gachapon && cd ~/prueba-gachapon
git init && git commit --allow-empty -m "inicio"

ea trabajo crear --titulo "Hola" --worktree ~/prueba-gachapon \
  --objetivo 'Crea hola.py que imprima "Hola, gachapon"' \
  --criterio 'python3 hola.py | grep -q "Hola, gachapon"'

ea trabajo listar          # en_cola → ejecutando → en_revision
ea trabajo ver <id>        # informe: resultado, evidencia y diff
ea trabajo cerrar <id>
```

El worktree tiene que ser un repo de git con al menos un commit. Mientras
trabaja, el panel muestra la cápsula viajando entre las máquinas.

## Uso diario

**Desde el planificador** (Claude Code):

| Skill | Qué hace |
|---|---|
| `/mision <objetivo>` | Formaliza el objetivo con un criterio y lo encola |
| `/obra <plan>` | Divide un plan en fases con entregable y criterio cada una |
| `/rutina <proceso>` | Convierte un proceso repetible en una rutina versionada y la programa |
| `/revisar` | Revisa lo entregado: cierra, reformula o continúa |
| `/equipo` | Reparte un objetivo entre los miembros de un equipo despierto |

**Desde la terminal**, lo más habitual:

```bash
ea status                                  # vista general
ea tui                                     # interfaz interactiva
ea trabajo listar | ver <id> | diff <id>   # seguimiento
ea trabajo reformular <id> --nota "…"      # devolverlo al ejecutor con indicaciones
ea trabajo cerrar | cancelar <id>
ea rutina listar | lanzar <slug>
ea propuestas listar                       # lo que los agentes proponen y espera tu decisión
ea propuestas decidir <id> --aceptar
ea diario listar --trabajo <id>            # qué pasó y quién lo hizo
```

`ea --help` y `ea <comando> --help` describen todos los comandos; la referencia
completa está en el [manual](docs/manual.md#uso).

**Desde el panel**: **Actividad** (tecla `2`) reúne lo que te espera (las
propuestas por aceptar o rechazar), un calendario y el archivador de cada día con
los trabajos en tarjetas; **Rutinas** (`3`) las enseña en tarjetas y crea una
nueva con un formulario. `1`-`9` y `0` cambian de sección, `/` filtra, `?` abre la
ayuda y `t` un recorrido guiado por cada vista.

## Configuración

Todo se guarda en un único fichero opcional,
`~/.config/ecosistema-asistente/ajustes.toml` (plantilla comentada en
[`crates/ea-ajustes/config/ajustes.example.toml`](crates/ea-ajustes/config/ajustes.example.toml)).
Sin él, todo funciona con los valores por defecto.

```bash
ea ajustes ver                                  # cada valor, de dónde sale y cuándo se aplica
ea ajustes establecer roles.ejecutor=opencode   # cambiar el agente de un rol
ea ajustes establecer agente.techo_topes.tiempo_s=1800
```

| Qué | Dónde (Linux) | Variable de entorno |
|---|---|---|
| Ajustes | `~/.config/ecosistema-asistente/` | `EA_CONFIG_DIR` |
| Base de datos y bóveda | `~/.local/share/ecosistema-asistente/` | `EA_DB_PATH`, `EA_BOVEDA_DIR` |
| Bus (colas y trabajos) | `~/.local/state/ecosistema-asistente/` | `EA_ESTADO_DIR` |
| Puertos | web `8092`, MCP `8765` | `EA_WEB_PORT`, `EA_MCP_PORT` |

Las rutas de macOS y Windows, y la tabla completa de claves, están en el
manual: [Configuración](docs/manual.md#configuración-ajustestoml) y [Dónde queda
cada cosa](docs/manual.md#dónde-queda-cada-cosa).

## Seguridad

gachapon está pensado para dejar trabajar a un agente desatendido sin darle las
llaves de tu equipo:

- **Sandbox del ejecutor**: solo puede escribir en el worktree y en sus propios
  directorios de estado; no tiene herramientas web ni acceso a la clave del
  panel. En Windows nativo no hay sandbox y el ejecutor se niega a arrancar.
- **Topes duros**: tiempo, turnos e iteraciones; al vencer, se mata el árbol de
  procesos entero.
- **Nada se confirma solo**: lo que un agente quiere cambiar (programar una
  rutina, reiniciar un servicio…) queda como propuesta hasta que la aceptas.
  Lo irreversible exige una terminal real.
- **Solo local**: todos los servicios escuchan en `127.0.0.1` y el panel
  necesita una clave de sesión para escribir.
- **Un worktree, un escritor**: un semáforo impide que dos agentes escriban a
  la vez en el mismo repo.

Lo que el ejecutor escribe en el worktree no se deshace al cancelar: trabaja en
una rama o en un repo con los cambios guardados. Garantías por sistema en el
manual: [Seguridad](docs/manual.md#seguridad) y [Limitaciones por
sistema](docs/manual.md#limitaciones-y-garantías-por-sistema).

## Problemas frecuentes

**El panel dice «solo lectura».** Se abrió sin la clave de sesión (o `ea-web` se
reinició). Ábrelo con `ea-escritorio` o `ea-web-open`.

**El trabajo queda bloqueado nada más empezar.** Mira el motivo con
`ea trabajo ver <id>`. Lo habitual: el binario del ejecutor no está en el `PATH`
del servicio o el modelo local no responde (`ea agentes` lo comprueba). En
macOS, tras instalar un agente nuevo, vuelve a ejecutar `ea servicios instalar`
para que el servicio vea tu `PATH`.

**«base de datos del esquema antiguo».** La base es de una versión anterior y
no se migra. Para los servicios, guarda una copia de
`~/.local/share/ecosistema-asistente/ecosistema.db*`, bórrala y vacía
`~/.local/state/ecosistema-asistente/`.

**Claude Code solo puede leer por MCP (actor `desconocido` en el diario).**
El MCP identifica al planificador por una credencial y no por el nombre que
declara: vuelve a ejecutar `ea instalar` para registrar el MCP con
`ea mcp cabeceras` y reconecta (`/mcp` en Claude Code). Ver «Identidad del
cliente» en el manual.

**He cambiado el código y no veo el cambio.** Los servicios ejecutan los
binarios de `target/release/`: compila en release y reinícialos
(`systemctl --user restart ea-web ea-orchestrator ea-mcp` en Linux).

## Documentación

| Documento | Para qué |
|---|---|
| [docs/manual.md](docs/manual.md) | Referencia completa: comandos, ajustes, panel, MCP, bóveda, equipos, sistemas |
| [orquestador-claude-qwen.md](orquestador-claude-qwen.md) | Diseño original y sus razones |
| [CLAUDE.md](CLAUDE.md) | Mapa del código y reglas para trabajar en el repo |

## Desarrollo

```bash
cargo test --workspace     # toda la suite
cargo test -p ea-bus       # un solo crate
```

| Crate | Qué es |
|---|---|
| `ea-cli` | Binario `ea`: CLI, TUI e instalador (`ea instalar`) |
| `ea-orchestrator` | El orquestador: toma trabajos, lanza al ejecutor y corre el criterio |
| `ea-web` | API JSON y panel web |
| `ea-mcp` | Servidor MCP |
| `ea-bus` | Bus de ficheros: colas, estados, semáforos, rutinas |
| `ea-store` | SQLite: diario, propuestas, trabajos, índice de la bóveda |
| `ea-acciones` | Catálogo de herramientas con riesgo, simulación y propuestas |
| `ea-boveda` | Bóveda de notas (único escritor) |
| `ea-biblioteca` | Inventario de skills, plugins, MCP y subagentes de los agentes |
| `ea-ajustes` | Ajustes, validación y escritura atómica |
| `ea-plataforma` | Diferencias entre Linux, macOS y Windows |

Fuera del workspace: `escritorio/` (app de escritorio), `instalador/` (el
instalador de Windows, `gachapon-instalar.exe`), `claude/` y `agentes/` (skills y
adaptadores de los planificadores), `qwen/skills/` (skills del ejecutor), `web/`
(la página del proyecto) y `bench/` (banco de pruebas de modelos).
`escritorio/` e `instalador/` tienen su propio `Cargo.lock`: al actualizar
dependencias, actualiza los tres (ver «Versión de Rust, perfil de release y
lockfiles» en el [manual](docs/manual.md)).

Todo el proyecto (código, mensajes y documentación) está en español. Las
mejoras salen de la rama `desarrollo` y vuelven a ella; `main` es la versión
estable. Si cambias comandos, API o comportamiento visible, actualiza este
README y el [manual](docs/manual.md).

## Licencia

[MIT](LICENSE) © 2026 Jesus
