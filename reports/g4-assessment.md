# G4 assessment: owned-RAM execution, return and cleanup

Assessment for [#71](https://github.com/skeet70/latent-image/issues/71) against [spec/03 Phase 4](../spec/03-phases-and-gates.md#phase-4--establish-a-reversible-execution-and-observation-loop). Evidence base: [the ledger](../docs/evidence.jsonl) through E-SELECT-BLINK-83-RUN-150 (2026-10-25), [spec/05 §11.0](../spec/05-autofocus.md#110-stock-first-sensor-backed-iteration), [the RAM-push session script](ram-push-session-script.md) and [the AF observation plan](af-observation-plan.md). Body: X-M1 1.50 (RC0), Viltrox AF 28mm F4.5 firmware 1.05. This document is a proposal for the owner. It accepts no gate, and it performed no camera operation.

## Summary

G4's acceptance line is written at mechanism level, and the recipe-bank volatile trials demonstrate most of it on hardware. Run B (E-RAM-PUSH253-CONVERGED-RUN-150) and the #83 run (E-SELECT-BLINK-83-RUN-150) install a 253-publication payload from a camera-retained allocation, exercise it through owner-observed menu use, end with full power removal, and post-reboot reads find all 253 publication words and the callback slot `0x015c22ac` at their stock values.

Issue #71 defines the gate's acceptance around the AF payload: hardware evidence of the AF mechanism entering, returning and being removed with stock behavior restored, supported by pre/post state and original-byte comparisons. That does not exist. The frozen AF ring hooks are rejected for live use (E-AF-RING-REVIEW-CORRECTION-150), their emulator run uses a synthetic producer only (E-AF-RAM-RING-E2E-150), and the first ordinary AF attempt stopped at the cardless CARD ERROR refusal before any AF ran (E-AF-CARDLESS-S1-BLOCKED-150). G4 is therefore not ready for acceptance as #71 scopes it. The cleanup-criterion wording conflict and the remaining coverage limits are recorded below.

## Acceptance criteria, one by one

### 1. Target identity and patch sites verified

Met for the bank mechanism. Fresh preflights verify the X-M1 product oracle, stored `0xf7`, service mode, table `0x015c22a0`, slot `0x00dc7874`, channel 58 and the trace state, and match all 253 publication old words immediately before each push (E-RAM-PUSH253-VOLATILE-PREFLIGHT-150, E-RAM-PUSH253-CONVERGED-RUN-150, E-SELECT-BLINK-83-RUN-150). Relinks enforce the bank ELF allowlist and pass independent actual-base review with byte reproduction (E-RAM-PUSH253-VOLATILE-ACTUAL-150; the reviews named in run B and the #83 run).

Missing for AF: the six AF publications are pinned in source only (E-AF-RAM-RING-DESIGN-150). No body preflight of the AF entry words has run, and the frozen patch sites are rejected (E-AF-RING-REVIEW-CORRECTION-150).

### 2. Owned RAM

Met for the bank mechanism. `ram-alloc` returns camera-retained allocations with owned bases and stored guards: `0x00a85fd8` (E-RAM-ALLOC-JOURNAL-STOP-150), `0x00aa0d60` (E-RAM-PUSH-INSTALL-150), `0x00a83bb0` (E-RAM-PUSH253-ALLOC-150), `0x00a85e38` (E-RAM-PUSH253-VOLATILE-ACTUAL-150), `0x00a83608` in run B, `0x00a83b38` with owned extent `[0x00a83b38, 0x00aa1530)` in the #83 run. Lifetime is established: the allocation, push state and manifest are valid only inside the powered session and are invalid after full power removal ([session script](ram-push-session-script.md) sections 1 and 5, E-RAM-PUSH253-VOLATILE-RESTART-150).

Missing for AF: the 0x6800-byte AF allocation exists in the emulator only (E-AF-RAM-RING-E2E-150 at `0x00a81d00`).

### 3. Demonstrated enter/return/cleanup

Met for the bank mechanism. The payload entered through the retargeted callback slot and returned to stock code across owner-observed menu use, C1 apply and NONE restoration (E-RAM-PUSH253-VOLATILE-FIRST-SELECT-150 through E-RAM-PUSH253-VOLATILE-APPLY-RESTORE-150; the owner replies recorded in run B and the #83 run). Cleanup is demonstrated by full power removal with post-reboot stock-word verification: all 253 words at their old values and the callback slot at `0x00dc7874` (E-RAM-PUSH253-VOLATILE-RECOVERED-150, run B, and the #83 run, whose exit used "no hot unload, free, restore or retry"). An earlier body `ram-unload` once returned rollback verified (E-RAM-PUSH-SESSION-150), but current procedure forbids using it ([session script](ram-push-session-script.md) section 4 step 7).

Missing for AF entirely: no AF payload has entered, returned or been removed on the body. Emulator unload is not a substitute ([AF plan](af-observation-plan.md) camera-run proposal step 6).

### 4. Reviewed external-call set

Met for the bank mechanism: the bank ELF allowlist check, `transport/ram-push-review.json` pins, ImportSafetyReview, MechReview (E-RAM-PUSH-MECHANISM-REVIEW-150) and the per-run actual-base reviews.

Missing for AF: the only reviewed AF call set is rejected (E-AF-RING-REVIEW-CORRECTION-150), and no hook has passed the per-hook necessity gate ([AF plan](af-observation-plan.md) "Per-hook necessity gate before rework"). Live `ram-relink`, `ram-push` and `ram-unload` refuse the AF payload.

### 5. Recovery level recorded

Met. [Recovery status](recovery-status.md) records R1 accepted, R2 and R3 demonstrated and formal R4 unclaimed (E-STAGED-RECOVERY-SUCCESS-150), with the demonstrated repair scoped to the recorder-v2-v5 rollback. The SD recovery route is unavailable (E-XM1-CARD-SLOT-UNAVAILABLE-150, [#85](https://github.com/skeet70/latent-image/issues/85)), and each go and session script section 2 describes that remaining limitation and the no-card exit exception (E-RAM-PUSH253-NO-CARD-EXIT-150), which satisfies G4's R2-conditional wording.

### 6. No unexplained state changes

Met for the bank mechanism. The earlier full-session hang is attributed (E-SAVE-HANG-ASPECT-150: aspect 0 seeded from raw per-slot defaults, a value no stock menu produces) and current admission refuses unoffered IMAGE SIZE pairs before any write ([session script](ram-push-session-script.md) section 1). Both trials end with no recipe rows or setup prompt, and the #83 run's post-reboot comparison matched the pre-run recorded baseline: M defaults except L 16:9 and C defaults except COLOR +2, with the owner confirming the settings. Scope limit: that comparison covers the 253 publication words and the callback slot, not every RAM byte (limits of E-SELECT-BLINK-83-RUN-150). The #83 SELECT blink fix is verified on the exercised C1-to-NONE path and the encountered C1 action page (NONE, not the previously active C1, received the confirmation blink); [#83](https://github.com/skeet70/latent-image/issues/83) stays open only for the selections, slots and action pages not yet exercised.

### 7. Session scope covers this execution

Met for the bank mechanism: per-run owner goes naming source identity and the bounded sequence, journals under `private-data/sessions/`, and standing read-grant coverage for pre/post reads (E-CAMERA-READ-GRANT-150, E-RAM-PUSH253-OWNER-GO-150, run B and the #83 run).

Missing for AF: the AF trial exists only as a proposal, not authorization ([AF plan](af-observation-plan.md) "Camera-run proposal, not authorization"). A future AF run needs its own H2 go naming the revised contract and actual-base digest.

## Phase 4 steps the acceptance line does not name

- Step 1, AF and useful preview/statistic production with the ordinary-boot route: partially met. Ordinary-boot RAM reads are hardware-observed (E-BULK-READ-RUN1-150, E-BULK-READ-RUN2-150, E-BULK-READ-RUN3-150) and the stock message trace is readable (E-AF-ORDINARY-TRACE-ROUTE-150, E-AF-STOCK-CONSOLE-RUN3-150), but the one ordinary AF attempt on 2026-10-25 stopped at the cardless CARD ERROR indication before any AF search (E-AF-CARDLESS-S1-BLOCKED-150).
- Step 3, characterization of the call mechanism: met for the bank (mailbox, rendezvous, spin bounds, peer reports in E-RAM-PUSH-INSTALL-150 and E-RAM-PUSH253-INSTALL-150) except cache coherence, which is inferred from verified readback, not observed. Reentrancy of the AF hooks is unsettled: the suspended-context hazard is the review's blocking finding (E-AF-RING-REVIEW-CORRECTION-150).
- Step 4, constant-or-checksum first payload: no such body run is in the ledger. The bank hooks' verified returns and original-word restorations serve the step's purpose for the shared mechanism, but the literal step has not been executed and should be folded into the first AF entry as record-only ring writes.
- Step 5, capture/match/verify/activate: met for the bank (72-window pre-push match, installer old-word validation, readback and mailbox verification).
- Step 7, bounded AF event ring and coherent snapshots: emulator-only (E-AF-RAM-RING-E2E-150) and rejected for live use; snapshot work sits in [#107](https://github.com/skeet70/latent-image/issues/107).
- Step 8, instrumentation overhead within stock variability: not measured.

## Cleanup criterion: unload or power removal

G4's cleanup criterion is not written around either route explicitly. The acceptance line says only "demonstrated enter/return/cleanup", and Phase 4 step 6 says "restore the entry mechanism, and return to normal camera operation. Repeat after an ordinary cold start", which reads as a software restore with the cold start as a repeat check.

That wording conflicts with the current rules and the evidence:

- [spec/05 §11.0](../spec/05-autofocus.md#110-stock-first-sensor-backed-iteration) says not to assume safe hot unload or checkpoint restoration on the body.
- Issue #71's acceptance requires the physical cleanup route recorded, with every supply including USB removed for at least ten seconds, and states that a switch cycle or emulator unload is no substitute.
- The demonstrated and only approved route is full power removal with post-reboot stock-word readback. The [session script](ram-push-session-script.md) section 4 step 7 forbids `ram-unload` and `ram-free` as cleanup, the #83 run ends with "no hot unload, free, restore or retry", and E-RAM-PUSH253-SWITCH-RECOVERY-FAIL-150 shows a switch cycle alone left the push active.

Recommendation: reword Phase 4 step 6 to name power removal plus post-reboot original-byte comparison as the cleanup of record, and treat any software unload as unproven for a new payload. One historical body unload succeeded (E-RAM-PUSH-SESSION-150), so hot unload is not impossible, but no rule or review carries it forward, and the AF hooks' suspended-context hazard shows why it must not be assumed. G5 already says to record the exact cleanup route; G4 should say the same.

## Smallest experiments that would close the gaps

| # | Experiment | Closes | Camera |
| --- | --- | --- | --- |
| 1 | Owner's card-admission decision among the three options in [the plan](af-observation-plan.md) (slot repair, stock no-card AF route, reviewed AF-only admission override) | The S1 blocker on every AF experiment below | Slot repair is owner physical work; the other options need camera later |
| 2 | The stock-first observation block, attempted 2026-10-25 and stopped at the cardless CARD ERROR indication before any AF search (E-AF-CARDLESS-S1-BLOCKED-150): one ordinary half-press with same-boot post-attempt trace reads | Step 1, and the per-hook necessity input | Yes, H2 block already scripted; blocked by experiment 1 |
| 3 | Per-hook necessity determination, hook rework with deliberate suspended-context cases, re-review by the same independent reviewer | Criterion 4 for AF, the entry/return calling convention, step 3's reentrancy gap | No |
| 4 | One bounded AF-ring body trial: preflight entry-word and ADD-word comparison, one install, one half-press, ring and trace reads, power-removal exit, post-reboot stock-word readback | The #71 acceptance core: AF enter/return/cleanup with pre/post state and original-byte comparisons | Yes, one H2 go naming the revised contract and actual-base digest |
| 5 | Stock versus instrumented AF timing within stock variability | Step 8 | Yes |
| 6 | Document the installer's instruction/data synchronization beside the verified readback | The inferred-cache caveat of E-RAM-PUSH-INSTALL-150 | No |
| 7 | Fold the literal constant/checksum payload of step 4 into experiment 4's first entry as record-only ring writes | Step 4 as written | Only if kept separate from experiment 4 |

Experiments 2 and 4 are blocked by experiment 1; experiment 4 is additionally blocked by experiment 3. If no hook survives the necessity gate, issue #71's AF-mechanism acceptance needs the owner's re-scope, because the gate as written requires the AF payload to run.

## Form of the eventual acceptance record

When the gaps close, the record follows the G3 pattern: a paragraph under spec/03's G4 acceptance naming the accepted scoped result, its ledger IDs and the owner's acceptance date, plus ledger entries for the new durable claims and the updated #71 body under [issues and comments](../docs/principles.md#issues-and-comments). No such record is drafted here.
