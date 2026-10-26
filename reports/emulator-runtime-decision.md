# X-M1 emulator runtime decision

## Adopted development direction, gated acceptance cutover

The maintainer's direction in [#97](https://github.com/skeet70/latent-image/issues/97) is to use jitl (justjake)'s [FujiMod Rust runtime](https://github.com/justjake/FujiMod) directly for development now. New emulator work targets FujiMod, with Haskell host orchestration. The retained Python engine is frozen legacy for surfaces not yet ported, not the default for new emulator logic.

FujiMod stays pinned by commit and recursive source/Cargo hashes in `build-support/fujimod.nix`; `build-support/fujimod-xm1.nix` builds our adapter against that source, with no sibling checkout. Update `rev`, `hash` and `cargoHash` together and review the upstream dependency and local patch changes. The [implementation principles](../docs/principles.md#implementation-and-build) own the current direction and legacy surface inventory.

Development adoption is not an immediate acceptance-gate switch. Retained routes continue to gate until their FujiMod counterparts pass the complete `verify-candidate` matrix with zero non-timing differences, followed by one reviewed cutover commit. The measurements below predate that qualification and do not establish it. Original project code remains AGPL-3.0-only; [LICENSING item 7](../LICENSING.md#release-decisions) still prohibits conveying FujiMod-linked binaries until an upstream licence file lands and its coverage is reviewed. Local development and permission to distribute remain separate decisions.

## Fresh current-main route comparison

**Emulated, 2026-10-25:** the comparison was executed from main `9b169a3cc668a09b3a955c120e16ffc8b602d566`, not imported from an earlier report. FujiMod is pinned at `ecdb6cb653ca0fa3372e4a4cf92c88bffa13ae29`. The six counterpart checks produce **five matches and one known mismatch**. No baseline was re-recorded, comparator changed or runtime implementation edited for these measurements.

The first `verify-candidate` invocation started at 15:46:59 UTC and took **2,099.4 s**. It ran 14 routes: both stock boot checks, all six FujiMod counterpart routes and all six retained counterparts. The verifier reports overall `pass` because the icon mismatch is non-gating. That aggregate verdict is not a cutover approval.

| Retained route | FujiMod route | Executions | Fresh observation comparison | Current disposition |
| --- | --- | --- | --- | --- |
| `flash-boot-check` | `fujimod-flash-boot-check` | Both exit 0 | Both match their own stored baseline with 0 differences; not a cross-engine byte comparison | Retained gates; FujiMod does not |
| `ram-push-volatile` | `fujimod-ram-push-volatile` | Both exit 0 | 7 files, **0 differences** | Retained gates; FujiMod comparison passes |
| `ram-push-cc-cardless` | `fujimod-ram-push-cc-cardless` | Both exit 0 | 7 files, **0 differences** | Retained reference; FujiMod comparison passes |
| `ram-push-cc-icon-cardless` | `fujimod-ram-push-cc-icon-cardless` | Both exit 0 | 11 files, **1 difference**, `e2e-observation-9.json.classic_chrome_site_entries` | FujiMod comparison fails, non-gating; #110 |
| `ram-push-cc` | `fujimod-ram-push-cc` | Both exit 0 | 7 files, **0 differences** | Retained gates; FujiMod comparison passes |
| `usb-toggle-e8` | `fujimod-usb-toggle-e8` | Both exit 0 | 7 files, **0 differences** | Retained gates; FujiMod comparison passes |
| `usb-toggle-emu` | `fujimod-usb-toggle-emu` | Both exit 1 | 35 scenario receipts, **0 differences**; each has 29 passes and 6 blocks | Retained reference; FujiMod parity passes, not full scenario success |

The icon difference is the first CLASSIC CHROME commit's converter entries falling into different develop passes in `cc-icon-film-stock`. The comparison's text output records one difference. The verifier's JSON `differences` field remains 0 for this external-comparator failure; its `result: fail` and `problem` are authoritative. Reading only the numeric field would misreport parity.

The six toggle blocks are `e4e5` and `e5u-cycle` for each of CC+toggle, RC0+toggle and CC-icon+toggle. Each fails the same detach-edge handler observation on both engines. Matching those blocks preserves the existing boundary; it does not establish working detach/re-enumeration.

### Remaining recorded routes

The second verifier invocation started at 15:48:41 UTC. Six routes completed with the exit codes and elapsed times below. The enclosing command reached its 3,600 s limit while `native-sd` was still running, before the verifier wrote its aggregate report. A subsequent `verify-candidate --from-run` judged the six completed receipts without executing them again; observed exits/times come from the original invocation, and baseline verdicts from that receipt-only pass. This is not a complete passing matrix.

| Retained gating route | Observed execution | Fresh baseline verdict | FujiMod counterpart in current verifier |
| --- | --- | --- | --- |
| `ram-push-cc-icon` | Exit 0, 727.2 s | Pass, 0 differences | None for this card-backed icon route |
| `flash-candidate-icon-b` | Exit 0, 1,292.8 s | Pass, 0 differences | None |
| `flash-candidate-q-tile-b` | Exit 0, 875.7 s | Pass, 0 differences | None |
| `updater-f7-rc0-toggle` | Exit 0, 1,272.0 s | Pass, 0 differences | Standalone FujiMod implementation exists, not wired into this verifier |
| `updater-f7-cc-toggle` | Exit 0, 999.6 s | Pass, 0 differences | Standalone FujiMod implementation exists, not wired into this verifier |
| `native-volatile` | Exit 0, 10.0 s | Fail, 3 differences in compiled-candidate identity | Separate bounded native contract, not a full-system replacement target |
| `native-sd` | Interrupted by the enclosing command deadline, no completed verdict | Not established | Separate bounded native contract |

The three volatile differences are the compiled-candidate directory plus image-4 and image-5 digests: the stored baseline names `package/image`, whereas the current script defaults to `package-20261024b/image`. The checked bytes differ as well as the path. No exclusion or baseline was changed to hide them; matching candidate provenance is a qualification prerequisite. The `native-sd` timeout is a measurement limit, not evidence that every case failed or a FujiMod regression.

### Speed measurements and their limits

These are measured on the shared Linux x86_64 workstation, using the checked-in devenv, one Bazel output base, `--jobs=4 --local_resources=memory=6144`, `XM1_EMULATOR_SLOTS=8` and four native jobs. The second invocation of the remaining routes overlapped the first. Other work also shared the host. Route wall time includes Bazel launch/build waiting, emulator-slot waiting, staging and receipt work; it is not pure execution time.

| Pair | Retained route wall | FujiMod route wall | Retained recorded E2E phases | FujiMod recorded E2E phases |
| --- | ---: | ---: | ---: | ---: |
| Stock flash boot | 63.0 s | 287.1 s | n/a | 25.6 s driver time, not an E2E phase sum |
| Volatile bank, cardless | 841.3 s | 151.7 s | 831.6 s | 148.6 s |
| Classic Chrome, cardless | 359.2 s | 580.5 s | 259.0 s | 68.2 s |
| Classic Chrome icon, cardless | 421.1 s | 619.3 s | 325.9 s | 93.6 s |
| Classic Chrome, card-backed | 348.0 s | 129.1 s | 244.1 s | 86.3 s |
| E8, Q-held attach | 357.0 s | 369.4 s | 248.2 s | 87.4 s |
| USB-toggle, 35 fresh-boot scenarios | 2,099.4 s | 680.4 s | n/a | n/a |

For the five RAM-push pairs, phase totals sum `e2e-result.json`'s recorded `phases[].seconds` on each engine. Each starts from a restored attach checkpoint; those sums cover the recorded start/allocate/relink/push/trajectory/unload phases, not all launcher work or the separately recorded cold-restart operation. FujiMod's sums are **2.8-5.6 times shorter**, but this is a mixed concurrent workload, not an isolated benchmark. The icon speed is not a parity pass.

The 35-scenario toggle route is **3.09 times faster** by whole-route wall time, consistent with the earlier main observation of 1,728 s versus 621 s, **2.78 times** ([runtime documentation](../emulation/README.md#candidate-verification-across-recorded-routes)). The fresh stock-boot and some shorter whole-route times are worse on FujiMod because launcher/queue costs dominate; this run does not support a claim that every user invocation is faster. No comparison converts guest instruction counts into physical time.

### Reproduction

Use the documented owner-supplied firmware/ROM/NAND/card inputs and the existing VolatileBank scenario plus its FujiMod timing variant. Private receipts are retained under `private-data/verify-candidate/runs/fujimod-decision-main-9b169a3` and its `-remainder` companion; the latter's completed receipts were judged in `fujimod-decision-main-9b169a3-remainder-judge`. The first `report.json` binds the source commit, verdicts and execution seconds; `differences/*.compare.txt` records counterpart file counts and failures. The receipt-only report has baseline verdicts, not the interrupted invocation's execution timings. Raw artifacts stay private. Evidence: `E-FUJIMOD-RUNTIME-DECISION-150` in the [ledger](../docs/evidence.jsonl).

```sh
export XM1_EMULATOR_SLOTS=8
BASE="$HOME/.cache/bazel/latent-image-FujiModDecision"
PAIRS=flash-boot-check,fujimod-flash-boot-check,ram-push-volatile,fujimod-ram-push-volatile,ram-push-cc-cardless,fujimod-ram-push-cc-cardless,ram-push-cc-icon-cardless,fujimod-ram-push-cc-icon-cardless,ram-push-cc,fujimod-ram-push-cc,usb-toggle-e8,fujimod-usb-toggle-e8,usb-toggle-emu,fujimod-usb-toggle-emu
REMAINDER=ram-push-cc-icon,flash-candidate-icon-b,flash-candidate-q-tile-b,updater-f7-rc0-toggle,updater-f7-cc-toggle,native-volatile,native-sd
devenv shell -- bazel --output_base="$BASE" run --jobs=4 --local_resources=memory=6144 \
  //emulation:verify-candidate -- --routes "$PAIRS" \
  --output private-data/verify-candidate/runs/DECISION-PAIRS \
  --bazel-option --jobs=4 --bazel-option --local_resources=memory=6144
devenv shell -- bazel --output_base="$BASE" run --jobs=4 --local_resources=memory=6144 \
  //emulation:verify-candidate -- --routes "$REMAINDER" --native-jobs 4 \
  --output private-data/verify-candidate/runs/DECISION-REMAINDER \
  --bazel-option --jobs=4 --bazel-option --local_resources=memory=6144
devenv shell -- bazel --output_base="$BASE" run --jobs=4 --local_resources=memory=6144 \
  //emulation:verify-candidate -- \
  --routes ram-push-cc-icon,flash-candidate-icon-b,flash-candidate-q-tile-b,updater-f7-rc0-toggle,updater-f7-cc-toggle,native-volatile \
  --from-run private-data/verify-candidate/runs/DECISION-REMAINDER \
  --output private-data/verify-candidate/runs/DECISION-REMAINDER-JUDGE
```

The first two commands above are shown sequentially for reproducibility; the measured invocations overlapped. The third reproduces receipt-only judging if a run is interrupted after those six routes complete. Use fresh output directory names. Do not use `--record-baseline` to make a changed candidate pass.

## What parity means

The six counterpart checks use `e2e-observation-compare`, not byte equality of complete guests. Its current implementation is [ObservationCompare.hs](../emulation/Emulation/ObservationCompare.hs), and the [trajectory rule](../emulation/README.md#trajectory-variants-on-fujimod) owns acceptance semantics.

- E2E comparisons omit wall seconds, instruction/virtual-tick counters, stack pointers, paths, frame numbers and fields named `sha256`. PNG content equality is therefore not established. They retain required menu states, message IDs, guest reads, hook/trace entries and register values other than the excluded stack pointer.
- Classic Chrome site entries are normalized into live-view develop passes. Consecutive identical passes collapse; idle passes outside the converter interval are dropped. A converter sequence split differently across frames can still fail. [#110](https://github.com/skeet70/latent-image/issues/110) owns that rule, not a blanket exemption for site entries.
- USB-toggle comparisons also omit engine/checkpoint identity, model-originated image-wide transfers, session ID `sid` and the attach snapshot `cells_after_start`. The last two exclusions still require an explicit maintainer ruling in #97. Zero differences under these exclusions is not proof those fields are equivalent.
- FujiMod trajectory variants may change press timing only. The volatile route uses its existing timing variant. The card-backed Classic Chrome and E8 retained baselines use fixed scene frame 7, whereas FujiMod refuses fixed scenes; current counterpart checks compare their semantic receipts, not identical source pixels.
- Stock boot is checked against a separate baseline for each engine. Equal baseline verdicts do not mean equal instruction counts, complete framebuffer bytes or boot timelines.

## Retained capabilities still missing from the supported FujiMod path

This is an integration inventory at main `9b169a3`, not a claim that the upstream Rust runtime cannot implement these features. Estimates are **[INFERENCE] engineering days** for one contributor familiar with this code, including behavioral tests, documentation and matched-input checks. They are ranges, not measured durations or commitments; upstream/legal waiting time is excluded. Several rows share work, so their totals are not a schedule.

| Retained capability / evidence | FujiMod gap and bounded work | Estimate | Gate |
| --- | --- | ---: | --- |
| Browser viewer: guest display, queued keys/dial, firmware picker, checkpoint controls, diagnostics and media API ([viewer contract](../emulation/README.md#interactive-x-m1-firmware-viewer)) | The supported `xm1-viewer` browser path still executes `start_setup_viewer.py`; `--fujimod` is admitted only for RAM-push E2E. Bind the existing browser contract to a Haskell/Rust session, including queue limits, rejected-upload preservation, snapshot eligibility, media insert/remove/protect/fault input and card-only cold restart. Upstream's own web app is not automatically this X-M1 integration. | 4-7 days | Required before retiring the supported retained viewer. |
| Loader-published flash-candidate UI trajectories, including icon and Q tile | `Xm1Viewer.hs` refuses `--fujimod --flash-candidate`. Stock FujiMod boot does not replace the no-upload, per-segment loader-publication lifecycle. Add that lifecycle to the existing Haskell harness and compare both recorded flash-candidate routes. | 1-2 days | Required before replacing those two gating routes. |
| Controlled fixed-frame synthetic source and its numerical/statistics fixture | Retained `--fixed-scene-frame` pins its scene generator; both FujiMod bridges refuse it. The engines currently have different scene sources. Implement an explicit source fixture and pin it in snapshot identity, then compare the intended display/statistics outputs. Do not claim visual equivalence from menu IDs alone. | 3-5 days | Required for deterministic visual baselines, or explicitly replace that acceptance contract with reviewed visual assertions. |
| Strict UI-conversion profile | FujiMod always registers the local declared UI-conversion adapter; the requested retained `strict` versus `declared-ui-cpu-fallback` profile is not forwarded. Preserve strict unsupported-operation refusal and key snapshots on the selected policy. | 0.5-1 day | Required before claiming viewer-option compatibility. |
| Closed stock service rehearsal | `xm1-viewer --ff80-rehearsal --fujimod` now runs the existing Rust endpoint bridge with Haskell orchestration. Descriptor, C0, identity/config, guest RAM read, scratch write/readback and stock-query operations retain their receipt assertions; C4 closes the service session. Context diagnostics use the bridge's site/core/register hits rather than retained task/stack/request decoding. | Implemented, qualification pending | Required before removing that command's retained runtime. |
| Bulk-read and AF-ring E2E launchers | Both launchers now accept `--fujimod` and `--compare-retained DIR`, using the same Haskell operations through the Rust endpoint bridge. Bulk comparison requires exact captured bytes, D1 windows and alias reads; AF comparison requires exact phase capture bytes, successful matching shared-decoder outcomes and matching mandatory unload. Retained remains the default until matched-input qualification. | Implemented, qualification pending | Required before those launchers cut over. Half-press/ordinary-AF acceptance remains separate research. |
| Interactive probe/stop diagnostics | Retained console and viewer expose bounded architecture/stack diagnostics, execution stepping, watch/hit/read and device status. The FujiMod JSON bridges expose only their route command subsets. Inventory actual callers and port the supported diagnostic effects, or explicitly retire an unused command with no remaining caller. | 2-4 days | Required for each retained diagnostic interface that remains supported. |
| Python-free staging and fixture preparation | FujiMod RAM-push now stages only the selected DAT and ROM, validates the DAT in Haskell and lets the Rust input path decode it directly. It does not stage `xm1-upstream`, require its Python/native environment or run `firmware_registry.py unpack`. Retained fixture-preparation callers still need individual migration before deleting their reference inputs. | Remaining callers only | The FujiMod RAM-push staging dependency is removed; this does not claim every fixture command is Python-free. |
| Pre-execution CPU-profile admission on mutable code | The bounded native runner rejects unsupported ISA before executing it. FujiMod's current R4F guard checks newly entered blocks only after windows of up to 1,000,000 instructions and caches by address/length. It cannot guarantee refusal before side effects or after same-size code replacement. Add pre-execution admission and correct translation/code invalidation, with rewritten-code negative controls. | 2-4 days | Required before treating the present guard as equivalent to pre-execution admission; otherwise adopt only with explicit documented acceptance of its weaker scope. |

Already present, not missing: cardless and card-backed RAM-push, ff80 endpoint transport, calibrated menu/mode/dial input, render settling, portable attach/install snapshots, the SD-host adapter, access probes, committed-NAND import/export and the standalone FujiMod updater. The [updater findings](../emulation/README.md#updater-f7-on-fujimod) report byte-identical exported NAND on both restore scenarios, but `verify-candidate` currently invokes only the retained updater routes. Adding FujiMod updater counterpart entries and their NAND comparison is approximately **0.5-1 day [INFERENCE]**; it is a cutover coverage requirement, not a missing NAND implementation.

The bounded C/Unicorn native-case and recovery-bootstrap tools are separate execution contracts. `native-volatile` and `native-sd` should remain independent gates, not be silently replaced by full-system FujiMod passes. Replacing necessary C engine boundaries is outside this runtime decision. Neither engine has established physical lens/AF accuracy, coherent sensor-backed AF observations or complete silicon behavior; those are not retained features FujiMod has lost.

## Licence status

**jitl has stated FujiMod will be AGPL; no licence file at the pinned commit yet.** This is the maintainer-reported upstream statement, not an inferred MIT/GPL choice. On 2026-10-25 the GitHub tree API for pinned [`ecdb6cb653ca0fa3372e4a4cf92c88bffa13ae29`](https://github.com/justjake/FujiMod/tree/ecdb6cb653ca0fa3372e4a4cf92c88bffa13ae29) shows no project-level licence. Recursive inspection finds component notices in the web device tree, Python/JavaScript bindings and the USB crate; those do not grant a licence for the runtime as a whole. GitHub's default-branch licence endpoint also returns 404.

The statement does not establish an AGPL version, an `only`/`or-later` choice, the grant's effective revision or coverage of Josef Miegl's contributions. Wait for the licence, review its scope and notices, then pin an actually licensed revision and repeat compatibility checks. [LICENSING.md item 7](../LICENSING.md#release-decisions) remains authoritative: local reference builds only, no conveyed FujiMod-linked binaries at this pin.

An AGPL grant does **not** resolve the statically linked GPL-2.0-only Unicorn fork or the local context-alignment modification. [Licensing item 2](../LICENSING.md#release-decisions) applies to both runtime choices. A distributed binary, bundled closure or deployed network service needs an artifact-specific assessment, compatible engine/grant or reviewed program boundary, plus complete source/notices and any network-source offer. Choosing Rust rather than Python does not change those obligations.

## Risks and their consequences

1. **Parity can hide the wrong thing.** Global name-based exclusions such as `sha256` are wider than an explicit PNG-only exemption. USB attach/session exclusions can conceal a real difference. Review exclusion scope and add behavior-level negative controls before promotion; do not enlarge exclusions merely to obtain green routes.
2. **One known semantic comparison is red.** The cardless icon develop-pass split must be resolved under #110 with an invariant that preserves converter order, multiplicity and arguments. A route exiting zero is insufficient while its observations differ.
3. **Device-policy parity is not hardware fidelity.** The SD adapter intentionally mixes retained register laws with FujiMod's delayed clock-stable law; several register/window bindings remain unchecked on X-M1. USB SetAddress recovery waits also differ between toggle and RAM-push bridges. Keep their provenance and route assertions, and do not generalize one passing route to arbitrary timing.
4. **CPU guard limitations matter for candidate code.** Post-window checks and stale same-size decode entries are weaker than pre-execution validation. A passing boot does not prove all exercised instructions are R4F-admissible under every mutation.
5. **Snapshots are runtime-specific artifacts.** Do not translate or reuse retained snapshots as FujiMod snapshots. Keep input/source/model identities in keys, test cold boot and restored/installed replay, and refuse incomplete device state instead of dropping it during serialization.
6. **The acceptance surface is larger than six pairs.** Browser behavior, flash-candidate trajectories, diagnostic workflows, service/bulk/AF launchers and updater/NAND output need their own coverage before deleting the retained engine. Shared Haskell orchestration is useful but cannot certify a new device/scheduler implementation.
7. **Performance is host-dependent.** Shared emulator slots, concurrent workers, build waits and snapshot availability affect route wall time. The measurements above are operational observations on this workstation, not isolated CPU microbenchmarks or a latency guarantee.
8. **Licensing and upstream churn can block shipping independently of local technical success.** Pin source, lockfiles, Unicorn and local patches; recheck the actual grant and closure when upgrading. Keep the retained source's attribution even after its runtime dependency is removed.

## Acceptance cutover and gates

1. **Implement the adopted direction.** New development uses the pinned FujiMod runtime now; freeze the retained Python engine except for the surfaces not yet ported. The maintainer still rules on `sid`/`cells_after_start` and supported versus explicitly retired diagnostic interfaces. Retained routes continue deciding candidate acceptance until qualification and the reviewed cutover.
2. **Close correctness and coverage gaps.** Resolve #110 without losing converter assertions; validate every timing-only variant; add FujiMod flash-candidate and updater/NAND counterparts; provide browser and service/bulk/AF integrations that remain supported. Define the intended fixed-scene/visual comparison instead of accepting PNG exclusions as visual proof. Resolve or explicitly accept the documented CPU-guard limitation.
3. **Qualify one exact candidate.** Run the complete `verify-candidate` matrix on the proposed integration commit with no ad hoc baseline recording: every retained gating baseline must pass, every promoted counterpart must exit successfully and have zero allowed semantic differences, and both updater NAND exports must match. For USB-toggle, the accepted current result is the same 29 passes and six explicit model-boundary blocks, not 35 successful scenarios; approve that blocked inventory explicitly and reject any new/earlier block. Exercise fresh boot and attach/install snapshot replay on the same payload and inputs, prove input/source/profile changes invalidate keys, and retain the independent native suites. Use an explicit hardware-free qualification run for the viewer's externally visible controls and the maintainer's visual check.
4. **Clear distribution separately.** Local development adoption does not wait for binary distribution clearance. Pin an upstream licence with known version and contribution coverage when it lands. Do not convey FujiMod-linked binaries or deploy a modified network service until the upstream grant, Unicorn combination and all source/notices obligations have an artifact-specific disposition.
5. **Make one clean default switch.** In a reviewed cutover commit, route supported launchers to FujiMod, promote its qualified routes/baselines to gating and remove retained-only launch flags, embedded CPython paths and obsolete device/scheduler callers. Keep the shared Haskell harness and standalone bounded native gates. Update supported commands, build/dependency documentation and #97 together. Historical retained baselines and the previous engine remain recoverable in Git, not as permanent fallback aliases or a second default runtime.
6. **Recover by reverting the cutover if needed.** Keep the pre-cutover commit as the rollback point and preserve private receipts/snapshots separately. A regression reopens the affected gate; it is not grounds for silently weakening comparisons or treating a FujiMod-only pass as camera clearance.

This development decision authorizes no physical camera run. Full-system fidelity remains in [#51](https://github.com/skeet70/latent-image/issues/51), and model timing/input measurement remains in [#53](https://github.com/skeet70/latent-image/issues/53). Neither gate changes.
