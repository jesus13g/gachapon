#!/bin/sh
# Envoltorio del hook de subagentes (SubagentStart / SubagentStop): el trabajo
# lo hace `ea hook subagentes` (portable; ya no usa jq). Se mantiene para que
# las instalaciones existentes sigan funcionando.
#
# El binario: $EA_BIN; si no, el `ea` de release del repo en que vive este
# script (se sigue el enlace de ~/.claude/hooks); si no, `ea` del PATH.
# Un hook roto no puede estorbar a Claude Code: siempre sale con 0 y en silencio.
bin="${EA_BIN:-}"
if [ -z "$bin" ]; then
    script=$(readlink -f "$0" 2>/dev/null || echo "$0")
    bin="$(dirname "$script")/../../target/release/ea"
    [ -x "$bin" ] || bin=ea
fi
"$bin" hook subagentes "${1:-}" >/dev/null 2>&1
exit 0
