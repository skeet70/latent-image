# Judges the embassy phase-1 demonstration and its three controls from their e2e-result.json files:
#   jq -n -L replacement/embassy_platform --slurpfile demo D --slurpfile no_executors E \
#     --slurpfile no_pender_kernel P --slurpfile counter_up U --rawfile objects OBJECTS_SHA256 \
#     -f replacement/embassy_platform/check-demo.jq
# OBJECTS_SHA256 is `sha256sum bazel-bin/replacement/embassy_platform/*.o` taken before the runs.
# Expected values follow the `entry` of experiments/embassy_phase1/trajectory.json and the record
# layout in src/platform.rs, not a run.
include "decode-demo";

def read_at($phase; $label): [.observations[] | select(.phase == $phase) | .reads[]? | select(.label == $label) | .bytes][0];
def record($phase): read_at($phase; "demo") | if . == null then null else demo end;
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
  | $h != null and $a != null and ([$h.domains[] | on_schedule(.)] | all) and both_continue($h; $a);
def started_hosts($d): $d != null and $d.magic == "EMB2"
  and ($d.start_status.A == [0, 0, 0] or $d.start_status.A == [-41, 0, 0])
  and ($d.start_status.B == [0, 0, 0] or $d.start_status.B == [-41, 0, 0])
  and $d.domains.A.entered == 1 and $d.domains.B.entered == 1;
def no_faults($run): [$run | record("handback"), record("unloaded-shooting")] as $reads
  | ($reads | all(. != null)) and ([$reads[] | .panics == 0] | all)
  and ([$reads[1].domains[] | .sleeps_other == 0 and .pender_errors == 0 and .early_kernel_wakes == 0 and .clock_regressions == 0] | all);
def screens: [.menu_receipts[] | {label, menu, png: .sha256, planes: .scanout_evidence.source_bytes.sha256}];
def start_point: .emulator_start | {directory, inputs, key, start};
def builder_identity: .relink.source_identity | with_entries(select((.key | startswith("experiments/") or startswith("prelinked-source:")) | not)) | del(.["experiment-prelinked.o"]);
def variant_file_digest($name; $file): .relink.source_identity["experiments/" + $name + "/" + $file];
# Source digests the builder recorded for the linked object; receipts relinked before that listing existed have none.
def prelinked_sources: .relink.source_identity | with_entries(select(.key | startswith("prelinked-source:")));
# The object digest recorded for each variant before the runs.
def object_digest($object): $objects | split("\n") | map(select(endswith("/" + $object)) | split(" ")[0]) | .[0];
def is_variant($name; $object): .relink.source_identity["experiments/" + $name + "/prelinked.json"] != null
  and object_digest($object) != null
  and .relink.source_identity["experiment-prelinked.o"] == object_digest($object);

