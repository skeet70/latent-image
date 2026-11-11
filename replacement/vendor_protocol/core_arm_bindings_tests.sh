#!/bin/sh
# The ARM object kernel and SD interrupt router bind the local mask slots, and the timer and
# detection producers register their handlers through the leaf's C-ABI entry.
set -eu
nm="$1"
rlib="$2"
symbols="$(mktemp)"
trap 'rm -f "$symbols"' EXIT
"$nm" --undefined-only "$rlib" > "$symbols"
for symbol in xm1_rtos_own_first_slot_164 xm1_rtos_own_first_slot_168 xm1_power_owned_callback_register; do
    if ! grep -q " $symbol\$" "$symbols"; then
        echo "core-arm does not reference $symbol" >&2
        exit 1
    fi
done
