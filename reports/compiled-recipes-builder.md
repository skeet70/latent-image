# Compiled recipes and the composed candidate (1.50): rebuild and review package

Host-only integration record for RECIPE-D3-PATCH-01's rebuild. It owns the rebuilt artifact set, the plan checks, the composed container, the D3-row consumer acceptance and the exact package an independent final review reads. Nothing here is installable, no camera or USB device was touched, and no firmware candidate is emitted. The design of the two compiled rows is [the D3 patch report](recipe-patch-d3.md); the bank's design is [the recipe design](recipe-design.md), with private integration provenance in `private-data/analysis/x-m1-1.50/recipes/bank-integration-contract.md`; the container operations are [the rebuild contract](update-container-rebuild.md) and [the image-2 record operation](image2-record-operation.md).

The historical composed container is not an installation candidate. Section 3 records that it has no boot-carve or stub write, and section 2's linked executable blob is a separate artifact, not part of the container chain. The owner-selected next sequence is recipe-only preparation, with recorder v3 reviewed separately. The install procedure for a complete recipe-only image is [the recipe-only install procedure](recipe-install-session-script.md), which binds a candidate only after the gates in its section 2. Verification uses both the bounded software emulator and the evolving full-system hardware emulator; a hardware-emulator stop alone does not identify a firmware defect.

The full-system control results are E-RECIPE-HARDWARE-EMU-CONTROLS-150. The combined container stops at CPU0 `0x0061d074`, unsupported native exception 4, in the stock registry walker. The frozen runtime boots both stock and a D3-only isolation control to the startup screen. The latter reaches the actual film-simulation menu, renders `WARM STANDARD`, and selecting it visibly changes ISO to 400 and dynamic range to 200%. Reopening that list displays the first PROVIA row, so selection-highlight retention is not accepted.

For owner visual feedback, the diagnostic control is `private-data/analysis/x-m1-1.50/recipes/d3-isolation-control/d3-control.DAT`, SHA-256 `45e560cc06228c27e52fefe8c4f591fcfbb2b8ca9b60ff024c5e878a69f0a9a5`. It carries the existing compiled rows, not the seven-slot bank, and is not the requested final deliverable or cleared install bytes. Launch with `devenv shell -- bazel run //emulation:xm1-viewer -- --firmware private-data/analysis/x-m1-1.50/recipes/d3-isolation-control/d3-control.DAT --port 0`. After startup, open SHOOTING MENU → FILM SIMULATION; check the two extra rows, name positioning, selection and parameter readouts. Q-cell interaction was not exercised. Bounded software-emulator checks remain required separately.

## 1. What was rebuilt

| Artifact | Path | SHA-256 |
| --- | --- | --- |
| 127-write bank plan | `private-data/analysis/x-m1-1.50/recipes/runtime/builder-out/bank-plan.json` | `10bfdb37a3f57131194767b40c12813b7539e04ef371c61dd0cbac97711eb751` |
| Bank data region (462 B) | `recipes/runtime/builder-out/recipe-bank-blob.bin` | `c016fe59f3cc03f3d3940bd314ab2f837df80412edba71e8d2a48d46434d0071` |
| Slot payloads (280 B) | `recipes/runtime/builder-out/slot-payloads.bin` | `c13b3d342969ea34cb19e513c8e099674f73a659de48fff9206e06875677d94a` |
| Slot names (182 B) | `recipes/runtime/builder-out/slot-names.bin` | `223159884b3d13096b295771c97afd3c2773bcd8c78e6a796f04428e69fb8ceb` |
| Name records (14 x 66 B) | `recipes/runtime/builder-out/name-records.bin` | `8e88789f63fe2610255732f5086117e54a6492c254c2bca582f66e8df54ee2e5` |
| Bank file (512 B) | `recipes/runtime/builder-out/recipe-bank.bin` | `dd549fb07bda5d4d7edbcc496c48313c91b1151ab8e747af6af6f33a2da61857` |
| D3 hook (76 B) | `recipes/runtime/d3-compiled/hook.bin` | `a073fdf7399c5c17076b5dd34eb5b461f4c98cccf11d30515564559faa2985ee` |
| D3 image-4 manifest | `recipes/runtime/d3-compiled/patch-set-image4.json` | `964d239e5e67e6104ce62f9a882e20731f2ab7b2f8974acaac0c2266eb5ba9aa` |
| D3 image-5 manifest | `recipes/runtime/d3-compiled/patch-set-image5.json` | `b25d36a839865154c3fff84ea8e3528684756ae04c1f0916635e560c37857118` |
| Compiled image 2 | `recipes/runtime/d3-compiled/id2-patched.bin` | `1b8a281c34450ae7d9a195298fb66398e750ddbb599c3b98e152782416dcf294` |
| Composed container | `recipes/runtime/combined-candidate/combined-candidate.DAT` | `b1aabeb61f8ae19c83e4f76efcf0710687e8008daa998c625b7c0bf0f5ca0757` |
| Candidate manifest | `recipes/runtime/combined-candidate/combined-candidate.json` | `e208bbbf77463daedbf64c3b032a8f228016500ba5a1428a8821421db5aa723d` |
| Carrier chain log | `recipes/runtime/combined-candidate/chain.log` | (85 calls, each output hash and main-checksum step) |

