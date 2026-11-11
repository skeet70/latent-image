# Kernel-timer up-count fix: patch and adoption assessment

[The timer laws report](xm1-timer-interrupt-laws.md) (`E-XM1-TIMER-IRQ-LAWS-150`) found from two hardware console captures that the X-M1 kernel counter `0xfff91000` rises within each tick in control mode `0x23`, while pinned FujiMod `3d90035d` counts it down. The up-count patch is the default FujiMod model (section 5); sections 1 to 4 are the patch and the assessment the maintainer decided on. Ledger: `E-TIMER-UPCOUNT-DEFAULT-150`.

## 1. The patch

`build-support/fujimod-timer-upcount.patch` (in the default stack of `build-support/fujimod.nix`, after the five earlier patches) changes only what a guest read of `0xfff91000` returns while the kernel timer is in mode `0x23`: `reload − remaining`, where `remaining` is the model's existing count of ticks to the next expiry. Both the settled read and the clocked fast read use one mapping.

Unchanged: expiry ticks, the line-32 edge to both cores, mode `0x63`, channels `+0x10..+0x3c`, writes, the `+0x04/+0x08` reads and the checkpoint state format. Declared assumptions (in `MAIN_ASSUMPTION` and the doc comment): the register range is `0..reload−1`, and after the stock sequence writes the first-period reload, starts `0x23` and then writes 30000, the register continues from `30000 − remaining`. The stock loader does not read the counter between those writes (`0x40c3c`, `0x40c44`). A stopped or zero-reload timer reads its stored count, as before.

Model tests added to the timer crate (all pass in the default check phase, together with the existing ones, one of which changes from 2 to 1 because the register now reads `reload − remaining`):

- `kernel_counter_rises_within_each_period_and_wraps_at_expiry`: after the stock switch sequence, `reload − count` falls within each period, rises only at the two expiries, and line 32 reaches both cores at each.
- `first_period_keeps_its_reload_and_expiry_tick`: the first period still expires after the first-period reload, then every 30000 counts.
- `stock_busy_wait_lasts_its_requested_duration`: a rendering of image 4 `0x723ffc` (reload read, then reload and count per sample, `+1` wrap term) waits its requested µs within derived bounds for 10, 200, 9000 and 25000 µs, including a start 300 ticks before a wrap. On the unpatched model every case returns after about one counter step (independent review computed 35 ticks against a 2400-tick target).
- `clocked_kernel_counter_matches_settled_reads_through_a_period`: fast reads equal settled reads through a full period and a short first period.

## 2. What adoption would invalidate

Inventory over all 279 `emulated` ledger records, `emulation/README.md`, consumers and checkpoint stores (scout inventory, corrected after independent review; section 4). The switch to mode `0x23` happens between about 6.14M and 6.43M entered instructions on the natural route (installer entry 6,136,330 in `E-KERNEL-INSTALL-NATURAL-COMPARISON-150`; first B103 wrapper entry 6,433,547 in `E-STARTUP-TASK-STOCK-PRODUCER-150`). After it, every busy-wait in drivers waits its real length, so instruction counts, interleavings and fixed-count endpoints move, and every sub-tick value written to RAM changes. FujiMod `timer.rs` is byte-identical at the earlier pin `ecdb6cb6`, so earlier-pin FujiMod records carry the same law.

| Verdict | Ledger ids | Guest runs |
| --- | --- | --- |
| (a) re-record under the patched SDK | 39, plus `private-data/verify-candidate/baselines/fujimod-flash-boot-check.json` | about 137 |
| (b) re-consume or re-compare once the (a) runs exist | 13 | 0 new guest runs |
| (d) evidence about the prior model only; adopting needs an adoption note where cited | 5 | 0 |
| (c) nothing | 222 | 0 |

Counts are of the ledger at `5aaff17c`. The later #148 ledger curation folded the pair's journal-control record into the pair record and removed three refused heap-observer records; the lists below name current records.

(a) families, runs in parentheses:

