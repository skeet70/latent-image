#!/usr/bin/env bash
set -euo pipefail
generator="$(realpath "${1:?recipe-bank-ui-cases executable}")"
work="${TEST_TMPDIR:?}/bank-plan"
mkdir -p "$work"
cat > "$work/kernel.syms" <<'SYMBOLS'
00be1f00 00000000 T __rb_image_start
00be3000 00000000 T __rb_text_end
00be2200 0000000c T rb_stage_edit_get_arm
00be220c 0000000c T rb_stage_edit_set_arm
00be2f00 00000044 T rb_select_values
00be2f44 00000080 T rb_select_labels
SYMBOLS
cat > "$work/stale.json" <<'PLAN'
{"writes":[{"region":"id4_code","address":6417092,"new":"0c22be00","role":"getter dispatch arm"},{"region":"id4_code","address":6417200,"new":"0021be00","role":"setter dispatch arm"}]}
PLAN
if "$generator" check-plan --build "$work" --plan "$work/stale.json"; then
  printf 'stale callback pointers were accepted\n' >&2
  exit 1
fi
"$generator" bind-plan --build "$work" --plan "$work/stale.json" --out "$work/bound.json"
"$generator" check-plan --build "$work" --plan "$work/bound.json"
printf '{"writes":[{"region":"menu_values","address":12491264,"new":"%064d0022be000c22be0000000000","role":"select list node header"}]}\n' 0 > "$work/stale-select.json"
if "$generator" check-plan --build "$work" --plan "$work/stale-select.json"; then
  printf 'stale SELECT array pointers were accepted\n' >&2
  exit 1
fi
"$generator" bind-plan --build "$work" --plan "$work/stale-select.json" --out "$work/bound-select.json"
"$generator" check-plan --build "$work" --plan "$work/bound-select.json"
cat > "$work/unknown.json" <<'PLAN'
{"writes":[{"region":"menu_values","address":12491264,"new":"0022be00","role":"unbound callback"}]}
PLAN
if "$generator" check-plan --build "$work" --plan "$work/unknown.json"; then
  printf 'unbound bank code pointer was accepted\n' >&2
  exit 1
fi
printf 'UI plan binding and refusal tests passed\n'