The `hook.bin`, the two D3 manifests and the compiled image 2 reproduce the frozen bytes byte for byte, and so do the five builder outputs that the integration contract records: the data bank `c016fe59...`, the payloads `c13b3d34...`, the names `22315988...`, the record table `8e88789f...` and the 127-write plan. The bank file `dd549fb0...`, the 512-byte card file the storage kernel loads, is reproduced as well.

## 2. The linked code blob: lineage and the recorded-hash delta

The integration contract records the bank's linked code blob as `29a7dba7434c68de565906ce8f3355c347a56da72cd65d63b3bc20be7af95f74`, span 12,654 bytes (`.text` 10,838 plus `.data` 1,816) ending `0x011dfe10`, the freeze state after the staged-edit fixes. The final rebuild from the current sources links `d9a7e98c98405ef23c114b6c39e621fff52ff958beb775b3fe0bd70d1a305391`, span 14,384 bytes (`.text` 12,370 plus `.data` 2,014); the intermediate post-audit, pre-F1/F2 link was `41e8f7066dac702a74a98ff9be3a04c6a46ad71af5e3fe31847a763fbb983720` (13,804 bytes). The lineage is three source states, each supersession a landed review-closed change and not a defect of the frozen files: the audit fix set (`menu/` +575/-304 lines against commit `72d8be6`, E-AUDIT-SOURCE-STATE-150) and then the two closed UI-acceptance defects of section 6 (the dialog cursor write-back in `recipe_bank_edit.c`, the visit-latched single-load refresh in `recipe_bank_hook.c` with the new `rb_name_resident` export in `recipe_bank_storage.c`).

