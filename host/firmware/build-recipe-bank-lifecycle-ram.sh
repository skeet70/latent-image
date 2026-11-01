#!/usr/bin/env bash
set -euo pipefail
RB_PUBLICATION_GROUPS=lifecycle exec "${BASH:?}" "${0%/*}/build-recipe-bank-ram.sh" "$@"
