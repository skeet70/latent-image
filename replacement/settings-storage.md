# Settings and record-storage workflows

[Replacement index](README.md). [Backlog #127](https://github.com/skeet70/latent-image/issues/127) owns settings open work. The [settings manifest](../reports/settings-service-manifest-draft.md), [record-service stock contract](../reports/record-service-stock-contract.md) and [owned record-store design](../reports/owned-record-store-design.md) own recovered semantics, design and qualification scope. Commands below are host-only; persistence on a camera requires the separate [camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs).

## Property service

[`property_service/`](property_service/) provides the classifier, table generator, Rust service, artifact admission, stager and native oracle. The [build declarations](property_service/BUILD.bazel) name its artifacts and controls. Generated encoding is defined by the [classification/generation code](property_service/Replacement/PropertyService/), not a second hand-maintained property table here.

### Qualification driver

```sh
XM1_EMULATOR_SLOTS=1 devenv shell -- host-build run --jobs=2 \
  //replacement/property_service:qualify -- \
  --dat SOURCE_DAT --image4 IMAGE4 --loader-a LOADER_A --loader-b LOADER_B \
  --image5 IMAGE5 --plain PLAIN --image2 IMAGE2 \
  --out private-data/scratch/PropertyQualification/job \
  --cache private-data/scratch/PropertyQualification/cache
```

Bazel supplies the current linked image, symbols, emulator-slot wrapper, oracle, stager, staging tools and defect images. `--ids HEX,...` narrows property comparisons, but all controls still run. Without it the driver runs the full native domain. Both output and cache must resolve under `private-data/scratch/`.

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

Bazel supplies the staging tools and oracle dependencies. Staging output paths must not exist. The oracle restages from canonical sources and the linked artifact and refuses a candidate-image mismatch. `--smoke` runs one canonical batch; `--ids HEX,...` selects ids plus every control's cases; `--cache CACHE_ROOT` enables the same raw-execution reuse contract. Batch summaries and `results.json` carry identities, boundary verdicts, control outcomes and bounded differences.

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

Without `--runner` the command generates/validates cases only. Without `--candidate` it compares the stock role with the independent model. `--filter FAMILY` selects identifiers containing the text. `--stock-output DIR` reuses stock observations only when regenerated execution inputs (excluding `expected`) and runner digest match. `--control` requires the supplied candidate to differ on at least one case while the model still agrees with stock; use it for each control artifact. Keep native batches serial with `--native-jobs 1` and the documented 2 GiB/no-swap cap.

`summary.json` is the verdict. Compare caller-visible statuses, preserved registers/SP, client words and caller buffers, returned pointers/records, skipped counts, callback order, programs/erases with payloads and final medium. Void-close r0, raw reads, caches, private stack/wrapper state and unobservable staging bytes are excluded. Generic transport `mismatches` can include those excluded internals and are not the record-store verdict. The stock contract leaves malformed kinds, zero-size division and invalid pointers outside the safe domain; the medium model does not establish physical NAND laws.

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

Bazel supplies the current carrier, symbols and pinned runner. `--image`, `--loader` and `--index` override stock paths, with identities still pinned; `--runner` overrides execution. `--cache ROOT` reuses integrity-checked observations and rejudges them. Output must be fresh; output/cache resolve under `private-data/scratch/` or `private-data/verify-candidate/runs/`, cannot overlap and refuse cache-owned symlinks beneath the canonical root.

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