- #149 loader elapsed relation 6368/6370 and the 382M/menu(1,5,2) natural route: `E-LOADER-ORCHESTRATION-NATURAL-QUALIFIED-150` (2), `E-LOADER-CONTINUATIONS-NATURAL-ADMITTED-150`, `E-LATER-PLAIN-PRODUCER-NATURAL-150` (3), `E-IMAGE4-DISPATCH-CALLERS-NATURAL-150`, `E-IMAGE4-DISPATCH-CALLERS-OWNED-A-NATURAL-150`, `E-PRIMARY-OWNED-MENU-CONTINUATION-150`, `E-PRIMARY-LEAF-DATA-READS-150` (4), `E-STARTUP-DISPATCH-NATURAL-150` (4).
- #150-#155 startup tasks and kernel joins: `E-STARTUP-TASK-STOCK-PRODUCER-150` (1), `E-STARTUP-TASK-WRAPPER-QUALIFIED-150`, `E-STARTUP-TASK-COMPOSITION-QUALIFIED-150`, `E-STARTUP-TASK-COMPOSITION-PREWATCH-150` (45), `E-INITIAL-IDLE-NATURAL-QUALIFIED-150` (2), `E-TASK103-CRITICAL-PAIR-NATURAL-150`, `E-TASK-STORAGE-NATURAL-150` (22), `E-STARTUP-CONSUMER-COMPLETE-ROOT-CAPTURE-150` (3), `E-STARTUP-NATURAL-V10-150` (8; its schema6 pair may end before the switch).
- #148 heap: `E-STOCK-HEAP-NATURAL-TRACE-150`, `E-GENERAL-HEAP-CUTOVER-NATURAL-PAIR-150` (4).
- #128 modal and packaged UI routes with fixed instruction holds: `E-MODAL-RETAINED-FILM-ROUTE-150`, `E-MODAL75-DEVICE-GUARD-POLICY-150`, `E-MODAL75-PACKAGED-DEVICE-150`, `E-MODAL75-RENDER-DIFFERENTIAL-150`, `E-MODAL224-PACKAGED-FILMSIM-150`, `E-MODAL-LIFETIME-GUARD-150`, `E-RECIPE-UI-PRIMARY-PRESERVATION-150`, `E-SERVICE-PACKAGED-RETAINED-150`, `E-SAVE-LIFECYCLE-NATURAL-PREFIX-150` (14).
- #124/#138 capture: `E-CAPTURE-PROTOCOL2-A-150`, `E-CAPTURE-PROTOCOL2-B-RESTORE-150`, `E-SECOND-FILTER-DIAGNOSTIC-A-150`, `E-FUJIMOD-XM1-IRQ41-POST-WINDOW-150` (restores the card-backed attach checkpoint at 302.5M, binds end-state RAM at 877,360,000), `E-CAPTURE-EXCHANGED-PRIMARY-GATE-LIMIT-150` (enable-readback variant, binds `0x38c` store counts from 817,095,633) (5).
- #148 heap replay: `E-STOCK-HEAP-REPLAY-150`, eight `--mode run` replays seeded from its first-heap-call checkpoint, plus one seed run (9).
- FujiMod side of `verify-candidate` and its baseline: `E-CC-ICON-DEVELOP-PASS-PARITY-150` and the seven `fujimod-*` routes (7).
- Menu-only and calibration: `E-FIRST-STAGE-MENU-CONTROLS-150`, `E-FIRST-STAGE-COMBINED-SDK-MENU-CONTROLS-150` (2), `E-RAM-CALIBRATION-150` (1), `E-RTOS-SLOT-CENSUS-STOCK-150` (1, discovery census only).

