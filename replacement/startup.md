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
