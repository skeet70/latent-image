#!/usr/bin/env bash
set -uo pipefail
cd "$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
S=private-data/scratch/ControllerStrategy
M=$S/menu-engine
O=${RENDER_OUTPUT:-$S/render-pairs}
render_build=${RENDER_BUILD:-private-data/analysis/x-m1-1.50/ram-push/CutoverTooling-source-bound}
render_banks=${RENDER_BANKS:-[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18]}
stack_base=${RENDER_STACK_BASE:-10800640}
fold=$S/picker-fold
runs4=$O/runs
trajectories=$O/trajectories
archive=$render_build/bank-modal75-engine-render.a
pickers=(15 26 36 163 211 214 239 250 260 270 271 272 273 297 312 314 320 450 488)
oracles=(wrap push pop open push-restored push-classifier scroll scroll-up scroll-down pop-deep
  pop-remember pop-remember-off push-same-parent push-remember-off close frame-seek-down frame-seek-up
  frame-seek-step frame-pop-down frame-pop-up frame-fixed-open fixed-wrap close-property group-left
  group-right group-up group-down group-right-wrap group-left-wrap group-down-wrap group-up-wrap nowrap
  nowrap-edge open-seek reopen step-rows step-rows-wrap frame-push-probe
  framed-open framed-open-restored framed-pop framed-pop-back framed-pop-group41 framed-push frame-push-up-probe
  fn fn-down fn-auto-open)
output_base=${RENDER_BAZEL_OUTPUT_BASE:-$HOME/.cache/bazel/latent-image-CutoverTooling}
B=(devenv shell -- host-build --output_base="$output_base")
firmware=private-data/analysis/x-m1-1.50/update/prefix/FWUP0002-prefix.DAT
nand=private-data/analysis/x-m1-1.50/stored-f7/emulation/inputs/f7-1.nand
boot_rom=private-data/analysis/x-m1-1.50/bootrom/bootrom-0x80000000-4000.bin
run_document=emulation/fujimod-xm1/data/xm1-150.json
source_commit=${RENDER_SOURCE_COMMIT:-$(git rev-parse --short=8 HEAD)}
sdk=${RENDER_SDK:-}
frozen_manifest=${RENDER_FROZEN_MANIFEST:-}
frozen_trajectories=${RENDER_FROZEN_TRAJECTORIES:-}
config_dir=${RENDER_STACK_DIAGNOSTIC_CONFIGS:-}
allocation_base=${RENDER_ALLOCATION_BASE:-0xa81be0}
prelinked=${RENDER_PRELINKED:-private-data/scratch/RenderCutover/prelinked-proof}
prelinked_base=$allocation_base
owned_retained=${RENDER_OWNED_RETAINED_CONFIG:-private-data/scratch/RenderCutover/runs/stack-diagnostic-complete-services/owned-config.json}
admission=${RENDER_ADMISSION:-bazel-bin/emulation/render-pair-admission}
stock_packet=${RENDER_STOCK_REUSE_PACKET:-}
stock_source=${RENDER_STOCK_REUSE_RECEIPT:-}
archive_sha=${RENDER_ARCHIVE_SHA:-}

reply() { jq -c '.key_reply.menu | [.menu_id, .setting_id, .cursor]' "$1/menu-observe.json"; }
landing() {
  case "$1" in
    close|close-property|frame-push-probe) reply "$S/inc3/$1" ;;
    frame-push-up-probe) reply "$S/inc4/$1" ;;
    *) echo null ;;
  esac
}

sample() {
  case "$1" in
    fn*|frame-pop-down|frame-pop-up|framed-open|framed-open-restored) jq -nc --argjson open "$(reply "$S/menu-oracle-open")" --argjson close "$(reply "$S/inc3/close")" \
      '[["ok"] + $open, ["back"] + $close]' ;;
    *) echo null ;;
  esac
}


