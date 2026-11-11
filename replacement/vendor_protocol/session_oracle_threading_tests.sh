#!/bin/sh
# The native session oracle drains the runner's report and bounds the wait from other threads.
# Both need the threaded RTS: without it a safe waitpid call holds the only capability, the
# runner's report fills its stdout pipe and the pair deadlocks with no output and no bound.
set -eu
info="$("$1" +RTS --info -RTS)"
case "$info" in
    *'"RTS way", "rts_thr"'*) ;;
    *) echo "native-session-oracle does not link the threaded RTS" >&2; exit 1 ;;
esac
