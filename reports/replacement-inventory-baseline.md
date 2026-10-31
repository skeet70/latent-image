# Replacement inventory and iteration baseline

Status: host-only inventory and measured baseline for [epic #117](https://github.com/skeet70/latent-image/issues/117) and [#118](https://github.com/skeet70/latent-image/issues/118). No camera or USB device was opened, no firmware was installed, and no live operation was run. The inventory reuses existing records; it does not select a pilot target, add a runner or define a replacement schema. [Section 8](#8-current-ownership-and-retirement-view) is the current ownership and retirement view maintained for [#127](https://github.com/skeet70/latent-image/issues/127); sections 1 to 7 are the #118 baseline and first-wave record.

Evidence labels follow [the principles](../docs/principles.md): **byte-read**, **static-inference**, **emulated**, **hardware-observed**, **reported** and **independently-reproduced** are inherited from the named ledger entries and reports. A statement without one of those labels is a repository or command fact observed in this pass.

## 1. Evidence basis and limits

The inventory reads the pinned X-M1 1.50 artifacts and the existing reports and ledger only. The relevant starting records are:

- Firmware correspondence and boot: E-G2-ACCEPTANCE-150, E-XM1-MPU-BOOT-RECORD-150, E-BOOT-LOADPATH, E-FUJIMOD-XM1-FLASH-BOOT-150.
- Recipe bank and Classic Chrome: E-RECIPE-BANK-V2-CODEC-150, E-RECIPE-BANK-PLAN-FREEZE-150, E-RECIPE-KERNEL-ACCEPT-150, E-RECIPE-BANK-SELECTION-150, E-RECIPE-BANK-DIAL-150, E-CC-BANK-FORMAT-3-150, E-CC-BANK-RAM-150, E-CC-LIVEVIEW-FRAGMENT-150, E-CC-ICON-RAM-PUSH-SESSION-150.
- Emulator and host entry points: E-XM1-EMU-CHECKPOINT-KEYS-150, E-VERIFY-CANDIDATE-150, E-FUJIMOD-RUNTIME-DECISION-150, E-HOST.
- Settings, recovery and interfaces: E-CUSTOM-SETTING-MAP-150, E-RAM-CALIBRATION-150, E-RECOVERY, E-STAGED-RECOVERY-SUCCESS-150, E-TRANSPORT, E-NATIVE-USB, E-BULK-READ-E2E-150.

The reports that own the detail are [firmware extraction](firmware-extraction.md), [full-system emulation](full-system-emulation.md), [emulator runtime decision](emulator-runtime-decision.md), [validation](validation.md), [recipe design](recipe-design.md), [compiled recipes](compiled-recipes-builder.md), [Classic Chrome patch](classic-chrome-patch.md), [Classic Chrome icon](classic-chrome-icon.md), [custom settings](custom-settings.md), [recovery status](recovery-status.md) and [transport](../transport/README.md).

Limits:

- Historical recipe-bank and Classic Chrome records supply build paths, retained interfaces, pass counts and artifact identities. No historical duration or effort was reconstructed, and none is used as a measurement below.
- The coarse outline names unknowns as unknowns. It does not recover every mode, lens, condition or dependency; that belongs to a selected pilot and its prerequisites.
- Emulated, full-system, hardware-observed and owner-reported results are not interchangeable. A passing native case is bounded function execution, not scheduler, DMA, cache or peripheral evidence.

## 2. Coarse capability outline

| Capability | Observed result or reusable mechanism | Evidence | Unknowns and qualification gaps |
| --- | --- | --- | --- |
| Startup and shutdown | Stock 1.50 boots in the retained engine and in FujiMod; the flash-boot route reaches the shooting screen; the decoded images match the installed bytes over the accepted byte-equal ranges, with the 781 excluded differences named by G2. RAM payloads inherit stock startup. Orderly power-off persistence and jig-cycle reset origins are mapped. The End Setting candidate was rejected. | E-G2-ACCEPTANCE-150, E-FUJIMOD-XM1-FLASH-BOOT-150, E-XM1-MPU-BOOT-RECORD-150, E-BOOT-LOADPATH, E-END-SETTING-150-REJECTED, E-RESET-ORIGIN-BOOT-150 | Replacement startup is not demonstrated. SD cold entry is timing-dependent and not a reproducible recipe. Internal-flash deployment, replacement shutdown and general startup recovery are unproven. |
| Menus and controls | The viewer navigates language, shooting menus, SETUP and Q with host keyboard input; twelve calibrated ADC3 positions select stock shooting modes; the input-row table and key sampling path are mapped. Recipe-bank pages, dial handling and selection have bounded UI cases. | E-XM1-VIEWER-MENUS-150, E-XM1-VIEWER-CONTROLS-150, E-XM1-VIEWER-ISOLATED-150, E-INPUT-ROWS-150, E-XM1-MAIN-DIAL-ADC4-SOURCE-150, E-XM1-KEY-SAMPLING-PATH-150, this report section 5 | Full menu coverage, physical key timing and unexercised surfaces are unknown. The bank UI drawer behind the stock tail is modeled, so markers are checked through stock snapshot accessors rather than drawn pixels. |
| Live view | The viewer produces guest live view at 720x480 from a synthetic scene. The Classic Chrome live-view fragment passes 46/46 site cases and 368 total site executions, and emits the expected develop-record marker in full-system segments. | E-XM1-VIEWER-MENUS-150, E-CC-LIVEVIEW-FRAGMENT-150, E-CC-ICON-B-COMBINED-150 | Physical panel output, sensor-derived live view and color fidelity are unproved. The post-power-on stored-film timing is not established. RAW conversion playback stops on a known MMIO gap. |
| Focus | The stock AF orchestration, lens messages, statistics producers and input path are mapped. An owned contrast policy and AF RAM-ring e2e exist. | E-AF-MENU-MAP, E-AF-CORE-MAP, E-AF-LENS-MAP, E-XM1-AF-PRODUCER-SNAPSHOT-150, E-AF-RAM-RING-E2E-150, E-AF-CARDLESS-S1-BLOCKED-150 | A captured stock AF search is not qualified by the cited evidence; the cardless S1 path is blocked by the reported card error. Lens-dependent and physical closed-loop behavior remains unmeasured. No owned AF capability is qualified. |
| Exposure and capture | Capture and develop descriptors, completion slots and diagnostic selectors are mapped. The viewer drives S1 and S2 through the source-bound GPIO/IRQ input stages, and a bounded stock S1 admission trajectory exists. Owner hardware captures and ordinary still/playback reports exist. | E-XM1-VIEWER-INPUT-SOURCES-150, E-XM1-S1-ADMISSION-EMU-150, E-CARDLESS-RAM-CAPTURE-MAP-150, E-CARDLESS-PRODUCER-READ-REACH-150, E-CARD-DETECT-LIVE-150, E-AF-CARD-MEDIA-PRODUCER-150 | Sensor-backed capture and the full exposure effect chain are not qualified in emulation or by owned code. Physical capture remains owner-reported, and the body's card-detect input reads present with no card. |
| Development | Film-simulation offers, develop dispatch and Classic Chrome/D3 parameter data are mapped. D3 rows, the Classic Chrome flash candidate, the icon candidate and the RAM payloads build and pass their native/full-system cases. Render repeatability has a hardware reference analysis. | E-FILMSIM-MAP, E-FILMSIM-EMUL-150, E-RECIPE-D3-PATCH-150, E-CC-BANK-RAM-150, E-CC-ICON-B-COMBINED-150, E-FILMSIM-RENDER-REPEAT-ANALYSIS-150, E-XM1-MATCHING-C-PILOT-150 | Emulator develop rendering and RAW conversion are not executed. The Classic Chrome color model is a sibling-derived approximation, not X-M1 ground truth. Instruction counts are not physical latency. |
| Save and playback | The recipe bank v1/v2 codecs, save, reset, rename, selection and slot reload have bounded kernel/UI cases. Settings persistence through area 2 and the C-slot record is mapped. RAM unload and cold restart restore old words. Playback state and a modal path are mapped. | E-RECIPE-BANK-V2-CODEC-150, E-RECIPE-KERNEL-ACCEPT-150, E-RECIPE-BANK-SELECTION-150, E-CC-BANK-RAM-150, E-PLAYBACK-A4D-DEAD-150, E-SETTINGS-ACTS-150 | Emulator playback and RAW conversion are blocked. Playback rendering, zoom and card-backed save are not covered. Physical save/playback evidence remains owner-reported. |
| Settings and calibration | The custom-slot record, payload fields, area-2/area-3 maps, adjustment cache, `f7` gate, settings reset cause and calibration capture exist. Durable 1.00 and 1.50 adjustment-area backups exist, and the recorder v5 rollback is exact for its scope. The ram-calibration driver reaches a known menu state. | E-CUSTOM-SETTING-MAP-150, E-CUSTOM-ISO-150, E-SETTINGS-ACTS-150, E-ADJ-GATE-BYTE-150, E-RESET-ORIGIN-BOOT-150, E-RAM-CALIBRATION-150, E-S03-DURABLE-PRESERVATION, E-ADJ-BACKUP-DURABLE-150, E-RECOVERY-FRAGMENTED-V5-STATUS20-150 | Calibration restore, a complete physical-storage image, arbitrary interrupted-flash rollback and damaged-ROM/controller recovery are not qualified. Some fields and writers remain unbound. Jig-cycle reset and persistent-write hazards bound what a session may claim. |
| Interfaces | The ff80 USB transport supports identify, read-config, probe, dump, read-bulk, read-media, RAM push and unload. SD host, PTP read-only storage and jig mode are mapped. The bank ELF allowlist and RAM-push review records exist. | E-TRANSPORT, E-NATIVE-USB, E-BULK-READ-E2E-150, E-RAM-PUSH-MECHANISM-CONVERGE-150, E-PTP-STORAGE-READONLY-150, E-USB-TOGGLE-RAM-PUSH-150 | USB toggle detach/re-enumeration blocks remain. PTP storage is read-only apart from deletion. Jig persistence hazards remain. FujiMod route parity is comparison evidence, not a gating cutover. |
| Failure recovery | R1 is accepted, R2 and R3 are demonstrated by SD marker/bootstrap and the exact recorder v5 rollback, and a source-bound scheduler snapshot executes. Recovery card builders are host-only. | E-RECOVERY, E-SDBOOT-ENTRY-TIMING-150, E-STAGED-RECOVERY-SUCCESS-150, E-RECOVERY-TASK-STATE-EXEC-150, E-RECOVERY-TASK-STATE-BINDING-150 | Formal R4 is unclaimed. There is no general cold-entry recipe, arbitrary interrupted-flash recovery, calibration restore or damaged-ROM/controller recovery. |

## 3. Build and verification entry points

| Entry point | Public command | Evidence scope | Existing evidence |
| --- | --- | --- | --- |
| Host suite | `devenv shell -- bazel test --lockfile_mode=error --repository_disable_download --nocache_test_results --test_output=errors //...` | Host-only schemas, strict JSON, providers, native runner, transport tests, AF model and builders. Transport tests inject `UsbApi`, except `//transport:native-usb-tests`, which enumerates the host bus with no camera claimed or opened. | E-HOST, [validation](validation.md) |
| Native runner | `devenv shell -- bazel build --jobs=2 //emulation:native-runner` then `bazel run //emulation:native-case -- --runner bazel-bin/emulation/native-runner CASE_JSON` | One bounded function closure on the closed Cortex-R5F A32 profile; exact registers, final memory and stub events; no scheduler, interrupt, DMA, cache or peripheral model. | [emulation README](../emulation/README.md), E-AREA3-VALIDATOR-NATIVE-150 |
| Recipe kernel cases | `devenv shell -- bazel build --jobs=2 //emulation:native-runner //emulation:recipe-bank-cases //emulation:recipe-bank-interop` then `devenv shell -- bash emulation/build-recipe-bank-cases.sh` | Builds the six bank modules and the synthetic media model, then runs bounded kernel cases. Synthetic media, not a card or a display. | E-RECIPE-KERNEL-ACCEPT-150 |
| Recipe UI cases | `devenv shell -- bazel build --jobs=2 //emulation:native-runner //emulation:recipe-bank-ui-cases //host/firmware:build-recipe-bank` then `BUILD_DIR=... OUT_DIR=... JOBS=2 devenv shell -- bash emulation/build-recipe-bank-ui-cases.sh` | Modal, navigation and drawer boundary modeled; stock property getter, setter, commit dispatcher and consumer cells execute stock 1.50 instructions. No physical input or display. | this report section 5, [emulation README](../emulation/README.md#recipe-bank-runtime-scope) |
| UI plan binding | `devenv shell -- bazel test --jobs=2 --nocache_test_results //emulation:recipe-bank-ui-plan-tests` | Refuses stale callback pointers and unbound code pointers in the bank plan. Host-only. | [emulation README](../emulation/README.md#recipe-bank-runtime-scope) |
| Firmware builders | `bazel run //host/firmware:build-recipe-bank`, `//host/firmware:build-recipe-package`, `//host/firmware:apply-patch-set`, `//host/firmware:rebuild-update` | Host-only plan, patch, decode, re-encode and container checks. Artifact identity, not installation. | E-RECIPE-BANK-PLAN-FREEZE-150, E-RECIPE-PACKAGE-HASKELL-150 |
| Classic Chrome builders | `bazel run //host/firmware:build-classic-chrome`, `//host/firmware:build-classic-chrome-ram`, `//host/firmware:build-classic-chrome-icon-ram` | Host-only flash and RAM candidate construction, with pinned parts, digests and reviewed publication contracts. Installation, camera runs and RAM push remain separate operations. | E-CC-BANK-RAM-150, E-CC-ICON-B-COMBINED-150, E-CC-BANK-FORMAT-3-150 |
| Extension build | `devenv shell -- bazel run //extensions:build-extension -- EXTENSION_JSON STOCK_DAT OUTDIR` | Builds a patched `FWUP0002.DAT` from a manifest, checks input and output hashes. No camera I/O and no install. | [extensions README](../extensions/README.md) |
| RAM push and allowlist | `bazel test //transport:ram-push-review-verdicts //transport:ram-push-review-bytes`, `bazel run //transport:camera -- ram-push MANIFEST OUTDIR` | The build path pins and relinks; the camera command is a live operation under the camera-run procedure. The bank allowlist checks the relinked bank part. | [transport README](../transport/README.md#ram-push-review-record) |
| Full-system emulator | `bazel run //emulation:xm1-viewer -- --firmware DAT --port 0`, `bazel run //emulation:verify-candidate -- [--routes NAME,...]` | Retained engine gates the full-system routes; FujiMod counterparts are comparison-only until a reviewed cutover. Interactive viewer uses a synthetic scene. | E-VERIFY-CANDIDATE-150, E-FUJIMOD-RUNTIME-DECISION-150 |
| Flash boot checks | `bazel run //emulation:flash-boot-check`, `bazel run //emulation:fujimod-flash-boot-check` | Cold boot of a candidate to a known screen over the emulated route. Not hardware boot or flash deployment. | E-FUJIMOD-XM1-FLASH-BOOT-150, E-CC-ICON-B-COMBINED-150 |
| Recovery builders | `bazel run //host/firmware:build-recovery-card`, `//host/firmware:build-staged-recovery-card` and the related card targets | Host-only card construction; physical entry remains separately evidenced. | E-RECOVERY, [recovery status](recovery-status.md) |
| Matching reference | `bazel test //matching:...` with the pinned GCC 4.4.7/4.6.4 from `build-support/exr2-gcc.nix` | Function-extent byte comparison only. It is not the new-code toolchain and establishes no new behavior. | E-XM1-MATCHING-C-PILOT-150 |

The C/Clang recipe-bank path uses `clang --target=armv7r-none-eabi -march=armv7-r -marm -Oz`, LLD and `llvm-objcopy`. The current full-system gate is the retained engine. FujiMod is pinned and runs the comparison routes, but the [runtime decision](emulator-runtime-decision.md) and the incomplete route matrix bound its use.

## 4. Fixed comparison workload

The comparison workload is fixed before any pilot changes the workflow. It is a behavior-preserving refactor of the recipe-bank UI token and page decoder in `menu/recipe_bank_ui.h`. The same workload is repeated by [#121](https://github.com/skeet70/latent-image/issues/121) when its increment replaces or moves the decoder.

### 4.1 Public behavior that must not change

`rb_ui_page(cls, slot_out)` returns:

| Class | Result |
| --- | --- |
| `0x281` | `RB_PAGE_SLOT` |
| `0x270` to `0x276` | `RB_PAGE_SUB`, and `*slot_out = cls - 0x270` when `slot_out` is non-null |
| `0x285` | `RB_PAGE_SELECT` |
| `0x131` | `RB_PAGE_EDITOR` |
| `0x277` | `RB_PAGE_EDIT` |
| `0x27d` | `RB_PAGE_DIALOG` |
| `0x267` to `0x26f` | `RB_PAGE_FIELD` |
| anything else | `RB_PAGE_NONE` |

`slot_out` is not written for any other class. `rb_ui_decode(token)` returns:

| Token | Result |
| --- | --- |
| `0x40` to `0x5b` | `RB_ITEM_VERB`, `slot = (token - 0x40) >> 2`, `verb = (token - 0x40) & 3` |
| `0x60` to `0x67` | `RB_ITEM_SELECT`, selection code `token - 0x60` |
| anything else | `RB_ITEM_NONE` with zero slot, verb and selection |

The refactor replaces the paired comparisons with one `always_inline` unsigned range test, `value - first < count`. That is equivalent for all `uint32_t` values for the constants used here because none of the tested spans wraps.

### 4.2 Fixed patch

The baseline patch is kept separately so it is not part of the production change:

```diff
diff --git a/menu/recipe_bank_ui.h b/menu/recipe_bank_ui.h
index 0bce6d4..61d277b 100644
--- a/menu/recipe_bank_ui.h
+++ b/menu/recipe_bank_ui.h
@@ -251,13 +251,19 @@ typedef enum {
   RB_PAGE_FIELD
 } RbBankPage;
 
+static inline __attribute__((always_inline)) bool rb_ui_range_contains(
+  uint32_t value, uint32_t first, uint32_t count)
+{
+  return value - first < count;
+}
+
 /* Page kind of a frame class; RB_PAGE_SUB also yields the slot. */
 static inline RbBankPage rb_ui_page(uint32_t cls, uint8_t *slot_out)
 {
   if (cls == RB_UI_CLASS_SLOT) {
     return RB_PAGE_SLOT;
   }
-  if (cls >= RB_UI_CLASS_SUB0 && cls < RB_UI_CLASS_SUB0 + RB_BANK_SLOTS) {
+  if (rb_ui_range_contains(cls, RB_UI_CLASS_SUB0, RB_BANK_SLOTS)) {
     if (slot_out != 0) {
       *slot_out = (uint8_t)(cls - RB_UI_CLASS_SUB0);
     }
@@ -275,7 +281,8 @@ static inline RbBankPage rb_ui_page(uint32_t cls, uint8_t *slot_out)
   if (cls == RB_UI_CLASS_DIALOG) {
     return RB_PAGE_DIALOG;
   }
-  if (cls >= RB_UI_CLASS_FIELD0 && cls <= RB_UI_CLASS_FIELD_LAST) {
+  if (rb_ui_range_contains(cls, RB_UI_CLASS_FIELD0,
+                            RB_UI_CLASS_FIELD_LAST - RB_UI_CLASS_FIELD0 + 1u)) {
     return RB_PAGE_FIELD;
   }
   return RB_PAGE_NONE;
@@ -288,16 +295,15 @@ static inline RbBankItemRef rb_ui_decode(uint32_t token)
 {
   RbBankItemRef item = { RB_ITEM_NONE, 0u, 0u, RB_SELECTION_NONE };
   uint32_t u;
-  if (token >= RB_UI_TOKEN_VERB_BASE &&
-      token < RB_UI_TOKEN_VERB_BASE + RB_BANK_SLOTS * RB_UI_VERBS_PER_SLOT) {
+  if (rb_ui_range_contains(token, RB_UI_TOKEN_VERB_BASE,
+                            RB_BANK_SLOTS * RB_UI_VERBS_PER_SLOT)) {
     u = token - RB_UI_TOKEN_VERB_BASE;
     item.kind = RB_ITEM_VERB;
     item.slot = (uint8_t)(u >> 2);
     item.verb = (uint8_t)(u & 3u);
     return item;
   }
-  if (token >= RB_UI_TOKEN_SELECT_BASE &&
-      token < RB_UI_TOKEN_SELECT_BASE + RB_UI_SELECT_ROWS) {
+  if (rb_ui_range_contains(token, RB_UI_TOKEN_SELECT_BASE, RB_UI_SELECT_ROWS)) {
     item.kind = RB_ITEM_SELECT;
     item.selection = (RbSelection){(uint8_t)(token - RB_UI_TOKEN_SELECT_BASE)};
     return item;
```

The patch above is the complete baseline change. It adds 12 lines and deletes 6 in `menu/recipe_bank_ui.h`. It changes no public symbol or function signature.

### 4.3 Expected result fixed before measurement

- The three fixed targets build successfully.
- `emulation/build-recipe-bank-ui-cases.sh` generates the same 55 case IDs and every case reports `status: passed` and `runner_status: returned`.
- Negative scenarios such as `n-refused-push`, `n-save-write-fail` and `n-select-unset-refused` keep `passed`; they are expected refusals inside a passing case, not failing cases.
- `//emulation:recipe-bank-ui-plan-tests` passes.
- The public caller interface used by the plan and cases keeps its names and behavior. Code layout may change; record any symbol address, size or ordering change rather than treating it as a failure.

`native_case_status` is expected to remain `failed` for every case: the native case compares whole memory and stub-event order, while the UI comparator excludes declared ABI workspaces and incidental stub order. The UI `status` and the public claims are the behavior assertion. Do not replace this field check with per-run bytes.

### 4.4 Manual steps and interfaces

1. Read `menu/recipe_bank_ui.h` for the class and token domains, `menu/recipe_bank_actions.c` for the call sites, and `menu/recipe_bank_allowlist.json` for the `slot_out` store declaration.
2. Read `emulation/build-recipe-bank-ui-cases.sh` for the build and case command, and `emulation/RecipeBankUiCases.hs` for the result comparison.
3. Apply the patch, build the kernel, model and UI model, bind the plan, generate cases and run the native runner.
4. Compare the 55 case IDs, `status`, `runner_status`, `native_case_status`, `mismatches` and `native_case_mismatches` against the unmodified tree.
5. Restore the temporary patch; do not commit the baseline change.

Interfaces and artifacts involved: the bank plan `bank-plan-ui.json`, `kernel.bin` and `kernel.syms`, the model and UI model binaries, the `results.json` and `findings.txt` outputs, and the `native-runner` binary. Inputs are the pinned candidate image 4 and 5, `reports/custom-settings.fields.json`, `RB_BASE=0x00be1f00` and `RB_NAME_RECORD=0x00beb784`.

## 5. Baseline measurement

Measured 2026-10-27 local (2026-10-28 UTC) on the host, before any pilot workflow change.

| Item | Value |
| --- | --- |
| Machine | AMD Ryzen 7 7800X3D 8-Core Processor, 16 threads, 30 GiB RAM |
| OS | Linux 7.2.3-ogc3.1.fc44.x86_64 |
| Toolchain | clang 21.1.8 (`armv7r-none-eabi`, `-march=armv7-r`, `-marm`, `-Oz`), LLD 21.1.8, LLVM objcopy/nm/readelf 21.1.8, GHC 9.10.3, Bazel 7.6.0, devenv 2.2.2 |
| Engine | bounded `//emulation:native-runner`, Unicorn 2.1.4, closed Cortex-R5F ARM little-endian profile; not a full-system engine |
| Cache state | Nix store and Bazel output base `/home/murph/.cache/bazel/latent-image-issue117-inventory` warm; first target build reported 54 disk cache hits and 196 internal actions; no remote cache; no network fetch during the timed runs; case output directories were fresh; OS page cache warm |
| Inputs | pinned `package-20261024b/image` image 4 and image 5, `reports/custom-settings.fields.json`, `RB_BASE=0x00be1f00`, `RB_NAME_RECORD=0x00beb784` |
| JOBS | 2 for the bank build and case execution |

| Measurement, same script invocation | Wall time |
| --- | ---: |
| Unmodified tree, total | 45.29 s |
| Unmodified tree, build phase (outer start to case start) | 1.07 s |
| Unmodified tree, case generation and native execution (case start to case end) | 44.22 s |
| Refactored tree, total | 42.58 s |
| Refactored tree, build phase | 0.98 s |
| Refactored tree, case generation and native execution | 41.59 s |

| Other samples, whole run unless stated | Wall time |
| --- | ---: |
| Three-target tooling build (`native-runner`, `recipe-bank-ui-cases`, `build-recipe-bank`) | 4.56 s (250 actions, 54 disk cache hits) |
| Unmodified tree, earlier whole-run sample | 42.22 s |
| Refactored tree, earlier whole-run sample | 42.07 s |
| Refactored tree, `git apply` patch to all 55 results | 43.73 s |
| UI plan test, Bazel wall including sandbox | 0.24 s to 0.49 s; test itself 0.0 s |

The phase rows for each tree come from one invocation. The build phase is the outer start to the first case-generator call; the execution phase is that call to the generator's end. They add to the total within 0.001 s. They are not an additive decomposition across runs: earlier build-only samples (1.07 s unmodified, 0.98 s refactored) and case-only samples (41.54 s, 41.13 s) were independent invocations, and their sums exceed the matching whole-run totals by 0.39 s and 0.04 s. Use the same-run rows when the separation matters.

The unmodified instrumented sample is 3.07 s above its own earlier whole-run sample and 2.72 s above the refactored instrumented sample; the two earlier whole-run samples differ by 0.15 s. Treat that instrumented difference as load variation, not a refactor effect. The 43.73 s patch-to-result sample also ran under sibling load. Case execution dominates, and the one-time Nix toolchain acquisition and initial Bazel analysis are not counted as iteration cost. The source-change-to-result path is about 42.6 s in the refactored same-run sample, or 43.7 s including patch application in the separate whole-run sample.

Investigation effort, recorded rather than reconstructed:

- Six artifacts were read for the workload: the UI header, the action call sites, the allowlist store declaration, the UI case build script, the UI case harness and the transport review-record section.
- The decoder's class and token domains were already stated in the header, so no stock hook, call-site ABI or plan layout was rediscovered.
- Two source edits produced a buildable patch on the first attempt; no compile or case iteration was needed.
- The investigation was not separately clocked because it was interleaved with this inventory. The mechanical patch-to-result interval is the measured 43.73 s above.

Behavior comparison:

- The kernel binary changed bytes but kept the same 31,788-byte size.
- `kernel.syms` has 194 defined code symbols in both builds, with no added or removed symbol. `rb_ui_page` remains at `0x00be71c0` size 160. `rb_bank_interpose`, `rb_stage_edit_get_arm`, `rb_stage_edit_set_arm`, `rb_select_values` and `rb_select_labels` keep their addresses and sizes. This is a recorded observation for this C refactor, not a qualification requirement; a Rust replacement may change internal layout and only records the delta.
- All 55 case result objects compare equal between the unmodified and refactored trees. That includes `status`, `runner_status`, `native_case_status`, `mismatches`, `native_case_mismatches`, public claims, initial state, final state and stub events.
- 55 of 55 cases are `passed` with `runner_status: returned` in both trees. The underlying `native_case_status` is `failed` for all 55 in both trees for the declared ABI-workspace and stub-order differences; it is not a regression from the refactor.

Unknowns and confounders:

- The cached toolchain, Bazel disk cache and warm page cache make the 1 s build a warm incremental figure, not a cold-machine figure.
- Case execution uses two jobs; sibling work and CPU frequency moved the same-run execution phase from 41.59 s to 44.22 s across the two instrumented samples. Report the cache state, load and case count with every repeat.
- The 55 cases cover the modeled UI boundary and the declared stock cells, not physical input, display rendering or full-system scheduling.
- The refactor's equivalence is argued from the unchanged behavior table and the identical case results. It does not prove the two instruction sequences are equivalent on unexercised inputs.

## 6. Comparison protocol for #121 and the first-wave checkpoint

Use the same fixed workload, inputs, machine and engine. Change only the decoder implementation. Record each of the following.

1. Record `git rev-parse HEAD`, the input image hashes and the toolchain versions.
2. Build the three fixed targets and record the wall time, action count and cache state:

   ```sh
   devenv shell -- bazel --output_base="$HOME/.cache/bazel/latent-image-issue117-inventory" build --jobs=2 //emulation:native-runner //emulation:recipe-bank-ui-cases //host/firmware:build-recipe-bank
   ```

3. Run the fixed workload into fresh output directories and record build and execution within the same invocation. Use a generator wrapper that logs the timestamp of its first case call and the timestamp when it returns; build is the outer start to that first call, execution is that call to its end, and the two add to the total. Do not add separate build-only and case-only invocations as an additive decomposition. The full command is:

   ```sh
   BUILD_DIR="$PWD/private-data/analysis/x-m1-1.50/recipes/issue117-inventory/ui-repeat-build" \
   OUT_DIR="$PWD/private-data/analysis/x-m1-1.50/recipes/issue117-inventory/ui-repeat-cases" \
   JOBS=2 devenv shell -- bash emulation/build-recipe-bank-ui-cases.sh
   ```

4. Run the plan gate:

   ```sh
   devenv shell -- bazel --output_base="$HOME/.cache/bazel/latent-image-issue117-inventory" test --jobs=2 --nocache_test_results --test_output=errors //emulation:recipe-bank-ui-plan-tests
   ```

5. Compare the result sets by case ID and the fields `status`, `runner_status`, `native_case_status`, `mismatches` and `native_case_mismatches`. A convenient projection is:

   ```sh
   nix shell nixpkgs#jq -c jq -S '.cases | map({id, status, runner_status, native_case_status, mismatches, native_case_mismatches})' results.json
   ```

6. Report whether the decoder change required rediscovering stock hooks, call-site ABIs, plan offsets or the `slot_out` store contract. Record the exact files and symbols consulted.
7. Report measured changes only against the same cache, machine, toolchain, engine, case count and input state. If any of those changed, label the run as a changed condition and do not present the difference as an improvement.
8. Keep emulator packaging, device-driven input and full-system acceptance separate from this host/native loop. #121's emulator evidence is additional evidence, not part of the build/execution baseline.

Expected repeat result at this fixed workload: the same 55 case IDs, every case `passed` with `runner_status: returned`, the plan test passing, and the public caller interface preserved. A Rust implementation behind the existing C ABI may change internal layout; record address, size and ordering changes as observations and do not fail the workload on layout alone. If it adds cases, report the added cases separately rather than mixing them into the 55-case timing.

The first-wave checkpoint applies the same rule to the measured cost. A lower time is an improvement only when the workload, machine, toolchain, engine, input state and cache state are the same. Otherwise the checkpoint records the changed condition and does not credit the difference to the replacement work.

## 7. First-wave checkpoint

Decision: continue with owned controller, lifecycle and startup boundaries. Do not spend the next wave only on detached leaf ports or blanket C rewrites.

The matched C/Rust table in [the ownership report](recipe-bank-ui-ownership.md#measurement) does not demonstrate an end-to-end verification speedup. Its comparison values stay in that table. Credit the exercised source-level migration, ABI reuse and consumed contracts, and leave verification-time improvement unclaimed. Firmware investigation time remains unknown where it was not separately measured.

The first wave qualifies bounded toolchain, validator, service and UI paths. It does not qualify camera-equivalent application/runtime behavior or startup. #121 is the bounded pilot and is qualified; #117 full application/runtime/startup milestones remain open. #85 remains hardware blocked and persistence is unpursued.

Exposed dependencies:

- preservation observation separates identity CPU stores from changed stores;
- the direct `Cpu::write_memory` host-write hole was fixed at the SDK boundary;
- natural mode and overlay-entry assumptions remain explicit;
- retained modal, render, storage, scheduler, init and resource behavior still requires contracted retirement.

Canonical SDK evidence and native C runner evidence remain separate; neither substitutes for the other.

## 8. Current ownership and retirement view

This view is maintained for [#127](https://github.com/skeet70/latent-image/issues/127) and uses the capability categories it accepts. Retirement criteria are in the accepted design's [ownership stages](../spec/11-incremental-firmware-replacement.md#ownership-stages); acceptance and dependencies of open work are in the linked issues. An unknown entry is an open obligation. This view does not complete #127 or any #117 milestone.

Terms: **owned, qualified** is owned code with a published qualification record. **Retained executable** is stock code that an owned path calls, or that runs because nothing replaces it. **Shared data** is stock cells, tables and records that owned code reads or writes. **Initialization and resources** are setup inherited from stock and resources the payload holds. Source increments under an open issue are named by issue only and are not qualified.

### 8.1 Qualified owned increments

| Increment | Owned | Retained | Runtime position |
| --- | --- | --- | --- |
| [#119](https://github.com/skeet70/latent-image/issues/119) Rust build and ABI | Pinned `no_std` cross-build; AAPCS facts for integer, pointer and in-memory composite boundaries ([record](../replacement/abi/README.md), E-XM1-RUST-ABI-150) | No stock boundary. Float, variadic and retained-service ABIs are not qualified by it. | Build and bounded native case |
| [#120](https://github.com/skeet70/latent-image/issues/120) area-3 validator | Rust replacement of `0x006a84d4`, equivalent on the selected record's complete single- and two-bit domain and 56 constructed cases ([record](../replacement/validator/README.md), E-AREA3-VALIDATOR-REPLACEMENT-150) | In firmware: the stock validator, write callback `0x006a8484`, parity builder `0x006a83f4` and the area-3 log registration at `0x006a8abc` ([custom settings](custom-settings.md)) | Native harness only; installed in no route. Save-path integration needs persistent-storage acceptance. |
| [#123](https://github.com/skeet70/latent-image/issues/123) field-row lane | Typed field-row binding in `replacement/service` ([record](../replacement/service/README.md), E-SERVICE-PACKAGED-RETAINED-150) | `FUN_00d52684` (`0x6038` bytes) with its callees, tables and epilogue; the installer and publication mechanism | Packaged FujiMod route; the stock probe executes for every row |
| [#121](https://github.com/skeet70/latent-image/issues/121) recipe-bank classifiers | Page, slot and token classification behind the unchanged C ABI ([record](recipe-bank-ui-ownership.md), E-RECIPE-UI-PRIMARY-PRESERVATION-150, E-RECIPE-UI-VISUAL-150) | Dispatcher `FUN_00bfb028`; modal-75 actions `FUN_00c72e10`, `FUN_00c75ed4`, `FUN_00c76650`, `FUN_00c77794`; transition `FUN_00c12aac`; redraw, draw, storage and probe paths (E-RECIPE-MODAL-CONTRACT-150) | Packaged FujiMod primary over a 450-byte preserved range |

The published qualified increments retired only project C, the inline classifier if-chains; none retires a stock executable body. Each runs with its stock dependencies in the packaged route or is not integrated into one.

### 8.2 Payload dependency surface

The [bank ELF allowlist](../transport/README.md#bank-elf-allowlist) machine-checks the owned payload's direct stock surface. For the qualified #121 packaged artifact at the first-wave checkpoint, `menu/recipe_bank_allowlist.json` declared 73 stock call targets, 12 of them Classic Chrome, and 9 stock store ranges, 1 of them Classic Chrome. Call targets include instruction-built `movw`/`movt` addresses, and pointer stores the checker cannot prove are declared under `unproven`. That qualified route published 222 old words at install and restored them at unload; the 122,008-byte allocation stays held until power-off. RAM payloads run in stock task context after stock initialization. These counts describe that artifact, not later source.

The allowlist bounds what the payload calls and stores. It does not cover stock callers into a retained body, callbacks registered with stock services or computed references to them. No qualified bank payload establishes that closure or a forbidden-fetch guard for a retained body. The separate loader-B prefix positives below exercise a scoped core-1 forbidden-fetch guard, not global caller or exception closure.

For the global dispatcher `FUN_00bfb028` ([#128](https://github.com/skeet70/latent-image/issues/128)), the caller, event-store and MOVT-limit obligations in the checker inventory `closure-inventory-1.json` (checker `9804fb2`, not admitted) arise where a candidate entry splits a straight-line window after the instruction that sets the value: an aligned word equal to a window start, or a B/BL-shaped word targeting it, in one of the five pinned images. The inventory carries each data-word origin; E-MODAL-ENTRY-SOURCES-150 checks them against the bytes and locates the MOVT-limit splits.

One source class has a consumer path in the bytes. Image 4 holds three register-write streams of (value, register) pairs at `[0x007a2430, 0x007b0838)`, `[0x007b0840, 0x007c2d18)` and `[0x0081c630, 0x00820f10)`, submitted through `0x006598ec` to the engine at `0xfe80c1c0`. Value slots in the first two supply the entry candidates for the callers at `0x00c0c040`, `0x00c0c0f0`, `0x00c0c200` and `0x00c0f8ec`, the event stores (`0x00c0c014` also has plain glyph-row sources), the MOVT limits at `0x00c0dcf0`, `0x00c0ecf0` and `0x00d80000`, and 14 of the 81 table-name words. Those edges are not yet discharged. E-MODAL-STREAM-RESIDUALS-150 bounds the code-pointer table at `0x007a22c4` to its four entries and reduces the other CPU paths to named residuals: the plain size constant `0x007b7000`, which reaches media-message payloads and three request records; reads of the 10×12 table at `0x007a20d4`, which reach only a compare or a pin-register store, and whose row is unchecked; and a 640×480 source transfer from `0x007a20d0`. In addition, 193 aligned words in other objects name the streams and close only with the objects that hold them.

E-MODAL-ID4-DATA-ROLES-150, E-MODAL-ID5-DATA-ROLES-150 and E-MODAL-PLAIN-DATA-ROLES-150 bind additional objects and consumers: image-4 transfer descriptors and sample buffers, image-5 fixed-point parameters and halfword tables, and plain-image upload, coordinate, rule and bitmap data. These are conditional premises, not checker discharges. Computed references, escaping pointers, unbounded indices, unresolved transfer semantics and other entry sources remain open. The checker does not yet consume these object-role proofs.

The checker can discharge a candidate only by object-consumer closure checked against the pinned bytes. Each entry, predecessor and table-name obligation carries its source word. A declared object names an extent, a role predicate the bytes must satisfy and its consumers. Every materialization of an address in the extent must be a declared consumer: a MOVW/MOVT or MOV/MOVT pair, a PC-relative literal, an aligned word in any `address & 0x3fffffff` alias class, or a base whose index bound reaches the extent. Each consumer is walked by the existing census, and a loaded value that reaches a PC write, a register branch, an escape or an unmodeled instruction keeps the edge; a hardware sink is a declared register store. A word naming the object is closed with the object that holds it, and a B/BL-shaped word is an entry only if its object can be fetched. A failed predicate, an undeclared materialization or a changed byte leaves the obligation open.

The checker now classifies stream consumers this way (E-MODAL-SCALAR-CONSUMER-150). Its `declared_objects` output carries the three streams as conditional objects, checks the register-word predicate against the bytes (it holds for all three) and marks each enumerated MOVT, PC-relative literal, aligned-word and constructed consumer discharged, return-conditional or open. A constructed consumer (E-MODAL-CONSTRUCTED-CONSUMER-150) is a register that a MOV, MVN, ORR or other data-processing instruction, including PC-relative arithmetic, or a MOVW, extend, multiply, bitfield or base-writeback instruction computes, and that takes a value naming the object on some path of the checker's construction scan through each image: along fallthrough, and restarted with unknown registers and flags at every branch target and data-word entry. A call or SVC leaves every register and the flags unknown there. Values that reach an instruction only through a branch, a call, a load other than a PC-relative literal word or an unknown index register, and bases indexed into an extent, are still not enumerated. A MOVT, literal or constructed consumer is discharged only when every path from it reads the value as a TST, TEQ, CMP or CMN operand until it is overwritten. It is return-conditional when a path instead reaches a return with the value in r2, r3 or r12; it then rests on the premise that no continuation reads that register first. Each return-conditional consumer carries a continuation witness. Its scope is the code that reaches the materialization through fallthrough and through B edges from between the nearest frame prologue below it and the materialization. The witness lists the B, BL, aligned-word, MOVW/MOVT, PC-relative and Thumb BL/BLX references into that scope and follows each BL caller's continuation, with conditionals taken both ways, to its first read or write of the register; a branch matching only through an alias class counts as an entry. The premise narrows to direct-continuations-overwrite only when every continuation path writes the register first, with no call, unresolved transfer or live cycle before it, no other reference, outside branch or unreferenced start enters the scope, and no return through lr follows an lr write. The narrowed premise still rests on no unenumerated entry, saved-return-slot integrity, no asynchronous write and code integrity, and the consumer stays return-conditional. No consumer is discharged on the pinned images. The plain MOVT at `0x012369e8` is return-conditional: its `0x007b7000` is only compared at `0x012369ec` and is overwritten at `0x012369f8` on one path, but stays in r3 at the return `0x01236a2c` on the other. Its continuation witness (E-MODAL-RETURN-WITNESS-150) finds BL `0x011f4fec` as the sole caller and no other reference into its scope, and that continuation writes r3 at `0x011f4ff8` or `0x011f5010` before any read, so the premise narrows to direct-continuations-overwrite; computed, register-built and exception entries remain unexcluded. Apart from frame-save stores, the examined path's data writes are confined to `[0x014fe898, 0x014fe954)`. It stays conditional until the saved return slots are shown disjoint from that range and safe from asynchronous writes. The MOVTs at `0x01239e48` and `0x0123c1ac` store the same value. Four stream-C MOVTs with unresolved lower halves reach address uses at `0x00673780`, `0x006a00b8`, `0x006e7998` and `0x00732580`, and two stay live across the calls at `0x0082ee8c` and `0x0082f158`. The 198 aligned words naming the streams stay open with the objects that hold them. The construction scan adds 30 constructed consumers, 3 in stream A and 27 in B, and finds no others. Each is a value word of its own stream that decodes as data processing on pc and stays open at the unmodeled register word after it. Streams A, B and C keep 39, 70 and 127 open consumers. Entry and table-name obligations sourced from stream words now name their object and stay open. The array and table census still traces pointers only from MOVT and PC-relative literal constants and aligned words, so its obligations, writers and uncovered-constant-forms are unchanged.

The checker no longer assumes callees preserve registers (E-MODAL-CLOSURE-CALL-INVALIDATION-150). After a call or SVC every general register and the flags are unknown. A tracked value held across a call stays tracked as a derived pointer, and a tracked pointer that a callee may read in any of r0-r14, or that is live in any of them at a return, is an escape; lr is excluded only at a BL or BLX, which overwrite it. The current inventory therefore has 6,904 obligations: the 114 of the earlier call model unchanged, 286 stores into the state/event arrays through a pointer held across a call, and 6,504 pointer escapes, 6,061 of them a pointer left in a register at a return (5,873 in r3). Many are conservative and none is waived. Callers, controllers, cells and targets are unchanged. Exactly one store left the resolved writes: the bank's `rb_ui_pop` write of 0 to the modal-75 state cell at `0x00a8654c`, whose address is held across two calls; it is now a computed-index-store obligation.

Byte reads of that store's two calls do not resolve it (E-MODAL-RBPOP-CALLEES-150). `rb_call_stage_open` reaches `0x00d3d858`, whose call tree has no register call, computed jump or published word. It saves r5 at entry, and its only normal return to the caller restores r5, SP and the return address from that frame; its other normal-route stores are lower stack words and five words at `0x0094f64c`. A zero divisor at `0x0004d978` takes `svc #0x1010` at `0x0004360c`. The [SVC frame analysis](recipe-bank-ui-ownership.md#stage-open-svc-frame-and-remaining-closure), E-MODAL-SVC-FRAME-LIMIT-150, binds the existing loader artifact's wrapper/handler words and identifies a conditional 64-byte frame, code/context publication to `0x4004f118` and `[0x4004f11c,0x4004f15c)`, reporter effects, callback word `0x4004f114` and resume word `0x4004f10c`. Nonzero resume selects the explicit exception return at `0x40d30..0x40d38`, bypassing the wrapper's SPSR reload: its two pops consume the saved SPSR only as a transient LR value and `movs pc,lr` uses the then-current SPSR. With original frame/r4/current SPSR and SVC banking and A32 exception LR it resumes at `0x43610`, not at the stage-open caller. No termination, no-return or preservation claim follows from this selfbranch or an absent edge. The source installer for base + `0x28` does not establish the active vector or its lifetime. Callback/reporter effects, callback and resume-word writers, exception/frame/current-SPSR integrity and physical alias/TCM separation remain open; DAT provenance is inherited rather than freshly reproduced. The normal-return r5 fact still requires pinned publication-adjusted instructions, no intervening exception or other writer, physically distinct frame/stores/code and a nonwrapping 100-byte interval below first-call SP. `rb_call_transition` reaches `0x00c12aac`, which never writes r5 but calls the global dispatcher for controller 100, where r5 survives only in a stack slot across a register call to the cell action. With controller 100 in state 0, the 91 event stores of `0x00c12aac` select 61 action functions, and only the event-0 action is a leaf. Decoded with `host/firmware/A32.hs`, their direct closures have at least 122 register-call or computed-jump sites, an SVC and 21 encodings the decoder does not admit, re-enter the dispatcher and reach 32 bank functions through published branches. The store at `0x00a8654c` remains an OPEN computed-index-store obligation. Its action/controller/table/reentry and other-entry premises, and all obligation counts, are unchanged.

### 8.3 Capability view

Section 2 holds the evidence behind each category. Gaps that no open child issue covers belong to #127.

| Capability | Owned, qualified | Retained executable and shared data | Open issue for the next action | Not covered by an open child issue (#127) |
| --- | --- | --- | --- | --- |
| Startup and shutdown | Owned first-stage entry, loader-A handoff, negative controls and retained-menu continuation on SDK97 (#126); owned record-2 scenarios, four endpoint comparisons and publication/stack controls on SDKq4 (#135); current SDK7360429 loader-B prefix positives, eight standalone retained/owned guests and four comparisons (#136, E-LOADER-B-PREFIX-POSITIVE-150), not full startup | Boot ROM (declared retained), stock record1 in standalone schema4, record-2 helpers/vectors/data, core0 loader-B continuation `0xd09d0`, core1 initialization from `0xd0aac`, kernel/application and later setup; vector SRAM, workspace and ATCM frames remain resources | [#135](https://github.com/skeet70/latent-image/issues/135): actual forbidden-stock-fetch control (provider hardware-key authentication held); [#136](https://github.com/skeet70/latent-image/issues/136): discriminating producer/consumer fault gates remain unaccepted; [#137](https://github.com/skeet70/latent-image/issues/137): composed guest positives and controls still require separate accepted proof. SDK97 first-stage evidence stays separate in [#126](https://github.com/skeet70/latent-image/issues/126). Hardware route [#85](https://github.com/skeet70/latent-image/issues/85), [#60](https://github.com/skeet70/latent-image/issues/60) | Later startup ownership, replacement shutdown, power-off persistence |
| Controls and menus | #121 classifiers; #123 field-row binding | `FUN_00bfb028` for every controller; modal-75 actions in 8.1; the snapshot service `FUN_0061ca2c`, `FUN_0061cd14`, `FUN_0061ceac` and accessors for every stock caller (#132 owns the bank's store call in source); key handlers `FUN_00c6f14c`, `FUN_00c70528`; render probe `FUN_00d52684`. Data: cell table `0x00cd1ae4`, action table `0x00d04080` with word 1 interposed at install, SET gates `0x009544f8`, `0x00950474`, `0x00950478`, `0x00954500`, cursor cells `0x009571d8`/`0x009571dc`, current row `0x0095732c`, saved-snapshot tables `0x0093eb74`, `0x009412d4`, `0x00944414` | [#128](https://github.com/skeet70/latent-image/issues/128) dispatcher and editor cancellation; [#132](https://github.com/skeet70/latent-image/issues/132) snapshot-service publication and guard; [#133](https://github.com/skeet70/latent-image/issues/133) SET, confirm and leave; [#124](https://github.com/skeet70/latent-image/issues/124) PLAY and physical shutter inputs | Other controllers behind the global dispatcher; other menus; physical key timing |
| Live view | None | Sensor, processing and display pipeline; the Classic Chrome fragment is a project patch over it | None | Whole category |
| Autofocus and manual focus | None; the contrast policy and AF RAM ring are unqualified | AF orchestration, lens messages, statistics producers | None; [#109](https://github.com/skeet70/latent-image/issues/109) and [#113](https://github.com/skeet70/latent-image/issues/113) keep their own acceptance | AF ownership; manual focus is not inventoried |
| Exposure and capture | No owned firmware capture behavior; #138 qualifies protocol2 package/host collection, genuine SourceBit0 A collection and fresh checkpoint creation, with separately accepted B restoration/replay through the strict failure | S2 admission through `0x00c58fc4`, `0x00d0da4c`, `0x00d126f0` and `FUN_00c12aac` ([cardless capture](cardless-capture-feasibility.md)); capture descriptors, JPEG encoder and DMA, sensor services | [#138](https://github.com/skeet70/latent-image/issues/138) reporting remains open for the frozen A/B stop-classification defect and new-code qualification; [#124](https://github.com/skeet70/latent-image/issues/124) still needs owned comparison and capture acceptance; [#139](https://github.com/skeet70/latent-image/issues/139) independent second-filter device law remains open; [#129](https://github.com/skeet70/latent-image/issues/129) retains its ownership/lifecycle gates | Exposure control and capture paths outside #129's selection |
| Image development | None | Develop dispatch and processing services; D3 and Classic Chrome data are project data over them | #129 development-step lifecycle | Other develop paths, RAW conversion |
| Save and playback | None of stock; the recipe-bank storage lane is project C over stock media workers | Ordinary save chain `0x00ea0934` → `0x00e99d4c` → `0x00e518c0`; media open, write, close and completion `0x007115a8`, `0x007116e8`, `0x0071176c`, `0x0071c634`; playback | #129 save facade, phase completion and media permission; #124 PLAY entry and card-sector inspection | Playback rendering, zoom, deletion |
| Persistent settings | #120 validator, native only | Area-3 log with both callbacks; stock getter `0x0061e110` and commit `0x00d4fc74` used by the bank; live settings record `0x0096cc64` | None | Validator integration and the area-3 writer, both gated on a persistence decision; unbound fields and writers |
| Calibration consumption | None | Adjustment cache, `f7` gate and their consumers; calibration data is declared retained resource data | None | Consumer code is not inventoried |
| External interfaces | Packaged emulator RAM-push publication and rollback (#121, #123); the host transport is project code | Firmware USB, PTP, SD host and jig handlers; the installer's CPU1 job channel | #124 read-only card inspection and [#125](https://github.com/skeet70/latent-image/issues/125) native backend are emulator tooling, not firmware ownership | Firmware-side interface handlers |
| Failure recovery | None | Stock recovery, recorder and boot-entry paths; R1 to R3 are stock-firmware evidence | Hardware route #85, #60 | R4 and replacement recovery |
| Runtime services, across categories | None | Scheduler, tasks, queues and mailboxes, interrupts, allocator, render and display, storage drivers | #129 names its retained sensor, processing, storage, scheduler and driver services | Contracted retirement of each service |

The unqualified #129 source in `replacement/save` implements the selected save facade, callbacks and phase completion. Its retained ABI converts flags to typed values, and its guest fault controls use the production validator with exact return-register or memory mismatches. Host compilation and typed-value smoke checks passed. Original-versus-owned execution, guest-control outcomes and lifecycle integration remain pending; no timing, device behavior or runtime retirement is qualified.

The unqualified media-permit source in `replacement/media_permit` retains the selected frame, command and bitmap services. Review found that its defect-control acceptance ignored truncated provenance and writes outside the declared allowance. A throwaway host smoke reproduced both false acceptances; the corrected predicate preserves its positive control and rejects both cases. The poisoned-body control stops after the stock entry veneer and is not a pre-fetch retirement guard. Original-versus-owned execution, guest-control outcomes and integrated capture remain pending.

The historical SourceBit0 A accepts only its fresh checkpoint (`E-CAPTURE-PRIMARY-BIT0-CHECKPOINT-150`), not its later route, a restored route, JPEG or capture. Its later route strictly stops at instruction 837,529,073 on the `0xfff30010` store at PC `0x00689490`; 1273 details were retained and 366 dropped, with the strict-stop interval uncollected. The old observer's nonconsuming pages cannot reclaim lifetime per-site capacity; RAM/context gaps remain distinct from detail loss (`E-CAPTURE-COLLECTION-LIFETIME-LIMIT-150`). Those immutable records and the original strict failure remain unchanged.

`E-CAPTURE-LOSSLESS-HOST-150` qualifies [#138](https://github.com/skeet70/latent-image/issues/138)'s SDK source/package at `7360429` and the actual public Haskell CLI against a synthetic external protocol2 bridge. The fixed collector completes both 2048-page/131,072-event and 8192-page/524,288-event streams under the 2 GiB address-space envelope, with exact journal/reply/ACK equality, 32 sites, 512 decoded RAM bytes per event, bounded verdict samples and matching configuration folding. Both retain the 8M/8M/4M/4M phases plus 40M window and explicit synthetic collected-through endpoint 164M, with pending/dropped zero. Fixed2048 syscall evidence proves complete page writes and successful fsync before every ACK, including initial journal and directory fsyncs; this is not a power-loss proof. The before8192 collector instead reaches GHC OOM after 5538 journaled/ACKed pages, leaving the next 64-event reply unjournaled/unACKed and no result file. Four streams run once each, with zero retries; the earlier V1 navigation failure remains excluded.

GNU Time peak RSS includes children: 36,172/37,316 KiB for the fixed short/long streams, not a pure-collector heap profile or timing/performance result. Artifact monitoring used allocated blocks, 3,394,756,608 bytes below 12 GiB; apparent sparse lengths total 24,808,526,202 bytes and exceed that cap, so no unqualified artifact-cap pass is claimed. The public CLI performs real card traversal over verified existing bytes, not a new JPEG capture. Complete streams preserve the original post-stream no-display-plane `BridgeError` and exit 3. Synthetic armed-site zero gaps do not close real RAM/context, IRQ/registration, uninstrumented coverage or #139 device-law gaps.

`E-CAPTURE-PROTOCOL2-A-150` accepts one genuine protocol2 SourceBit0 A run through the normal public CLI on SDK `7360429`/readback, the same supported collector as the host proof, and the separately frozen launcher/run document. It is not the later map-fix SDK. A fresh checkpoint is created at 302,500,000 preattach instructions with no installed candidate. Independent review reads the actual metadata, recomputes its path-excluded canonical key and checks both actual checkpoint-file hashes. This record qualifies creation; B restoration/replay is separately qualified below. Neither record accepts capture.

The genuine collector retains all 1639 events, contiguous sequence 0 to 1638, from 178 armed sites (117 nonzero): 34 wire pages are journaled, 28 nonempty pages exactly ACKed, final ACK 1639, zero dropped/pending. Two actual capacity yields resume only after complete, empty, fully ACKed drains. The 8M/8M/4M/4M half/full/half/released phases complete from 813,368,000 through 837,368,000, followed by one intended 40M window. The original strict `0xfff30010` write4 refusal at CPU0 PC `0x00689490` stops execution at 837,529,073. Hits remain available after failure; finish reports `finished:true` while preserving the original `BridgeError`, with finish rejection separate and collector/service exit 3.

Last successfully collected and observed boundary is 837,529,073, not the last retained event at 836,409,315. The executed strict tail has no detail loss. The remaining 39,838,927 instructions through intended endpoint 877,368,000 were not executed; they are not dropped executed events. Forty-two events still have incomplete RAM context, including 29 outside-RAM spans; 13 incomplete events contain only complete retained spans. Coverage remains armed-sites-only with unknown `uninstrumented_gap`, not complete firmware/IRQ/registration/alias coverage. The actual wire/journal/ACK ordering is checked, but this run has no syscall trace: fsync-before-ACK relies on the immutable same collector and the separate accepted host syscall proof.

`E-CAPTURE-PROTOCOL2-B-RESTORE-150` accepts one restored B invocation on the unchanged SDK/bridge, public collector and run document. The actual ready event reports `start: restored`, the accepted A attach key and 302,500,000 preattach instructions; both staged checkpoint files exactly match A's hashes with distinct inodes, while retained A files remain unchanged. All 1,639 event objects and 34 page objects match A, including registers, RAM, counters and bounds. The original strict stop at 837,529,073, zero dropped/pending details, 42 RAM-context gaps, unknown unarmed coverage and 39,838,927 unexecuted intended instructions remain. Actual collector/service exit is 3; SDK `ExitSuccess` is receipt production, not route success. Resource admission and full process/thread, unit/cgroup and slot teardown are independently reviewed; this adds no new syscall trace.

Both frozen A/B results misclassify `stopped_at` as `"no-strict-model-stop"` despite correctly preserving the original strict failure in `original_bridge_error` and the bridge boundary. The receipts remain unchanged; E839 accepts restoration/replay/collection only, not flawless historical reporting. `E-CAPTURE-STOP-REPORTING-HOST-150` separately qualifies the integrated typed correction through four actual public-CLI runs against a synthetic external bridge: structured machine failure wins over opaque text, phrase-bearing command error with null machine failure remains a command failure, distinct tail/finish/session-evidence errors do not replace the original, and malformed success remains a harness error. Expected collector exits are 3/3/3/1; fixture-bridge exits are successful. Complete empty tails/finish succeed only in the first two cases; all four retain the incomplete `[100000000,124000000]` interval and zero synthetic events/completed advances. Host mode plans add exactly eight sites, 177 versus 169, not a comparison with the genuine SDK's 178-site count. Invocation/result/journal/transcripts agree, seven actual inputs and original cards remain unchanged, and allocated/apparent output stays under 20 GiB. The recorded 27,300 KiB `time -v` RSS is per process, not aggregate memory. #138 remains open pending current genuine-firmware code-path qualification; the matching corrected diagnostic SDK matrix is now separately source-qualified, not a fresh diagnostic A run. #124's owned comparison and capture, #129's owned lifecycle, #139's unknown device law and #117's application/runtime milestones remain open. No genuine SDK/firmware/checkpoint, JPEG, physical capture or new accepting MMIO law is qualified by the synthetic host results.

#139 has source-bound selector/routing encoding, parameter validation/packing and control-before-configure order, including valid zero, but no independent X-M1 selector meanings, units, algorithm, activation, latch/reset/readback or processing law (`E-SECOND-FILTER-SOURCE-LIMIT-150`). Historical A did not retain the required operand/control/routing observations. The new genuine loss-accounted collection does not establish an operand or device law without further causal source proof. The runtime-filter commit at `0xfff3805c` belongs to a different register family. Strict refusal remains required. See the [current capture prerequisites](../emulation/README.md#irq41-stop-in-physical-1).

`E-SECOND-FILTER-DIAGNOSTIC-SOURCE-150` accepts the separate corrected `f6cf677` canonical source/package matrix: 102 default, 109 scoped and 102 readback checks, 110 logical-target-qualified names, 101 shared default/scoped names and readback repeating default. Three SDK/workspace packages are fresh; three unchanged engines are cached with zero fresh engine checks. No attempted test fails, is ignored, filtered or retried. Six diagnostic names pass in every variant, covering 24 source-defined synthetic loop cases per variant. Real 4 KiB ATCM fixture backing on both cores permits core0 overlap to return explicit empty gaps while core1 TCM leaves current-core RAM visible; the earlier `28873bb` absent-TCM failure remains frozen. Two separate scoped temporal regressions cover four Early/Late/Missing/Invalid synthetic cases. Six installed no-input CLI usage refusals exit 1 before machine construction. These are source/synthetic SDK results, not firmware receipts, genuine operand observations, device-law or initializer acceptance. Fresh diagnostic A inputs are materializing; compilation, freeze and runtime remain ungranted.

### 8.4 Milestone blockers

| #117 milestone | Prevented by | Owner |
| --- | --- | --- |
| Own accepted application behaviors | Published qualified ownership covers only bounded decisions and one service lane. #138's genuine loss-accounted collection, checkpoint creation and restored B replay qualify tooling, not owned capture. Its typed reporting correction separately passes four synthetic public-CLI cases; current genuine-firmware reporting remains open. The modal controller, capture and save lifecycle and remaining state machines run stock code; owned comparison and the second-filter device law remain open. | #128, #129, #124, #139; remaining categories #127 |
| Retire proprietary application/runtime implementations | Record-2 orchestration and the loader-B core1 prefix have bounded guarded original/owned comparisons; actual #135 retired-fetch and #136 discriminating prefix faults remain unaccepted. The #132 snapshot service has static caller closure and native original/owned qualification, but its stock entries are not published over. General application/runtime body retirement and native tail-call/media-permission qualification remain open. | #135, #136, #125, #128, #129, #132; remaining bodies #127 |
| Replacement startup in emulation | SDK97 first-stage handoff, intended controls and cold-menu continuation pass. SDK374 qualifies retained peer startup; SDKq4 qualifies ten retained/owned record-2 scenarios, four comparisons and two handoff controls. Current SDK7360429 qualifies #136's eight standalone prefix positives and four comparisons, preserving the frozen q5 refusal. #135 retired-fetch and #136 fault gates remain open. #137's mapping and cross-core-order firmware refusals remain history. Isolated SDK94ceeac passes 96/103/96 source checks (`E-COMPOSED-TEMPORAL-SOURCE-150`); separate corrected diagnostic SDKf6cf677 passes 102/109/102 source checks (`E-SECOND-FILTER-DIAGNOSTIC-SOURCE-150`). Both include synthetic temporal regressions, not composed firmware acceptance or initializer qualification. No composed firmware positive or comparison is accepted. Core0 loader-B continuation, core1 initialization from `0xd0aac` and later startup remain retained. | #135, #136, #137; remaining startup #127 |
| Replacement startup on hardware | No qualified physical route. | #85, #60 |
| Persistent deployment | Not pursued. | None |

### 8.5 Candidate next boundaries

Selected boundaries retain their qualification and retirement gates.

**A. Menu-frame snapshot service: `FUN_0061cd14` with its store `FUN_0061ceac`, classifier `FUN_0061ca2c` and word accessors. Selected as [#132](https://github.com/skeet70/latent-image/issues/132).**

- Behavior: fills eleven words (depth, root, node type, class, cursor token, current-value token, raw cursor, visible ordinal, scroll state, page calculation, navigation word) from the current frame, its values array and the stock navigation cells. It writes only the destination snapshot and call frames ([contract](recipe-bank-ui-ownership.md#owned-menu-frame-snapshot-service)).
- Source increment (#132, not qualified as a retirement): `replacement/owned_menu` implements the classifier, snapshot, store and all eight accessors from the 1.50 bytes. Original and owned execution match on 307 bounded native pairs with literal expectations and refusal controls (E-MENU-FRAME-SNAPSHOT-NATIVE-150). The bank's SELECT refresh calls the owned store; the bank no longer calls `0x0061ceac`.
- Caller closure: the recorded constant-reference scan found 3,027 direct `BL` sites to the eleven entries in loaders A and B, image 2, images 4 and 5 (stock and package) and plain image 6, with no detected data word, formed address or interior branch. `FUN_0061ca2c` has no detected caller outside the service. This does not establish the absence of computed references.
- Remaining prerequisites for global retirement: reviewed publications at the eleven stock entries, a forbidden-fetch guard on the retired bodies in a packaged menu route, which the packaged FujiMod route lacks, and a device-driven packaged run with it. `FUN_0061ce1c` and the three saved-snapshot tables stay stock.
- Risk: a wrong word moves cursor, marker or scroll state across every menu.

**B. Modal-75 SET, confirm and leave actions: `FUN_00c72e10`, `FUN_00c75ed4`, `FUN_00c76650`. Selected as [#133](https://github.com/skeet70/latent-image/issues/133).**

- Evidence: E-RECIPE-MODAL-CONTRACT-150 binds actions `0x5a`, `0x73` and `0x81` to them. The [ownership report](recipe-bank-ui-ownership.md#retained-modal-contract) records the SET gates and the post-transition frame reads. [Recipe design](recipe-design.md) section 17.8 records `FUN_00c72e10`'s special-case classes and generic leaf tail. The bank reaches all three through `menu/recipe_bank_arm.S` trampolines and returns them for unclaimed pages.
- Value: #128 is planned to own the dispatcher and `FUN_00c77794` and routes the remaining delegates to #127. If #128 qualifies, these three remain the stock actions on the bank's cells, and owning them would complete the bank's entry, SET, confirm and leave lifecycle over an owned dispatcher.
- Source increment (#133, unqualified): `replacement/owned_menu/src/lifecycle.rs` implements all three source-bound action bodies, including the other modal-75 pages' special classes 5, `0x184`, `0x141`, `0xf5`, `0x167`, `0x26`, `0x19`, `0xed`, `0xee` and node `0x5b`. The host build and emitter validated 85 original-versus-owned pairs, one real reference and six guest controls through the unchanged case loader, 177 cases without execution (E-MODAL75-LIFECYCLE-SOURCE-150, E-MODAL75-CONTROLS-SOURCE-150). Candidate A remains a retained input to its frame reads.
- Remaining prerequisites: qualify the original-binary comparisons and refusal controls through #125, then migrate the bank's three stock calls and affected UI cases to the real owned controllers, and qualify an integrated lifecycle with the #128 dispatcher and forbidden original-action execution. The unchanged historical 55 UI cases use synthetic functions at these addresses and do not qualify #133. No global ownership or retirement is established.
- Risk: global use by modal 75 and the gate cells' unqualified device-input states.

The area-3 write callback `0x006a8484` is not ranked. It would reuse #120's contract and runner, but its runtime caller is the area-3 append path `FUN_006ad3f4`, so integration needs persistent-storage acceptance.

### 8.6 Retained secondary-core startup boundary

[#134](https://github.com/skeet70/latent-image/issues/134) qualifies a core-specific observation of retained record-2 startup through pre-execution loader-B entry, with natural, held-sentinel and delayed-release acceptance on the public runtime. It is not an owned startup increment or stock-code retirement. The separate first-stage witness observes core 0; zero rendezvous cells alone do not identify their writers. The source contract is supported by `E-PEER-ENTRY-FRAME-HANDOFF-SOURCE-150`; configured engine laws and observation semantics are supported by `E-PEER-MODEL-OBSERVATION-SOURCE-150`.

Wrong-core/wrong-handoff serialized controls qualify host-consumer rejection, not executed guest fault behavior. The natural and guest-control results below establish the retained boundary consumed by [#135](https://github.com/skeet70/latent-image/issues/135); remaining startup ownership stays open.

#### Exact X-M1 1.50 source contract

The original DAT has SHA-256 `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`. The retained record-2 decode and existing re-extraction both hash to `2f1bf3f0e483b892d2890599679c1b515cfc1be8b8c27cbb06d4d913e428abb4`, length `0x4c80`, loaded at `0x10000`. Direct word reads independently agree with the existing listing at record offsets `0x1800`–`0x19a4`, `0x1cc0`–`0x1ccc` and `0x1f40`–`0x1fb8`. These are byte/source findings, not observed execution.

| Observation boundary | Source-bound fact | Qualification limit |
| --- | --- | --- |
| Record-2 entry `0x11800` | First instruction calls execution-state initialization `0x11854`; the vector word at record offset `0x20` supplies `0x11800`. | Entry precedes initialization. Incoming registers, current/banked SP and LR, CPSR/SPSRs and exception history are not established by the vector literal. |
| State and stack setup | `0x11854` selects little endian, masks IRQ/FIQ, clears asynchronous-abort mask and initializes privileged modes/SPSRs. `0x118b8` programs TCM and initializes SP banks from literal `0x70002000`: IRQ `0x70002000`, FIQ `0x70001000`, SYS/USR `0x70000e00`, ABT `0x70000c00`, UND `0x70000a00`, SVC `0x70000800`. | These are the values the instructions request, not measured physical TCM or banked-register readback. A receipt of current registers does not capture every bank. |
| Main-call frame and tail `0x11f40` | `BL 0x11cc0` at `0x11838` supplies LR `0x1183c`. The push saves `{r4,r5,r6,r7,r11,lr}`, **24 bytes**; then `r11=initial_sp-4`, `sp=initial_sp-64`. The saved frame starts at `r11-20` and saved LR is at `r11`. Thus tail `r11=sp+60`. The discriminator saved in r6 is 1 on the configured secondary path. | The saved incoming register values other than LR are observations, not fixed constants. The frame is not 16 bytes. Restored SP means tail SP + 64, not the unconstrained SP sampled at record-2 entry. |
| Rendezvous `0x11f4c`–`0x11f7c` | Store zero to guest `0x40020004`, DSB, then read guest `0x40020000` and repeat only while it equals **exactly `0x22b`**. The retry body has a 100-count busy delay. | Any non-`0x22b` value exits this comparison. The source has no finite wait bound or general readiness check. |
| Target and argument publications | `0x11f98` writes `0x14c80` from segment-id-1 record `+4` → header `+0xc`. `0x11fb0` writes `0x14c84` from original image-4 word `0x61c40c`. Retained loader-B header gives load `0xd0000`, entry `0xd0020`; original image-4 word `+0xc` is `0x0069cdf8`. | Missing segment lookup has no null check at the dereference. Original image-4 argument is distinct from loader-B entry and does not establish application execution. |
| Handoff `0x11850` → loader-B entry `0xd0020` | The epilogue restores the six saved words and SP, returns to `0x1183c`, selects SVC, loads r3 from `0x14c80`, r0 from `0x14c84`, then executes `mov pc,r3`, not `bx`. | Acceptance requires masked A32 little-endian SVC, r3/target `0xd0020`, r0/argument `0x0069cdf8` and restored SP. Reaching loader-B entry would not establish loader-B wait completion or an application handoff. |

Record 1's source writes `0x22b` to both cells at `0x1e64`/`0x1e68`, then clears its own cell at `0x1ff8`. It clears completion guest `0x400a00c4` at `0x1fec`. The [boot report](update-validation-and-boot.md#second-supplied-boot-record-companion-initialization-and-shared-image-handoff) retains the broader initialization and lookup analysis. The qualification below separates core-attributed write attempts from committed readback.

#### Configured model laws and unresolved physical facts

The pinned FujiMod source is `3d90035d7d8d651e126f89f94f30765506e6f53f`, with the checked-in Unicorn context-alignment patch. Its EXR II catalog declares two separately scheduled Cortex-R5F proxy contexts, with the adapter's R4F guard. CP14 selector `(opc1,crn,crm,opc2)=(0,2,0,0)` returns configured identities `0x4003`/`0x5003`. The X-M1 discriminator at `0x11aa0` tests equality to `0x4003`, returning 0 for equality and 1 otherwise; it does not prove the other physical identity is `0x5003`.

Core 1 is configured held at PC `0x3fff0000`. Record 1 copies `0x3e8` bytes from `0x10000` through guest alias `0x7fff0000`. The catalog maps `0x3fff0000` and `0x7fff0000` to the same secondary SRAM backing on both cores and additionally maps core 1's `0xffff0000` page there. The declared IRQ1 software pulse for line 120 drives the one-shot `core-release` model; that model deasserts reset without inspecting code or setting PC. These are configured release/mapping laws, not measured silicon reset circuitry. The copied exception literals `0x3fff004c`, `0x3fff0064`, `0x3fff0050`, `0x3fff0058`, `0x3fff0060`, `0x3fff00c0` and `0x3fff0090` remain unresolved physical vector aliases/handlers. They must not be relabeled as captured-ROM handlers.

The model initially aliases low 16 KiB to captured ROM and exposes a decoder choice for low DRAM; guest `0x40000000` aliases DRAM physical zero on both cores. A watch must include current/static maps and decoder choices before either core runs. Relevant physical spans are rendezvous `[0x20000,0x20008)`, completion `[0xa00c4,0xa00c8)` and target/argument `[0x14c80,0x14c88)`. CPU records carry core, PC, guest/physical address, width, value, before bytes, byte order, aggregate instructions and tick. They are **attempted accesses**, not proof a store committed. Handoff/loader-entry readback must independently show both cells zero and the expected target/argument. Completion is observed, not required to remain zero while core 0 runs concurrently.

Transfer records distinguish `model-bus-read`/`model-bus-write` from `host-read`/`host-write`, with null core/PC/instruction/tick fields and no CPU value/before/byte-order metadata. Snapshot physical readback itself adds `host-read` rows. These records cannot prove a guest CPU writer. The watch also collects CPU reads/fetches: polling can exhaust its bound and must retain a violation rather than truncate into a passing receipt.

#### Current secondary-startup qualification

Natural reset, held sentinel and delayed release pass the SDK and actual production Haskell receipt consumers at observer source `374e8d2`, on public model `3d90035d` with the context-alignment patch. The model has two scheduled R5F proxy contexts with R4F restrictions; CP14 identities are configured inputs, not measured silicon identities. These results bind the older CLI `246cf6b`/c546975-era host and guard cohort, not post-PR122 affinity propagation. The first-stage SDK97 qualification remains separate. Exact receipts, source/artifact identities, bounds, resource admissions and teardown evidence live in the linked ledger rows.

- Natural prefix (`E-PEER-LIFECYCLE-NATURAL-RESET-150`): all four pre-execution stops are on core 1, at `0x11800`, `0x11f40`, `0x11850` and `0xd0020`. The saved 24-byte frame, restored SP, r3/target `0xd0020` and r0/argument `0x69cdf8` satisfy the source contract. The watch attributes primary/peer clear and publication attempts to their core/PC, while boundary readbacks independently show committed shared state. The natural later-boundary zero is not immediate post-STR completion proof.
- Held sentinel (`E-PEER-LIFECYCLE-HELD-SENTINEL-150`): its own core-0 gate validates registers, low-RAM/TCM bindings, forty private TCM stack bytes and exact virtual/physical code. Four original core-1 loads return `0x22b`, with captured post-LDR/pre-CMP registers and independent cell readback; no primary clear, nonzero publication or handoff is observed. Final core-0 PC/registers/SP/LR/CPSR equal the gate context. The fourth CMP/branch execution is unobserved and final private stack memory is not resampled. Completed-load attempt counts may equal their following unconsumed stop counts.
- Delayed release (`E-PEER-LIFECYCLE-DELAYED-RELEASE-150`): its own actual gate validates the context, mapping, code and private frame before the veneer acknowledgment. The original core-0 STR is observed at `0x1ff8`; one microstep has aggregate entered delta exactly one, advances only core 0, and ends at `0x1ffc` with original bytes `00 20 83 e5`, unchanged CPSR and no machine failure. Matching core-0/PC attempted-write metadata and immediate physical cell-0 zero readback occur before another CPU entry. This qualifies that clear, not a general retirement counter. Subsequent core-1 target/argument publication attempts precede independent handoff/loader-entry readback with both cells zero and source-bound r3/target, r0/argument and restored SP.

Veneer stops are phase-scoped. At reset the low 16 KiB maps captured ROM; its `0x14c` contains `CMP r0,#1`, not the delayed veneer instruction. The historical SDK97 failure stopped there before gate bindings (`E-PEER-COMBINED-DELAYED-RELEASE-REJECTION-150`). The ROM-path attribution is a reconstruction from pinned model/source, ROM bytes and failed registers; that failed receipt did not sample the decoder or fetched bytes (`E-PEER-VENEER-RESET-ROM-LIFECYCLE-SOURCE-150`). SDK374 arms delayed acknowledgment/failure stops only after the actual primary gate validates program and mapping bindings, retaining wrong-core/out-of-phase rejection once armed.

CPU watch rows are attempts; committed readback is separate. Model-bus transfers and host snapshots do not identify CPU or physical-device writers. Original `0x3fff00xx` vector aliases/handlers, loader-B continuation and later application startup remain unresolved or retained. Neither this observation nor its guest controls own or retire record 2, establish full startup, or qualify physical behavior. The next owned secondary-stage boundary is [#135](https://github.com/skeet70/latent-image/issues/135).

#### Owned record-2 source increment (#135)

[#135](https://github.com/skeet70/latent-image/issues/135) implements the owned boundary in [`replacement/secondary_stage`](../replacement/README.md). `E-SECONDARY-STAGE-SCHEMA3-HOST-150` records its 1280-byte descriptor-by-address build, staged DATs and host witness; `E-PEER-STARTUP-OBSERVED-SHAPE-150` preserves the observer corrections. `E-SECONDARY-STAGE-SCHEMA3-SCENARIOS-150` accepts all five retained and five owned scenarios, four endpoint comparisons and the publication/stack fault refusals ([witness](../emulation/README.md#owned-record-2-startup-witness-schema-3)). The actual forbidden-stock-fetch fault control remains open. [#136](https://github.com/skeet70/latent-image/issues/136) tracks the following loader-B prefix; neither increment establishes full startup or hardware behavior.

`E-LOADER-B-PREFIX-POSITIVE-150` accepts the current [#136](https://github.com/skeet70/latent-image/issues/136) standalone prefix positives on scoped SDK source `7360429`: eight actual retained/owned natural, held, delayed and non-one guests, each with launch-time and exactly one independent production Haskell consumption, plus four accepted matching comparisons. This is a fresh current-SDK cohort, not a relabeled older result. The historical host/SDK qualification and frozen q5 retained-natural refusal remain unchanged in `E-LOADER-B-PREFIX-HOST-SCOPED-SDK-150` and `E-LOADER-B-PREFIX-INITIAL-REFUSAL-150`.

Consumer-visible state and effects agree at the selected boundary: the four-word frame and seven vector words have matching virtual/physical views, and the scoped observer records all 22,112 zero-write words covered. Natural observes the original core0 completion store and committed readback before core1's first nonzero load and preexecution exit at retained `0xd0aac`. Held records four post-gate zero polls without completion or an endpoint, not indefinite waiting. Delayed preserves the original store after 429 retained and 256 owned additional zero loads, with NZCVQ, SP and r11 preserved. Non-one commits and reads back `0x400a0000` and reaches the same endpoint.

The corrected full initial record2 identity is now guest-exercised at preexecution secondary entry. The mandatory live immutable projection excludes exactly `[0x14c60,0x14c6c)`, retaining the final twenty bytes through `0x14c80`; loader/view/map/TCM checks remain required. Continuous immutable-byte, core1 forbidden stock-prefix fetch/read and write-confinement enforcement is bounded to the declared scoped interval. Current/static mappings, decoder choice, both-core TCM and observable alias premises remain explicit. This does not prove global caller/exception closure, invisible aliases, silicon identity or physical timing.

The prefix owns vector installation, workspace clearing, the nonzero completion predicate and the selected assembly setup/frame, linked with the owned secondary stage in one record2 image. Retained executable includes stock record1 in schema4, core0 continuation `0xd09d0`, core1 initialization from `0xd0aac`, exception helpers/vector handlers and the later kernel/application. Shared data includes loader/header/id tables, vector slots, completion and rendezvous cells, publication globals and record2 log state. Stock initialization, loader workspace `[0x400ddaa0,0x400f3420)`, the core1 vector SRAM view and ATCM call frames remain dependencies; no resource release is established.

Actual `core-refusal`, `vector-fault`, `fill-fault`, `predicate-fault`, `frame-fault`, `stack-fault`, `retired-fetch-fault` and `write-confinement-fault` acceptance remains open under [#136](https://github.com/skeet70/latent-image/issues/136). Private partial fault artifacts and preparation are not executed controls; no new negative launcher or fault guest is accepted. Provider hardware-key authentication holds #136/#137 negative-control preparation, [#135](https://github.com/skeet70/latent-image/issues/135)'s actual `0x12960` retired-fetch control and [#125](https://github.com/skeet70/latent-image/issues/125)'s native/F01/wire work. The positive cohort does not close #117, #136 or #137 or establish general retirement.

`E-COMPOSED-STARTUP-HOST-SDK-150` records the historical [#137](https://github.com/skeet70/latent-image/issues/137) source qualification, including two demonstrated Haskell qualification repairs integrated in `5e64dcc`. The builder stages one owned first-stage image in record1 `[0x6000,0x64ac)` and one linked secondary-plus-prefix image in record2 `[0x10400,0x10a6c)`, with interior prefix entry `0x1042c`. Ten distinct host test targets pass, nine fresh and one cached, with 501 container assertions. Twelve actual source/standalone/composed DATs each pass 43 checks plus two carried checks; eight schema4 and four schema5 prepared contracts bind their actual identities. An independent proof derives aggregate extents from linked symbols and fixed source metadata without importing project staging/container code, recomputes checksums and identities, proves bytes outside those extents unchanged, checks selected completion-control deltas and compares four frozen normal images. Standalone schema4 requires stock record1 and cannot qualify this composition.

That source-only qualification records 84 default, 89 scoped and 84 readback package checks, 90 distinct names total; three unchanged engine packages are cached, with zero fresh engine tests. Its scoped schema4/5 parser-contract CLI smokes deliberately refuse quantum1 scheduling with zero guest instructions; default/readback reject those schemas and 28 parser mutants refuse without receipts. These historical source/admission results are not composed runtime acceptance. The actual first-entry extent/mapping admission refusal remains frozen, before either handoff, secondary/prefix execution or any comparison. Source-adjacent identity-resolver fix `8c8ad50` is integrated and pushed. Its fresh canonical SDK packages pass 96 default, 101 scoped and 96 readback checks, 102 distinct logical-target-qualified names; unchanged engine dependencies remain cached with zero fresh engine tests. A separate disposable four-case actual-resolver probe passes and is excluded from canonical coverage (`E-SDK-CONTIGUOUS-SPAN-150`).

`E-COMPOSED-HANDOFF-ORDER-REFUSAL-150` preserves the next composed-natural attempt on that map-fixed SDK. Record1 entry `0x6000`, secondary entry `0x10400` and record2 handoff `0x10420` are observed at aggregate instructions 59,997, 75,001 and 5,851,498. The full initial record2 identity is observed; the observer then rejects the absence of an earlier first-stage handoff, before live-projection/loader binding or scoped-watch creation. Machine failure is null. Neither first-stage handoff nor prefix/completion/endpoint is observed, and the receipt cannot establish whether core0 would later reach its handoff. Launch-time and exactly one independent host consumption reject the failed receipt. The SDK child's exit was not separately captured. The other eleven guests and every comparison remain unstarted; cleanup records all owned processes/threads absent, an inactive unit, absent cgroup and unheld slot.

The owned first stage releases core1 before retained staging and waits on the peer's readiness cell before its final branch. Those cells do not order the two final handoff instructions. Secondary entry must precede record2 handoff and prefix entry; the mandatory core0 first-stage handoff at final `mov pc,r2` `0x6118` may occur before or after them or core1 waiting polls, but must precede core0's held gate, delay entry or completion-store boundary. The integrated producer and consumer retain that handoff's breakpoint/context checks across the classified-to-scoped transition without advancing the guest; a scoped handoff carries its counter/watch cursor. Full initial and later immutable-projection identities, image/vector, alias, decoder, TCM and scoped access guards remain required. The composed guard protects the owned first-stage image/vectors and only stock entry instruction `[0x1800,0x1804)`, not the whole retained first-stage body or pre-handoff record2 immutability. Haskell qualification on `6aa5c92` passes one fresh loader-prefix witness target, builds the host consumer and rejects one no-argument usage invocation; these are host source/package checks.

`E-COMPOSED-TEMPORAL-SOURCE-150` accepts the separate exact `94ceeac` SDK source/package matrix: 96 default, 103 scoped and 96 readback checks, 104 distinct logical-target-qualified names. Default/scoped share 95 names and readback repeats default; repeated variants and earlier waves are not added into a larger distinct total. All three SDK and workspace packages are fresh; the three unchanged engine dependencies are cached with zero fresh engine checks. There are no failed, ignored or filtered tests. Two public scoped regressions exercise four synthetic loop cases: Early/Late accept handoffs on either side of record2 handoff through the held gate; Late reaches prefix entry first and retains a zero-only scoped poll counter/watch cursor; Missing refuses at the held gate and Invalid at first-stage handoff. Both record identity observations and existing guards remain required. Four loop cases are not four test names or new firmware receipts. Six installed no-argument driver/bridge smokes refuse usage before machine construction.

Both earlier SDK qualification failures remain frozen: `6aa5c92` rejects initial Graph mapping and `505f548` rejects colliding synthetic prefix-entry/load sites during contract parsing, before temporal assertions. The isolated accepted fixture fixes initial mappings and gives prefix entry/load/post-load distinct sites `0x1042c`/`0x10430`/`0x10434`. Its source qualification does not qualify the later diagnostic SDK or initializer changes. No after-correction composed firmware positive or comparison is accepted. Fresh positive-cohort source preparation remains separate from compilation and guest execution. Separately accepted schema5 observations and matching retained-schema4 endpoint comparisons remain required. Standalone positives and prior first-stage/schema3 results cannot substitute. Discriminating composed controls remain authentication-held and unaccepted. The [witness instructions](../emulation/README.md#composed-startup-witness-schema-5) retain the concrete route and limits.

| Category | Entries |
| --- | --- |
| Owned (Rust) | Publication-global clear `[0x14c80,0x14ca0)`; one CP14 read and the `0x4003` discriminator; the `0x4003`-path call of `0x12204`; MPU regions 0–3, region 4 off that path, SCTLR OR `0x1005` with cache invalidation; the `0x11cc0` sequence: log init/events `0x01000300`, `0x01000400`, `0x11000400`, `0x11000300`, the `0x12ad0` call on descriptor `0x146a8` by address, the `0x13244`/`0x13124`/`0x137a8` calls, the `0x1386c` selection predicate with its conditional record-class read, the `0xfff55104`/sample/`0xfff89014` eligibility test, the companion power sequence and its refusal, `0x13b80` and `0x130c8`; own-cell clear, DSB and the exact-`0x22b` wait with a 100-iteration delay; id-1 lookup and the unchecked record `+4` → header `+0xc` loads, target and argument publication |
| Owned (assembly) | Entry calling `0x11854` and `0x118b8` before Rust; SVC selection, published-word loads and `mov pc,r3` |
| Retained executable | The services below; vector handlers `[0x10040,0x100f0)`; loader B |
| Shared data | Vector store `[0x10000,0x103e8)` with reset literal `0x10400`; `0x118b8` literals `0x11acc`/`0x11ad0`; `0x12204` table `0x11af0`; descriptors `0x146a8`, `0x14710`; id table `[0x14804,0x14bf4)`; log state `0x40014c60`–`0x40014c68`; globals `0x14c80`–`0x14ca0`; companion region `0x400a0000`–`0x400a0548`; peer cells; segment table `*(0x61c000)`; image-4 word `0x61c40c` |
| Candidate forbidden fetch | `[0x11800,0x11854)`, `[0x119e4,0x11abc)`, `[0x11cc0,0x11fe4)`, `[0x12960,0x12974)`, `[0x12abc,0x12ad0)`, `[0x13490,0x13574)`, `[0x1386c,0x138ac)` |
| Candidate forbidden read | `[0x11ac4,0x11acc)`, `[0x11ad4,0x11ae8)`, `[0x11fe4,0x11fec)` |

Retained-service ABI, from an all-path walk of each body in the decoded record. Every AAPCS service restores its saved registers, SP (through its `r11` frame) and the return address on each return path; none writes an unsaved r4–r11. The deepest nested frame is 68 bytes below the owned frame (32 bytes for `xm1_secondary_main` in the current build, 72 in the first), inside the 2 KiB SVC stack `0x118b8` requests.

| Service | Arguments → result | Saved | Calls | Effects and waits |
| --- | --- | --- | --- | --- |
| `0x11854` | none; not AAPCS | none | none | SVC, IRQ/FIQ masked, A clear, little endian; SPSRs of UND, ABT, FIQ, IRQ, SVC; clobbers r0–r2; returns through the SVC LR, so entry in SVC is a premise shared with stock |
| `0x118b8` | none; not AAPCS | none | none | ATCM register `0x70000011`, zero `[0x70000000,0x70002000)`, banked SPs and SPSRs; leaves r0=`0x70002000`, r1=0, r2=`0x1f`, r4 = masked CPSR, r5 = r4\|`0x13`, r6=0, r7=`0x70000000`, r8–r11=0 |
| `0x12204` (`0x4003` only) | none | r11, lr | `0x11fec`(0), `0x11fec`(1) | Interrupt banks and pins from table `0x11af0`, 16 words to `0x400a0000`–`0x400a003c`, `0x400a00c8` = 0 |
| `0x1270c` | units → unread | r4, r5, r11, lr | `0x135d8` | Timer channel `0xfff91010 + 16n`; no-op above `0xaec33e` units; unbounded counter wait |
| `0x12ad0` | descriptor, command → response halfword | r4, r5, r11 | none | Reads descriptor words `+0`–`+0x1c` before returning and stores no pointer to it; engine `0xfff84040`, pins `0xfff89300` for `0x146a8`/`0x14710`; unbounded status wait; clobbers r12 |
| `0x13244` | none → 0 or `0xffffff01`, unread | r4, r5, r11, lr | `0x12ad0` ×16 | Stack copy of `0x14710`; bytes `0x400a00d0`–`0x400a00df`; `0x400a00cc` = 1 or 0 |
| `0x13124` | table → 0, unread | r4–r8, r11, lr | `0x127b4`, `0x130ec` | Conversion registers at `0xfff8a000`; unbounded wait; ten table words when the table is nonzero |
| `0x137a8` | sample, table → flags `0x20`/`4`/`2`/`1` | r3–r6, r11, lr | `0x1366c` | Stores the sample at `0x14c90`; `0x1366c` reads table `0x14804`, pins and table words, and calls row word `+0xc` for kind-5 rows: of the queried ids 9, 11, 6, 13, `0x206`, `0x205` only id 6 does, reaching leaf `0x13628` (reads `0x14c90` bit 0) |
| `0x13b80` | (unread), base, length → 0 | r4, r5, r11, lr | `0x13b10`, `0x13b60` | Zero `[base, base+length)`, then header words `+0`–`+0x10` and `+0x228` for length ≥ 956; nothing for base 0 |
| `0x130c8` | none → 0, unread | r11 | none | `0xfff8a000` = `0x8004` |
| `0x13574` | id → record or 0 | r11 | none | Reads `*(0x61c000)` and 36-byte records to the matching id or a `-1` terminator; no stores |

The owned image's direct transfers are BL to these eleven entries and the handoff; it holds no literal naming a forbidden range. The only other reference into a forbidden range in records 1/2 and loaders A/B is the replaced reset literal, and no retained service's static closure enters one. The `0x1366c` indirect call depends on the unchanged id table. Other computed references and the vector handlers' targets through the words at `0x3fff0040`–`0x3fff0048` are unresolved.

The owned handoff differs from the original only in registers loader B overwrites first. Both leave masked A32 little-endian SVC, r0 = image-4 word `+0xc`, r3 = target = id-1 header `+0xc`, SP = `0x70000800` and r6–r11 as `0x118b8` leaves them. Stock `0x119e4` leaves r4 = `0x1f` and r5 = `0x11834`; r1, r2, r12, LR and the flags also differ. An interprocedural all-path walk from `0xd0020` through loader B's callees finds each of them overwritten before any read: r5 at `0xd0020`, LR at `0xd0024`, r1 at `0xd27b8`, r2 at `0xd27bc`, flags at `0xd27c4`, r4 at `0xd0038` and r12 at `0xd0914`, before any indirect transfer. The walk assumes callees return to their callers and models no exception.

Staging the original DAT (stored record-2 slot SHA-256 `909db53de40a0319541a30314bd942cb119eca3de1147e2b856eecf71c1a4809`) changes only the main checksum, reset literal and image bytes; the record-1 slot, other parts and record-2 bytes outside `[0x20,0x24)` and the image are unchanged, as independent re-extraction confirms. The image sits in record-2 padding with no source-word reference found. The accepted schema-3 scenarios observe the candidate under the declared immutable-byte and forbidden-access guards. The owned wait polls every 512 core-1 entries versus stock's 304; held and delayed-release controls qualify those paths, not other inputs, exception paths or all-core write closure.
