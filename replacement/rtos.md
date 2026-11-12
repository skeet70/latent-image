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
| Platform union | `//replacement/rtos_platform:platform-a`, `:platform-b`, `:platform-tests`; `:place-platform`, `:stage-platform` | [Aggregate carrier](../reports/rtos-platform-carrier.md), [platform section](#platform-union-and-images) |

Current results belong in these technical homes and the ledger. A host model, native return, dispatcher stop and resumed caller establish different scopes; matching unsupported stops are not acceptance.

## Platform union and images

`//replacement/rtos_platform` links one relocatable bodies object from the family archives, places every section with `:place-platform` and links the explicit script. `carriers.json` declares each domain's entry veneers, carrier extents, retained externals, emitted demand and reserved section prefixes. Carrier groups are `s1`, `s2`, `s3a`-`s3d`, `own-first`, `semaphore`, `fixed-pool`, `task-records`, `data-queue`, `global-lock`, `cyclic`, `timer`, `shared` and `event-flag`.

The event-flag group has thirteen carriers per domain: the eight flag service extents (`create` `0x4d39c`/`0xdd360`, `delete` `0x4d434`/`0xdd3f8`, `set` `0x4813c`/`0xd80fc`, `set_handler` `0x4839c`/`0xd835c`, `clear` `0x485c0`/`0xd8580`, `wait` `0x486f8`/`0xd86b8`, `poll` `0x489ac`/`0xd896c`, `timed_wait` `0x48b30`/`0xd8af0`) and the five family helper extents (`family_release` `0x44568`/`0xd4528`, `helper_release` `0x44744`/`0xd4704`, `helper_block` `0x44800`/`0xd47c0`, `helper_block_timed` `0x448e4`/`0xd48a4`, `helper_check` `0x480cc`/`0xd808c`). All thirteen entries are four-byte veneers at their stock addresses. The five helper entries are marked `pinned`, so `:place-platform` places each entry section at the start of the run that follows its veneer: `0x4456c`, `0x44748`, `0x44804`, `0x448e8` and `0x480d0` (`0xd452c`, `0xd4708`, `0xd47c4`, `0xd48a8`, `0xd8090` in loader B).

A reserved group's section prefixes are packed into that group's own runs before the global best-fit search: pinned sections first, then a depth-first search over the leftover runs. The thirteen veneers take 52 of the group's 4,532 carrier bytes, and the adapter's reserved sections occupy 38 (A) / 37 (B) placements inside those runs. Against the previous image, all 252 pre-existing placements per domain stayed at their addresses and every pre-existing symbol row is unchanged; the thirteen event-flag sections and symbols are additions. `entry_snapshot` and `unmask_i` are the only source-level cfg differences; the aggregate inlines them (`not(feature = "component")` keeps the standalone stage's separate sections, because it binds one stock extent per function), and the rlib exports generic instantiations as global, so the two builds' machine code also differs in shared functions without a behavioural difference.

At tip `3bc19f3d` the four linked images are platform-a `f5f9c935707f18e7…`, platform-b `41adbaed3d18d88d…`, objects-family-stage-a `402cd9df96c889b2…` and objects-family-stage-b `102c357a8dcc8950…`; the digests are unchanged since `6f234db4`. `:stage-platform stage` on the two platform images wrote `candidate.DAT` `a78209f9…`, `loader-a.bin` `fe28e845…`, `loader-b.bin` `43cf5c57…` and `contract.json` `55f127de…` (run `20261111T223051Z-pfc-stage-platform5`; `carriers.json` is unchanged since `6f234db4`). Image identities, the staged digests and the boot pair are in E-RTOS-EVENT-FLAG-AGGREGATE-150.

The event-flag family's stock-versus-owned comparisons, all on the four images above: the standalone family stage passes 100 cases, 50 per domain, with 416 observation controls and no failure (run `20261112T011447Z` executed both role batches fresh at `6f234db4`; run `20261112T011549Z` reused them and retained the result). Against the aggregate, the family suite passes 100 cases with 440 observation controls, the semaphore suite 64 cases with 16 observation controls, the fixed-pool suite 90 compared cases with 104 observation controls and two wrong-variant rows, and the S3b suite 7,968 cases (664 per service per domain). Each of the four per-domain report pairs carries a `//emulation:judge` verdict `equal`: family `4951c0a5…`/`e83588c2…`, semaphore `c2babaf1…`/`f1d070c6…`, fixed-pool `2ac06a9f…`/`10f55f04…`, S3b `aef54617…`/`4e18dae1…`. The reruns at the merged tip dd368fb0 are family `20261112T160118Z`, semaphore `20261112T160803Z`, fixed-pool `20261112T160924Z` and S3b `20261112T141114Z`; every batch executed (`reuse` all `BatchExecuted`, S3b `reused_observations` 0), the S3b report carries a `differences` array on every case row, all empty, and the judge detects six of six wrong variants. The forced release at A `0x44744` / B `0xd4704` removes the timer, unlinks a task whose state carries `OBJECT_QUEUED` and stores −49, matching stock `0x446fc` / `0xd46bc`; `release_with` unlinks only for the forced completion, the satisfied release at `0x44568` never does, and the standalone stage carries the same body. The semaphore and fixed-pool delete rows that run through `helper_release` are declared differences: stock delete calls only `0x42560`/`0x42568` and never `0x44744`, so the owned role is checked against an owned expectation and the stock role against the waiter staying queued. The family's `wait-resume-delete` row is also a declared difference: its stock side reaches the 20,000-instruction budget without returning, and its owned side expects the wait to return −49 with the task ready and the queue head unchanged. The deliberately wrong variant for that body is the current tree with the forced-release unlink block removed from `wake.rs`: `inputs/wrong-variant/no-unlink-a.bin` `a9a501fc…` and `no-unlink-b.bin` `e1f0c269…`, which differ from the platform images in 668 bytes between A `0x44584` and `0x4d49b` because the removal moves the sections that follow; `helper-release-queued` and both `delete-waiter` rows differ on them. Each suite's wrong-variant control row carries the digest of the image it ran: `a9a501fc…` for domain A and `e1f0c269…` for domain B. The rows the rerun at dd368fb0 detects are `helper-release-queued` (family), `delete-waiter`, `delete-two-waiters`, `delete-handler-defers` and `wait-delete-return` (semaphore), and `delete-waiter`, `delete-two-waiters` and `get-resume-delete` (fixed-pool).

Stock-side branch coverage of the family's stock functions (run `20261111T222032Z`, aggregate inputs): per domain 16 functions and 151 conditional branches, of which 21 are reached in both directions, 111 in one and 19 never, with 314 unexecuted instructions; 800 declared cases, 0 foreign and 0 unobserved. The 19 never-reached branches are the `direction: never` rows and sit in `helper_release`, `helper_block`, `helper_block_timed`, the eight flag services and the two mailbox receives.

The boot pair from reset on the staged DAT, the stock role at `1b1c7e4a` and the owned role at `15801022`, with one observer (`ae499af5…`): stock `20261112T062301Z-pfc20-boot-stock` (passed) executes 383,750,000 instructions with 532,483 entries and 532,481 returns; owned `20261112T065628Z-pfc20-boot-owned` (passed) executes 384,500,000 instructions with 186,647 entries and 186,644 returns. Both reach the cold start (69,52,12) at 80,000,000 and the shooting menu (1,5,2) at their instruction stop, fetch no fill word, keep 0 untracked calls and 0 unarmed entries, and finish with 112 of 112 and 120 of 120 final readback ranges equal to their plans and no view mismatch. Comparison run `20261112T092750Z-pfc20-boot-compare2` passes every required field, with equal run-document, boot-ROM, trajectory and observer digests and equal menu state. The entry totals differ because the plans count entries at stock entry addresses, and in the aggregate owned callers reach owned bodies directly, so only retained stock callers enter through those addresses (A acquire and release 164,471 against 81,993 each, select 27,507 at `0x436ac` against 0 at the owned plan's `0x436a4`, find 21,203 against 0).

| service | A stock | A owned | B stock | B owned |
| --- | ---: | ---: | ---: | ---: |
| `family-flag-clear` | 592 | 592 | 0 | 0 |
| `family-flag-create` | 74 | 74 | 10 | 10 |
| `family-flag-delete` | 1 | 1 | 0 | 0 |
| `family-flag-poll` | 0 | 0 | 0 | 0 |
| `family-flag-set` | 343 | 350 | 0 | 0 |
| `family-flag-set-handler` | 690 | 690 | 0 | 0 |
| `family-flag-timed-wait` | 1,034 | 1,041 | 1 | 1 |
| `family-flag-wait` | 0 | 0 | 0 | 0 |
| `family-release` | 18,416 | 791 | 186 | 12 |
| `helper-block` | 0 | 0 | 0 | 0 |
| `helper-block-timed` | 599 | 141 | 1 | 0 |
| `helper-check` | 1,476 | 0 | 1 | 0 |
| `helper-release` | 0 | 0 | 0 | 0 |

Neither route enters `family-flag-poll`, `family-flag-wait`, `helper-block` or `helper-release` in either domain, nor `family-flag-clear`, `family-flag-delete`, `family-flag-set` or `family-flag-set-handler` in domain B. Where both sides enter a service, the counts agree except `family-flag-set` (343/350), `family-flag-timed-wait` (1,034/1,041), `family-release` (18,416/791), `helper-block-timed` (599/141) and `helper-check` (1,476/0). The extra `family-flag-set` and `family-flag-timed-wait` entries fall inside the shifted window: stock's last calls are at 313,669,055 and 313,695,949 instructions and the owned role's at 320,340,453 and 320,394,088, so the owned route makes those calls later in the same window before its stop, not after the stock stop.

In the aggregate the six mailbox extents are retained stock; in the standalone stage they are owned and three are entered (`send`, `receive`, `timed receive`), while `create`, `delete` and `poll` are not. The boot route reaches no flag wait or poll. Run results, the four verdicts and the image identities are in E-RTOS-EVENT-FLAG-AGGREGATE-150.

### Declared differences

**A removed timer entry's index.** Trigger: the stock timer tick expires the heap head (`kernel_timer_tick`, A `0x44d8c` / B `0xd4d4c`), and stock `kernel_remove_timer` (A `0x44ca8` / B `0xd4c68`) removes an armed entry. Original behaviour: both leave the removed entry's index word at its last heap position. Intended behaviour: the owned timer core (`replacement/rtos_timer/src/lib.rs`) writes `TimerIndex::Detached` (0) when an entry leaves the heap and refuses to insert an entry still marked linked. Remaining retained boundary: the entry record is shared with retained stock callers through the `timer_*` veneers, so the owned tick and remove write a word stock owns; no retained stock caller reads it. A consumer that depended on the stale value would show a retained load of `entry+0x00` or an owned `insert` returning `AlreadyLinked`; `reports/rtos-platform-options.md` records the same stale `entry->index` as the stock race `timeout-entry-not-cleared`, where a waker in the window removes slot 1 or underflows the count. If one appears, recreate the stale write at a named shim with a removal condition. The family suite clears that one word (A `0x40055a3c` in the last observed kernel region) before comparing a `WakerTick` case and compares every other kernel word; the owned `insert` refusal is covered by the timer host tests.

Earlier receipts do not transfer. The composed timer, cyclic and app receipts and the cached family observations (`timer-effective-06`, `cyclic-effective-05`, `task-records-effective-01`, `s3b-effective-02`, `s3a-effective-02`, `semaphore-effective-03`, `context-transfer-07`, `native-increment-06`) bind their own effective slices — the timer and task-record receipts name `ee74cc3d…` / `a88f9423…` — and none names a current platform or standalone image digest. The baseline images `88e35bc2…` / `69244d9c…` entered the tree at `c2fdc01c`, after those receipts, and no `docs/evidence.jsonl` record names either. `E-RTOS-PLACEMENT-BOUNDED-HOST-150` remains the host solver's record; the current image and family qualification is E-RTOS-EVENT-FLAG-AGGREGATE-150.

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
| `:s3d-oracle` | `--loader-a LOADER_A --loader-b LOADER_B --results RESULTS` |

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

Family CLIs use `Emulation.CachedNativeBatch`: completed raw observations can be reused while current artifact admission and semantic comparison run again. Every family CLI defaults `--cache-root` to the shared bounded `private-data/cache/native`; pass it only to override that root. Bazel supplies the runner, generated layout, owned images/symbols and timer dependencies where needed. Inspect the family's `BUILD.bazel` and CLI when overriding those inputs; their argument sets differ.

Family CLIs stage their batch directories outside that cache root, beside their `--results`, `--output` or `--work` output; `rtos_irq_control` uses `$XM1_RUN_WORK/irq-control` when `run-output` exports it and otherwise a `batches` directory beside `--results`. The cache root holds only reusable observations, so concurrent suites never overwrite each other's batches.

- Flags/mailboxes: `//replacement/rtos_objects:family-suite-runner` takes `--layout`, `--loader-a`, `--loader-b`, `--owned-a-bin`, `--owned-b-bin`, `--results`; aggregate mode replaces the owned bins with `--aggregate-{a,b}-bin`, `--aggregate-{a,b}-symbols`, `--aggregate-{a,b}-placements` and `--aggregate-carriers`. The usual layout is `bazel-bin/replacement/rtos_scheduler/oracle/layout.json`. Aggregate mode takes the linked platform image, not a pre-composed slice: it installs carrier bytes over the stock loader and leaves non-carrier extents stock. Its [family contract](../reports/rtos-platform-options.md#414-owned-eventflag-and-mailbox-family-the-recovered-0xd4-body-and-the-family-design) distinguishes host pending-token semantics, selected-task arrival and actual suspended-service resumption.
- Global lock: `//replacement/rtos_global_lock/oracle:global-lock-oracle --loader-a LOADER_A --loader-b LOADER_B --results RESULTS`. Direct-helper ABI, stamp conventions and exclusive-monitor limits are in its [package](rtos_global_lock/) and platform report.
- Data queues: `//replacement/rtos_data_queue:data-queue-oracle --loader-a LOADER_A --loader-b LOADER_B --results RESULTS --review REVIEW`. The [family contract](../reports/rtos-platform-options.md#417-owned-data-queue-family-blocking-returns-and-the-timer-seam) distinguishes actual blocking caller returns, per-role timer membership, the retry IRQ window and the task-context caller requirement. Inspect the semantic results/review packet separately from the host-build launcher verdict.
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
  --results NEW_RESULTS_JSON --admission-only
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

After explicit slot and output admission, use Bazel's `--run_under` with the parent-verified main slot wrapper. Host-build owns `--script_path` internally; supplying another one creates the requested launcher but breaks host-build's follow-on handoff. `--run_under` starts the contained qualifier after build admission is released. Keep `XM1_EMULATOR_SLOTS=1` and explicit 2 GiB for these small native scopes. `RUN` must be a newly admitted output directory, `P` the newly staged package and `B` this output base's `bazel-bin`. The native cache root defaults to the shared bounded `private-data/cache/native`. Replace `smoke` with `full` only under its own allowance.

The wrapper normally honors `BUILD_WORKING_DIRECTORY`; remove that variable only around a Bazel `--run_under` wrapper so relative runfiles arguments retain Bazel's working directory. The executed private plans instead record absolute argv for every linked input and avoid this cwd dependency.

```sh
STOCK="$PWD/private-data/analysis/x-m1-1.50"
B="$PWD/bazel-bin"
P="$PWD/private-data/scratch/TimerAppSystemJoin/root-closure-composed-01"
mkdir -p "$RUN"
SLOT_WRAPPER=/var/home/murph/Code/latent-image/private-data/scratch/HostAdmission/bin/emulator-slot-six
XM1_EMULATOR_SLOTS=1 XM1_EMULATOR_MEMORY_MAX=2G devenv shell -- "$HB" --output_base="$OB" run --jobs=4 --run_under="env -u BUILD_WORKING_DIRECTORY $SLOT_WRAPPER" \
  //replacement/rtos_timer/oracle:timer-oracle -- --scope smoke \
  --loader-a "$STOCK/decode/loader-40000.bin" --loader-b "$STOCK/decode/loader-d0000.bin" \
  --aggregate-a-bin "$P/effective-carrier-a.bin" --aggregate-a-symbols "$B/replacement/rtos_platform/platform-a.symbols" \
  --aggregate-a-placements "$B/replacement/rtos_platform/placement-a.json" \
  --aggregate-b-bin "$P/effective-carrier-b.bin" --aggregate-b-symbols "$B/replacement/rtos_platform/platform-b.symbols" \
  --aggregate-b-placements "$B/replacement/rtos_platform/placement-b.json" \
  --aggregate-carriers "$PWD/replacement/rtos_platform/carriers.json" --results "$RUN/timer/results.json"
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
  --aggregate-carriers "$PWD/replacement/rtos_platform/carriers.json" --results "$RUN/cyclic/results.json"
XM1_EMULATOR_SLOTS=1 XM1_EMULATOR_MEMORY_MAX=2G devenv shell -- "$HB" --output_base="$OB" run --jobs=4 --run_under="env -u BUILD_WORKING_DIRECTORY $SLOT_WRAPPER" \
  //replacement/rtos_cyclic/oracle:join-oracle -- --scope smoke \
  --image4 "$STOCK/lz/id4-0061c400.bin" \
  --loader-a "$STOCK/decode/loader-40000.bin" --loader-b "$STOCK/decode/loader-d0000.bin" \
  --packaged-loader-a "$P/loader-a.bin" --startup-loader-a "$P/startup-loader-a.bin" \
  --packaged-image4 "$P/image4.bin" \
  --aggregate-bin-a "$B/replacement/rtos_platform/platform-a.bin" \
  --aggregate-symbols-a "$B/replacement/rtos_platform/platform-a.symbols" \
  --aggregate-placements-a "$B/replacement/rtos_platform/placement-a.json" \
  --aggregate-carriers "$PWD/replacement/rtos_platform/carriers.json" --results "$RUN/app/results.json"
```

The actual `root-closure-composed-01` package passes file-only loader/app admission. Its two independently guarded leaf smokes pass 2 comparisons and 4 fresh observations each: timer has 72 checks/56 controls; cyclic has 80 checks/23 controls. All semantic checks hold. The eight generic exact-memory native receipts retain `failed` statuses for writable-region changes; all eight runner observations returned. Guarded launcher intervals overlap for 0.326 seconds. Both have observed 2 GiB memory limits and no reserve or retention violation; their subsecond cgroups have journal start records but no journal end records. `stage-receipt.json`, `smoke-plan.json` and `smoke-receipt.json` retain exact commands, identities and measured allocation. These are bounded semantic-oracle results, not generic native-case passes, natural root boot, physical arena exclusivity or a core-1 app caller.

The default-full timer, cyclic and packaged app cohorts pass 56/48/28 comparisons or records, 3444/1512/596 checks and 280/179/21 controls, with zero semantic failures. They contain 242 observations: 238 fresh and four reused initialization observations. Guarded launcher intervals overlap for 7.034 seconds; journal-recorded cgroup lifecycle intervals overlap for 6.884 seconds. The later workspace-fixed app run rejudges all 28 unchanged original cases from cache and retains every phase judgment. [Issue 162](https://github.com/skeet70/latent-image/issues/162) records the ownership defect, fix, retained original evidence and remaining limits. Exact allocations, resource peaks, raw terminal exceptions and receipt hashes are in `full-receipt.json`, `cgroup-journal-receipt.json` and the [platform report](../reports/rtos-platform-options.md).

The launcher commands above describe the completed legacy admission recipe, not a new launch authorization. New host builds use the parent-approved `/nix/store/4daiijs9y231gjv2i8q5251hpcpkxrj8-host-build/bin/host-build` through devenv. New native scopes use `private-data/scratch/HostAdmission/bin/emulator-slot-weighted-5d1ea738 --native COMMAND`; new full-system guests remain paused. Completed scopes retain their original admission identities.

## Current-base candidate producer

`//replacement/rtos_timer:stage-owned-candidate` consumes the complete startup manifest and the current platform/property/record artifacts. It preserves the startup attach guard, full resource set, exact changed extents, carrier collision checks, readback and inverse admission. `stage-root-platform` retains its existing guard-manifest argument and guard-preserving behavior.

```sh
devenv shell -- host-build run --jobs=2 \
  //replacement/rtos_timer:stage-owned-candidate -- \
  CANONICAL_DAT STARTUP_MANIFEST PLAIN IMAGE2 FUNCTION_INDEX \
  PRIVATE_SCRATCH NEW_OUTPUT
```

Bazel supplies the tools, both linked platform images and placements, canonical property bytes/symbols/ELF and record-store bytes/symbols. Canonical property admission uses the current Settings production artifact policy, including ELF/mapping identities and retirement-reference guards. An older candidate receipt does not admit a new stage-4 image or discharge current retained references. All non-image4 DAT resources remain unchanged by the final image4 replacement.

Outputs include the candidate DAT, decoded targets, composition contract and `stock-task-record-plan.json`/`owned-task-record-plan.json`. The contract binds both task-plan hashes. Plans derive from the original canonical DAT/loaders, the packaged platform and final candidate DAT; there is no task-plan-directory option.

`Replacement.RtosTimer.BankPackage` exposes an opaque source-bound `RecipeEditPackage` and `GetterBinding`. Preliminary admission binds the real bank ELF/runtime, allocation, zero reserved config/table and six edit-session source identities. `bind-recipe-edit PREPUBLICATION EDIT_C EDIT_H STORAGE_C ARM_S FIELDS UI_H NEW_OUTPUT` emits the actual package-specific `recipe-edit-getter.ld`. Its `xm1_recipe_edit_get` symbol names the ordinary AAPCS `rb_stage_edit_get`, not a legacy frame-popping arm. The getter implementation remains retained C.

Set `RB_PROPERTY_PUBLICATION=1` with the `modal75,menu-engine,render` RAM builder variant to reserve the property publication layout. Its runtime allocation is 253056 bytes (`0x3dc80`), with capacity for 3977 total publication records and the inverse/service extents. Actual publication counts derive from the admitted canonical-to-recipe delta, not padding or a fixed historical example count. The original 228-record vector loses exactly eighteen obsolete getter/setter table writes before the property delta is inserted ahead of activation.

After linking the recipe property image with the generated include, append `PREPUBLICATION RECIPE_BIN RECIPE_SYMBOLS RECIPE_ELF EDIT_C EDIT_H STORAGE_C ARM_S FIELDS UI_H SDK_QUALIFICATION` to candidate generation. Admission consumes the actual ELF with bytes and symbols. Combined output adds completed `runtime.bin`, `kernel.elf`, `ram-push-manifest.json` and `modal-lifetime-source.json`; config/table capacity, readonly runtime code, canonical preimages, collision, readback and inverse checks remain active. Property-mode installer publication stores use the `STR r3,[r1]` lifetime-consumer ABI. Other installer variants retain their existing store path.

`SDK_QUALIFICATION` is the immutable `private-data/scratch/CandidateMainIntegration/sdk-context-qualification.json`, SHA-256 `fe928fec604f0c950970423c9c098f563d49adcd8209ab79b09f4ae7c3aab678`. It binds Main's complete exercised SDK gate and exact source/model/output identities. Admission checks its entire byte identity and passing verdict, then reads and hashes the compiler-selected driver, RAM-push bridge and run document at those exact store paths. Missing artifacts, changed bytes, another output path or another packet refuse before final candidate generation. The historical SDK qualification packet remains unchanged.

The manual `//host/firmware:synthetic-host-source-validation` target emits actual runtime/ELF, linked recipe, publication table, directly parseable manifest and proof with explicit synthetic provenance. Its separately declared `runtime-source.json` binds the runtime, ELF, manifest and qualified SDK without a loader catalogue. Supply absolute `--repo_env=SYNTHETIC_SPEC=...`, `SYNTHETIC_IMAGE4`, `SYNTHETIC_IMAGE5`, `SYNTHETIC_PLAIN`, `SYNTHETIC_SERVICE_ARCHIVE`, `SYNTHETIC_MENU_TABLES`, `SYNTHETIC_MODAL75_TABLE`, `SYNTHETIC_SDK_QUALIFICATION` and `SYNTHETIC_SOURCE_ROOT` inputs. Its local repository watches every input. Private-input helpers are manual and ordinary wildcard builds do not require these inputs. With none of them set the repository still loads, so `query //...` works, and each input fails when a build asks for it; a partial set is refused at load. The declared `0x02000000` base is synthetic, not an allocation witness or candidate admission.

Current-base host qualification emitted the real 253056-byte runtime, ELF, directly parseable manifest, qualified-SDK runtime source and a 2552-record table: 2342 actual property delta words, 2455 setup records and 97 activation records. All thirteen synthetic source/artifact/collision/count/capacity/stale/inverse/readback/SDK controls passed, and the four-view transaction inverted exactly. The immutable output archive is `private-data/scratch/CandidateComposition/synthetic-combined-current-02.tar`; it does not reuse the earlier 1639-record receipt. Sixteen unique host test targets passed, including the actual owned LoaderA/B catalogue and retained worker-source mutation controls. The strict recorded-stock reader also compared the original picker15 stock/owned documents successfully, preserving the ordered setter changes 73→201→73 without guest replay or document changes.

Main integration separately generated `private-data/scratch/CandidateMainIntegration/synthetic-main-01.tar` against its compiler-selected SDK and immutable Main qualification packet. Its runtime and SDK identities are distinct from the frozen current-port archive. The old certificate/compiler mismatch remains a refused counterexample. SDK test-suite cardinality is not an admission identifier.

This is synthetic host source validation, not an admitted candidate DAT. The current canonical stage-4 producer still refuses its 193 retained references; that guard was not bypassed. The `0x02000000` layout has no live allocation witness. A permanent boot producer, actual retained allocation lifetime, current installer/task identity, worker delivery and both-core cache handback remain runtime prerequisites. The edit getter and application draw resolver remain retained C/application code. `total`, candidate admission for synthetic outputs and installed-lifetime admission remain false.
