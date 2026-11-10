# RTOS platform workflows

[Replacement index](README.md). [RTOS project #156](https://github.com/skeet70/latent-image/issues/156) owns open work. The [platform report](../reports/rtos-platform-options.md), [manifest draft](../reports/rtos-platform-manifest-draft.md) and [slot contracts](media_permit/rtos_slot_contracts.json) own semantics, intended corrections, retained dependencies and qualification limits.

Use `devenv shell -- host-build` for every Bazel build/test/run, normally with `--jobs=2` and `XM1_EMULATOR_SLOTS=1`. Native firmware execution takes the declared emulator slot; file-only admission/comparison does not. Fresh work, cache and result paths belong under named `private-data/scratch/` directories. A native-only scratch carrier is not admitted live placement.

## Shared layout and family entry points

[`rtos_layout/`](rtos_layout/) supplies typed domain/task/priority identities and the canonical guest record views to RTOS families. Loader domain, task class and executing core are distinct. Use the shared views and each family's region metadata instead of copying addresses or layouts into a new package.

| Family | Build/test/oracle entry points | Human contract |
| --- | --- | --- |
| Own-first slots | `//replacement/rtos_own_first:own-first-a`, `:own-first-b`, `:own-first-host-tests`; oracle/stager below | [Manifest](../reports/rtos-platform-manifest-draft.md), [regions](rtos_own_first/regions.json) |
| Scheduler | `//replacement/rtos_scheduler:all`; oracle/stager below | [Thread/shim design](../reports/rtos-platform-options.md#413-owned-thread-layer-and-stock-abi-shim-design-input-from-the-decomp) |
| Flags/mailboxes | `//replacement/rtos_objects:objects-rust`, `:objects-host-tests`, `:family-suite-runner` | [Family contract](../reports/rtos-platform-options.md#414-owned-eventflag-and-mailbox-family-the-recovered-0xd4-body-and-the-family-design) |
| Data queues | `//replacement/rtos_data_queue:data-queue-host-tests`, `:data-queue-a`, `:data-queue-b`, `:data-queue-oracle` | [Blocking returns and timer seam](../reports/rtos-platform-options.md#417-owned-data-queue-family-blocking-returns-and-the-timer-seam) |
| Task records | `//replacement/rtos_task_records:task-records-host-tests`; [native targets](rtos_task_records/BUILD.bazel) | [Create/delete contract](../reports/rtos-task-records.md) |
| Timer heap | `//replacement/rtos_timer:timer-rust`, `:timer-host-tests`; [native targets](rtos_timer/oracle/BUILD.bazel) | [Membership and arenas](../reports/rtos-platform-options.md#415-owned-timer-heap-membership-host-increment) |
| Global lock | `//replacement/rtos_global_lock:global-lock-rust`, `:global-lock-a`, `:global-lock-b`, `:global-lock-host-tests`; `//replacement/rtos_global_lock/oracle:global-lock-oracle` | [Package/staging](rtos_global_lock/), [platform contract](../reports/rtos-platform-options.md) |
| Exception entry/exit | `//replacement/rtos_exception:exception-a`, `:exception-b`, `:exception-host-tests`, `:exception-artifact-tests`; `//replacement/rtos_exception/stage:stage-exception` | [Exception contract](../reports/rtos-exception-entry-exit.md) |
| IRQ control | `//replacement/rtos_irq_control:irq-host-tests`, `:irq-a`, `:irq-b`, `:irq-native-suite` | [IRQ contract](../reports/rtos-platform-options.md#422-owned-irq-control-and-handler-registration-host-and-native-increment) |
| Cyclic handlers | `//replacement/rtos_cyclic/oracle:cyclic-oracle`; [host/native targets](rtos_cyclic/BUILD.bazel) | [Lifecycle, expiry and reentrancy](../reports/rtos-platform-options.md#421-owned-cyclic-handler-family-lifecycle-expiry-and-reentrancy) |

Current results belong in these technical homes and the ledger. A host model, native return, dispatcher stop and resumed caller establish different scopes; matching unsupported stops are not acceptance.

## Own-first staging and oracle

```sh
devenv shell -- host-build run //replacement/rtos_own_first/stage:stage-own-first -- \
  SOURCE_DAT LOADER_A LOADER_B NEW_OUTPUT_DIRECTORY
XM1_EMULATOR_SLOTS=1 devenv shell -- host-build run --jobs=2 \
  //replacement/rtos_own_first/oracle:own-first-oracle -- \
  --loader-a LOADER_A --loader-b LOADER_B --work NEW_DIRECTORY --summary NEW_JSON
```

Bazel supplies linked candidates, regions and the pinned runner. Staging checks canonical loader/DAT identities, entries, extents, fill and composed frame demand, confines patches/checksum and requires exact restore. It writes `candidate.DAT`, `loader-a.bin`, `loader-b.bin` and `contract.json`.

The oracle compares returned status, r0, preserved registers/SP, CPSR control bits, public data/caller bytes and ordered writes. Private stack, flags, LR and scratch registers are excluded. Its SVC/SYS native cases do not establish IRQ-mode execution, private TCM or concurrent cores. `:own-first-oracle-tests` is the sample; manual `:own-first-oracle-full-tests` runs the full grid. `--timing-only` selects structured cost rows without controls; instruction counts are not silicon cycles.

### Generate and replay

```sh
XM1_EMULATOR_SLOTS=1 devenv shell -- host-build run --jobs=2 \
  //replacement/rtos_own_first/oracle:own-first-oracle -- \
  --loader-a LOADER_A --loader-b LOADER_B \
  --generate 32 --seed 0x2545f4914f6cdd1d --shrink-budget 32 \
  --work private-data/scratch/OwnFirst/NEW_GENERATED \
  --summary private-data/scratch/OwnFirst/NEW_GENERATED.json
XM1_EMULATOR_SLOTS=1 devenv shell -- host-build run --jobs=2 \
  //replacement/rtos_own_first/oracle:own-first-oracle -- \
  --loader-a LOADER_A --loader-b LOADER_B \
  --replay private-data/scratch/OwnFirst/NEW_GENERATED.json \
  --work private-data/scratch/OwnFirst/NEW_REPLAY \
  --summary private-data/scratch/OwnFirst/NEW_REPLAY.json
```

`--generate COUNT --seed SEED` compares valid current-task-query inputs against stock, owned and the mutant in both loaders. Counts are 1..256 per loader. Inputs use actual task-record starts, SVC/SYS and zero/nonzero context; null, misaligned and negative task offsets are outside this corpus. Unsigned 64-bit decimal/hex seeds fold to a 32-bit xorshift seed, so distinct seeds may collide.

`--shrink-budget` is 1..256, default 32. Reduction preserves validity and the original typed boundary while lowering task record, flags, F, mode and context. `fixed-point` means local minimality; budget exhaustion proves no minimum. Replay re-executes original/reduced cases after admitting runner, loaders, linked input, cases and wires; it copies/verifies selected artifacts without altering preserved reports. Nonempty typed boundaries, not diagnostic text, determine reproduction. Work and summary paths are canonical across the private-data symlink.

Every mode uses the existing slot, 2 GiB cap and unchanged storage reserve. Generate exits 1 for a stock stop or missing returned control witness, 2 for an owned difference, 0 for scoped agreement with the control refused. Replay exits 1 for incomplete execution or changed boundary, 2 for reproduced owned discrepancy, 0 for reproduced controls. Admission refusals stop before execution. Completed attempts print a compact result and summary path.

## Scheduler oracles and staging

The [scheduler report](../reports/rtos-platform-options.md#413-owned-thread-layer-and-stock-abi-shim-design-input-from-the-decomp) owns ABI/comparison details, intentional corrections and system coverage. [Region files and linker scripts](rtos_scheduler/) own entry placement and frame declarations. Do not treat a pinned local frame size as a composed bound across callbacks or context switches.

| Oracle target under `//replacement/rtos_scheduler/oracle` | Arguments after Bazel-supplied inputs |
| --- | --- |
| `:scheduler-oracle` (S1) | `--loader-a LOADER_A --loader-b LOADER_B --work NEW_DIRECTORY --summary NEW_JSON` |
| `:scheduler-s2-oracle` | `--loader-a LOADER_A --loader-b LOADER_B --work NEW_DIRECTORY --summary NEW_JSON` |
| `:s3a-oracle` | `--loader-a LOADER_A --loader-b LOADER_B --work NEW_DIRECTORY --summary NEW_JSON` |
| `:s3b-oracle` | `RUNNER STOCK_LOADER_A STOCK_LOADER_B OWNED_A_BIN OWNED_B_BIN NEW_WORK_DIRECTORY` (all positional, including under Bazel) |
| `:s3c-oracle` | `--loader-a LOADER_A --loader-b LOADER_B --work NEW_DIRECTORY` |
| `:s3d-oracle` | `--loader-a LOADER_A --loader-b LOADER_B --cache-root CACHE --results RESULTS` |

S1 compares lock/ready/notification behavior and checks barrier order separately. S2 stops before exception-return execution beyond the runner's admitted subset and uses independent expected repairs for tagged defects. S3 services may stop at a dispatcher entry with an active frame; private active frames are not returned-call ABI. Blocking stops alone do not establish scheduling or resumed callers.

S3d's six lifecycle services have `//replacement/rtos_scheduler:s3d-host-tests` and the cached native oracle above. The same image carries the owned timeout and delay completion callbacks at A `0x44318`/`0x44634` and B `0xd42d8`/`0xd45f4`, with bodies in the native scratch region; the tick still runs from the timer carrier. Each role must reach its declared return value, including zero, or its exact dispatcher stop and switch reason. Matching refusals are not acceptance. The [lifecycle contract](../reports/rtos-platform-options.md#423-owned-task-lifecycle-services-s3d) separates synthetic save/wake/return evidence from full-dispatcher, aggregate placement and system qualification.

```sh
devenv shell -- host-build run //replacement/rtos_scheduler/stage:stage-scheduler -- \
  candidate SOURCE_DAT LOADER_A LOADER_B NEW_OUTPUT_DIRECTORY
devenv shell -- host-build run //replacement/rtos_scheduler/stage:stage-cutover -- \
  all SOURCE_DAT LOADER_A LOADER_B NEW_OUTPUT_DIRECTORY
```

The first stages S1 alone, not own-first. It also accepts `stock-insert`, `silent-notify`, `wrong-predicate`. The combined stager joins S1/S2/S3a/S3b/S3c with disjoint own-first slots. `all` writes the candidate and six controls; individual modes are `candidate`, `restore-s1`, `restore-s2`, `restore-s3`, `silent-notify`, `wrong-predicate`, `wake-no-insert`. Each candidate/control has a DAT, loaders and contract. The output root's `own-first-guards.json` is a guard-only table, not semantic-entry evidence. Restored controls retain candidate readback expectations; semantic controls must fail the semantic checker.

## Cached family oracles

Family CLIs use `Emulation.CachedNativeBatch`: completed raw observations can be reused while current artifact admission and semantic comparison run again. Bazel supplies the runner, generated layout, owned images/symbols and timer dependencies where needed. Inspect the family's `BUILD.bazel` and CLI when overriding those inputs; their argument sets differ.

- Flags/mailboxes: `//replacement/rtos_objects:family-suite-runner` takes `--layout`, `--loader-a`, `--loader-b`, `--owned-a-bin`, `--owned-b-bin`, `--cache-root`, `--results`. The usual layout is `bazel-bin/replacement/rtos_scheduler/oracle/layout.json`. Its [family contract](../reports/rtos-platform-options.md#414-owned-eventflag-and-mailbox-family-the-recovered-0xd4-body-and-the-family-design) distinguishes host pending-token semantics, selected-task arrival and actual suspended-service resumption.
- Global lock: `//replacement/rtos_global_lock/oracle:global-lock-oracle --loader-a LOADER_A --loader-b LOADER_B --cache-root CACHE --results RESULTS`. Direct-helper ABI, stamp conventions and exclusive-monitor limits are in its [package](rtos_global_lock/) and platform report.
- Data queues: `//replacement/rtos_data_queue:data-queue-oracle --loader-a LOADER_A --loader-b LOADER_B --cache-root CACHE --results RESULTS --review REVIEW`. The [family contract](../reports/rtos-platform-options.md#417-owned-data-queue-family-blocking-returns-and-the-timer-seam) distinguishes actual blocking caller returns, per-role timer membership, the retry IRQ window and the task-context caller requirement. Inspect the semantic results/review packet separately from the host-build launcher verdict.
- Cyclic: `//replacement/rtos_cyclic/oracle:cyclic-oracle` supplies the current cyclic/timer dependencies through [its build target](rtos_cyclic/oracle/BUILD.bazel). The [lifecycle contract](../reports/rtos-platform-options.md#421-owned-cyclic-handler-family-lifecycle-expiry-and-reentrancy) covers callback reentrancy and zero-period policy. Native scratch placement does not admit installation over overlapping stock code.

## Manual timer composition

The [timer contract](../reports/rtos-platform-options.md#415-owned-timer-heap-membership-host-increment) owns arenas, capacity, membership and callback/lock premises. The manual composition target requires private canonical inputs and an existing scratch directory:

```sh
mkdir -p private-data/scratch/timer-compose
XM1_EMULATOR_SLOTS=1 devenv shell -- host-build run --jobs=1 \
  //replacement/startup_artifact:timer-compose-tests -- \
  "$PWD/private-data/firmware/x-m1-1.50/FWUP0002.DAT" \
  "$PWD/private-data/scratch/timer-compose"
```

Bazel supplies linked images; the command admits the actual Timer and LoaderStartup pieces and bounded joins. A plain test invocation without the private arguments fails its contract. Host composition and a role-grounded native oracle do not establish runtime arena ownership, independent-lock ownership or integrated hardware behavior.

### Packaged domain-A app join

`//replacement/rtos_cyclic/oracle:join-admission-tests` checks packaged-loader admission without firmware or emulator execution. `:join-oracle` accepts the additional packaged inputs below after aggregate staging. The stock loader arguments remain canonical references; `--packaged-loader-a` is the actual staged loader, not the aggregate flat binary.

Add `--admission-only` to validate the actual private inputs and write their identities without starting a guest. Its result records `runtime_executed: false`; it is a host smoke check, not a native verdict.

```sh
XM1_EMULATOR_SLOTS=1 devenv shell -- host-build run --jobs=4 \
  //replacement/rtos_cyclic/oracle:join-oracle -- \
  --image4 IMAGE4 --loader-a STOCK_LOADER_A --loader-b STOCK_LOADER_B \
  --packaged-loader-a STAGED_LOADER_A \
  --aggregate-bin-a AGGREGATE_BIN_A --aggregate-symbols-a AGGREGATE_SYMBOLS_A \
  --aggregate-placements-a AGGREGATE_PLACEMENT_A --aggregate-carriers CARRIERS_JSON \
  --cache-root SHARED_NATIVE_CACHE --results NEW_RESULTS_JSON --admission-only
```

This bounded qualifier executes registration, repeated ticks, delivery and unregistration through the published root/interface tables into the actual union image. Admission reconstructs that loader from pinned stock, declared UDF-filled carriers, emitted functions and every allocated section, including readonly zeros. The shared cyclic aggregate binder preserves readonly exclusions and public ABI fetch boundaries. Every owned observation must retain its complete loader bytes. Contexts, registry, heap and lock callbacks remain fixture state; this does not boot the staged DAT or exercise a core-1 app caller. Run only with an admitted emulator slot and retained-output allowance.

### Composed root and timer package

`//replacement/rtos_timer:stage-root-platform` binds the existing nine-piece J5 composition, including the reduced-bound task-storage helper, owned root task and owned app dispatcher/caller, to the actual platform union. Bazel supplies all linked inputs and tools. The stager verifies the admitted startup bytes, rejects intersecting startup/platform carriers, stages both aggregate loaders and requires inverse platform restoration to recover the admitted startup DAT. It writes `composed.DAT`, all six decoded targets, `composed-contract.json`, the two pre-platform `startup-loader-*.bin` references and two `effective-carrier-*.bin` slices. The effective slices include actual staged gaps and carrier fill, not unallocated raw linker bytes.

Supply the existing J5 manifest as the guard source. The stager preserves its `primary-publisher/entry-core0` peer, residency, register, allowance and relation declarations, then appends resident coverage for the actual admitted app carriers. Image4 is resident at that boundary. Empty residency declarations are refused. The old empty-guard counterexample is preserved in `TimerAppSystemJoin/root-closure-compose-control-2tbhayg8/stage.log`.

The nested startup contract describes the pre-platform input. Its attach, application and frame runtime witnesses require requalification on the final package. No continuous arena or natural-root acceptance follows from byte composition. The earlier `TimerAppSystemJoin/root-closure-stage-01` remains loader-only and has stock image4; its closed allowance must not be reused for this package.

The completed file-only invocation used the following inputs. Its output now exists; another invocation requires its own grant and a fresh output directory.

```sh
HB=/nix/store/aab2pmlkgb0nvabv3g8hhxnbw6dk8l41-host-build/bin/host-build
OB=/home/murph/.cache/bazel/latent-image-TimerAppSystemJoin
devenv shell -- "$HB" --output_base="$OB" run --jobs=4 \
  //replacement/rtos_timer:stage-root-platform -- \
  "$PWD/private-data/firmware/x-m1-1.50/FWUP0002.DAT" \
  "$PWD/private-data/scratch/J5Root155/manifest-j5-v1.json" \
  "$PWD/private-data/scratch/TimerAppSystemJoin" \
  "$PWD/private-data/scratch/TimerAppSystemJoin/root-closure-composed-01"
```

For that package, the app join additionally takes `--startup-loader-a PACKAGE/startup-loader-a.bin --packaged-image4 PACKAGE/image4.bin`. Loader admission reconstructs the platform change over the recorded admitted startup loader instead of canonical stock; stock comparison still uses pinned canonical input. App admission checks the emitted dispatcher and caller spans in final image4. Owned execution maps its app, dispatcher and retained callback/checker bytes from that final image4. These bounded mappings do not execute the complete image4 or boot its root task.

The app join owns a fresh `<results-directory>/stages` workspace. Publication, each of the five ordinary ticks and every fault stage have separate directories; an existing workspace is refused before case materialization. The shared cache stores reusable observations, not mutable stage work. The ownership-fixed full join rejudges all 28 original wires from cache with 596 checks and 21 controls, zero semantic failures, and retains 28 separate case files and 28 generic judgments. Its 316 historical cache-workspace files remain unchanged, with no added/removed cache keys or shared-cache growth. Generic judgments retain 28 failed exact-memory statuses; terminal observations remain 27 returned and one declared stop-address.

### Bounded native scopes and admission plan

The timer, cyclic and app-join CLIs accept `--scope smoke|full`, defaulting to `full`. Smoke selection changes case selection, not expectations or applicable status/admission checks.

| Qualifier | Smoke | Full | Proposed additional retained cap, smoke / fresh full |
|---|---|---|---|
| Timer | Initialize A/B: 2 comparisons, 4 observations, 56 controls | 56 comparisons, 118 observations including 6 mutants, 280 controls | 8 / 128 MiB |
| Cyclic | Equal-period receipt/rearm A/B: 2 comparisons, 4 observations, 23 controls | 48 comparisons, 96 observations, 179 controls | 8 / 64 MiB |
| Domain-A app join | Publication plus seven lifecycle stages: 16 observations, 21 controls | Publication plus thirteen stages: 28 observations, 21 controls | 160 / 256 MiB |

These bounds were proposed from the earlier measured cohorts, then separately admitted for the recorded leaf smokes and full qualifiers. The app smoke-only flag remains unexecuted. Prior timer work/raw-cache payloads are 54,112,256/38,262,420 bytes; cyclic work/raw-cache payloads are 22,102,016/16,083,462 bytes. Timer initialize produced 869,984 raw bytes across four observations; cyclic receipt/rearm produced 689,994. Historical app observations span multiple generations: the largest individual raw receipt is 3,285,481 bytes, not the whole old 487,075,840-byte cache. The app bounds cover raw copies plus state/input blobs. Actual current totals are recorded below and in the platform report. File-only composition completed under a 64 MiB retained grant and a separate 512 MiB transient codec grant.

The measured canonical sizes give a 46,682,424-byte baseline for all normal retained composition payloads, including decoded targets, both startup-loader references and both effective carrier slices. Compressed image4 changes, JSON and filesystem allocation are additional. The 64 MiB proposal includes those normal outputs, not transient codec scratch. Platform inverse restoration is checked in memory and produces no separate restored DAT. Persisting an extra restored DAT raises the baseline to 85,087,572 bytes; that variant needs a proposed 96 MiB retained allowance plus transient headroom.

Cache reuse requires the canonical immutable runner path and its bytes, the complete ordered expectation-free batch wire, and valid raw/exit/provenance digests. Expectations are judged again on every read. Changing the ordered cohort changes the batch key: cyclic smoke cannot be assumed to reuse a full cohort. Timer initialization batches and the app lifecycle prefix can be reused only if those identities remain equal. The current runner hash is `2bd58d3bc0d30d6fd9de6c22cd94c2c95c40189a0c55bb8232996a421583e8f3`; older `21ff2236ea7060c7c51958668c1debf0b560855b106c9375629a154d50b3aefc` receipts cannot be preclaimed as hits. Budget fresh observations until admission proves otherwise.

After explicit slot and output admission, use Bazel's `--run_under` with the parent-verified main slot wrapper. Host-build owns `--script_path` internally; supplying another one creates the requested launcher but breaks host-build's follow-on handoff. `--run_under` starts the contained qualifier after build admission is released. Keep `XM1_EMULATOR_SLOTS=1` and explicit 2 GiB for these small native scopes. `RUN` must be a newly admitted output directory, `P` the newly staged package, `B` this output base's `bazel-bin`, and `CACHE` the shared native cache. Replace `smoke` with `full` only under its own allowance.

The wrapper normally honors `BUILD_WORKING_DIRECTORY`; remove that variable only around a Bazel `--run_under` wrapper so relative runfiles arguments retain Bazel's working directory. The executed private plans instead record absolute argv for every linked input and avoid this cwd dependency.

```sh
STOCK="$PWD/private-data/analysis/x-m1-1.50"
B="$PWD/bazel-bin"
P="$PWD/private-data/scratch/TimerAppSystemJoin/root-closure-composed-01"
CACHE="$PWD/private-data/scratch/TimerAppSystemJoin/native-cache"
mkdir -p "$RUN"
SLOT_WRAPPER=/var/home/murph/Code/latent-image/private-data/scratch/HostAdmission/bin/emulator-slot-six
XM1_EMULATOR_SLOTS=1 XM1_EMULATOR_MEMORY_MAX=2G devenv shell -- "$HB" --output_base="$OB" run --jobs=4 --run_under="env -u BUILD_WORKING_DIRECTORY $SLOT_WRAPPER" \
  //replacement/rtos_timer/oracle:timer-oracle -- --scope smoke \
  --loader-a "$STOCK/decode/loader-40000.bin" --loader-b "$STOCK/decode/loader-d0000.bin" \
  --aggregate-a-bin "$P/effective-carrier-a.bin" --aggregate-a-symbols "$B/replacement/rtos_platform/platform-a.symbols" \
  --aggregate-a-placements "$B/replacement/rtos_platform/placement-a.json" \
  --aggregate-b-bin "$P/effective-carrier-b.bin" --aggregate-b-symbols "$B/replacement/rtos_platform/platform-b.symbols" \
  --aggregate-b-placements "$B/replacement/rtos_platform/placement-b.json" \
  --aggregate-carriers "$PWD/replacement/rtos_platform/carriers.json" --cache-root "$CACHE" --results "$RUN/timer/results.json"
```

The cyclic and app commands use separate work directories:

```sh
XM1_EMULATOR_SLOTS=1 XM1_EMULATOR_MEMORY_MAX=2G devenv shell -- "$HB" --output_base="$OB" run --jobs=4 --run_under="env -u BUILD_WORKING_DIRECTORY $SLOT_WRAPPER" \
  //replacement/rtos_cyclic/oracle:cyclic-oracle -- --scope smoke \
  --loader-a "$STOCK/decode/loader-40000.bin" --loader-b "$STOCK/decode/loader-d0000.bin" \
  --aggregate-a-bin "$P/effective-carrier-a.bin" --aggregate-a-symbols "$B/replacement/rtos_platform/platform-a.symbols" \
  --aggregate-a-placements "$B/replacement/rtos_platform/placement-a.json" \
  --aggregate-b-bin "$P/effective-carrier-b.bin" --aggregate-b-symbols "$B/replacement/rtos_platform/platform-b.symbols" \
  --aggregate-b-placements "$B/replacement/rtos_platform/placement-b.json" \
  --aggregate-carriers "$PWD/replacement/rtos_platform/carriers.json" --cache-root "$CACHE" --results "$RUN/cyclic/results.json"
XM1_EMULATOR_SLOTS=1 XM1_EMULATOR_MEMORY_MAX=2G devenv shell -- "$HB" --output_base="$OB" run --jobs=4 --run_under="env -u BUILD_WORKING_DIRECTORY $SLOT_WRAPPER" \
  //replacement/rtos_cyclic/oracle:join-oracle -- --scope smoke \
  --image4 "$STOCK/lz/id4-0061c400.bin" \
  --loader-a "$STOCK/decode/loader-40000.bin" --loader-b "$STOCK/decode/loader-d0000.bin" \
  --packaged-loader-a "$P/loader-a.bin" --startup-loader-a "$P/startup-loader-a.bin" \
  --packaged-image4 "$P/image4.bin" \
  --aggregate-bin-a "$B/replacement/rtos_platform/platform-a.bin" \
  --aggregate-symbols-a "$B/replacement/rtos_platform/platform-a.symbols" \
  --aggregate-placements-a "$B/replacement/rtos_platform/placement-a.json" \
  --aggregate-carriers "$PWD/replacement/rtos_platform/carriers.json" --cache-root "$CACHE" --results "$RUN/app/results.json"
```

The actual `root-closure-composed-01` package passes file-only loader/app admission. Its two independently guarded leaf smokes pass 2 comparisons and 4 fresh observations each: timer has 72 checks/56 controls; cyclic has 80 checks/23 controls. All semantic checks hold. The eight generic exact-memory native receipts retain `failed` statuses for writable-region changes; all eight runner observations returned. Guarded launcher intervals overlap for 0.326 seconds. Both have observed 2 GiB memory limits and no reserve or retention violation; their subsecond cgroups have journal start records but no journal end records. `stage-receipt.json`, `smoke-plan.json` and `smoke-receipt.json` retain exact commands, identities and measured allocation. These are bounded semantic-oracle results, not generic native-case passes, natural root boot, physical arena exclusivity or a core-1 app caller.

The default-full timer, cyclic and packaged app cohorts pass 56/48/28 comparisons or records, 3444/1512/596 checks and 280/179/21 controls, with zero semantic failures. They contain 242 observations: 238 fresh and four reused initialization observations. Guarded launcher intervals overlap for 7.034 seconds; journal-recorded cgroup lifecycle intervals overlap for 6.884 seconds. The later workspace-fixed app run rejudges all 28 unchanged original cases from cache and retains every phase judgment. [Issue 162](https://github.com/skeet70/latent-image/issues/162) records the ownership defect, fix, retained original evidence and remaining limits. Exact allocations, resource peaks, raw terminal exceptions and receipt hashes are in `full-receipt.json`, `cgroup-journal-receipt.json` and the [platform report](../reports/rtos-platform-options.md).

The launcher commands above describe the completed legacy admission recipe, not a new launch authorization. New host builds use the parent-approved `/nix/store/4daiijs9y231gjv2i8q5251hpcpkxrj8-host-build/bin/host-build` through devenv. New native scopes use `private-data/scratch/HostAdmission/bin/emulator-slot-weighted-5d1ea738 --native COMMAND`; new full-system guests remain paused. Completed scopes retain their original admission identities.