Per the contract the delta is reported rather than rebuilt over. The plan and the container are code-independent and were re-derived at the final revision: the builder re-emitted the byte-identical plan `10bfdb37...` with every check passing, and the chain re-ran to the byte-identical container `b1aabeb6...` with `verify` passing. The only moved hashes are the linked blob, the model and the case evidence; the earlier 126-write composition and its container `e87fdc06031d1b6138dff272feddc2c589d56644c0845b81d55b4d7eb534207d` remain superseded (the 126-write plan predates the seven stock-mode film edit label quadruples, the builder's derived carrier offsets and the audit fixes).

## 3. The 127-write plan and its checks

`bank-plan.json` carries 127 ordered writes over nine declared regions, 4,826 bytes of new content (the 85 carrier writes 4,664 bytes, the 42 image word writes 162). The builder refuses to emit unless `validatePlan`, `validateCarrierSpans` and the boot-separation check pass, and the rebuild's plan was re-checked independently of the builder:

- **127 writes**, each inside its declared region, runtime spans pairwise disjoint across regions.
- **85 carrier writes** (the plain-image window regions): every write's `flash_offset` equals its region's declared offset plus its offset in the region, where the region offset is the loader mapping `flash = RAM - 0xB60000`; all 85 spans lie inside the window `[0x5a0000, 0x900000)` and are pairwise disjoint in flash. The plan's own carrier notes name each call's flash offset, the span file `span-<plan index>.bin` and the chained input/output; all 85 note lines agree with the derivation.
- **The label-word rule**: the 22 direct label gates hold the imm12 encoding of `0x849`, the eight id-minus-one gates `0x848`, and the six record-base `movw`/`movt` pairs `0x011ac900`, each write preserving the site's other bits (`0xfff0f000` mask).
- **Region and overlap checks**: no write leaves its region; no write overlaps the boot carve `[0x00be1a00, 0x00befffc)` or the stub entry `0x0078319c`. The plan carries no carve or stub write at all: the boot-copy publication is not part of this candidate.

The carried label-class edit set is the 42 writes the recipe design section 17.6 names, and the film edit label quadruples carry the seven pinned stock film tokens.

## 4. The composed container

The chain is recorded in `combined-candidate.json`, and `chain.log` holds every step's output. From the pinned stock DAT `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`:

| Step | Output | SHA-256 |
| --- | --- | --- |
| D3 image-4 apply (155 word writes) | `id4-patched.bin` | `6e5df0b7a8a8083679d5cd78e314598e192f7aa04969ca296f5ea381ea5661e0` |
| D3 image-5 apply (1 word) | `id5-patched.bin` | `5626062e760975dee8cff85c0d162e86e0be2f36df738af47df9dfd8cc85c8bb` |
| bank id4 words (34) | `id4-final.bin` | `84d18cbd04c9708b0b27add2cdaf60dd1e9e6501a630311eac9a955d486b8b28` |
| bank image5 words (8) | `id5-final.bin` | `c9a2226704043e76a6865f835356cdf4233357e1e95d6e78416edc9fcc13ec1a` |
| `lz-encode 0x160000` | `record4.bin` | `5f47dbee3ac91312ed578eb383884b1927f20fe5afaf8eb8a0b31b4cb2e9605c` |
| `lz-encode 0x2e0000` | `record5.bin` | `d9d65065a0f20968b74b335ad78bd5617d3ec06ed4e10def0915d1f65f48c751` |
| `replace-record4 --load-address 0x0061c1fc` | `step1.DAT` | `173ebb75019348243ac1fe84e2c7c226275d76d71136ebe93f3db992f544308e` |
| `replace-record5` | `step2.DAT` | `2b06e98b14e638235dc8e4bafd4957059249fd7b9327220cf06453e45d46b1b2` |
| `write-label "WARM STANDARD"` | `step3.DAT` | `f9e55942c131b444566a48057794681f0bd270d0a3b5400dc9a9fa5b0e8ed6e3` |
| 85 x `replace-plain --flash-offset` | `combined-candidate.DAT` | `b1aabeb61f8ae19c83e4f76efcf0710687e8008daa998c625b7c0bf0f5ca0757` |

Gates: `apply-patch-set` ran four times over 198 ordered word writes with every input hash and current-bytes check passing; `lz-encode` passes the recorded structure and F1/F2/F3 scans; `replace-record4`, `replace-record5` and `write-label` pass their full gate sets with the written-file read-back and zero carried bytes differing outside their extents and the main checksum word; each of the 85 `replace-plain` calls reports 42/42 contract fields and zero carried bytes differing outside its span; `rebuild-update verify` passes 42/42 on the final file. `rebuild-update diff` against stock: the carried parts (identity records, selector word, range fill, both auxiliary restore lists) are 0 bytes changed; the changed parts are the segment table (4), image-4 header (4) and stream (1,281,358), the main gap between the records (345), image-5 header (2) and stream (2,715,042), the main post-head (3,372,399), the image-2 extent (47) and the trailer (4); the main checksum is `0x8f080f30` against stock `0x8f0ab18f`.

The main checksum walked `0x8f0ab18f -> 0x8f0a2413 -> 0x8f0a192f -> 0x8f0a323a -> ... -> 0x8f080f30` across the chain's steps.

## 5. The D3-row consumer acceptance

The instrument is the kernel case pipeline, [emulation/RecipeBankCases.hs](../emulation/RecipeBankCases.hs) with [build-recipe-bank-cases.sh](../emulation/build-recipe-bank-cases.sh); the family named `d3-*` composes each case from the composed candidate's own `id4-final.bin` and `id5-final.bin` at their runtime addresses and runs it under the bounded native runner. Its shape is the kernel family's real-stock chain: the stock commit dispatcher `0x00d4fc74` with the case-`0xfe` site patched to the compiled hook at `0x0061c3ac`, the prefix-resident hook and its two row tables and property table, the chain, the pinned conversion tables and pointer block, the consumer cell block and the settings mirror. Only the six notification/display/mode-state calls are register-only stubs, and the two copy wrappers are one `bx lr` word each, exactly as the kernel family substitutes them.

The five cases and what they assert at the consumer:

| Case | Entry | Asserted at the consumer |
| --- | --- | --- |
| `d3-render-provia` | property `0xfe`, row `0` | film cell `0x80` = `0`, record film byte `+0x8a` = `0x0a`; the seven parameter cells stay `0` |
| `d3-render-pro-neg-std` | `0xfe`, row `5` | film cell = `0xb` (11), film byte = `0x9a` (code 9) |
| `d3-render-pro-neg-hi` | `0xfe`, row `6` | film cell = `0xc` (12), film byte = `0xaa` (code 10) |
| `d3-row-a-consumer` | `0xfe`, row `2` | hook walk; film byte `0xda` (code `0xd`), film cell `0`; cells DR 2, highlight 1, shadow 3, colour 2, sharpness 2, NR 1, WB 0; record bytes `0xca 0x2a 0x6a 0x6a 0x4a 0x2a 0x0a 0xab` for the eight properties |
| `d3-row-b-consumer` | `0xfe`, row `4` | hook walk; film byte `0xea` (code `0xe`), film cell `3`; cells DR 1, highlight 2, shadow 2, colour 3, sharpness 1, NR 0, WB 0; record bytes `0xaa 0x4a 0x4a 0x8a 0x6a 0x0a 0x0a 0xa8` |

Every expectation is read from the candidate's own bytes at case-build time (the row tables from the prefix, col0/col2 from the pinned conversion tables), not restated from the patch manifests; the three stock rows are the film modes the body's nine-shot render check selected on 2026-10-10 (PROVIA, PRO Neg Std, PRO Neg Hi, E-FILMSIM-RENDER-150/E-FILMSIM-RENDER-REPEAT-RUN-150), so the render claims ride on that measured visual difference and the host record proves the renderer-input equivalence at the consumer.

Renderer input reading: the two compiled rows change it measurably. PROVIA's input is code `0` with downstream `0` and no parameter writes; recipe A stores code `0xd` with PROVIA's downstream `0` **and** moves the seven parameter cells to the recipe's own values (DR 200 %, highlight medium hard, shadow medium soft, colour high, sharpness standard, NR medium low, WB auto, ISO 400 -> the ISO record byte `0xab`); recipe B stores code `0xe` with Velvia's downstream `3` and its own parameter set. The three rows are distinct from each other, and each recipe differs from PROVIA's input in both the stored film code and at least six consumer cells.

Outcomes (final run of 2026-10-11, `recipes/runtime/kernel-cases/results.json` and `run-provenance.json`): all 34 cases pass, the 29 kernel cases and the five `d3-*` cases, against candidate `id4-final` `84d18cbd...` and `id5-final` `c9a22267...`; the run linked the code blob `d9a7e98c...` (14,384 bytes) and the model `9d821c40...`, and `run-provenance.json` pins the six module digests and the compiled-recipe hook source the run used. Cross-checks: the expectations the D3 cases generate are byte-identical to the retained D3/Q family's verified expectations for the same rows (`recipes/runtime/d3-cases/`, E-RECIPE-D3Q-CASES-150: `rowpath-a`, `rowpath-b`, `stockrow-0/5/6`), and the compiled emitter's own printed rows and effective values agree with the table-derived ones.

Still outside this family: the hook's instruction fetch from the prefix on hardware (the emulated fetch executes; the MPU residual of [the D3 report](recipe-patch-d3.md) section 5.7 stands), the placement, the menu draw, camera rendering, persistence and the ordinary-mode/C-position behavior. The Q cell and label/selection evidence is the retained D3/Q family and the label-resolve cases over the same compiled bytes.

## 6. The two closed UI-acceptance defects (history)

Both were found by the UI acceptance of 2026-10-11 and their reopened bytes are review-closed (ReviewReopenedSol: correct); they are fixed in the sources this rebuild used:

- F1 (edit module): the dialog OK/CANCEL resolution left the caller page's cursor on the hidden dialog row (cell `0x009571d8` observed `4`), so the next SET re-entered the dialog with no pending confirmation. Closed by the dialog cursor write-back in `menu/recipe_bank_edit.c`.
- F2 (hook and storage): a card swap mid-scan let one page discovery publish a mix of two cards' names, because each of the seven `rb_slot_load` calls rescanned. Closed by the visit-latched single-load refresh in `menu/recipe_bank_hook.c` with the `rb_name_resident` export in `menu/recipe_bank_storage.c`.

The fixes moved the linked code blob from `41e8f706...` to `d9a7e98c...` (section 2); the plan and the container are unchanged by them.

## 7. What the rebuilt candidate claims, and what it does not

The candidate claims, byte for byte: the D3/Q film-simulation list with nine entries including the two compiled rows (`WARM STANDARD` as the id-`0x83d` inline name; the second row carrying its base mode's stock quadruple), the two film conversion rows rewritten to stored codes `0x0d`/`0x0e` with their base modes' downstream values, the 76-byte commit hook and its tables at the prefix base `0x0061c1fc`, the fourteen availability cells, the eight pointer relocations, the compiled image-2 name, the bank's seven slot pages, editor and staged-edit pages, registration words and the 42 label-class edits, and the 127-write plan's placement in the plain window and both image decodes.

It does not claim: installability (every artifact carries `installable: false`); the boot-copy publication (no carve or stub write is in the plan); placement ownership of the plain-image gaps; that any emulated or static result is a camera observation for the compiled rows; rendering of the compiled rows on the body (the measured nine-shot rendering is the stock three modes'); the bank's runtime behavior beyond the modeled cases; and any recovery capability.

## 8. Acceptance and residual summary

Union host gate at this revision (`bazel test //host/... //lz/... //emulation/... //transport/... //tests:research-contracts --cache_test_results=no`, re-run for this package on 2026-10-11): **24 of 24 test targets pass** in 1,032 s, including `//emulation:native-runner-tests` and `//host/firmware:recipe-bank-tests`. An earlier run of the same command at the pre-fix revision returned 22 of 24 (the host codec's unbound-bit zeroing and the runner's signed-load admission); that result is superseded history, kept only because it named two peer-slice defects that the subsequent fixes closed.

Accepted at this scope, with the record that carries each:

- The composed candidate's artifact set and plan checks: 127 writes, the 85 carrier writes' derived flash offsets, the label-word rule and the region/overlap/boot-separation checks, the container's 42/42 verify and unchanged carried parts (sections 1-4).
- The compiled rows' consumer behaviour: 34 of 34 cases (29 kernel plus the five `d3-*`), with the renderer-input equivalence bound to the measured nine-shot rendering (section 5).
- The kernel runtime acceptance for the bank's state-apply and file-ownership lanes (E-RECIPE-KERNEL-ACCEPT-150): 29 of 29 with the real stock chain, and its substitution limits standing as named - media worker bytes modeled at stock bases with in-call completion, a four-slot pool against the stock sixteen, the two copy wrappers as `bx lr` words, two register-only stubs on the arm path, and the runner's CLZ and LDRSB/LDRSH handling. These are modeled boundaries, not camera observations.
- The reopened F1/F2 bytes: review-closed at source level (ReviewReopenedSol correct, E-REOPENED-BYTES-REVIEW-150), which is deliberately not the same as complete UI acceptance.

Residuals the owner-facing evaluation carries:

- UI behaviour: the harness record is 17 of 25 (`recipes/runtime/ui-cases/results.json`); `n-slot-0` through `n-slot-6` stop at a function alignment and containment boundary and `n-save-ok` at an expected-memory-bounds fix. Its consumer is synthetic: the cases drive a modeled consumer and execute no stock instructions, so this evidence is a navigation and claim check, not a stock-chain result. The F1/F2 source defects are closed (section 6); the eight harness boundary cases are not.
- Boot publication: the source-copy store-pass boundary stays conditionally accepted under the named stable-source coherence prerequisite, and builder emission and provisional placement remain outside that source-level acceptance (E-AUDIT-SET-CLOSED-150). This candidate carries no boot-carve write (section 3).
- Placement: three marker-0 observations are missing - the driver's claim behaviour through the runtime-populated vtable at `0x0096bd20` and its struct method fields, the creation-site population and id ranges, and a hook-time table or last-created-id capture; no proven-free id exists and placement stays provisional on the gap-1 carrier. The read plan is review-closed but owner-gated (E-PLACEMENT-MARKER0-LIMIT-150, E-MARKER0-READ-PLAN-150, `reports/marker0-read-plan.md`).
- Recorder: revision-2 emission, its fixture and the native-suite results are complete host-side; the camera-side capture session consuming `fixture-rev2` is the open owner-run observation (E-RECORDER-EMIT-150).
- Camera-side and acceptance boundaries untouched here: the compiled rows' rendering, selection and persistence on the body, the bank's installed behaviour, and the install decision (section 7).

## 9. The review package

Read, in this order: this report; [the D3 patch report](recipe-patch-d3.md) sections 2-5 and 10 for the write set and the hook; [the recipe design](recipe-design.md) sections 15-20 for the bank, placement, menu and label interfaces, with private integration provenance in `private-data/analysis/x-m1-1.50/recipes/bank-integration-contract.md` (the public reports do not reproduce that private contract); [the rebuild contract](update-container-rebuild.md) section 8 and [the image-2 record operation](image2-record-operation.md) for the container operations.

Verify against these inputs and hashes: `bank-spec.json` and `d3-compiled.json` (the frozen specs), `builder-out/bank-plan.json` plus the 127 `span-*.bin` files and the five builder outputs of section 1, `d3-compiled/*` (hook and manifests), the container chain of section 4 with `chain.log` and `combined-candidate.json`, and the case evidence `recipes/runtime/kernel-cases/{results.json,run-provenance.json,findings.txt,d3-*/}`. The sources: `host/firmware/BuildRecipeBank.hs`, `BuildCompiledRecipes.hs`, `ApplyPatchSet.hs`, `RebuildUpdate.hs`, `host/FujiAF/RecipeBank.hs` and `UpdateContainer.hs`, `menu/recipe_compiled.S` with `recipe_compiled.ld`/`recipe_compiled_layout.S`, the six `menu/recipe_bank_*` modules, and `emulation/RecipeBankCases.hs` with its build script; `run-provenance.json` pins the module digests the acceptance run linked.

The recorded-hash delta is the code-blob lineage of section 2; every other recorded hash matches and the plan and container are unchanged by the last two source revisions. The acceptance state and every remaining residual are section 8; the review's open questions are the D3 report's residuals, the bank's placement and boot publication, and the owner's install decision, none of which this rebuild closes.
