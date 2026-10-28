# Flujo · rutina

Procedimiento neutral del **planificador** para formalizar un proceso repetible
como rutina versionada (TOML). El planificador define el proceso **una vez**; el
ejecutor lo corre cada vez. El planificador no ejecuta la rutina.

## Reglas

- **Ningún agente se confirma a sí mismo.** Registrar, programar, retirar o
  corregir una rutina por MCP solo **simula o propone**; el usuario acepta en
  ◇ Propuestas del panel o con `ea propuestas decidir <n> --aceptar`. No pidas
  la terminal ni lances `systemctl`/`launchctl`/`schtasks` a mano.
- El criterio de éxito lo ejecuta el sistema; un criterio vacío no es éxito.

## Crear o actualizar

1. Entiende el proceso y descríbelo en pasos que el ejecutor actual pueda seguir
   (`ea agentes` dice cuál es y con qué modelo): concretos, con rutas y
   comandos. Un `criterio_exito` en shell que pase o falle.
2. Escribe `<slug>.toml` (slug: `[A-Za-z0-9_-]`, ≤ 64) en
   `rutinas/` en la raíz del repo o donde el usuario indique:

```toml
[rutina]
slug = "resumen-diario"
version = 1                      # solo puede subir
descripcion = "Qué hace, en una línea"
worktree = "/ruta/absoluta"      # raíz de un repo git con commits (nunca $HOME)
criterio_exito = "test -s {salida}"
salida_esperada = "Qué debe quedar hecho / dónde"

[entradas]                       # clave = valor por defecto; {clave} se sustituye en pasos y criterio
salida = "resumen.md"

[[pasos]]
texto = "Lee los ficheros de notas/ y escribe un resumen en {salida}."

[[pasos]]
texto = "Revisa que {salida} no esté vacío."

[topes]                          # opcional
turnos = 30
tiempo_s = 900
llamadas_herramienta = 60
iteraciones = 3
```

3. Una rutina **nueva** (su `version = 1`): `ea rutina registrar <slug>.toml`
   (valida; error si la versión no sube o si el worktree no es la raíz de un
   repo git con commits). Comprueba con `ea rutina ver <slug>`. Una **versión
   nueva** de una rutina que ya existe no la registra `ea rutina registrar` sin
   terminal (se niega y lo explica): **proponla** con `ea_registrar_rutina`
   (ver «Corrección»); el usuario la acepta, o la registra él desde su terminal
   con `ea rutina registrar` (la simula y le pide confirmar). Si el usuario
   prefiere decidirlo él también para una nueva, proponla igual o que la pegue
   en «＋ rutina» del panel (↻ Rutinas). Cada registro, por cualquier camino,
   queda en el diario como `rutina_registrada`.
4. Si se pide lanzarla ya: `ea rutina lanzar <slug> [--entrada clave=valor]`
   imprime el id del trabajo; su revisión sigue `agentes/flujo/revisar.md`.

## Programarla

El servicio `ea-rutina@<slug>.service` ya existe y solo ejecuta
`ea rutina lanzar <slug>` (crea el trabajo; no ejecuta nada). El horario se
**propone** por MCP con `ea_programar_rutina` (`slug` de una rutina registrada y
`calendario`, una expresión `OnCalendar` de una línea: `*-*-* 08:00:00` cada día,
`Mon *-*-* 09:00` los lunes, `daily`; fuera de Linux solo vale el subconjunto portable: `daily`, `HH:MM`, `Mon..Fri 09:00`…, ver «Rutinas programadas» en docs/manual.md):

1. Primero con `simular: true`: valida el calendario (`systemd-analyze` en Linux; fuera de Linux, el subconjunto portable) y
   enseña las próximas 3 ejecuciones y el horario actual. Si da error, corrige.
2. Luego sin simular: encola una propuesta. Dile al usuario que la acepte; al
   aceptarla se escribe el temporizador de la rutina (en Linux, `~/.config/systemd/user/ea-rutina@<slug>.timer`; un LaunchAgent en macOS o una tarea de `schtasks` en Windows) y se
   activa. También puede hacerlo él en la ficha de la rutina (↻ Rutinas,
   «◷ guardar»).

Para quitar el horario, lo mismo con `ea_desprogramar_rutina` (`slug`). Para
dejar de usar una rutina, propón `ea_retirar_rutina` (`slug`): su TOML va a la
papelera de `rutinas_dir` y se desprograma su timer; es reversible con
`ea_restaurar_rutina`, que no vuelve a programarlo.

## Corrección (paquete de corrección)

Si una rutina falla, el usuario confirma el paquete y llega a la cola del
planificador como trabajo `en_revision` con `trabajos/<id>/paquete-correccion.md`
(en `~/.local/state/ecosistema-asistente/`). Entonces:

1. Lee el paquete (definición, salida, error, contexto del usuario) y
   `ea trabajo ver <id>`.
2. Diagnostica: ¿paso ambiguo, criterio mal puesto, entrada que cambió? Corrige
   el TOML (parte de la definición registrada: `ea rutina ver <slug>`).
3. Sube `version` (+1) y **propón** la vN+1 por MCP con `ea_registrar_rutina` y
   el TOML completo: primero con `simular: true` (enseña el diff frente a la
   versión registrada y el criterio resultante; si da error, corrígelo), luego
   sin simular, que encola una propuesta. Dile al usuario que la acepte. Si
   prefiere hacerlo a mano, puede pegar el TOML en «✎ nueva versión» de la ficha
   de la rutina (↻ Rutinas).
4. Cuando la propuesta esté aceptada (`ea rutina ver <slug>` muestra la vN+1),
   `ea trabajo cerrar <id>`. Para probar la nueva versión: `ea_encolar_rutina`
   (otra propuesta) o que el usuario la lance desde la ficha.
