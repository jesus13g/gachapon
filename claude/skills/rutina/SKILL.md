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

3. `ea rutina registrar <slug>.toml` (valida; error si la versión no sube). Comprueba con `ea rutina ver <slug>`.
4. Si se pide lanzarla ya: `ea rutina lanzar <slug> [--entrada clave=valor]` → id del trabajo; su revisión se hace con `/revisar`.

## Programarla

El servicio `ea-rutina@<slug>.service` ya existe y solo ejecuta `ea rutina lanzar <slug>`. Para el horario, crea (o explica al usuario) `~/.config/systemd/user/ea-rutina@<slug>.timer` copiando `crates/ea-orchestrator/systemd/ea-rutina@.timer` y ajustando `OnCalendar=` (p. ej. `*-*-* 08:00:00`; `Mon *-*-* 09:00` para semanal). Luego `systemctl --user daemon-reload && systemctl --user enable --now ea-rutina@<slug>.timer`. Pide confirmación antes de activar timers.

## Corrección (paquete de corrección)

Si una rutina falla, el usuario confirma el paquete y llega a la cola `claude-code` como trabajo `en_revision` con `trabajos/<id>/paquete-correccion.md` (en `~/.local/state/ecosistema-asistente/`). Entonces:

1. Lee el paquete (definición, salida, error, contexto del usuario) y `ea trabajo ver <id>`.
2. Diagnostica: ¿paso ambiguo, criterio mal puesto, entrada que cambió? Corrige el TOML.
3. Sube `version` (+1), `ea rutina registrar` y confirma con `ea rutina ver <slug>`.
4. `ea trabajo cerrar <id>`. Si quieres probar la nueva versión: `ea rutina lanzar <slug>`.
