#!/bin/sh
# Hook de Claude Code para SubagentStart / SubagentStop: deja un fichero por
# subagente en <estado>/subagentes/<sesión>/<agente>.json (<estado> =
# $EA_ESTADO_DIR o ~/.local/state/ecosistema-asistente, como el bus) para que
# ea-web sepa cuáles siguen en marcha. No lee ajustes.toml: un rutas.estado
# puesto solo allí no llega aquí. Nunca falla: un error aquí no debe estorbar a
# Claude Code.
#
#   start → {"agente","tipo","transcript","desde"}
#   stop  → añade "hasta" (crea el fichero si faltaba)

accion="${1:-}"
entrada="$(cat 2>/dev/null)" || exit 0
command -v jq >/dev/null 2>&1 || exit 0

raiz="${EA_ESTADO_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/ecosistema-asistente}/subagentes"
sesion="$(printf '%s' "$entrada" | jq -r '.session_id // empty' 2>/dev/null)"
agente="$(printf '%s' "$entrada" | jq -r '.agent_id // empty' 2>/dev/null)"
[ -n "$sesion" ] && [ -n "$agente" ] || exit 0
case "$sesion$agente" in */* | *..*) exit 0 ;; esac

dir="$raiz/$sesion"
fichero="$dir/$agente.json"
ahora="$(date +%s%3N)"
mkdir -p "$dir" 2>/dev/null || exit 0

case "$accion" in
  start)
    find "$raiz" -mindepth 1 -maxdepth 1 -type d -mtime +2 -exec rm -rf {} + 2>/dev/null
    printf '%s' "$entrada" | jq -c --argjson desde "$ahora" \
      '{agente: .agent_id, tipo: (.agent_type // ""), transcript: (.transcript_path // ""), desde: $desde}' \
      >"$fichero.tmp" 2>/dev/null && mv "$fichero.tmp" "$fichero"
    ;;
  stop)
    if [ -f "$fichero" ]; then
      jq -c --argjson hasta "$ahora" '. + {hasta: $hasta}' "$fichero" >"$fichero.tmp" 2>/dev/null
    else
      printf '%s' "$entrada" | jq -c --argjson hasta "$ahora" \
        '{agente: .agent_id, tipo: (.agent_type // ""), transcript: (.transcript_path // ""), desde: $hasta, hasta: $hasta}' \
        >"$fichero.tmp" 2>/dev/null
    fi && mv "$fichero.tmp" "$fichero"
    ;;
esac
rm -f "$fichero.tmp" 2>/dev/null
exit 0