build() {
  "${B[@]}" build --jobs=2 --define=fujimod_store=pfra //emulation:xm1-viewer //emulation:cardless-ram-push-bridge //emulation:render-compare \
    //emulation:modal-settings-compare //replacement/modal_runtime:menu-engine-pair-check \
    //replacement/modal_runtime:menu-engine-tree //emulation:render-pair-admission \
    //host/firmware:check-bank-allowlist || exit 1
  prepare
}

prepare() {
  mkdir -p "$runs4" "$trajectories"
  [ -f "$archive" ] || { printf 'render archive is missing: %s\n' "$archive" >&2; exit 2; }
  prepare_trajectories || exit 1
  prepare_diagnostics || exit 1
}

prepare_trajectories() {
  if [ -z "$frozen_manifest" ]; then
    write_trajectories
    return
  fi
  [ -f "$frozen_manifest" ] || { printf 'missing frozen manifest: %s\n' "$frozen_manifest" >&2; return 2; }
  [ -n "$frozen_trajectories" ] || frozen_trajectories=$(jq -r '.trajectories.source_directory // empty' "$frozen_manifest")
  [ -d "$frozen_trajectories" ] || { printf 'missing frozen trajectories: %s\n' "$frozen_trajectories" >&2; return 2; }
  local name expected source target actual files
  files=$(jq -r '.trajectories.files[] | "\(.name)\t\(.sha256)"' "$frozen_manifest") || return 2
  while IFS=$'\t' read -r name expected; do
    [ -n "$name" ] || continue
    source="$frozen_trajectories/$name"
    target="$trajectories/$name"
    [ -f "$source" ] || { printf 'frozen trajectory is missing: %s\n' "$source" >&2; return 2; }
    actual=$(sha256sum "$source" | cut -d' ' -f1)
    [ "$actual" = "$expected" ] || { printf 'frozen trajectory %s is %s, not %s\n' "$name" "$actual" "$expected" >&2; return 2; }
    if [ -f "$target" ]; then
      actual=$(sha256sum "$target" | cut -d' ' -f1)
      [ "$actual" = "$expected" ] || { printf 'cohort trajectory %s is %s, not the frozen %s\n' "$target" "$actual" "$expected" >&2; return 2; }
    else
      cp "$source" "$target" || return 2
    fi
  done <<< "$files"
}

prepare_diagnostics() {
  [ -n "$config_dir" ] || return 0
  [ -f "$config_dir/stock-config.json" ] || { printf 'missing stock diagnostic configuration in %s\n' "$config_dir" >&2; return 2; }
  [ -f "$config_dir/owned-config.json" ] && [ -f "$prelinked/ram-push-manifest.json" ] && return 0
  local relink="$prelinked"
  if [ -e "$relink" ]; then
    printf 'owned relink directory exists without a configuration: %s\n' "$relink" >&2
    return 2
  fi
  mkdir -p "$relink" || return 2
  RB_BAZEL_OUTPUT_BASE="$output_base" devenv shell -- bash replacement/render_adapter/relink.sh \
    "$allocation_base" "$archive" "$relink" || return 2
  "$admission" config --elf "$relink/kernel.elf" --manifest "$relink/ram-push-manifest.json" \
    --base "$allocation_base" --stock-config "$config_dir/stock-config.json" \
    --owned-retained-config "$owned_retained" --out "$config_dir/owned-config.json" || return 2
}

check_end() {
  jq -n --arg name "$1" --slurpfile graph "$S/crawl-5/crawl-graph.json" --slurpfile t "$2" \
    -f emulation/menu-engine-pair-trajectories/end-check.jq > /dev/null || exit 1
}

