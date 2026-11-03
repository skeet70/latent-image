# Judges the embassy phase-1 demonstration and its three controls from their e2e-result.json files:
#   jq -n -L replacement/embassy_platform --slurpfile demo D --slurpfile no_executors E \
#     --slurpfile no_pender_kernel P --slurpfile counter_up U -f replacement/embassy_platform/check-demo.jq
# Expected values follow the `entry` of experiments/embassy_phase1/trajectory.json and the record
# layout in src/platform.rs, not a run.
include "decode-demo";

def read_at($phase; $label): [.observations[] | select(.phase == $phase) | .reads[]? | select(.label == $label) | .bytes][0];
def record($phase): read_at($phase; "demo") | demo;
def run_clean:
  .checks == {"install": "PASS", "trajectory": "PASS", "unload": "PASS"}
  and ([.observations[].steps[].matched] | all)
  and .forbidden_execution.complete == true and .forbidden_execution.forbidden_fetch_count == 0;
def wake_hits($r0): [.observations[].trace_hits[]? | select((.site | startswith("wakeup-")) and .r0 == $r0)] | length;
def fill_words: bytes as $b | [range(0; ($b | length) / 4) as $w | ($b[($w * 4):($w * 4 + 4)] == [119, 119, 119, 119])];
def high_water($stack): ($stack | index(false)) as $first | if $first == null then 0 else 2048 - 4 * $first end;
# ticker_iterations against the tick span: each iteration is 10 ticks (100 ms) after the first.
def on_schedule($d): $d.ticker_iterations >= 10 and ((($d.last_tick - $d.first_tick) / 10 - ($d.ticker_iterations - 1)) | fabs) <= 1;
# Both tickers advanced between the handback read and the after-unload read.
def both_continue($before; $after): $after.magic == "EMB2"
  and $after.domains.A.ticker_iterations > $before.domains.A.ticker_iterations
  and $after.domains.B.ticker_iterations > $before.domains.B.ticker_iterations;
def timers_run($run): ($run | record("handback")) as $h | ($run | record("unloaded-shooting")) as $a
  | ([$h.domains[] | on_schedule(.)] | all) and both_continue($h; $a);

($demo[0]) as $run
| ($run | record("handback")) as $d
| ($run | record("unloaded-shooting")) as $after
| ($run | read_at("handback"; "stacks") | fill_words) as $stacks
| {
    demo: {
      run_clean: ($run | run_clean),
      record_magic: ($d.magic == "EMB2"),
      no_panics: ($d.panics == 0 and $after.panics == 0),
      created_and_activated: ($d.start_status.A == [0, 0, 0] or $d.start_status.A == [-41, 0, 0]) and ($d.start_status.B == [0, 0, 0] or $d.start_status.B == [-41, 0, 0]),
      both_entered: ($d.domains.A.entered == 1 and $d.domains.B.entered == 1),
      direction_down_with_sub_ticks: ([$d.domains[] | .direction_measured == 2 and .ticks_only == 0] | all),
      timers_on_schedule_and_continuing: timers_run($run),
      no_kernel_errors: ([$after.domains[] | .sleeps_other == 0 and .pender_errors == 0] | all),
      doorbell_through_kernel: ($d.doorbell_sent >= 1 and $d.domains.B.doorbell_received >= 1
        and $d.domains.B.doorbell_received >= $d.doorbell_sent - 1
        and $d.domains.B.pender_task_calls >= $d.doorbell_sent and ($run | wake_hits("0x00020077")) >= 1),
      stack_high_water_bytes: {A: high_water($stacks[0:512]), B: high_water($stacks[512:1024])},
      measures: {
        iterations: ($d.domains | map_values(.ticker_iterations)),
        tick_span: ($d.domains | map_values(.last_tick - .first_tick)),
        late_max_counts: ($d.domains | map_values(.late_max)),
        late_over_two_ticks: ($d.domains | map_values(.late_over_two_ticks)),
        early_kernel_wakes: ($d.domains | map_values(.early_kernel_wakes)),
        clock_regressions: ($d.domains | map_values({count: .clock_regressions, max: .clock_regression_max})),
        sleeps: ($d.domains | map_values({woken: .sleeps_woken, timed_out: .sleeps_timed_out})),
        doorbell: {sent: $d.doorbell_sent, received: $d.domains.B.doorbell_received, latency_max_counts: $d.domains.B.doorbell_latency_max, kernel_wakeups_traced: ($run | wake_hits("0x00020077"))}
      }
    }
  }
| ($no_executors[0]) as $none
| ($none | record("handback")) as $n
| .no_executors = {
    run_clean: ($none | run_clean),
    hook_ran_without_creating: ($n.started == 1 and ([$n.start_status[][]] | all(. == "not-called")) and ([$n.domains[] | .entered] == [0, 0])),
    # Restates the step matches of both run_clean checks plus equal (possibly empty) menu messages.
    same_menu_messages_as_demo: ([$none.observations[].menu_messages] == [$run.observations[].menu_messages])
  }
| ($no_pender_kernel[0]) as $quiet
| ($quiet | record("handback")) as $q
| .no_pender_kernel = {
    run_clean: ($quiet | run_clean),
    timers_still_run: timers_run($quiet),
    doorbell_not_delivered: ($q.doorbell_sent >= 1 and $q.domains.B.doorbell_received == 0 and ($quiet | wake_hits("0x00020077")) == 0)
  }
| ($counter_up[0]) as $up
| ($up | record("handback")) as $u
| .counter_up = {
    run_clean: ($up | run_clean),
    mismatch_falls_back_to_ticks: ([$u.domains[] | .direction_measured == 2 and .ticks_only == 1] | all),
    timers_still_run: timers_run($up)
  }
| .verdict = ([.demo, .no_executors, .no_pender_kernel, .counter_up | to_entries[] | .value | select(type == "boolean")] | all)
