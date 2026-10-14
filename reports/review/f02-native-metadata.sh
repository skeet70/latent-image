#!/usr/bin/env bash
set -euo pipefail
# Static public ELF metadata only. The compiled reader never calls ldd/dlopen.
"$1"