write_trajectories() {
  mkdir -p "$trajectories"
  D=emulation/menu-engine-pair-trajectories
  for role in stock owned; do
    for p in "${pickers[@]}"; do
      jq -L "$D" --argjson render true --argjson render_banks "$render_banks" --argjson render_stack_base "$stack_base" --arg role "$role" -f "$D/picker.jq" "emulation/modal75-picker-trajectories/picker-$p.json" \
        > "$trajectories/picker-$p.$role.json" || exit 1
      check_end "picker-$p" "$trajectories/picker-$p.$role.json"
    done
    for n in "${oracles[@]}"; do
      local generator="$D/oracle.jq" receipt=()
      case "$n" in fn*)
        generator="$D/fn-oracle.jq"
        receipt=(--arg receipt_source "$S/inc5/$n/menu-observe.json" --slurpfile receipt "$S/inc5/$n/menu-observe.json")
        ;; esac
      jq -n -L "$D" --argjson render true --argjson render_banks "$render_banks" --argjson render_stack_base "$stack_base" --arg role "$role" --arg name "$n" --slurpfile config "emulation/menu-oracle-$n.json" \
        --slurpfile graph "$S/crawl-5/crawl-graph.json" --argjson landing "$(landing "$n")" --argjson sample "$(sample "$n")" \
        "${receipt[@]}" -f "$generator" > "$trajectories/oracle-$n.$role.json" || exit 1
      check_end "oracle-$n" "$trajectories/oracle-$n.$role.json"
    done
  done
}

sha() { sha256sum "$1" | cut -d' ' -f1; }

trajectory_sha() {
  local name="$1.$2.json" frozen
  if [ -n "$frozen_manifest" ]; then
    frozen=$(jq -er --arg name "$name" '.trajectories.files[] | select(.name == $name) | .sha256' "$frozen_manifest") || {
      printf 'frozen manifest does not bind %s\n' "$name" >&2
      return 2
    }
    printf '%s\n' "$frozen"
    return 0
  fi
  sha "$trajectories/$name"
}

archive_digest() {
  if [ -n "$archive_sha" ]; then printf '%s\n' "$archive_sha"; else sha "$archive"; fi
}

receipt_head() {
  local receipt="" arg
  while [ $# -gt 0 ]; do
    case "$1" in
      --receipt) receipt=${2:-}; shift 2 ;;
      *) shift ;;
    esac
  done
  [ -n "$receipt" ] && [ -f "$receipt/admission.json" ] && jq -r '.head_commit // empty' "$receipt/admission.json" 2>/dev/null
}

admission_run() {
  local verb="$1" name="$2" role="$3"
  shift 3
  local config="$config_dir/$role-config.json" trajectory="$trajectories/$name.$role.json"
  local trajectory_digest config_digest args head_commit recorded_head role_source
  [ -n "$sdk" ] || { printf 'render admission needs RENDER_SDK\n' >&2; return 2; }
  [ -f "$config" ] || { printf 'missing diagnostic config: %s\n' "$config" >&2; return 2; }
  [ -f "$trajectory" ] || { printf 'missing trajectory: %s\n' "$trajectory" >&2; return 2; }
  trajectory_digest=$(trajectory_sha "$name" "$role") || return 2
  config_digest=$(sha "$config") || return 2
  head_commit=$(git rev-parse HEAD)
  role_source=$source_commit
  if [ "$role" = stock ]; then
    recorded_head=$(receipt_head "$@")
    [ -n "$recorded_head" ] && head_commit=$recorded_head
    if [ -n "$stock_packet" ]; then
      role_source=$(jq -er '.source_commit' "$stock_packet") || { printf 'stock packet lacks source_commit\n' >&2; return 2; }
    fi
  fi
  args=(--name "$name" --role "$role" --source-commit "$role_source" --head-commit "$head_commit"
    --sdk "$sdk" --fujimod-paths bazel-bin/emulation/FujiModPaths.hs
    --trajectory "$trajectory" --trajectory-sha256 "$trajectory_digest"
    --firmware "$firmware" --nand "$nand" --boot-rom "$boot_rom" --run-document "$run_document"
    --config "$config" --config-sha256 "$config_digest")
  if [ "$role" = owned ]; then
    args+=(--archive "$archive" --archive-sha256 "$(archive_digest)")
    [ -f "$(dirname "$archive")/render-source-identity.json" ] && args+=(--source-identity "$(dirname "$archive")/render-source-identity.json")
    [ -f "$prelinked/ram-push-manifest.json" ] || { printf 'missing prelinked manifest: %s\n' "$prelinked/ram-push-manifest.json" >&2; return 2; }
    args+=(--prelinked "$prelinked" --prelinked-base "$prelinked_base" --prelinked-size "$(jq -r .size "$prelinked/ram-push-manifest.json")")
    args+=(--checker "$PWD/bazel-bin/host/firmware/check-bank-allowlist")
  fi
  "$admission" "$verb" "${args[@]}" "$@"
}

