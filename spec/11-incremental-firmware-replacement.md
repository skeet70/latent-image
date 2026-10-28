# Incremental X-M1 firmware replacement

## Status and authority

Design accepted by the maintainer after review. Implementation may be scheduled according to the issue dependencies. Design acceptance does not accept a research gate or authorize a camera operation. The [current principles](../docs/principles.md), [camera-run procedure](07-governance-and-handoff.md#3-camera-runs), [runtime contracts](08-artifact-and-runtime-contracts.md) and [publication boundaries](../docs/principles.md#publication-and-asset-boundaries) remain authoritative. GitHub Issues owns task status and dependencies; this document owns the replacement process.

[Epic #117](https://github.com/skeet70/latent-image/issues/117) tracks review and milestones. The first wave comprises [existing-state inventory and baseline #118](https://github.com/skeet70/latent-image/issues/118), [Rust build/ABI #119](https://github.com/skeet70/latent-image/issues/119), [bounded computation and executable contract #120](https://github.com/skeet70/latent-image/issues/120), [integrated retained-service qualification #123](https://github.com/skeet70/latent-image/issues/123), and [user-visible ownership increment #121](https://github.com/skeet70/latent-image/issues/121). The computation depends on the build/ABI contract, not installer qualification. Inventory work does not gate compiler or computation work.

## Goal and completion boundary

Establish an owned firmware platform on which features can be implemented, tested and deployed through normal source-level development, progressively removing dependencies on proprietary application and runtime code. The objective is less reverse engineering and binary-patch integration per feature, not only enabling features stock makes impossible. Byte matching remains evidence rather than a prerequisite for replacement code. Function boundaries may change inside an owned subsystem; contracts at retained boundaries remain explicit.

Full replacement means the accepted capability inventory no longer depends on executing proprietary application or runtime code. Inventory retained boot ROM, peripheral microcode, calibration, fonts and other resource data separately. Such retained dependencies must be named in the final claim. Internal-flash deployment and permission to distribute a combined artifact are separate milestones, not prerequisites for demonstrating replacement execution from an admitted alternate boot route.

Start with X-M1 firmware 1.50. Target-specific evidence requirements follow the [scope and evidence principles](../docs/principles.md#scope-and-evidence). Evidence scope follows the body and matched bytes, not the repository containing it.

Start the capability inventory as a coarse outline: startup/shutdown, controls and menus, live view, autofocus/manual focus, exposure and capture, image development, save/playback, persistent settings, calibration consumption, external interfaces and failure recovery. Mark unknowns explicitly and populate details from existing evidence only. Recover modes, lenses, conditions and dependency closure only for the selected pilot and its dependencies; a whole-camera recovery exercise is not a first-wave prerequisite. Preserve stock behavior unless an intended difference is accepted explicitly. An untested capability is not implicitly preserved; exclusions require an explicit scope decision.

## Current implementation and reuse

Stage one is already underway. [The recipe-bank build](../emulation/build-recipe-bank-cases.sh) compiles C using Clang `--target=armv7r-none-eabi -march=armv7-r -marm -Oz` and links through LLD. Existing recipe-bank UI/edit code, RAM-push installation and payload checks are infrastructure to reuse, not tasks to recreate. The [transport documentation](../transport/README.md#ram-push-review-record) owns the review records and the [payload allowlist](../transport/README.md#bank-elf-allowlist) contract. The [RAM execution gate #71](https://github.com/skeet70/latent-image/issues/71) distinguishes demonstrated recipe-bank execution from remaining AF-specific acceptance.

The baseline task inventories concrete build, installation and emulator routes, including existing recipe-bank UI cases and `extensions/BuildExtension.hs`, and records their evidence scope. Existing working C remains reference and integration infrastructure; this epic does not require a blanket rewrite. New owned camera-side logic defaults to Rust, with necessary C/assembly stock-ABI adapters, under the existing [implementation policy](../docs/principles.md#implementation-and-build). Existing C does not decide the language of new implementations.

The [firmware extraction report](../reports/firmware-extraction.md) records X-M1 checkpoints in x70view with stock bytes equal to ours (E-XM1-DECOMP-CHECKPOINTS-150); those two checkpoint functions were NONMATCHING C at that revision. Separately, ten X-M1 functions were rebuilt exactly in the [matching pilot](../matching/README.md) (E-XM1-MATCHING-C-PILOT-150), using GCC 4.4.7/4.6.4 pinned in [exr2-gcc.nix](../build-support/exr2-gcc.nix). Use that work for interface recovery, byte/extent checks and original-binary references. Matching establishes only its stated function extents, not new-code behavior or a firmware-wide ABI. Matching compilers are reference tooling, not a requirement for Rust replacements; reusable findings may be contributed upstream under the existing publication policy.

## Publication

Apply the existing [publication and asset boundaries](../docs/principles.md#publication-and-asset-boundaries). Original implementations from recovered contracts and mechanically translated/decompiled implementations are eligible for publication under the existing provenance and redistribution review; derivation alone imposes no private-by-default rule. Copied proprietary code, instruction sequences, tables, resources and firmware artifacts remain subject to that policy's restrictions. Mechanical translation does not clear embedded proprietary material, and writing from a contract is not a claim of a clean-room process.

## Ownership stages

| Stage | Outcome | Incremental unit | Exit evidence |
| --- | --- | --- | --- |
| Extend stock | Normally compiled features operate through bounded stock interfaces | Feature or adapter | Qualified entry, behavior, effects and lifecycle |
| Own the application | Our state machines own user-facing behavior over retained services | Complete behavior, including cancellation and handback | Capability scenarios pass without relying on the replaced stock behavior |
| Replace the platform | Our implementations replace stock services, runtime and drivers | Service and its dependency boundary | Callers, callbacks, shared data, startup and hardware contracts are accounted for; the retired implementation is no longer required |

Stages overlap across subsystems. Do not wait for application-wide replacement before replacing an isolated service. Do not count bypassed entry points or unobserved execution as proof that a dependency is retired. Preserve the original firmware as a private verification reference, not an indefinite second production implementation.

For each retirement, distinguish executable code, data, initialization duties and intentionally retained resources. Scan for remaining references, including pointer tables and instruction-built addresses; retain unresolved computed references as obligations. Trap fetches from retired code and prohibited accesses to retired data during capability scenarios. Where feasible, build and boot an artifact with the retired implementation absent. If removal is not yet possible, record the remaining dependency rather than declaring full retirement. These checks establish independence within the declared scenario and artifact scope, not all possible executions.

## Replacement contract

Each selected target has a versioned, machine-readable contract and a short human explanation. Develop its typed representation with the bounded computation pilot in #120, extending existing artifact/native-case contracts only where the running consumer needs it. #118 supplies an inventory and baseline, not a prerequisite schema or a second pilot-selection process.

Required information:

- Target identity: body, firmware, source/input provenance, replacement identity and compatibility or redesign mode.
- Owned capability: behavior moved into our code, explicit non-goals and accepted differences.
- Boundary: arguments/results, ABI, retained stock symbols, shared structures, device protocols and all known callers, callbacks and indirect references. Unresolved references remain explicit obligations.
- Lifetime: admitted initial states, task/interrupt context, synchronization, ownership, asynchronous completion, cancellation, repeated entry, teardown and host-loss behavior where applicable.
- Effects: permitted memory changes, device operations, persistent changes and resource budgets. Bounds cover the operation lifetime, not only an entry snapshot.
- Verification: independently grounded expectations, reproducible input generators and seeds, scenario selection, observation projections, timing bounds and negative controls.
- Evidence limits: unsupported engine/device paths, unmeasured hardware behavior, provenance class and the exact scope established by each check.
- Promotion: unresolved question, least-risk qualifying experiment, prerequisites and decision authority under existing governance.

Qualification maps existing execution diagnostics onto pass within scope, behavioral failure, blocked prerequisite and unknown/unsupported execution. A native `returned` outcome passes only if its observations satisfy the contract; `unknown_call` or `unsupported_instruction` prevents qualification when required execution remains unsupported. A known forbidden effect is a behavioral failure, not a missing-model excuse. Preserve the underlying runner status and diagnostics. Required blocked or unknown cases prevent qualification. Matching expected refusals can validate a negative case, but two matching failures do not establish successful capability execution.

## Repeatable replacement loop

1. Select a useful boundary from the capability/dependency inventory. Prefer high feature leverage, observable behavior and bounded effects over raw function count.
2. Establish original behavior and the boundary contract from source, original-binary execution and available hardware observations. Record unsupported paths before implementing the replacement.
3. Implement or translate the candidate using the normal build. Generated code is a candidate, not evidence of correctness. Keep recovered addresses and layouts in the adapter rather than throughout application code.
4. Run bounded component checks, effect guards and negative controls. Reduce failures to reproducible counterexamples.
5. Exercise the actual integrated candidate through device-driven emulator scenarios, including entry, repeated use, cancellation and handback. Keep injected-state probes separate from reachable-state acceptance.
6. Run affected scenarios during iteration and the existing complete candidate matrix before qualification or live promotion. Review intentional differences rather than weakening comparisons or re-recording baselines to manufacture agreement.
7. Request the least-risk human or hardware check that answers a named remaining question. Apply the existing camera-run procedure independently of task completion.
8. Record the scoped decision, integrate the candidate and update the capability/dependency inventory. Remove obsolete production paths after cutover and select the next bounded target from exposed dependencies.

Reuse [Emulation.CheckpointKey](../emulation/Emulation/CheckpointKey.hs) and its existing guest-state, procedure and route identities (E-XM1-EMU-CHECKPOINT-KEYS-150, refined by E-XM1-EMU-CHECKPOINT-PROCEDURE-150). Invalidate checkpoints when initialization, executable memory or dependency assumptions change; startup changes require cold-boot scenarios. Qualification must exercise the packaged artifact and installation/boot path being claimed, not only a separately linked component.

## Automatic verification

### Compatibility and redesign

Compatibility targets compare original and replacement results at the consumer boundary: outputs, errors, externally visible state, artifacts, effects and required ordering. Ignore internal function layout and scratch registers except where they are part of the actual ABI. Define observation normalization explicitly and reject unexplained differences.

Redesign targets use an accepted behavioral specification for intended changes and preserve compatibility checks for unaffected behavior. Do not treat stock defects as required behavior without a decision. Timing, image-quality and numerical tolerances require their own evidence; instruction counts are not physical latency.

Exercise boundary values, invalid inputs, repeated operations, resource exhaustion, cancellation and relevant completion orderings. Scheduling exploration is bounded and reports the explored domain; it is not a proof over every interleaving. A plausible deliberately incorrect implementation must fail the pilot's oracle. Expectations must not be derived solely from the candidate implementation.

### Mechanical work

Follow the existing [implementation and build policy](../docs/principles.md#implementation-and-build). The new work is a pinned Rust native build interoperating with the established C/Clang infrastructure, not a replacement build system. Qualify `no_std`, `core` and compiler-builtins provenance, panic behavior, linking and the float ABI at each retained boundary; a target name alone does not establish stock ABI compatibility.

Automate artifact extraction and identity checks, reviewed binding generation, linker assertions, candidate translation, compiler configuration runs, bounded input/sequence generation, original-versus-candidate execution, effect checking, counterexample minimization, regression selection and readiness reporting. Cache only with keys covering source, contract, toolchain, engine, fixtures, checkpoint and comparison policy. Preserve diagnostics for failed or incomplete runs under ignored private storage.

Dependency discovery combines static references and runtime observation. Direct-call graphs alone miss indirect calls, aliases, DMA and initialization dependencies. Tools report unresolved edges instead of recursively guessing or declaring a closed graph. Bound work queues and execution attempts; do not introduce camera retries.

Use existing native-case and verify-candidate infrastructure. New full-system work targets pinned FujiMod under [the migration direction](../emulation/README.md); retained gates remain in force until their separately reviewed cutover. Missing coverage creates a named prerequisite, not permissive stubs or a new competing runtime.

### Independent evidence

A candidate and its emulator model cannot be each other's only oracle. Ground model extensions separately in X-M1 source or hardware evidence and retain comparison with the unmodified firmware. Pin engine changes separately from candidate changes, and rerun qualification after either changes. Differential agreement in one model does not establish silicon fidelity.

Respect the [native runner's admitted execution subset](../emulation/README.md#outputs-and-limitations) and [target CPU evidence](../docs/principles.md#scope-and-evidence). Missing support for an evidenced instruction creates an engine qualification task. Isolated native-case success does not establish scheduler, DMA, cache or peripheral correctness.

## Human verification and promotion

Prefer automated checks, then a human emulator check, then a bounded RAM trial, then SD boot, then internal-flash deployment. These are evidence methods, not an automatic progression or a single confidence score.

| Method | Question it can answer | Limitation |
| --- | --- | --- |
| Automated host/emulator checks | Reproducible bounded behavior, effects, lifecycle and regressions | Limited by cases, contracts and model fidelity |
| Human emulator check | Visible behavior, interaction and usability | Adds no physical timing or unmodeled hardware evidence |
| RAM trial | Behavior on the body after stock initialization | Volatile placement does not prevent persistent or harmful hardware effects |
| SD boot | Candidate startup and operation through the admitted card route | Does not establish internal-flash startup or general recovery |
| Internal flash | Persistent deployment and its boot path | Not inherently better algorithm evidence; requires separate update/recovery prerequisites |

A target's promotion request names the question, exact candidate, existing evidence, missing prerequisites and expected observations. Reuse applicable evidence only while its inputs and assumptions remain valid. Do not require hardware on every source revision or flash merely to close a software milestone.

The [camera-run procedure](07-governance-and-handoff.md#3-camera-runs) owns operation classes, authorization and review. The future orchestrator stops at those boundaries rather than authorizing its own promotion.

Power, lens-motor, shutter, flash-charge and other potentially damaging driver replacements require a subsystem-specific safety contract before a first-run plan is discussed. Cover sequencing, timing, feedback, limits, interlocks and failure handling. A register/value allowlist grounded in stock traces can support that contract, but observed writes are not safe independently of state and ordering. Review the first-run plan separately without relabeling all driver work as Invasive. Emulator-only pending suitable instrumentation or hardware evidence is an admissible outcome.

Track three distinct startup milestones: replacement startup in emulation; replacement startup on hardware through a qualified route; and persistent internal-flash deployment. A RAM payload inheriting stock startup establishes none of these by itself. Hardware startup needs an available route; [SD repair #85](https://github.com/skeet70/latent-image/issues/85) blocks treating the card route as available. Governance approval does not create a boot route, and flash is not a substitute for a missing lower-risk experiment.

Persistent deployment retains the existing flash-boot gate and boot-path prerequisites. A replacement may preserve stock menus; if its accepted behavior eventually requires a different gate, propose a separate reviewed governance change. Host/emulator development can continue while physical startup or deployment is blocked.

## First implementation wave

### Qualification contract and capability inventory

In #118, inventory existing mechanisms and evidence, create the coarse capability outline, and capture the iteration baseline described below. Use existing records; unknowns are valid inventory entries. Pilot issues own their exact target selection. No new runner, schema or whole-camera recovery is required to complete #118.

### Native build and retained-service boundary

#119 qualifies the Rust build/ABI contract using the current C/Clang path as a reference. Cover pinned cross-toolchain integration, `core`/builtins, panic behavior, emitted ISA, actual calling convention, stack alignment, preserved state, linking and runtime dependencies. Bounded execution and negative controls establish this contract without waiting for installer publication or a stock service scenario. The qualified interface and facts are in [`replacement/toolchain`](../replacement/toolchain/README.md) and [`replacement/abi`](../replacement/abi/README.md).

#123 separately qualifies Rust execution through the existing installer and a grounded retained-service call, including publication, task context, effects and lifecycle. Reuse current probes/features where possible. Extend [#71](https://github.com/skeet70/latent-image/issues/71) infrastructure without claiming its AF-specific gate. #120 depends on #119 only; #123 can proceed alongside #120 after #119, reusing contract conventions as they emerge.

### Bounded computation replacement

Use the area-3 record validator at `0x006a84d4` as the proposed bounded target, subject to confirming its existing documented input/closure contract in #120. Its [hardware-grounded native cases](../emulation/README.md#stock-native-case-150-area-3-record-validation) anchor accept/correct/reject behavior and correction effects. Keep this pilot host/emulator-only: integration into the custom-setting save path would require separate persistent-storage acceptance.

Implement the Rust candidate and the minimal executable contract/runner integration together. Exercise existing cases and deterministic mutations: all 352 protected-data single-bit flips and 61,776 distinct two-bit flips for a selected valid record. This exhausts those mutations for that record, not all record values or parity-field corruption. Add separately specified record and parity-field cases with independent expectations. If using stock writer `0x006a8484` to generate valid records, ground its own contract first; it supplements rather than replaces the hardware-derived cases.

Acceptance includes observable effects, ABI preservation, reproducible inputs, refusal of unsupported execution, rejection of a plausible faulty candidate and a reduced counterexample that retains its preconditions. Translation tools can assist recovery or candidate production; their output is not an oracle. Apply the publication policy above.

### User-visible ownership pilot

#121 inspects the existing recipe-bank UI and selects the smallest missing ownership or development-workflow increment. Reuse existing rendering, input and lifecycle evidence; do not build a duplicate browser or force a rewrite of existing C. The increment should demonstrate new Rust-owned behavior or an exercised behavior-preserving refactor without rediscovering stock hooks. Selection names the concrete gap and expected iteration benefit.

Bound the selected interaction to read-only effects over its complete entry/navigation/exit call chain. Exclude new setting application, persistent writes, capture and a complete UI redesign. Use [#80](https://github.com/skeet70/latent-image/issues/80) as a design reference, not an implicit expansion of that issue.

Final qualification depends on #120's demonstrated runner and #123's integrated boundary, plus the baseline in #118 for the comparison. Boundary research can proceed after design acceptance. Acceptance exercises the packaged candidate with device-driven emulator input, repeated entry/exit, cancellation, resource/effect checks and a human emulator assessment; hardware qualification remains separately scoped.

## Iteration baseline and first-wave decision

#118 establishes a baseline before pilot implementation changes the measured workflow. Recipe-bank and Classic Chrome records supply historical build/verification paths, retained interfaces and examples of firmware-specific investigation. Record effort or timing only where measurements exist; missing history remains unknown.

Choose one bounded existing feature change or behavior-preserving refactor as the comparison workload, with expected behavior and reproduction steps fixed before measurement. Measure source-change-to-emulator-result time, manual steps, firmware-specific edits and interfaces/artifacts that must be understood. Separate build time, execution time and investigation effort, and record cache state, toolchain, machine and engine identity. The baseline run is host/emulator work and does not require repeating historical camera experiments.

Repeat a comparable workload after the first wave. Report confounding differences instead of presenting cold-versus-warm caches or changed hardware as development gains. Assess whether the refactor required rediscovering hooks or changing integration contracts, not merely whether it passed.

After the pilots, record one decision in #117: continue, change approach, defer a subsystem, or pause. Use measured iteration cost, qualified capabilities and exposed dependencies to select the next work. The checkpoint evaluates the approach; it does not require re-justifying the platform objective or immediate feature payoff from the validator pilot.


## Existing work and subsequent targets

The epic coordinates rather than duplicates [FujiMod migration #97](https://github.com/skeet70/latent-image/issues/97), [emulator boundaries #51](https://github.com/skeet70/latent-image/issues/51), [timing calibration #53](https://github.com/skeet70/latent-image/issues/53), [AF candidates #109](https://github.com/skeet70/latent-image/issues/109), and [cardless AF/capture #113](https://github.com/skeet70/latent-image/issues/113). Their existing acceptance remains independent. In particular, a generic replacement framework cannot waive post-admission storage and ownership obligations.

After each pilot, rank newly exposed targets by capability leverage, dependency retirement, verification cost and risk. Keep a small ready queue with explicit prerequisites; do not pre-create an issue per recovered function. Later milestones cover application-wide ownership, service/runtime retirement and alternate-boot full replacement, followed by separately qualified persistent deployment if desired.

## Review and handoff

The maintainer accepted the reviewed design; record that decision in the epic and schedule child issues according to their dependencies. Subsequent changes to completion scope, effect boundaries or promotion rules require explicit review. Keep this document and the linked issue bodies consistent.

Measure progress by qualified capabilities owned, retained dependencies retired or still unresolved, reproducible counterexamples, and time from source change to scoped verification. Track emulator, human and hardware qualification separately. Function counts and generated source volume do not establish replacement progress.

A handoff names the selected target, current contract and candidate, executable reproduction commands, scoped outcomes, unresolved dependencies, next permitted host action and any separate promotion decision. Durable research claims use the existing evidence ledger; design acceptance adds no new research evidence.
