#!/usr/bin/env bash
# Scope selection is observable before any input is read, so these cases supply no other inputs.
set -euo pipefail
oracle=$1
test -x "$oracle"
expect_failure() {
  description=$1
  expected=$2
  shift 2
  set +e
  output=$("$oracle" "$@" 2>&1)
  status=$?
  set -e
  if [ "$status" -eq 0 ]; then
    printf 'scope boundary: %s exited zero\n' "$description" >&2
    exit 1
  fi
  case "$output" in
    *"$expected"*) ;;
    *)
      printf 'scope boundary: %s did not report %s:\n%s\n' "$description" "$expected" "$output" >&2
      exit 1
      ;;
  esac
}
expect_failure "unknown focus gate" "--focus takes preemption, ee or custom-lifecycle" --focus verdict
expect_failure "missing focus value" "--focus needs a value" --focus
expect_failure "joined and focused scopes" "--joined-only and --focus select different execution scopes" --joined-only --focus ee
expect_failure "repeated focus" "--focus accepts exactly one gate" --focus ee --focus preemption
expect_failure "focused plan" "is an execution scope, not a full qualification plan" --focus preemption --plan
expect_failure "joined stock runner outside the joined stage" "--joined-stock-runner requires the joined bank/custom/record stage" --focus custom-lifecycle --joined-stock-runner /bin/false
printf 'scope boundary controls pass\n'