guest_arguments() {
  local name="$1" role="$2" suffix="${3:+-$3}" dir="$1-$2${3:+-$3}"
  GUEST=(--workspace "$PWD" --ram-push-e2e --cardless --fujimod --unreviewed-relink
    --firmware "$firmware" --installed-nand "$nand"
    --trajectories "$trajectories/$name.$role.json"
    --output "$runs4/$dir-runtime" --receipt-output "$PWD/$runs4/$dir")
  case "$role" in
    stock) GUEST+=(--modal-lifetime-guard) ;;
    owned) GUEST+=(--ram-push-modal-scoped --modal75-owned-archive "$archive"
             --modal75-retired "$fold/modal75-retired-picker.json" --menu-engine-owned --render-owned
             --modal75-prelinked "$prelinked") ;;
    *) printf 'role must be stock or owned\n' >&2; return 2 ;;
  esac
  GUEST_CONFIG=""
  if [ -n "$config_dir" ]; then
    GUEST_CONFIG="$config_dir/$role-config.json"
    case "$GUEST_CONFIG" in /*) ;; *) GUEST_CONFIG="$PWD/$GUEST_CONFIG" ;; esac
    [ -f "$GUEST_CONFIG" ] || { printf 'missing diagnostic config: %s\n' "$GUEST_CONFIG" >&2; return 2; }
  fi
}

reuse_stock() {
  local name="$1" role="$2" dir="$1-$2$3"
  [ "$role" = stock ] || return 1
  [ -n "$stock_packet" ] || return 1
  [ -n "$stock_source" ] || { printf 'RENDER_STOCK_REUSE_PACKET needs RENDER_STOCK_REUSE_RECEIPT\n' >&2; return 2; }
  admission_run import "$name" "$role" --packet "$stock_packet" --receipt "$stock_source" --destination "$runs4/$dir" || return 2
  printf '%s imported from the preserved receipt\n' "$dir"
}

run() {
  local name="$1" role="$2" s="${3:+-$3}" dir="$1-$2${3:+-$3}" runs="$runs4"
  [ -e "$O/stop" ] && { printf '%s %s skipped\n' "$name" "$role"; return 0; }
  if [ -d "$runs/$dir" ] && [ -n "$(ls -A "$runs/$dir")" ]; then
    if admission_run verify "$name" "$role" --receipt "$runs/$dir" > /dev/null; then
      printf '%s reused\n' "$dir"
      return 0
    fi
    printf '%s has no matching admission; refusing to skip it\n' "$dir" >&2
    return 2
  fi
  if [ "$role" = stock ] && [ -n "$stock_packet" ]; then
    reuse_stock "$name" "$role" "$s" || return 2
    return 0
  fi
  guest_arguments "$name" "$role" "${s#-}" || return 2
  local bind_admission=()
  if [ "$role" = owned ] && [ -f "$runs/$name-stock$s/admission.json" ]; then
    bind_admission=(--bind-admission "$runs/$name-stock$s")
  fi
  admission_run inputs "$name" "$role" "${bind_admission[@]}" || return 2
  local launch=(env) started finished run_status
  [ -n "$GUEST_CONFIG" ] && launch+=("XM1_RENDER_STACK_DIAGNOSTIC=$GUEST_CONFIG")
  [ -n "${RENDER_VIEWER_GHCRTS:-}" ] && launch+=("GHCRTS=$RENDER_VIEWER_GHCRTS")
  started=$(date +%s%N)
  "${launch[@]}" XM1_EMULATOR_SLOTS=1 devenv shell -- bazel-bin/emulation/xm1-viewer "${GUEST[@]}" \
    < /dev/null > "$runs/$dir-viewer.log" 2>&1
  run_status=$?
  finished=$(date +%s%N)
  jq -n --arg role "$role" --arg name "$name" --arg started "$started" --arg finished "$finished" \
    --argjson status "$run_status" --argjson elapsed "$((finished - started))" \
    '{name:$name,role:$role,guest_exit_code:$status,started_unix_ns:$started,finished_unix_ns:$finished,elapsed_ns:$elapsed}' \
    > "$runs/$dir-run.json"
  printf '%s exit=%s\n' "$dir" "$run_status"
  if [ "$run_status" -eq 0 ]; then
    admission_run bind "$name" "$role" --receipt "$runs/$dir" || return 2
  fi
  return "$run_status"
}

admit() {
  local name="$1" role="$2" s="${3:+-$3}" dir="$1-$2${3:+-$3}"
  if [ -d "$runs4/$dir" ] && [ -n "$(ls -A "$runs4/$dir")" ]; then
    if admission_run verify "$name" "$role" --receipt "$runs4/$dir"; then
      printf '%s reused\n' "$dir"
      return 0
    fi
    printf '%s has no matching admission; refusing to skip it\n' "$dir" >&2
    return 2
  fi
  if [ "$role" = stock ] && [ -n "$stock_packet" ]; then
    reuse_stock "$name" "$role" "$s" || return 2
    return
  fi
  guest_arguments "$name" "$role" "${s#-}" || return 2
  local bind_admission=()
  if [ "$role" = owned ] && [ -f "$runs4/$name-stock$s/admission.json" ]; then
    bind_admission=(--bind-admission "$runs4/$name-stock$s")
  fi
  admission_run inputs "$name" "$role" "${bind_admission[@]}" || return 2
  printf 'admitted %s %s inputs; guest command:\n' "$name" "$role"
  local preview=(env)
  [ -n "$GUEST_CONFIG" ] && preview+=("XM1_RENDER_STACK_DIAGNOSTIC=$GUEST_CONFIG")
  [ -n "${RENDER_VIEWER_GHCRTS:-}" ] && preview+=("GHCRTS=$RENDER_VIEWER_GHCRTS")
  printf '%q ' "${preview[@]}" XM1_EMULATOR_SLOTS=1 devenv shell -- bazel-bin/emulation/xm1-viewer "${GUEST[@]}"
  printf '\n'
}

check() {
  local name="$1" s="${2:+-$2}" runs="$runs4" results="${RENDER_CHECK_OUTPUT:-$runs4}"
  local output
  mkdir -p "$results" || return 1
  for output in render-mode.json pair-check.txt settings.json settings-check.txt render.json render-check.txt checks.json; do
    [ ! -e "$results/$name$s-$output" ] || { printf 'comparison output already exists: %s\n' "$results/$name$s-$output" >&2; return 2; }
  done
  if [ -n "$config_dir" ]; then
    admission_run verify "$name" stock --receipt "$runs/$name-stock$s" > /dev/null || { printf '%s stock receipt is not admitted\n' "$name$s" >&2; return 2; }
    admission_run verify "$name" owned --receipt "$runs/$name-owned$s" > /dev/null || { printf '%s owned receipt is not admitted\n' "$name$s" >&2; return 2; }
  fi
  local clock=()
  local stock_draws owned_draws mode=required reason='At least one role reached a render drawer entry.'
  local pair_check=bazel-bin/replacement/modal_runtime/menu-engine-pair-check
  stock_draws=$("$pair_check" --count-drawers "$runs/$name-stock$s") || { printf '%s stock observation count refused\n' "$name$s" >&2; return 2; }
  owned_draws=$("$pair_check" --count-drawers "$runs/$name-owned$s") || { printf '%s owned observation count refused\n' "$name$s" >&2; return 2; }
  local render_mode=(--render)
  if (( stock_draws == 0 && owned_draws == 0 )); then
    render_mode+=(--render-draw-free)
    mode=draw-free
    reason='Neither role reached tree, picker or pane entry in any observed segment.'
  fi
  jq -n --arg mode "$mode" --arg reason "$reason" --argjson stock "$stock_draws" --argjson owned "$owned_draws" \
    '{mode:$mode,reason:$reason,stock_draw_entries:$stock,owned_draw_entries:$owned}' > "$results/$name$s-render-mode.json"
  if [ -n "${RENDER_CLOCK_RESOURCE:-}" ]; then clock=(--render-clock-resource "$RENDER_CLOCK_RESOURCE"); fi
  bazel-bin/replacement/modal_runtime/menu-engine-pair-check "${render_mode[@]}" --render-banks "$render_banks" --render-stack-base "$(printf '0x%x' "$stack_base")" "${clock[@]}" --tables "$S/menu-tables.json" --package-plan "$M/bank-plan.json" \
    --stock "$runs/$name-stock$s" --owned "$runs/$name-owned$s" > "$results/$name$s-pair-check.txt" 2>&1
  local engine_status=$?
  printf '%s pair-check exit=%s\n' "$name$s" "$engine_status"
  bazel-bin/emulation/modal-settings-compare "$runs/$name-stock$s" "$runs/$name-owned$s" "$results/$name$s-settings.json" > "$results/$name$s-settings-check.txt" 2>&1
  local settings_status=$?
  [ -f "$results/$name$s-settings.json" ] || settings_status=1
  printf '%s settings exit=%s %s\n' "$name$s" "$settings_status" "$(jq -c '{equal, problem}' "$results/$name$s-settings.json" 2>/dev/null)"
  local exclusions=()
  case "$name" in
    oracle-frame-fixed-open|oracle-fixed-wrap) exclusions=(--clock-exclusions emulation/menu-engine-pair-trajectories/clock-exclusions.json) ;;
    oracle-framed-push) exclusions=(--clock-exclusions emulation/menu-engine-pair-trajectories/clock-exclusions-framed-push.json) ;;
  esac
  bazel-bin/emulation/render-compare "$runs/$name-stock$s/e2e-result-render.json" "$runs/$name-owned$s/e2e-result-render.json" "$results/$name$s-render.json" "${exclusions[@]}" > "$results/$name$s-render-check.txt" 2>&1
  local render_status=$?
  [ -f "$results/$name$s-render.json" ] || render_status=1
  printf '%s render exit=%s equal=%s\n' "$name$s" "$render_status" "$(jq -r .equal "$results/$name$s-render.json" 2>/dev/null)"
  jq -n --argjson engine "$engine_status" --argjson settings "$settings_status" --argjson render "$render_status" \
    '{menu_engine_pair_check:$engine,modal_settings_compare:$settings,render_compare:$render}' > "$results/$name$s-checks.json"
  (( engine_status == 0 && settings_status == 0 && render_status == 0 ))
}

names() { for p in "${pickers[@]}"; do echo "picker-$p"; done; for n in "${oracles[@]}"; do echo "oracle-$n"; done; }

case "${1:-}" in
  names) names ;;
  build) build ;;
  prepare) prepare ;;
  trajectories) write_trajectories ;;
  run) run "$2" "$3" "${4:-}" ;;
  admit) admit "$2" "$3" "${4:-}" ;;
  all) for name in $(names); do run "$name" stock "${2:-}" || exit "$?"; run "$name" owned "${2:-}" || exit "$?"; done ;;
  rerun) echo "Use run NAME ROLE SUFFIX for a separately named receipt" >&2; exit 2 ;;
  check) if [ -n "${2:-}" ]; then check "$2" "${3:-}"; else status=0; for name in $(names); do check "$name" "${3:-}" || status=1; done; exit "$status"; fi ;;
  *) printf 'usage: %s build|prepare|trajectories|names|run NAME ROLE [SUFFIX]|admit NAME ROLE [SUFFIX]|all [SUFFIX]|check [NAME [SUFFIX]]\n' "$0" >&2; exit 2 ;;
esac