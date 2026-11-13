# Settings and record-storage workflows

[Replacement index](README.md). [Backlog #127](https://github.com/skeet70/latent-image/issues/127) owns settings open work. The [settings manifest](../reports/settings-service-manifest-draft.md), [record-service stock contract](../reports/record-service-stock-contract.md) and [owned record-store design](../reports/owned-record-store-design.md) own recovered semantics, design and qualification scope. Commands below are host-only; persistence on a camera requires the separate [camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs).

## Property service

[`property_service/`](property_service/) provides the classifier, table generator, Rust service, artifact admission, stager and native oracle. The [build declarations](property_service/BUILD.bazel) name its artifacts and controls. Generated encoding is defined by the [classification/generation code](property_service/Replacement/PropertyService/), not a second hand-maintained property table here.

Custom-settings images may link memory intrinsics from the pinned compiler-builtins archive. Their linker still rejects unresolved symbols, writable statics and a text carrier larger than 64 KiB.

### Optional recipe producer source

The current-base source port adds `RecipeArtifact`, `RecipeEditNative`, `RecipeEditOracle` and `property_service/recipe.bzl`. The canonical property image remains the default build entry. Recipe builds retain the current Settings core, public getter/setter entries, ROM defaults, storage boundaries, header model and defect controls. Only recipe host ids `0x267..0x26f` bypass ordinary field dispatch: getters pass host-minus-one to the package getter and return its stored code unchanged; setters return zero without changing storage.

`property_service_recipe_image` consumes the linker include emitted by the opaque `RecipeEditPackage`. `deriveRecipeArtifact` takes that same package, current classified properties, the admitted carrier and `ArtifactInput` containing the actual binary, symbols and ELF. Admission binds the callback's actual entry/extent and package identity, retains ELF/mapping and ROM-default checks, and requires a decoded call to the bound callback. No historical callback address or fixed-word patch is used.

`derivePropertyVariantPublication` takes an admitted canonical artifact first and the recipe artifact second. It produces an opaque, bounded carrier-word delta with canonical/recipe words and linked/carrier digests; swapping those words restores the canonical carrier. The staged contract retains source, binary, symbols and ELF identities and records the recipe package identity. The native fixture maps actual package ELF sections and initially inactive runtime bytes; it refuses active sessions without actual open/load service context.

This is a source-only port, not a new qualification. No build, test, guest execution or artifact generation was performed for this port. Callback AAPCS behavior and stack depth, active-session initialization, parked consumers, publication, cache coherence, rollback, installed code lifetime and total admission remain unproved. Frozen private observations and artifacts are unchanged.

### Public-entry variant

`//replacement/property_service:property-service-public-image` places the same owned body behind `xm1_public_get` and `xm1_public_set` at `0x62c120` and `0x62c12c`. The getter saves and restores `r4`, `r5` and `r11`; the setter saves and restores `r3` through `r8` and `r11`, so the setter's original `r3` survives. Both then call the owned getter or setter. The canonical `property-service-image` keeps the direct branches and is unchanged. `derivePublicArtifact` admits the wrapper encoding; `nativeImage4View`, `validateNativeImage4View` and `stageNativeImage4` place and check it without a DAT or retirement claim. `stage-property-service-public`, `property-oracle-public` and `qualify-public` are the variant's stager, oracle and qualification driver; `property-service-public-artifact-tests` is its host test. The compact measurement, rejudgment and typed storage-admission targets are shared with the canonical qualification.

The oracle projects a case's compared registers by the entry's shape: the canonical entry is one branch word, so `r3` is caller-saved and is not compared, while the public wrapper's `push {r3,...}`/`pop {r3,...}` makes the setter's original `r3` observable, so the public comparison keeps it. The stock evidence is a call-site census: image4 has 87 direct `bl 0x624694` sites and 4 direct `bl 0x6309c4` sites (`0x61c924`, `0x630b0c`, `0x6319c8`, `0x6319e4`), and `0x6309c4` returns the setter's `r3` to its caller; image4 has 149 and image5 524 sites calling thunk `0x61c91c`, and 2 each calling thunk `0x61c92c` (image4 `0x63767c`, `0x637690`; image5 `0xd4e320`, `0xd4e34c`). A mechanical scan of all 768 sites finds none that reads `r3` before writing it or before the next call or return, and `0x637690` returns immediately after its call. No `b`, `blx` or address-taken reference to `0x624694` exists in image4, image5, the plain image or the loaders. The prologue at `0x624694` pushes `{r3, r4, r5, r6, r7, r8, r11, lr}`: six callee-saved registers plus `lr` are 28 bytes, so `r3` pads the frame to alignment, and `0x624694`-`0x6309c0` holds that one push and 114 pops of the same list, 95 `pop` and 19 `popne`, none writing the saved word. The two wrapper-veneer controls are built for the entry shape: the canonical image places the same defects in the carrier fill and enters them through the entry branch, so both shapes detect all 14 wrong variants.

The admission states one assumption it cannot prove from owned bytes. A stock leaf, stock site, callback or slot-word callee preserves `r4`-`r10` under AAPCS, on the same ABI that already backs the `sp`, `r11` and `lr` premises. A slot-word callee is the stock function `xm1_kernel_enter_slot` enters through its `extern "C" fn(u32, u32, u32) -> u32` (`replacement/kernel/src/slot_table.rs:180-182`); its tail `bx r3` is preserving on that assumption, and a linked slot-word call drops `r4`-`r10` in every walk. An owned callee keeps `r4`-`r10` only when the fixed point proves it: every function starts in the set, each walk treats a call or tail call to a callee outside the set as clobbering them, and each round reconsiders only current members, so the set only shrinks until the walks agree with it. Counters and caller-alias offsets are bounded by the stack limit (`4096`), so a guard refines only on a value the 32-bit register holds and a walk terminates on every input. The probe walk declares every argument register a frame pointer and skips the negative-offset refusal; that needs no trust, because the probe only decides which slots are declared, walking further can only add declarations, and every strict walk refuses a forwarded caller alias below its object at any call or tail call and a store below a frame argument. The probe runs before the frame-argument slots are known, so it cannot tell a frame pointer from an ordinary value, and the image computes `r2 = r1 - 5` in `stock_default` (`0x62c968`) where `r1` is not a frame pointer; its tail branches at `0x62c99c`-`0x62c9e0` pass `r2` on.

The local view changes only the fixed carrier and the two public root branches. Retirement is not admitted: the production stager refuses while any retained reference is unresolved. The current frontier is 188 findings, including all 93 GVFF sites and the six FDL sites; historical entropy windows and retained-target islands do not discharge them. A refusal is not a candidate or retirement claim.


### Qualification driver

```sh
XM1_EMULATOR_SLOTS=1 devenv shell -- host-build run --jobs=2 \
  //replacement/property_service:qualify -- \
  --dat SOURCE_DAT --image4 IMAGE4 --loader-a LOADER_A --loader-b LOADER_B \
  --image5 IMAGE5 --plain PLAIN --image2 IMAGE2 \
  --out private-data/scratch/PropertyQualification/job
```

Bazel supplies the current linked image, symbols, emulator-slot wrapper, oracle, stager, staging tools and defect images. `--ids HEX,...` narrows property comparisons, but all controls still run. Without it the driver runs the full native domain. Output must resolve under `private-data/scratch/`; the cache root defaults to the shared bounded `private-data/cache/native` and may resolve under `private-data/scratch/` or `private-data/cache/`.

Each invocation regenerates cases, admits current artifacts and rejudges model, candidate and controls. It never accepts an old passed verdict. The default runner is the pinned SDK's canonical Nix-store executable; `--runner PATH` overrides it. Raw batches are reusable only when complete and integrity-checked, keyed by ordered native wires and canonical runner path/digest. Mutable non-Nix runners execute without reuse. Changed expectations are rejudged without forcing unchanged execution.

The stage is keyed by source, candidate and staging-tool bytes, verified and atomically published under `OUT/stages/`. Incomplete/corrupt stages are restaged. A POSIX record lock serializes a job; a busy job exits nonzero without changing `OUT/current`. After interruption, repeat the same command to reuse completed stages/batches. Each attempt has immutable logs, verdict and review packet. `OUT/current` changes atomically only after publication; `OUT/attempts.jsonl` journals attempts and `OUT/results/` holds pre-attempt setup failures. A publication failure leaves the previous pointer unchanged.

The command prints one JSON verdict line and exits nonzero on stage, identity, model, candidate, control or publication failure. The verdict identifies inputs, executed/reused work, subset/full-native scope and the first failed predicate; complete diagnostics remain in files. Native success does not establish system, hardware, timing or persistent-settings acceptance.

To qualify an alternate candidate, pass `--linked BIN --symbols SYMBOLS`. For a known defect build, repeat `--defect NAME GOOD_BIN GOOD_SYMBOLS` with the cfg name it used; the later occurrence of a defect name wins. A directly invoked driver must also supply `--emulator-slot`, `--oracle`, `--stage`, `--stage-encoder`, `--stage-rebuild`, `--stage-decoder` and the defect inputs that Bazel normally supplies.

### Direct staging and oracle

These are the lower-level interfaces for diagnosis, not an alternative qualification result:

```sh
devenv shell -- host-build run //replacement/property_service:stage-property-service -- \
  SOURCE_DAT IMAGE4 LOADER_A LOADER_B IMAGE5 PLAIN IMAGE2 LINKED_BIN SYMBOLS \
  private-data/scratch/PropertyStage/RUN OUT_DAT OUT_CONTRACT OUT_IMAGE4
XM1_EMULATOR_SLOTS=1 devenv shell -- host-build run \
  //replacement/property_service:property-oracle -- \
  --image4 IMAGE4 --candidate-image4 STAGE/staged-image4.bin --contract STAGE/contract.json \
  --dat SOURCE_DAT --loader-a LOADER_A --loader-b LOADER_B \
  --image5 IMAGE5 --plain PLAIN --image2 IMAGE2 \
  --out private-data/scratch/PropertyOracle/NEW_RUN
```

Bazel supplies the staging tools and oracle dependencies. Staging output paths must not exist. The oracle restages from canonical sources and the linked artifact and refuses a candidate-image mismatch. `--smoke` runs one canonical batch; `--ids HEX,...` selects ids plus every control's cases; the cache root defaults to the shared bounded `private-data/cache/native`, and `--cache CACHE_ROOT` overrides it. Batch summaries and `results.json` carry identities, boundary verdicts, control outcomes and bounded differences. `--native-only --public-entries --smoke` runs the bounded public-entry cohort without a staged DAT, and `--emit-case-table PATH` writes the static audit's case table from the suite's own grid. `results.json` also carries `suite`, the firmware and owned image digests, a `cases` row per case with the entry it entered and the stock instructions the runner reports inside that entry's extent, and `controls` whose `holds` is the detection result, so `//emulation:judge` reads the same document. `--variant NAME ADDRESS MUTATED` repeats the case plan against the owned image with that one word changed, which is how the suite runs `//emulation:mutants` variants, and adds a control row per variant. `--storage-root` bounds where the output and cache may live; the batch checkpoint measures the allocated bytes of those two directories only.

### Property compatibility shims

This table owns the stage-3 shim/removal contract. The ledger owns the current candidate and qualification claims; [the settings manifest](../reports/settings-service-manifest-draft.md#property-get-and-set-services) owns stock behavior.

| Shim | Retained boundary | Removed when |
| --- | --- | --- |
| Repointed table words | Stock entry/dispatch tables select the owned getter/setter stubs | Owned entry and dispatch replace both stock dispatchers |
| Stock-frame stubs | `xm1_getter_stub` and `xm1_setter_stub` run in the stock prologues' frames and pop them | Owned entry and dispatch replace the stock frames |
| Shared exits and fallbacks | Only out-of-range ids reach the stock fallbacks and shared exits | Owned dispatch handles those ids |
| Base-fixed accessors | `xm1_{bank,save}_{load,store}{8,16}` use the stock bank/previous-value layout; slot/tail defaults and configuration loads use retained storage | Owned settings storage carries the data/defaults; configuration loads become owned state or a typed service |
| Named default leaves | Six stock default leaves called from Rust | Their configuration state and behavior are owned |
| Bank-0 bracket callees | `0x61da3c` and `0x61d944` refresh/write back the bank-0 header around the setter | Owned storage replaces that backup-memory contract |
| Carrier in retired setter arms | Owned code uses admitted setter space while the remaining stock bodies stay in place | Entry/dispatch cutover retires both bodies and places the owned service |

The [artifact policy and placement admission](property_service/Replacement/PropertyService/) own exact encodings, carrier/reference scans and stack premises. The [Rust service](property_service/src/) owns field/default behavior. Do not reintroduce delegated arms or treat native arm qualification as retirement of the retained entry bodies.

## Owned settings storage

[`settings_storage/`](settings_storage/) owns bank projections, previous values, ROM defaults, typed configuration, the shared header mirror and custom-record lifecycle. The custom-record admission and native comparison boundary are defined by [`bank-record-native-contract.md`](settings_storage/bank-record-native-contract.md). Property entry ownership, this storage implementation and full-system adoption are separate gates.

`//replacement/settings_storage:custom-oracle` qualifies bank/property composition, joined record operations, custom save/recall, preemption and EE helpers. Its Bazel defaults supply the linked artifacts and controls. `--runner`, `--stock-runner` and `--joined-stock-runner` select explicit immutable execution identities; they do not relabel existing observations. `--joined-stock-runner-cache-sha256` makes the joined stock role cache-only. Other roles can execute on a cache miss. Use the existing cache-only API when a rejudgment must forbid every new execution.

`--native-jobs N` bounds independent host pipelines inside one admitted native scope. Shared input pooling and the raw observation cache preserve exact wire/runner identities; current model and caller-frame checks run again on reuse. Successful staged files are compacted only after a verified cache binding exists. Failed/incomplete cases and their consumer judgments remain retained. Cache inspection and cache-only reads reject symlink escapes, missing/corrupt entries, ambiguous compressed payloads and incomplete provenance without running the SDK.

The frozen native cohort in `E-SETTINGS-STORAGE-COMPOSED-NATIVE-150` qualifies its own source, runner and artifacts. Importing its source into the current SDK does not rebind those observations. Composed cold boot, repeated set/get, custom save/recall, cancellation/restore, handback and physical persistence remain separate acceptance.

## Record-store build and staging

The core in [`record_store/`](record_store/) is allocation-free and address-independent; the guest adapter retains raw NAND and synchronization services. Its comparison boundary is [design §5](../reports/owned-record-store-design.md#5-native-oracle-host-qualification), not every internal native-runner diagnostic.

```sh
devenv shell -- host-build build //replacement/record_store:record-store-rust \
  //replacement/record_store:host-tests //replacement/record_store:host-adapter-tests \
  //replacement/record_store_carrier:record-store-image
devenv shell -- host-build test //replacement/record_store_carrier:record-store-artifact-tests
devenv shell -- host-build run //replacement/record_store_carrier:stage-record-store -- \
  DAT IMAGE4 IMAGE5 PLAIN FUNCTION_INDEX LINKED_BIN SYMBOLS PRIVATE_SCRATCH OUT_DAT OUT_CONTRACT
```

The host suites are build targets. The carrier emits `.elf`, `.bin`, `.symbols` and `.disasm`; `:record-store-image-control-N` selects one of the nine faults. The linker rejects reachable compiler-builtins. Staging binds the canonical images and pinned decomp index, rejects resolved references to retired code/data, confines replacement bytes and reads back the rebuilt DAT. Unresolved computed references and raw numeric coincidences remain separate runtime obligations. [Placement and owned RAM](../reports/owned-record-store-design.md#4-placement) have one home in the design.

## Record-store native oracle

Build `//replacement/record_store:oracle-cases`, `//emulation:native-runner` and `//emulation:emulator-slot` through host-build; test `//replacement/record_store:oracle-tests`. Then run the executable inside devenv with one emulator slot:

```sh
XM1_EMULATOR_SLOTS=1 devenv shell -- bazel-bin/emulation/emulator-slot \
  bazel-bin/replacement/record_store/oracle-cases --native-jobs 1 \
  --runner bazel-bin/emulation/native-runner \
  --candidate CARRIER.bin --symbols CARRIER.symbols \
  --out private-data/scratch/RecordOracle/NEW_RUN
```

Without `--runner` the command generates/validates cases only. Without `--candidate` it compares the stock role with the independent model. `--filter FAMILY` selects identifiers containing the text. `--stock-output DIR` reuses stock observations only when regenerated execution inputs (excluding `expected`) and runner digest match. `--control` requires the supplied candidate to differ on at least one case while the model still agrees with stock; use it for each control artifact. Keep this oracle serial with `--native-jobs 1`; the current [native admission policy](../build-support/README.md#commands) supplies RAM and swap limits.

`--emit-case-table PATH` writes the static audit's entered side from the suite's own entry table: one row per entry a case ends in. `summary.json` is the verdict; it also carries `suite`, `image4_sha256`, `owned_sha256`, a `cases` row per case with the last entry it entered and the stock instructions the runner reports inside that entry's extent, and `controls` for each `--control-image NAME BIN SYMBOLS` candidate with the cases that detected it, so `//emulation:judge` reads the same document. `--variant NAME ADDRESS MUTATED` patches that one word into the candidate at a carrier address and runs the case plan against it, which is how the suite runs `//emulation:mutants` variants. `--stop-on-first-detection` ends a control's scan at the first batch that differs and deletes that control's intermediate directories; `scanned_cases` in its row says how much of the plan it reached, and a control that never differs still scans every case. Compare caller-visible statuses, preserved registers/SP, client words and caller buffers, returned pointers/records, skipped counts, callback order, programs/erases with payloads and final medium. Void-close r0, raw reads, caches, private stack/wrapper state and unobservable staging bytes are excluded. Generic transport `mismatches` can include those excluded internals and are not the record-store verdict. The stock contract leaves malformed kinds, zero-size division and invalid pointers outside the safe domain; the medium model does not establish physical NAND laws.

## Generated record counterexamples

`//replacement/record_store:counterexample-loop` generates a bounded corpus, checks stock against the independent model, compares the candidate, and minimizes the first difference while preserving its typed boundary.

```sh
XM1_EMULATOR_SLOTS=1 devenv shell -- host-build run --jobs=2 \
  //replacement/record_store:counterexample-loop -- \
  --seed 0 --cases 8 --operations 6 --budget 32 \
  --out private-data/scratch/RecordCounterexample/RUN
XM1_EMULATOR_SLOTS=1 devenv shell -- host-build run --jobs=2 \
  //replacement/record_store:counterexample-loop -- \
  --candidate CANDIDATE.bin --symbols CANDIDATE.symbols \
  --replay SAVED.case.json --out private-data/scratch/RecordCounterexample/REPLAY
```

Bazel supplies the current carrier, symbols and pinned runner. `--image`, `--loader` and `--index` override stock paths, with identities still pinned; `--runner` overrides execution. The cache root defaults to the shared bounded `private-data/cache/native` and `--cache ROOT` overrides it; observations are integrity-checked and rejudged. Output must be fresh; output resolves under `private-data/scratch/` or `private-data/verify-candidate/runs/`, the cache resolves under `private-data/scratch/`, `private-data/verify-candidate/runs/` or `private-data/cache/`, output and cache cannot overlap, and cache-owned symlinks beneath the canonical root are refused.

The seed is unsigned 64-bit. Case count is 1..64, operation count 3..12 and shrink budget 1..256 actual oracle calls including baseline. Cases select one of the five stock geometries, two valid banks, an open/registration prefix and lifecycle-valid operations; callbacks are synthetic. Replay validates the same domain.

| Exit | Meaning |
| --- | --- |
| 0 | No difference in the generated domain, or a matching replay |
| 1 | Behavioral counterexample |
| 2 | Invalid input or an undecided prerequisite/execution outcome |

Counterexamples retain original/reduced cases, decisive observations, native replay inputs/results and identities. Inconclusive trials retain the first complete example per class. `fixed-point` means local minimality under implemented reductions; `budget-exhausted` and `unresolved-trials` establish no minimum. Unsupported execution and stock/model disagreement remain undecided, not non-reproduction.

## Record-store system pair

The [finite pair contract](../reports/owned-record-store-design.md#6-cutover-pair) owns the exact observation endpoint, exclusions and acceptance. Use `//emulation:record-store-pair-observe` and `//emulation:record-store-pair-check`; their [observer](../emulation/RecordStorePairObserverMain.hs) and [checker CLI](../emulation/RecordStorePairCheckMain.hs) define arguments. The checker requires `COMPARISON_JSON` before `OUTPUT_JSON`.

Run erased and populated stock/owned sessions serially inside devenv, under `XM1_EMULATOR_SLOTS=1` and `//emulation:emulator-slot`, using one frozen cohort and `--bridge IMMUTABLE_RAM_PUSH_EXECUTABLE`. Devenv supplies `XM1_OPENSSL` for sidecar hashing. The optional `--record-nand` is the bound populated area-2 input. Output preimages determine untouched-output preservation independently in each role; arbitrary residue is not cross-role equality.

The [fixture-bound intended IMAGE SIZE behavior](../reports/owned-record-store-design.md#fixture-bound-intended-image-size-oracle) applies only to its declared populated fixture and exact paths. It permits `accepted-functional-difference`, not a general menu/settings exclusion. The [current full-system boundary](../reports/owned-record-store-design.md#current-full-system-boundary) owns results and remaining reachability limits. A finite post-settle snapshot does not establish render quiescence, future stability or independent application acknowledgement of every key.

`//emulation:record-store-timing-control` is diagnostic: `--before-picker-down` places its idle perturbation before the asynchronous apply trigger; without it the perturbation precedes the final UP. A negative instruction shift must be smaller than ordinary pre-idle. Timing controls neither replace the pair verdict nor relax its comparator.