[
  {run: $demo[0], name: "embassy_phase1", object: "embassy-platform-demo.o"},
  {run: $no_executors[0], name: "embassy_phase1_no_executors", object: "embassy-platform-no-executors.o"},
  {run: $no_pender_kernel[0], name: "embassy_phase1_no_pender_kernel", object: "embassy-platform-no-pender-kernel.o"},
  {run: $counter_up[0], name: "embassy_phase1_counter_up", object: "embassy-platform-counter-up.o"}
] as $runs
| ($demo[0]) as $run
| ($run | record("handback")) as $d
| ($run | record("unloaded-shooting")) as $after
| ($run | read_at("handback"; "stacks")) as $stacks_hex
| (if $stacks_hex == null then null else $stacks_hex | fill_words end) as $stacks
| (if $stacks == null then null else {A: high_water($stacks[0:512]), B: high_water($stacks[512:1024])} end) as $water
| {
    runs: {
      each_named_variant: ([$runs[] | .name as $n | .object as $o | .run | is_variant($n; $o)] | all),
      same_emulator_start: ([$runs[] | .run | start_point] | unique | length == 1),
      same_trajectory: ([$runs[] | .name as $n | .run | variant_file_digest($n; "trajectory.json")] | unique | (length == 1 and .[0] != null)),
      same_hooks: ([$runs[] | .name as $n | .run | variant_file_digest($n; "hooks.json")] | unique | (length == 1 and .[0] != null)),
      same_builder_identity: ([$runs[] | .run | builder_identity] | unique | length == 1),
      # Reported, not judged: without a listing, source identity rests on the object digests alone.
      prelinked_source_entries: ([$runs[] | .run | prelinked_sources | length]),
      # Judged once receipts carry the listing; "absent" otherwise.
      same_prelinked_sources: ([$runs[] | .run | prelinked_sources] as $listings
        | if ([$listings[] | length] | all(. == 0)) then "absent" else ($listings | unique | length == 1) and ([$listings[] | length] | all(. > 0)) end)
    },
    demo: {
      run_clean: ($run | run_clean),
      hosts_started: started_hosts($d),
      no_faults: no_faults($run),
      direction_down_with_sub_ticks: ([$d.domains[] | .direction_measured == 2 and .ticks_only == 0] | all),
      timers_on_schedule_and_continuing: timers_run($run),
      doorbell_through_kernel: ($d.doorbell_sent >= 1 and $d.domains.B.doorbell_received >= 1
        and $d.domains.B.doorbell_received >= $d.doorbell_sent - 1
        and $d.domains.B.pender_task_calls >= $d.doorbell_sent and ($run | wake_hits("0x00020077")) >= 1),
      stacks_within_bounds: ($water != null and $water.A > 0 and $water.A < 2048 and $water.B > 0 and $water.B < 2048),
      measures: {
        stack_high_water_bytes: $water,
        iterations: ($d.domains | map_values(.ticker_iterations)),
        tick_span: ($d.domains | map_values(.last_tick - .first_tick)),
        late_max_counts: ($d.domains | map_values(.late_max)),
        late_over_two_ticks: ($d.domains | map_values(.late_over_two_ticks)),
        early_kernel_wakes: ($after.domains | map_values(.early_kernel_wakes)),
        clock_regressions: ($after.domains | map_values({count: .clock_regressions, max: .clock_regression_max})),
        sleeps: ($d.domains | map_values({woken: .sleeps_woken, timed_out: .sleeps_timed_out})),
        doorbell: {sent: $d.doorbell_sent, received: $d.domains.B.doorbell_received, latency_max_counts: $d.domains.B.doorbell_latency_max, kernel_wakeups_traced: ($run | wake_hits("0x00020077"))}
      }
    }
  }
| ($no_executors[0]) as $none
| ($none | record("handback")) as $n
| .no_executors = {
    run_clean: ($none | run_clean),
    hook_ran_without_creating: ($n != null and $n.started == 1 and $n.panics == 0
      and ([$n.start_status[][]] | all(. == "not-called")) and ([$n.domains[] | .entered] == [0, 0])),
    same_screens_as_demo: (($none | screens) == ($run | screens) and ($run | screens | length) > 0)
  }
| ($no_pender_kernel[0]) as $quiet
| ($quiet | record("handback")) as $q
| .no_pender_kernel = {
    run_clean: ($quiet | run_clean),
    hosts_started: started_hosts($q),
    no_faults: (
      [$quiet | record("handback"), record("unloaded-shooting")] as $reads
      | ($reads | all(. != null)) and ([$reads[] | .panics == 0] | all)
      and ([$reads[1].domains[] | .sleeps_other == 0 and .early_kernel_wakes == 0 and .clock_regressions == 0] | all)),
    direction_down_with_sub_ticks: ([$q.domains[] | .direction_measured == 2 and .ticks_only == 0] | all),
    timers_still_run: timers_run($quiet),
    doorbell_not_delivered: ($q.doorbell_sent >= 1 and $q.domains.B.doorbell_received == 0
      and $q.domains.B.pender_unavailable >= $q.doorbell_sent and ($quiet | wake_hits("0x00020077")) == 0)
  }
| ($counter_up[0]) as $up
| ($up | record("handback")) as $u
| .counter_up = {
    run_clean: ($up | run_clean),
    hosts_started: started_hosts($u),
    no_faults: no_faults($up),
    mismatch_falls_back_to_ticks: ([$u.domains[] | .direction_measured == 2 and .ticks_only == 1] | all),
    timers_still_run: timers_run($up),
    doorbell_through_kernel: ($u.doorbell_sent >= 1 and $u.domains.B.doorbell_received >= 1
      and $u.domains.B.doorbell_received >= $u.doorbell_sent - 1
      and $u.domains.B.pender_task_calls >= $u.doorbell_sent and ($up | wake_hits("0x00020077")) >= 1)
  }
| .verdict = ([.runs, .demo, .no_executors, .no_pender_kernel, .counter_up | to_entries[] | .value | select(type == "boolean")] | all)
