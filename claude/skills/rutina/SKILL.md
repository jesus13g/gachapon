---
name: rutina
description: Formaliza un proceso repetible como rutina versionada (TOML), la registra con `ea rutina registrar`, la lanza o la programa con un timer de systemd. También corrige una rutina cuando llega un paquete de corrección tras un fallo. Úsala para procesos que se repetirán.
---

# /rutina

Claude Code define el proceso **una vez**; el agente local lo ejecuta cada vez. Claude Code no ejecuta la rutina.

## Crear o actualizar

1. Entiende el proceso y descríbelo en pasos que un 30B local pueda seguir (concretos, con rutas y comandos). Un `criterio_exito` en shell que pase o falle.
2. Escribe `<slug>.toml` (slug: `[A-Za-z0-9_-]`, ≤ 64) en `~/Proyectos/ecosistema-asistente/rutinas/` o donde el usuario indique:

```toml
[rutina]
slug = "resumen-diario"
version = 1                      # solo puede subir
descripcion = "Qué hace, en una línea"
worktree = "/ruta/absoluta"      # repo git donde trabaja
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

3. `ea rutina registrar <slug>.toml` (valida; error si la versión no sube). Comprueba con `ea rutina ver <slug>`. Si el usuario prefiere decidirlo él, proponla con `ea_registrar_rutina` (ver «Corrección»), o que la pegue en «＋ rutina» del panel (↻ Rutinas).
4. Si se pide lanzarla ya: `ea rutina lanzar <slug> [--entrada clave=valor]` → id del trabajo; su revisión se hace con `/revisar`.

## Programarla

El servicio `ea-rutina@<slug>.service` ya existe y solo ejecuta `ea rutina lanzar <slug>` (crea el trabajo; no ejecuta nada). El horario se **propone** por MCP con `ea_programar_rutina` (`slug` de una rutina ya registrada y `calendario`, una expresión `OnCalendar` de una línea: p. ej. `*-*-* 08:00:00` cada día, `Mon *-*-* 09:00` los lunes, `daily`):

1. Primero con `simular: true`: valida el calendario con `systemd-analyze` y enseña las próximas 3 ejecuciones y el horario actual. Si da error, corrige el calendario.
2. Luego sin simular: encola una propuesta. Dile al usuario que la acepte en ◇ Propuestas del panel (o con `ea propuestas decidir <n> --aceptar`); al aceptarla se escribe `~/.config/systemd/user/ea-rutina@<slug>.timer` y se activa. También puede hacerlo él en la ficha de la rutina (↻ Rutinas, «◷ guardar»).

Para quitar el horario, lo mismo con `ea_desprogramar_rutina` (`slug`). No copies la plantilla ni lances `systemctl` a mano.

## Corrección (paquete de corrección)

Si una rutina falla, el usuario confirma el paquete y llega a la cola `claude-code` como trabajo `en_revision` con `trabajos/<id>/paquete-correccion.md` (en `~/.local/state/ecosistema-asistente/`). Entonces:

1. Lee el paquete (definición, salida, error, contexto del usuario) y `ea trabajo ver <id>`.
2. Diagnostica: ¿paso ambiguo, criterio mal puesto, entrada que cambió? Corrige el TOML (parte de la definición registrada: `ea rutina ver <slug>`).
3. Sube `version` (+1) y **propón** la vN+1 por MCP con `ea_registrar_rutina` y el TOML completo: primero con `simular: true` (enseña el diff frente a la versión registrada y el criterio resultante; si da error, corrígelo), luego sin simular, que encola una propuesta. No pidas la terminal ni registres tú: dile al usuario que la acepte en ◇ Propuestas del panel (o con `ea propuestas decidir <n> --aceptar`). Si prefiere hacerlo a mano, puede pegar el TOML en «✎ nueva versión» de la ficha de la rutina (↻ Rutinas).
4. Cuando la propuesta esté aceptada (`ea rutina ver <slug>` muestra la vN+1), `ea trabajo cerrar <id>`. Si quieres probar la nueva versión: `ea_encolar_rutina` (otra propuesta) o que el usuario eche la ficha.
