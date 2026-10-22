#!/bin/sh
# Envoltorio del hook de subagentes (SubagentStart / SubagentStop): el trabajo
# lo hace `ea hook subagentes` (portable; ya no usa jq). Se mantiene para que
# las instalaciones existentes sigan funcionando. EA_BIN apunta al binario `ea`.
exec "${EA_BIN:-ea}" hook subagentes "${1:-}"
