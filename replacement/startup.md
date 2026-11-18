# Startup and memory workflows

[Replacement index](README.md). The [startup project #155](https://github.com/skeet70/latent-image/issues/155) owns open work; the [startup roadmap](../reports/replacement-inventory-baseline.md#820-composed-startup-artifact-roadmap-155) and [application-readiness contract](../reports/startup-application-readiness.md) own the design and findings. Run builds, tests and Bazel CLI targets through `devenv shell -- host-build`; see [host admission](../build-support/README.md#commands).

## Composed artifact

`startup_artifact/` admits component artifacts, stages one DAT and restores it to the canonical input before accepting the edit set. Use the [manifest types and adapters](startup_artifact/Replacement/StartupArtifact/) rather than overlaying independently staged DATs. The [composed observer guide](../emulation/README.md#composed-startup-observer-155-step-0) documents runtime roles, guards, comparison and application controls.

```sh
devenv shell -- host-build run //replacement/startup_artifact:stage-startup-artifact -- \
  CANONICAL_DAT MANIFEST SCRATCH_DIR OUTPUT_DIR
devenv shell -- host-build test --jobs=1 --local_test_jobs=1 \
  //replacement/startup_artifact:startup-artifact-tests
```

Bazel supplies the encoder, rebuilder and decoder before the displayed arguments. Use a fresh output directory and an existing named scratch directory under `private-data/scratch/`. Component carriers and joins come from their admitted artifacts, not a handwritten byte patch. A pending observation-based join is not qualified by host staging.

Query a contract without executing firmware:

```sh
devenv shell -- host-build run //emulation:composed-startup -- \
  --check-contract CONTRACT \
  --qualifications replacement/startup_artifact/qualifications.json --output NEW_FILE
```

The result distinguishes `host-composed`, `compared` with its evidence record, and `pending` with outstanding joins. Contract bytes are immutable once evidence binds them. Add a [qualification registry](startup_artifact/qualifications.json) row in the same change as its established evidence record. The record must name the digest prefix and bind the actual staged `composed-contract.json`, not a control or restage. Registry lookup does not replace independent receipt comparison.

## Early stages and publishers

| Package | Build/stage entry point | Contract and observer usage |
| --- | --- | --- |
| [First stage](first_stage/) | `//host/firmware:build-first-stage` | [First-stage handoff witness](../emulation/README.md), [inventory](../reports/replacement-inventory-baseline.md) |
| [Secondary stage](secondary_stage/) | `//replacement/secondary_stage:secondary-stage-image`, `//host/firmware:build-secondary-stage` | [Record-2 witness](../emulation/README.md#owned-record-2-startup-witness-schema-3) |
| [Loader prefix](loader_prefix/) | `//host/firmware:build-composed-startup` links the first stage and single secondary-plus-prefix image | [Prefix witness](../emulation/README.md#loader-b-prefix-witness-schema-4), [composed witness](../emulation/README.md#composed-startup-witness-schema-5) |
| [Primary publisher](primary_initializer/), [secondary publisher](interface_initializer/) | Package `BUILD.bazel` declarations | [Publication and installation contract](../reports/replacement-inventory-baseline.md#812-image-4-phase-0-installation-contract-150) |
| [Loader orchestration](loader_orchestration/) | `:loader-a-orchestration-image`, `:loader-b-orchestration-image` | [Owned loader source](../emulation/README.md#owned-loader-orchestration-source-149) |

The shared-engine extraction changes first-stage executable bytes, not only ELF metadata. [Hash-validated old/current ELF snapshots and LLVM output](../private-data/scratch/LaterLoadingClosure/runs/20261116T042627Z-first-stage-shared-engine-runtime-comparison/result/) show identical entry/load layouts, all defined global exports and `.rodata`. Private function placement and corresponding branch encodings change: standalone `Session::open` moves from `0x6b7c` to `0x6d08`, and `close` from `0x6c88` to `0x6d5c`. Standalone remains `[0x6000,0x8c68)` and fixed-image remains `[0x6000,0x8e2c)`. Runtime-byte equivalence is not claimed.

The newly qualified standalone ELF is `0f1ea73c310bbb8844f713eaa7e355b28efbd0310702b6a8014633e10594de83`, binary `b60d63cb6e2ca73af7c58cd2f7ddf7a7f3ca077f27a1b4a28403362e5e759434`. Fixed-image ELF is `dfd17727a4d21c595112a9d759f8ac5cb5b5ee95fa0ef9a5a4bb3fac77f9c709`, binary `f45910c3364c76aed9e522790cbe93c7c2545fca6c413a339e16f9fb34636593`. Both binaries were extracted using the canonical `llvm-objcopy -O binary` recipe and independently rebuilt for the affected gate.

The [new RAM experiment](../private-data/scratch/LaterLoadingClosure/runs/20261116T042727Z-first-stage-shared-engine-ram-decode/result/e2e-result.json) cold-starts canonical firmware, installs the new `decode-test.o`, passes installation/trajectory/unload, and its [existing receipt consumer](../private-data/scratch/LaterLoadingClosure/runs/20261116T043257Z-first-stage-shared-engine-ram-consumer/result/verdict.json) accepts11 jobs,2,631,680 output bytes and pinned CRC-32 `0x21a1a3e5`, with a nonzero status-read count for every job. [Both stock- and RC0-derived new standalone packages pass both existing cold-menu backends](../private-data/scratch/LaterLoadingClosure/runs/20261116T043348Z-first-stage-shared-engine-boot-backends/result/). The [new fixed-image positive](../private-data/scratch/LaterLoadingClosure/runs/20261116T043724Z-first-stage-shared-engine-fixed-control/result/success/fixed-image.json) reaches the menu with six published-entry calls; its [actual body-bit corruption control](../private-data/scratch/LaterLoadingClosure/runs/20261116T043724Z-first-stage-shared-engine-fixed-control/result/corrupt/fixed-image.json) halts before handoff with zero body fetches or published-entry calls. These are host/emulator results, not hardware or Root's final composed-artifact qualification. Catalog and registry pin updates belong to the integration owner.

For standalone record-2 staging, build the native image and pass its actual symbols:

```sh
devenv shell -- host-build build //replacement/secondary_stage:secondary-stage-image \
  //host/firmware:build-secondary-stage
devenv shell -- host-build run //host/firmware:build-secondary-stage -- \
  SOURCE_DAT LINKED_BIN SYMBOLS OUT_DAT --load-offset 0x400 --entry 0x10400 \
  --expect-slot-sha256 SOURCE_RECORD2_SLOT_SHA256
```

The expected digest is the stored record-2 slot, not the whole DAT. The builder checks the declared slot, entry, retained extents and outside-byte preservation. Use the combined builder for combined startup; do not overlay standalone record-2 and loader-prefix images.

## Heap and record-5 staging

The [heap survey and cutover contract](../reports/stock-heap-survey.md) owns the allocator choice, retained interfaces, intentional differences and qualification limits. The [heap project #148](https://github.com/skeet70/latent-image/issues/148) owns remaining closure work.

- `//replacement/general_heap:general-heap-image` and `:general-heap-recipe-image` build the original and recipe-ceiling variants.
- `:general-heap-host-tests` and `:general-heap-recipe-host-tests` are **build targets** that run the pinned Nix behavioral suite, not Bazel test targets or independent Cargo commands.
- `:general-heap-tests` checks artifact admission; `:heap-bridge-replay` consumes the admitted lifetime tape through the production stock bridge.

```sh
devenv shell -- host-build run //replacement/general_heap:stage-general-heap -- \
  SOURCE_DAT IMAGE4 IMAGE5 LINKED_BIN SYMBOLS SCRATCH_DIR \
  OUT_IMAGE4 OUT_IMAGE5 OUT_DAT OUT_CONTRACT
```

The scratch directory must already exist under `private-data/scratch/`. The stager checks canonical inputs, carried-stream decode and outside-byte preservation. [Shared record-5 staging](record5_staging/) is reused by the heap, application pump and composed stager; its manual `:record5-staging-canonical-tests` takes `CANONICAL_DAT IMAGE4 IMAGE5 SCRATCH_DIR`. Host admission is not natural-run or silicon qualification.

The [current cutover design](../reports/general-heap-cutover.md) states the comparison projection and caller-grounded corrections. `:heap-mutant LINKED_BIN SYMBOLS OUT_BIN RECEIPT_JSON` creates a deliberately wrong owned binary: malloc enters a trampoline in the unused trim body, stores errno 12 through its context, then resumes malloc after its displaced prologue. Stage that binary through the same stager and run stock and owned roles against its contract. Both roles must complete their local checks, then `--cutover-check` must report different ordered interface traffic. The receipt binds the original/mutant digests and the branch site/target.

`heap-observe --cutover-stock` and `--cutover-owned` take `CONTRACT STOCK_DAT CANDIDATE_DAT ANALYSIS RUN ROM OUTPUT` and execute one cold-boot role each. Use a separate emulator slot for each role, then run `--cutover-check CONTRACT STOCK_DAT CANDIDATE_DAT ANALYSIS RUN ROM PAIR_DIRECTORY REPORT_JSON` outside the slot. `PAIR_DIRECTORY` contains the `stock` and `owned` role directories. The consumer requires identical global logical-call order, separate calloc arguments and matching final frames; it does not require matching allocation addresses.

## Component stagers

Bazel supplies `ENCODER REBUILD DECODER` for these CLI targets. All source arguments are canonical inputs; `LINKED_BIN SYMBOLS` name the actual emitted artifact. Unless a command creates its own output directory, create an existing named scratch directory first and use fresh output paths.

| Component | Target | Arguments after `--` |
| --- | --- | --- |
| [Task storage](task_storage/) | `//replacement/task_storage:stage-task-storage` | `SOURCE_DAT IMAGE4 LOADER_A LOADER_B LINKED_BIN SYMBOLS SCRATCH_DIR OUT_DAT OUT_CONTRACT` |
| [Plain producer](plain_load/) | `//replacement/plain_load:stage-plain-load` | `SOURCE_DAT IMAGE4 LINKED_BIN SYMBOLS SCRATCH_DIR OUT_DAT OUT_CONTRACT` |
| [Image5 producer](image5_load/) | `//replacement/image5_load:stage-image5-load` | `SOURCE_DAT IMAGE4 LINKED_BIN SYMBOLS NEW_OUTPUT_DIR` |
| [Idle task](idle_task/) | `//replacement/idle_task:stage-idle-task` | `SOURCE_DAT IMAGE4 LINKED_BIN SYMBOLS SCRATCH_DIR OUT_DAT OUT_CONTRACT` |
| [Root task](root_task/) | `//replacement/root_task:stage-root-task` | `SOURCE_DAT IMAGE4 LINKED_BIN SYMBOLS SCRATCH_DIR OUT_DAT OUT_CONTRACT` |
| [Task submission](task_submission/) | `//replacement/task_submission:stage-task-submission` | `SOURCE_DAT IMAGE4 LOADER_A LOADER_B LINKED_BIN SYMBOLS SCRATCH_DIR OUT_DAT OUT_CONTRACT` |
| [Phase dispatcher](startup_dispatch/) | `//replacement/startup_dispatch:stage-startup-dispatch` | `SOURCE_DAT IMAGE4 LOADER_A LOADER_B LINKED_BIN SYMBOLS SCRATCH_DIR OUT_DAT OUT_CONTRACT` |
| [Dispatcher and installer](startup_composition/) | `//replacement/startup_composition:stage-startup-composition` | `SOURCE_DAT IMAGE4 LOADER_A LOADER_B DISPATCH_BIN DISPATCH_SYMBOLS INSTALL_BIN INSTALL_SYMBOLS SCRATCH_DIR OUT_DAT OUT_CONTRACT` |
| [Joined task submission](startup_composition/) | `//replacement/startup_composition:stage-task-composition` | `DAT IMAGE4 LOADER_A LOADER_B WRAPPER WRAPPER_SYMBOLS CRITICAL CRITICAL_SYMBOLS STORAGE STORAGE_SYMBOLS SCRATCH_DIR OUT_DAT OUT_CONTRACT OUT_IMAGE4 OUT_LOADER_A OUT_LOADER_B OUT_RECORD2` |

Image5 staging creates `NEW_OUTPUT_DIR` under `private-data/scratch/` and writes `candidate.DAT`, `image5-load-v1.bin`, `image5-load-v1.symbols` and `owned-image5-stage-v1-contract.json`. Other component outputs are explicit paths. Composition admits joined frame demand at each actual call SP; an isolated component proof does not admit an owned provider automatically.

| Human contract | Authoritative reference |
| --- | --- |
| Task-storage transition, aliasing and frame premises | [Inventory §8.9](../reports/replacement-inventory-baseline.md#89-bounded-task-storage-source-increment-143), [artifact admission](task_storage/Replacement/TaskStorage/) |
| Plain/image5 producers and loading/publication limits | [Inventory §8.15](../reports/replacement-inventory-baseline.md#815-later-executable-loading-and-publication-153) |
| Idle/root build and runtime boundary | [Initial task-body guide](../emulation/README.md#initial-task-body-boundary-152), [package targets](idle_task/BUILD.bazel), [root targets](root_task/BUILD.bazel) |
| Task submission and joined task lifecycle | [Startup task lifecycle](../emulation/README.md#startup-task-lifecycle-observation-151) |
| Image5 initialization and constructors | [Application-readiness contract](../reports/startup-application-readiness.md#4-owned-units-after-j8), [constructor contract](../reports/startup-application-readiness.md#9-j10-task-constructor-contract) |

`//replacement/image5_init:image5-init-oracle` takes `STOCK_IMAGE5 REPORT_JSON` after its Bazel-supplied arguments and runs the bounded native contract. The manual `//replacement/startup_artifact:startup-artifact-j9-canonical-tests` takes `CANONICAL_DAT SCRATCH_DIR`; neither is a full-system run.

## Natural plain read-success witness

The [loader continuation guide](../emulation/README.md#loader-and-returned-code-continuation-observation-149-153) owns the runtime route. `--plain-witness retained|owned` selects the producer observation; the owned role also needs `--plain-artifact DIR` containing `owned-plain-stage-v1-contract.json`, `plain-load-v1.bin` and `plain-load-v1.symbols`. The analysis directory supplies independently bound image4, image5, plain image and `payload.bin`.

Each role exports/restores its own zero-instruction checkpoint. Run each observer serially under `XM1_EMULATOR_SLOTS=1` and `//emulation:emulator-slot`, with a new output directory. File-only commands are:

```text
loader-continuations --verify-plain ROLE_DIR --analysis ANALYSIS
loader-continuations --plain-controls ROLE_DIR --analysis ANALYSIS --output NEW_CONTROLS
loader-continuations --compare-plain-retained RETAINED --compare-plain-owned OWNED --analysis ANALYSIS --output NEW_COMPARISON
```

Read-success comparison is not a proof of nonzero mode, error-side publication, transient mapping exclusion, continuous stack confinement or complete retained-service/resource lifetimes. Progress and completion markers do not substitute for committed executable bytes.

## Bounded image5 skip/refusal and CPU-fill witness

`//replacement/image5_load:image5-native-routes` executes the stock flashloader task from its entry, the retained or emitted Rust image5 producer, the actual storage adapter's unconfigured refusal, and the actual byte/word BSS fill routines. Three cases select rejected aligned fill, unaligned BSS base and unaligned BSS length. The mailbox, mode, segment and RTOS/cache service sinks are synthetic. No decoder, NAND transfer or scheduler law is supplied by this route.

Build the executable, `//replacement/image5_load:image5-load-image`, `//emulation:native-runner` and `//emulation:emulator-slot`, then run:

```sh
run-output exec --label image5-native-routes-controls -- bash -c '
  exec bazel-bin/emulation/emulator-slot --native bazel-bin/replacement/image5_load/image5-native-routes \
    bazel-bin/emulation/native-runner "$1" \
    bazel-bin/replacement/image5_load/image5-load-image.bin \
    "$XM1_RUN_RESULT/routes.json" --native-jobs 1
' _ "$IMAGE4"
```

The consumer checks exact BSS zeroing and unchanged halo/destination bytes, command done before release, cleared busy at callback, error delivery and the cache boundary. It also executes producers with omitted CPU fill, erased error status and omitted cache publication. Unknown execution cannot count as a passing negative control. This is conditional producer/caller qualification, not a natural reset route or permission to enter stale image5 code after skip-mode completion.

## Missing-segment and plain skip caller witnesses

The same native executable also accepts these route selections:

```text
image5-native-routes RUNNER IMAGE4 IMAGE5_BIN PLAIN_BIN REPORT --missing-segments --native-jobs 1
image5-native-routes RUNNER IMAGE4 PLAIN_BIN REPORT --plain-skip --native-jobs 1
```

Run each under `run-output exec` and `emulator-slot --native` as above, with a fresh report path in `$XM1_RUN_RESULT`. The missing-segment route enters the actual flashloader and both retained/owned producers. Its oracle requires the source-bound halt PC at the instruction budget, command busy retained, done zero, no release/callback, no mode call and unchanged destination/BSS/flag. An executable premature-return control must instead complete the command and be refused by that oracle.

Plain skip qualification executes the actual progress reporter and notification wrapper through their message-send boundary, not only the producer's progress-call arguments. Zero and three-block cases check command/resource order, delay calls, actual notification headers and payloads, monotonically retained progress and the callback. The synthetic notification recipient does not initialize an extent or set its flag. Notifications and successful command completion with unchanged destination bytes are not fresh residency.

## Typed lower read and task60 owner

`//replacement/application_nand:arm` exposes the 128 KiB-block storage/DMA owner as an rlib. `//replacement/executable_loading:arm` exposes the sole typed queue60 sender, receiver, plain/image5 producer and completion owner. The installed composition is Root's fixed bank: `//replacement/saclac:saclac-image-loading-{fujimod,retention}` link `executable_loading:arm-root`, which uses the NAND owner already linked inside `power_lifecycle:services-arm`, the owned image5 decoder and one explicit policy into the single manifest/CRC/BSS owner. The default `saclac-image` does not link the loader. `//replacement/executable_loading:runtime-{native,retention}` and `runtime.ld` are native-qualification harnesses only. `//replacement/develop:develop-rust` consumes the typed client API and no longer publishes the overlapping `0x73b238`, `0x73aca4` or `0x73b728` entries.

`stage-saclac` publishes the stock constructor/request/cancel wrappers `0x73ac74`/`0x73ac94`/`0x73aca4` only when the linked bank carries all three loader bodies; a partial owner is refused, and admission checks the receiver against its `0xc00`-byte stock task stack. Startup obtains schema1 `LoadingConfiguration`, configures the owner, creates queue60 with the stock geometry (`dispatch_create_channel` takes message count `0x30`, then message bytes `0x10`: an 8-byte header and 8-byte payload), creates the priority4/`0xc00`-stack receiver task, and reports readiness only after both exist. Each message carries `(request id, command kind)`; the 24-byte envelope is an owned per-command record (`XM1_LOADING_ENVELOPES`) written before send and read after receive with release/acquire ordering. The Sender admits one outstanding request per kind, so a record is not rewritten before its completion. There is no deadline-less receiver or compatibility owner.

`LoadingConfiguration.deadline(context, command_kind, out_absolute_count)` is required and must return a finite absolute count on the qualified clock. Each posted request forms it once, stores that value in its envelope and uses it for read, stop, quiescence, release and publication. Cleanup receives no renewed budget. As in stock `flashloader_request`/`flashloader_cancel`, a kind whose completion latch is already published answers at once (request callback `(argument, 0)`) and posts nothing. Abort word `0x91482c` remains the task argument. The library defines no policy duration; `policy.rs` variants are explicit qualification inputs (`fujimod`: FujiMod `3d90035d` drain, `2^32` ticks; `retention`: false external quiescence; `external`: true external quiescence). None is a production default.

The lower owner authenticates the canonical plain and image5 descriptors (stock segment ids 6 and 5), transfers one `0x20000`-byte usable block at a time, skips bad physical blocks without advancing the destination, and keeps the destination, driver and channel when stop cannot prove drain. A returned failure carries the first primary error, a separate cleanup error and one of released, held-quiescent or unproved-DMA. Task60 publishes the command-completion latch before signal/callback for loaded, delegated and executed-skip outcomes; a cached skip remains `Skipped` and does not claim fresh executable bytes. The conditional FujiMod law is pinned to revision `3d90035d`; physical composition must instead supply an independently established quiescence leaf. `0x100000` is a controller budget, not a lease extent.

`runtime-artifact contract FFLZD CANONICAL_DAT STAGING_DAT CANONICAL_IMAGE4 FIXED_BODY SYMBOLS PUBLICATIONS_JSON CANDIDATE SCOPE TICKS OUTPUT` checks a staged Root bank: the image4 publications are exactly the three loader and five decoder veneers, the decoded candidate image4 equals canonical plus reservation and those rows, the fixed record carries the linked bank body and BSS, and the declared ticks are the linked policy's.

The affected gate and changed-path runs are:

```sh
H=/nix/store/bffmr8vszdqslmfar3yshn6swhq08vgc-host-build/bin/host-build
devenv shell -- run-output exec --label lower-read-root-bank-gate -- bash -c "$H test --jobs=1 --local_test_jobs=1 \
  //replacement/application_nand:host-tests //replacement/executable_loading:host-tests \
  //replacement/develop:pipeline-tests //replacement/develop:model-catalogue-test \
  //replacement/shim_check:shim-check-tests //replacement/saclac:saclac-placement-tests"
devenv shell -- run-output exec --label lower-read-loading-runtime -- \
  bash replacement/executable_loading/loading-runtime-runs.sh
```

The run script (after building `saclac-image-loading-*`, the stagers, `runtime-{native,retention}`, `runtime-native-qualify`, `native-runner`, `emulator-slot` and `loading-runtime-observer`) executes `runtime-native-qualify` under one native slot, stages each Root-bank variant through `stage-saclac` and `stage-fixed-image` on the first-stage DAT, checks it with `runtime-artifact contract`, and runs `loading-runtime-observer` from reset under one full-system slot. The native harness covers released success and primary-error-plus-expired-drain retention with synthetic device leaves. The observer sees the natural stock wrapper calls, the owned task60 constructor/receiver, the producer's segment lookup, decoder feed/IRQ/finish, NAND traffic, callbacks, latches and final storage ownership. Its retention role changes only the linked drain policy; it injects no message and reloads no code.

## Asynchronous image5 decoder composition

`//replacement/image5_load:decoder-arm` exposes the complete stream/engine unit as an rlib. The opt-in `image5-decoder-image` links it through the existing bank root and fixed-image layout, with one panic closure, a contiguous body and 52 private BSS bytes. It does not select a global firmware image or change Root's canonical composition.

The source census is `image5-decoder-source CANONICAL_DAT IMAGE4 IMAGE5 PLAIN LOADER_A LOADER_B OUTPUT_JSON`. It binds the committed stock function bytes, the conservative cross-image caller/reference scan, source98's route-table words and the adjacent settings stub. Computed runtime caller closure remains a separate obligation.

The component artifact tool has these commands:

```text
image5-decoder-artifact publications IMAGE4 PRODUCER_BIN PRODUCER_SYMBOLS BODY_BIN SYMBOLS OUTPUT_JSON
image5-decoder-artifact contract DECODER CANONICAL_DAT IMAGE4 PRODUCER_BIN PRODUCER_SYMBOLS BODY_BIN SYMBOLS CANDIDATE_DAT OUTPUT_JSON
image5-decoder-artifact omit-clock-release BODY_BIN SYMBOLS OUTPUT_BODY OUTPUT_RECEIPT
```

Use the existing `build-first-stage` fixed-image variant and `stage-fixed-image` workflow. Supply the publication rows once against canonical image4. The rows include the admitted image5 producer, five external-entry veneers and retirement traps over `[0x6841f4,0x6848d8)` and `[0x6ae18c,0x6ae638)`. Do not replace `[0x6ae638,0x6ae650)`, the unrelated settings return-zero stub. The contract command decodes the staged DAT and checks the exact image4 mutations, carried body record and zero-once BSS bounds.

The existing `loader-continuations` runner accepts `--decoder-witness retained|owned --image5-artifact PRODUCER_STAGE`. Owned additionally takes `--decoder-artifact DECODER_STAGE`, containing `decoder-contract.json`, `decoder.bin` and `decoder.symbols`. Both use the usual `--run`, `--update`, `--boot-rom`, `--analysis` and fresh `--output` arguments. Run under `run-output exec` and the current main `emulator-slot`, with `XM1_EMULATOR_SLOTS=1`; acquire a full-system slot only after staging/building.

This mode observes actual storage feeds, source98 attach/detach handles, LZ MMIO, CPU IRQ exceptions, resource48 waits/completion, flashloader publication, canonical image5/BSS readback and the first image5 entry. Exception hooks do not isolate a core or synthesize completion. Canonical Method4, one natural producer session, retained device/RTOS services and finite observed callers are its bounds. Cancellation, malformed input, missing device completion, repeated sessions and concurrent engine clients are not accepted by this route.

The retained IRQ dispatcher supplies word-aligned SP and can enter4 modulo8. Progress/final veneers preserve original SP/r4 and the return address, align before the Rust body and use at most12 bytes before that body. Service-return hooks are reused only for their identical purpose; each core/SP/LR invocation still records its own return.

The exact dispatcher call is `CMP r0,#0; MOVNE lr,pc; MOVNE pc,r0` at `0x402d4..0x402dc`, returning at `0x402e0`. The current native CFI does not admit that manual-link transfer into a callback. Do not replace it with a BLX fixture and call that the actual caller, or widen native acceptance to make this comparison pass.

`--decoder-irq-alignment progress-aligned-fixture|complete-aligned-fixture` selects a separate, explicitly synthetic full-system ABI control. From a fresh reset of the same staged image, it pads SP downward by4 before exactly one selected callback's actual `CMP/MOV` call sequence. It binds the original core/registers/frame word, checks the callback's LR, callee-saved registers and return SP, then restores the original SP before dispatcher continuation. The receipt records the pre-call padding and restoration. These controls do not qualify natural aligned-entry reachability; only the unmodified `natural` mode is firmware acceptance evidence.

The artifact tool's `omit-clock-release` command creates an explicitly synthetic engine-resource defect. It replaces exactly one direct call to retained clock release `0x649ddc` with an A32 NOP and records both body identities and the instruction preimage. Stage that body with the same fixed-image workflow. The observer records the real public clock reservation output and requires that handle's retained release call before engine resource release; private engine bookkeeping is not its oracle. A rejected verification receipt preserves the missing-release verdict.

Copy bounded observer JSON receipts and input/artifact identities into `XM1_RUN_RESULT` before successful managed-run closure. Bulk work is transient unless explicitly retained with `run-output keep`.


## Startup-consumer witness

[Publication and first-consumer findings](../reports/replacement-inventory-baseline.md#qualified-first-consumer-agreement) own scoped comparison results and unresolved lifetimes; [the installation contract](../reports/replacement-inventory-baseline.md#812-image-4-phase-0-installation-contract-150) owns source behavior. `//emulation:startup-consumers` produces receipts for retained, installer-only and joined dispatcher-plus-installer roles. A producer's completed observation is not consumer acceptance.

`//replacement/startup_consumers:startup-consumers-check` has this positional interface:

```text
startup-consumers-check ENCODER REBUILD DECODER MODE SOURCE_DAT IMAGE4 LOADER_A LOADER_B INSTALLER_BIN INSTALLER_SYMBOLS INSTALLER_CONTRACT DISPATCHER_BIN DISPATCHER_SYMBOLS DISPATCHER_CONTRACT INSTALLER_DAT JOINED_DAT JOINED_CONTRACT RUN_JSON BOOT_ROM OBSERVER SCRATCH_DIR OBSERVATIONS OUT_JSON
```

Bazel supplies the first three tool paths. `qualify` takes one observation directory at `OBSERVATIONS`; `compare` and `refusal-controls` take three directories in order `RETAINED_DIR INSTALLER_DIR JOINED_DIR`. Each contains `consumers.json` and its independently bound fresh `reset.fujimod`. Contracts must be the staging outputs for the supplied native images/symbols. `SCRATCH_DIR` is an existing named directory under `private-data/scratch/`. Run document, ROM and observer are bound by bytes, not just paths.

The producer's required flags are `--role retained|installer|joined --run RUN_JSON --update ROLE_DAT --canonical-dat SOURCE_DAT --boot-rom BOOT_ROM --analysis ANALYSIS_DIR --installer-image INSTALLER_BIN --installer-symbols INSTALLER_SYMBOLS --installer-contract INSTALLER_CONTRACT --dispatcher-image DISPATCHER_BIN --dispatcher-symbols DISPATCHER_SYMBOLS --dispatcher-contract DISPATCHER_CONTRACT --joined-contract JOINED_CONTRACT --output OBSERVATION_DIR`. Its executable runs under an emulator slot.

Admission obligations that matter when interpreting the result:

- The complete fetch prefix closes at the outer phase-0 return. The later scoped suffix covers owned carriers, family entries, atomic/message-prehistory accesses and armed source/dynamic sites, not whole-machine fetch continuity. CPU data still needs actual SDK own-core/latest-fetch attribution and gapless resource replay.
- Root extents derive from all published words, including the final extension. Complete preimages, ordered publications, readiness and actual consumption are required. Consumed startup mode comes from the canonical getter read, not an unadmitted later memory snapshot.
- The globally first invocation of each family must close in its own task/frame. `MissingFamily` and `UnclosedFamily` are refusals. `SelectedInvocationReturned`, `CensusEntryOnly` and `NoEntryObserved` distinguish completed behavior from coverage alone.
- Source-defined pointers alone are normalized. Outer caller-live state, public bytes, complete descriptors and typed operation-boundary effects are compared. Compiler scratch, later background census totals and final scheduler/task-stack state are not equality claims.
- Send preserves the first four acquired-buffer bytes through pre-submit, not after transfer. Receive ownership closes after release returns. Forward-copy validation respects actual byte/halfword/word accesses and overlap; send and receive are not assumed to describe one packet. Empty payloads require no pointer dereference.
- Continuous before/committed-byte replay, both-core aliases, non-CPU writer refusal and reservation-lock LDREX/STREX evidence remain individual receipt obligations. Watch selection uses whole-access overlap, without extending the admitted resource footprint. Installer cache continuation and final private LR are distinct values.

Schema1 ceilings are 80,000,000 absolute instructions; 256 instructions per complete segment and 4,096 per scoped segment; 65,536 rows per watch and 1,048,576 aggregate rows; 131,072 probes; 512 resources of at most 4 MiB each; 128 MiB retained snapshot bytes; 65,536 four-view snapshot sets; 32,768 segments; and 64 invocations. Carrier snapshot chunks are at most 1 MiB. Segment budgets are pause cadence, not protection against event-cap exhaustion. The [consumer implementation](startup_consumers/Replacement/StartupConsumers/) owns typed refusal rules. Finite first-consumer agreement does not establish post-return scheduler equality, whole resource lifetimes or production retirement.

## Joined loader/dispatcher/installer checker

`//replacement/startup_composition:stage-loader-startup-composition` stages both loaders before rebuilding image4/container once. The [joined composition contract](../reports/replacement-inventory-baseline.md#three-component-startup-composition-source-155) and [loader continuation guide](../emulation/README.md#loader-and-returned-code-continuation-observation-149-153) describe the source and runtime scope.

```text
stage-loader-startup-composition ENCODER REBUILD DECODER SOURCE_DAT IMAGE4 LOADER_A LOADER_B NATIVE_A SYMBOLS_A NATIVE_B SYMBOLS_B DISPATCH_BIN DISPATCH_SYMBOLS INSTALL_BIN INSTALL_SYMBOLS SCRATCH_DIR
```

Bazel supplies the first three tools. `SCRATCH_DIR` must exist under `private-data/scratch/`, with none of these fixed outputs present: `canonical.dat`, `canonical-image4.bin`, `canonical-loader-a.bin`, `canonical-loader-b.bin`, `candidate.dat`, `candidate-image4.bin`, `candidate-loader-a.bin`, `candidate-loader-b.bin`, `loader-a.bin`, `loader-a.symbols`, `loader-b.bin`, `loader-b.symbols`, `dispatcher.bin`, `dispatcher.symbols`, `installer.bin`, `installer.symbols`, `contract.json`. Output creation is exclusive.

`//emulation:loader-startup-capture` produces `startup-joined.json` with `--shared-role retained|joined`. Both roles use a frozen observer and their own zero-instruction reset. The consumer subwindow closes separately from the continuing loader guards; a capture split is not a lifetime endpoint.

```text
startup-joined-check ENCODER REBUILD DECODER qualify|compare|refusal-controls STAGED_DIR RUN_JSON BOOT_ROM OBSERVER HOST_CHECKER ANALYSIS_ROOT SCRATCH_DIR NEW_OUTPUT_DIR [CAPTURE_DIR|RETAINED_DIR JOINED_DIR]
```

The target is `//replacement/startup_consumers:startup-joined-check`; Bazel supplies the first three paths. `qualify` takes one capture; other modes take retained then joined captures. Each invocation needs fresh staging scratch and output directories. `qualification.json` is written on acceptance and refusal. `OBSERVER` is the frozen producing executable; `HOST_CHECKER` runs current route/dispatcher admission on copied, digest-checked inputs without rerunning firmware.

Shared captures require the route/dispatcher-derived observation policy, not standalone consumer-site replay. Haskell admits the Rust host check's byte-bound acquire/release policy and replays the union with consumer-owned sites. A mutated capture cannot acquire qualification by borrowing the original capture's policy. Joined comparison preserves the source-invariant caller boundary and finite operation projections, not active provider-private registers or scheduler equality.