(b): `E-LOADER-ORCHESTRATION-NATURAL-QUALIFIED-150`, `E-LOADER-CONTINUATIONS-NATURAL-ADMITTED-150`, `E-STARTUP-TASK-STOCK-QUALIFIED-150`, `E-STARTUP-TASK-EXISTING-RECEIPTS-150`, `E-STARTUP-CONSUMER-NATURAL-COMPARISON-150`, the journal controls of `E-GENERAL-HEAP-CUTOVER-NATURAL-PAIR-150`, `E-MODAL224-RENDER-DIFFERENTIAL-150`, `E-MODAL-LIFETIME-RENDER-150`, and the route records the seven `fujimod-*` route reruns re-compare: `E-FUJIMOD-XM1-FLASH-BOOT-150`, `E-FUJIMOD-XM1-RAM-PUSH-VARIANTS-150`, `E-FUJIMOD-XM1-SD-HOST-150`, `E-FUJIMOD-RUNTIME-DECISION-150`. Consumers bind the observer binary hash (for example `emulation/fujimod-xm1/src/heap_observe.rs:469`), so they re-consume new receipts from the patched binary, not the preserved ones. Two `source-inspected` checks outside the 279 also re-consume: the production-bridge tape replay in `E-GENERAL-HEAP-CUTOVER-ARTIFACT-150` and `E-LOADER-CONTINUATIONS-CONTROLS-150` (runs on the #149 receipts).

(d), still cited in `emulation/README.md` or reports but not route-compared: `E-FUJIMOD-XM1-PLAY-JPEG-150`, `E-FUJIMOD-XM1-S2-RAW-WINDOW-150`, `E-FUJIMOD-XM1-S2-SENSOR-OBSERVATION-150`, `E-FUJIMOD-XM1-PHYSICAL-RAW-OBSERVATION-150`, `E-XM1-KEYPULSE-150`. They remain valid as evidence about the down-counting model; the README sentences citing them need a note that the model changed.

(c) covers the native runner and host cases, the FujiMod bounded native backend (no timer device), every run on the retained `xm1-upstream` engine (its own down-counting timer at `emulator/interrupts.py:343,690-695`; it still carries the gating `verify-candidate` routes, `emulation/VerifyCandidate.hs:89-94`), x70view copies, FujiMod runs that end before the switch (first-stage, peer, loader-B prefix and composed-handoff records), and refused or superseded history that nothing cites. Eight borderline records near 6.14M (`E-KERNEL-INSTALL-NATURAL-COMPARISON-150`, `E-KERNEL-INSTALL-INVALID-MARKER-NATURAL-150`, `E-KERNEL-PHASE0-INSTALLATION-PREFIX-150`, `E-COMPOSED-NATURAL-POSITIVE-150`, `E-COMPOSED-COMPLETION-SCENARIOS-150`, `E-CORE1-INITIALIZER-NATURAL-150`, `E-LOADER-B-PREFIX-POSITIVE-150`, `E-STARTUP-FAULT-CONTROLS-2-150`) bind no counter-derived value [INFERENCE]; confirm against the `0x40b5c` entry count in a #149 receipt before closing them. The retained engine has the same direction error; fixing it is separate work.

Saved checkpoints: zero-instruction reset checkpoints restore safely (no `0x23` read has happened). Every checkpoint taken after the switch (RAM-push and USB-toggle attach/install caches, SourceBit0/protocol2 attach checkpoints at 302,500,000, the card-backed attach checkpoint, the first-heap-call checkpoint) would mix down-count history with up-count reads if restored under the patched SDK. The RAM-push, toggle, card-backed attach and protocol2 checkpoints are keyed on the bridge binary (`ram_push.rs:156-157`, `toggle.rs:116-117`, `emulation/README.md:1805,1965,1981`) and miss automatically. The heap-replay seed is passed by path (`--seed`, `emulation/README.md:1453`) and must be regenerated with `E-STOCK-HEAP-NATURAL-TRACE-150`.

Fixed inputs tuned on the current model that the reruns must re-validate: trajectories (`emulation/*trajectories*.json`, `scenario.fujimod.json` settle counts), key holds and release caps, and exact counts that consumers require (382,000,000 in `emulation/Emulation/StartupTaskWitness.hs:921`, menus in `heap_observe.rs:468`). With busy-waits at full length the menu may no longer be current at 382M [INFERENCE]; that endpoint may need moving.

Text citing the old values: `reports/rtos-platform-manifest-draft.md:78`, `reports/replacement-inventory-baseline.md:1559-1563`, `replacement/media_permit/rtos_surface.json:21`, `reports/subsystem-replacement-map.md:47`.

Cost: about 137 guest runs, at most two emulator slots host-wide. Recorded wall times: FujiMod reset to 382M without observer 4.4 s (`E-FUJIMOD-XM1-FLASH-BOOT-150`); the whole `E-SECOND-FILTER-DIAGNOSTIC-A-150` observer run through 837,529,073 instructions 102 s (`emulation/README.md:2100`); FujiMod USB-toggle suite 621-680 s; full `verify-candidate` 1,021-2,194 s, both measured at 8 slots (`emulation/README.md:2301,2305`), so slower at 2. Memory-watch observer runs have no recorded wall time. Each family also needs its own independent review of re-recorded receipts. [INFERENCE] Several days of worker time, dominated by the 45 startup-task runs and 22 task-storage runs, unless one fresh retained/owned natural pair per observer family is accepted in place of the full scenario matrices.

## 3. Options

1. **Fix now.** Make the patch default, re-record the 39 (a) families (about 137 runs), re-consume the 13 (b) records, note the 5 (d) records, regenerate post-switch checkpoints, re-validate trajectories and endpoints, update the four citing texts. FujiMod counter direction then matches hardware; frequency, divider and the instruction-to-tick ratio remain model policy. Highest cost; with two slots it would occupy the emulator for days [INFERENCE].
2. **Fix behind a flag (this branch as is).** Keep the default SDK; use the `timer-upcount` variant for new work whose claims depend on counter values or µs waits (the embassy time driver of #156, any owned driver with busy-waits), and record which SDK each new record used. Existing evidence stays valid as evidence about the current model. Cost: one SDK build per variant; the two models coexist until a later cutover.
3. **Defer.** Leave the branch unmerged and keep the mismatch documented in the laws report. Zero cost now; every new FujiMod run deepens the eventual rerun set.

Recommendation [INFERENCE]: option 2 now, with option 1 scheduled when the current #128/#148/#156 qualifications are next re-recorded for their own reasons. Most current qualifications compare stock against owned code on the same model, which the direction mismatch affects equally on both sides; the exceptions are `E-RAM-CALIBRATION-150` (engines against a hardware capture), the census, the capture records and the FujiMod-against-retained route comparisons, whose absolute values are model-specific already. New timer-dependent work should not be built on a known-wrong law.

## 4. Review

Patch: independent review found no required change and six advisories, all applied (assumptions declared, fast-path test extended to a short first period, test variable and bound comments corrected, README entry).

Assessment: independent review sampled 21 ids and found three required corrections, all applied: `E-STOCK-HEAP-REPLAY-150` moved from (b) to (a) with nine runs; two cited post-switch records added to (a); earlier-pin FujiMod records classified explicitly as (b) route records or (d). Advisories applied: checkpoint keying, two more borderline records, cost units and slot counts, options wording, `VerifyCandidate.hs` line range, the two non-`emulated` re-consumers.

## 5. Decision: default model

The maintainer adopted the patch as the default emulator model on 2026-11-03. `build-support/fujimod.nix` applies it to every FujiMod build, so the default SDK, `lab_fujimod`, `lab_fujimod_xm1` and the scoped-watch and enable-readback variants all count up; the opt-in `fujimod-timer-upcount.nix` and `fujimod-xm1-timer-upcount.nix` are removed. Existing records stay valid as evidence about the down-counting model. The (a) and (b) items of section 2 are re-recorded or re-consumed under the default only when each next comes up for re-recording for its own reasons, and a post-switch checkpoint taken under the old model is not restored into the new one. The binary-keyed checkpoints miss automatically; the heap-replay seed passed by `--seed` does not, and it has been regenerated on the default model with `heap-observe --endpoint first-heap-call` and re-pinned, while the replayed tape is still the down-counting `E-STOCK-HEAP-NATURAL-TRACE-150` recording (`E-STOCK-HEAP-REPLAY-150`). The reported-only `fujimod-flash-boot-check` baseline of `verify-candidate` was recorded on the old model.

The (d) records' citations in `emulation/README.md` carry a note that the model changed. The embassy phase-1 demonstration (`E-EMBASSY-PHASE1-EMULATED-150`) now builds its demo as `counter-up` with a `counter-down` mismatch control; under this default both domains measure the up count, and the control falls back to tick-only time.

The retained `xm1-upstream` engine keeps its down-counting timer for now: it is the acceptance reference for 10 of the 12 gating `verify-candidate` routes, so the fix would change those baselines at once, and it is frozen legacy Python. The change it would take is in [the device-fidelity audit](emulator-device-fidelity-audit.md#5-retained-engine-timer).
