# Host autofocus experiments

## Shared Rust candidate controller

`//af:candidate-controller` builds the allocation-free Rust library in [candidates/controller.rs](candidates/controller.rs). Its explicit `Controller::step(Event) -> Action` interface runs A0-structural's decode-bound direction/peak/three-position model, A1 coarse/fine scanning and A2 adaptive local peak estimation. The [candidate report](../reports/af-candidates.md) records source reproducibility, synthetic parameter provenance, the bounded Run-3 calibration gap and resource bounds.

The Haskell comparison harness sends all three candidates the same linearly interpolated synthetic focus curves and initial position. It prints CSV with `origin=simulated`, accepted sample count, simulated travel in artificial position units and absolute final-position error against the declared optimum. A refusal has no final-position error. A0-structural is an explicitly synthetic parameter instantiation, not calibrated stock X-M1 behavior or camera timing.

```sh
devenv shell -- bazel test --lockfile_mode=error //af:candidate-replay-tests //af:candidate-compare-tests
devenv shell -- bazel run //af:candidate-compare -- af/candidates/fixtures/single-peak.curve af/candidates/fixtures/two-depths.curve af/candidates/fixtures/edge-peak.curve af/candidates/fixtures/noisy-peak.curve af/candidates/fixtures/flat.curve
```

The library has no I/O, allocator, floating-point dependency or global mutable state. The stdio adapter is host-only. It does not supply a camera movement, completion or freshness contract.

## Legacy synthetic C prototype

This package exercises one bounded coarse/fine contrast search in portable C. It has no camera adapter, firmware bytes, lens calibration, or target addresses. A `move_to` is an output decision consumed by an artificial lens model. `hand_back_to_stock` requests hand-back; it does not demonstrate restoration of any target state.

The implemented C kernel and stdio bridge remain migration debt under [current implementation policy](../docs/principles.md#implementation-and-build), not a policy for new decision code. New models, fixtures, tests and native decision/execution code use Haskell/Rust; C needs a technically necessary engine or ABI boundary.

Run from the repository root in the checked-in environment:

```sh
devenv shell -- bazel test --lockfile_mode=error --repository_disable_download //af:kernel-tests //af:simulation-tests
devenv shell -- bazel run --lockfile_mode=error --repository_disable_download //af:simulate -- af/fixtures/peak-delayed.json
devenv shell -- bazel run --lockfile_mode=error --repository_disable_download //af:simulate -- af/fixtures/peak-delayed.json --trace
```

`simulate` consumes a regular JSON fixture and prints a synthetic summary, optionally including the bounded trace. It does not write an output path. The Haskell coordinator uses pinned Aeson with the shared strict duplicate-key/nonfinite guard to consume the contracts. The decision path remains C; Bazel is the only build system. The decision and schema-checker subprocess executables in the Bazel wrapper come from the locked build environment. The underlying Haskell CLI accepts executable paths for host integration and assumes those executables are trusted.

## Contracts and arithmetic

The [simulation schema](../schemas/af-simulation.schema.json) requires origin `synthetic`, status `synthetic-only`, controller `A1-coarse-fine-v1`, clock `synthetic-ms`, and position units `synthetic-lens-unit`. Unknown fields, hardware origins, unknown versions, and stock performance fields are rejected. The [observation/action schema](../schemas/af-observation-action.schema.json) closes both envelope shapes. Decimal string IDs/timestamps preserve integer precision; consumers additionally check uint32 IDs and uint64 ticks. No real clock is synchronized with the artificial clock. Its origin is zero, tick rate is 1000 ticks per arbitrary synthetic second, and wrap is rejected rather than modeled. Model timestamps are exact within this artificial system, with no claim about physical uncertainty.

`af_kernel.h` describes an in-process state/step interface. Call `af_init` with valid distinct output/state/configuration objects and use the initialized state in subsequent `af_step` calls with valid pointers. State and observations are caller-owned; the kernel does not allocate, perform I/O, access global mutable state, or serialize structs. Structure padding, enum layout, and ABI are intentionally absent from interchange. State initialization uses `memset`; target linkage and its implementation remain future work.

Positions are signed int32 values; scores, command IDs, sequences and counters are uint32; clock ticks are uint64. Position differences and search increments use int64 intermediates. Range span must be 1–1,000,000, and `1 <= fine_step <= coarse_step <= span`. Commands clamp search endpoints within declared bounds. The command counter fails before uint32 wrap, sequence reuse/reordering causes a hold, and clock regression or acquisition after delivery causes failure. Time differences are subtracted only after ordering checks. Confidence requires a nonzero configured contrast span and a minimum peak score; these thresholds have artificial contrast units and no calibrated probability meaning.

Each observation retains the command ID and commanded position in effect **at acquisition**. Delivery can occur after later commands; mismatched samples do not enter the search. Reported position and completion describe acquisition-time synthetic measurements and remain separate from commanded position. Nulls become explicit C known flags; the associated zero placeholders in the text bridge have no measured meaning. The default fixtures require both telemetry signals. The optional-telemetry fixture disables both requirements and treats commanded position as a proxy after the declared settling interval; its instantaneous artificial movement is an explicit assumption, not evidence that this policy is valid on a lens. A known false completion or known mismatched position still causes a hold regardless of the optional policy.

The kernel scans the entire declared range at coarse increments, includes its upper endpoint, then scans the clamped bracket one coarse step either side of the best coarse position at fine increments. Strictly greater scores update the best result; ties retain the earlier sampled position. It moves to that best result and requires a fresh matching observation for final verification. A flat/low peak fails with `low_confidence`; a changed final score can fail with `final_verification_failed`. There is no fit, retained-scene state, learned predictor, retry controller, or stock comparator.

Clock/order and exhausted budgets are checked before cancellation, unsupported mode or exposure-epoch changes. Those changes request terminal hand-back. Otherwise duplicate, wrong-command, early, stale, unfinished, unknown-required, mismatched, and invalid observations produce explicit `hold` reasons. Holds consume step/time budgets, so missing data cannot wait forever. Moves and accepted measurements consume separate budgets. Terminal states issue no further movements. This implementation emits `move_to`, `hold`, `finish`, `fail`, and `hand_back_to_stock`; the common action envelope reserves `move_by` for a future consumer, and A1 does not emit it.

## Explicit host bridge

`decision-stdio` takes no hardware arguments. Its ASCII protocol carries named fields in a fixed order, not host C memory. Every line must end in a newline and fit within 1023 bytes; missing/extra fields, unsupported record versions, out-of-range integers and nonboolean flags fail. `AF1` initializes a state, then `OBS1` supplies observations; each response is `ACT1`.

| Record | Ordered fields after the prefix |
| --- | --- |
| `AF1` | min, max, coarse_step, fine_step, min_peak_score, min_contrast_span, confirmation_drop, settle_ms, max_age_ms, max_moves, max_measurements, max_steps, max_duration_ms, require_measured_position, require_completion, exposure_epoch, started_at |
| `OBS1` | sequence_id, command_id_at_acquisition, commanded_position_at_acquisition, acquired_at, delivered_at, position_known, reported_position, completion_known, reported_completion, valid, score_known, sharpness, exposure_epoch, cancelled, supported_mode |
| `ACT1` | kind, reason, command_id, trigger_known, triggering_sequence_id, target_known, target_position, measurements, moves, steps, terminal |

The C bridge accepts at most 1025 observation lines and stops at a terminal action. EOF before termination is a failure. Raw bridge stdin waits for its caller; the supported coordinator bounds each response to 3 real seconds, total exchanges to 30 real seconds, and shutdown to 1 second. These are host execution safeguards, not lens timing or target compute budgets.

The coordinator rejects leaf symlinks and special files before opening fixture input, then uses `O_NOFOLLOW | O_NONBLOCK` and descriptor checks to guard replacement by a FIFO/device. It reads at most 1 MiB, rejects duplicate JSON keys and nonfinite numbers, and validates an owned temporary copy. Parent directories and trusted build tools are host assumptions. Structural checks take at most 15 seconds per pre/post validation subprocess. A run has at most 4096 acquisition/delivery events, 8192 trace records and 8 MiB of trace/schema documents. Event/trace exhaustion is a host error with no successful controller result. Process failures kill the direct C child and preserve a failure exit; no host output artifact is advertised as successful.

## Artificial model and coverage

At acquisition the model captures its current command, actual position, completion and score; delivery queues preserve that snapshot. Acquisition wins ties with delivery in the artificial event ordering. Motion uses integer linear interpolation toward a command, rounding distance toward zero, with configurable arbitrary travel/base times. Completion occurs when the declared duration elapses. Parabolic/flat contrast and seeded bounded noise are constructed formulas. See [fixture provenance](fixtures/README.md).

The C test covers peak/flat/final-verification behavior, positions near both int32 extremes, uint64 clock regression/wrap rejection, command overflow, duplicate/invalid/stale/mismatched records, optional unknown telemetry, budgets and terminal handling. The caller-level test runs 21 finite closed-loop scenarios, validates every emitted envelope, and checks that accepted measurements belong to the pending acquisition command. It also rejects unsupported contract fields/versions, malformed bridge input, integer overflow, FIFO/directory/device/symlink/oversized fixture input, a regular-file-to-FIFO replacement, and duplicate JSON keys.

These checks demonstrate implementation and contract behavior under synthetic assumptions. They do not reproduce a recorded path, measure stock AF, accept G6, calibrate lens bounds, or establish target memory/runtime limits. The [sensor-backed process](../spec/05-autofocus.md#110-stock-first-sensor-backed-iteration) starts with stock observations and coherent image/statistic provenance, then measured lens operations and reusable datasets. Its camera adapter, recorded-input provider and host optical-feedback loop are proposed work, not capabilities of this synthetic-only CLI.
