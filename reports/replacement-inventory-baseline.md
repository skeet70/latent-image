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

The allowlist bounds what the payload calls and stores. It does not cover stock callers into a retained body, callbacks registered with stock services or computed references to them. No published qualified increment establishes that closure or a forbidden-fetch guard for a retained body.

For the global dispatcher `FUN_00bfb028` ([#128](https://github.com/skeet70/latent-image/issues/128)), the caller, event-store and MOVT-limit obligations in the checker inventory `closure-inventory-1.json` (checker `9804fb2`, not admitted) arise where a candidate entry splits a straight-line window after the instruction that sets the value: an aligned word equal to a window start, or a B/BL-shaped word targeting it, in one of the five pinned images. The inventory carries each data-word origin; E-MODAL-ENTRY-SOURCES-150 checks them against the bytes and locates the MOVT-limit splits.

One source class has a consumer path in the bytes. Image 4 holds three register-write streams of (value, register) pairs at `[0x007a2430, 0x007b0838)`, `[0x007b0840, 0x007c2d18)` and `[0x0081c630, 0x00820f10)`, submitted through `0x006598ec` to the engine at `0xfe80c1c0`. Value slots in the first two supply the entry candidates for the callers at `0x00c0c040`, `0x00c0c0f0`, `0x00c0c200` and `0x00c0f8ec`, the event stores (`0x00c0c014` also has plain glyph-row sources), the MOVT limits at `0x00c0dcf0`, `0x00c0ecf0` and `0x00d80000`, and 14 of the 81 table-name words. Those edges are not yet discharged. E-MODAL-STREAM-RESIDUALS-150 bounds the code-pointer table at `0x007a22c4` to its four entries and reduces the other CPU paths to named residuals: the plain size constant `0x007b7000`, which reaches media-message payloads and three request records; reads of the 10×12 table at `0x007a20d4`, which reach only a compare or a pin-register store, and whose row is unchecked; and a 640×480 source transfer from `0x007a20d0`. In addition, 193 aligned words in other objects name the streams and close only with the objects that hold them.

E-MODAL-ID4-DATA-ROLES-150, E-MODAL-ID5-DATA-ROLES-150 and E-MODAL-PLAIN-DATA-ROLES-150 bind additional objects and consumers: image-4 transfer descriptors and sample buffers, image-5 fixed-point parameters and halfword tables, and plain-image upload, coordinate, rule and bitmap data. These are conditional premises, not checker discharges. Computed references, escaping pointers, unbounded indices, unresolved transfer semantics and other entry sources remain open. The checker does not yet consume these object-role proofs.

The checker can discharge a candidate only by object-consumer closure checked against the pinned bytes. Each entry, predecessor and table-name obligation carries its source word. A declared object names an extent, a role predicate the bytes must satisfy and its consumers. Every materialization of an address in the extent must be a declared consumer: a MOVW/MOVT or MOV/MOVT pair, a PC-relative literal, an aligned word in any `address & 0x3fffffff` alias class, or a base whose index bound reaches the extent. Each consumer is walked by the existing census, and a loaded value that reaches a PC write, a register branch, an escape or an unmodeled instruction keeps the edge; a hardware sink is a declared register store. A word naming the object is closed with the object that holds it, and a B/BL-shaped word is an entry only if its object can be fetched. A failed predicate, an undeclared materialization or a changed byte leaves the obligation open.

The checker now classifies stream consumers this way (E-MODAL-SCALAR-CONSUMER-150). Its `declared_objects` output carries the three streams as conditional objects, checks the register-word predicate against the bytes (it holds for all three) and marks each enumerated MOVT, PC-relative literal, aligned-word and constructed consumer discharged, return-conditional or open. A constructed consumer (E-MODAL-CONSTRUCTED-CONSUMER-150) is a register that a MOV, MVN, ORR or other data-processing instruction, including PC-relative arithmetic, or a MOVW, extend, multiply, bitfield or base-writeback instruction computes, and that takes a value naming the object on some path of the checker's construction scan through each image: along fallthrough, and restarted with unknown registers and flags at every branch target and data-word entry. A call or SVC leaves every register and the flags unknown there. Values that reach an instruction only through a branch, a call, a load other than a PC-relative literal word or an unknown index register, and bases indexed into an extent, are still not enumerated. A MOVT, literal or constructed consumer is discharged only when every path from it reads the value as a TST, TEQ, CMP or CMN operand until it is overwritten. It is return-conditional when a path instead reaches a return with the value in r2, r3 or r12; it then rests on the unverified premise that no continuation reads that register first. No consumer is discharged on the pinned images. The plain MOVT at `0x012369e8` is return-conditional: its `0x007b7000` is only compared at `0x012369ec` and is overwritten at `0x012369f8` on one path, but stays in r3 at the return `0x01236a2c` on the other. Manual byte reads narrow that premise. A scoped scan of the reference forms above plus B/BL, PC-relative and Thumb call targets finds BL `0x011f4fec` as the sole caller, and its continuation overwrites r3 before reading it; computed, register-built and exception entries remain unexcluded. Apart from frame-save stores, the examined path's data writes are confined to `[0x014fe898, 0x014fe954)`. It stays conditional until the saved return slots are shown disjoint from that range and safe from asynchronous writes. The MOVTs at `0x01239e48` and `0x0123c1ac` store the same value. Four stream-C MOVTs with unresolved lower halves reach address uses at `0x00673780`, `0x006a00b8`, `0x006e7998` and `0x00732580`, and two stay live across the calls at `0x0082ee8c` and `0x0082f158`. The 198 aligned words naming the streams stay open with the objects that hold them. The construction scan adds 30 constructed consumers, 3 in stream A and 27 in B, and finds no others. Each is a value word of its own stream that decodes as data processing on pc and stays open at the unmodeled register word after it. Streams A, B and C keep 39, 70 and 127 open consumers. Entry and table-name obligations sourced from stream words now name their object and stay open. The array and table census still traces pointers only from MOVT and PC-relative literal constants and aligned words, so its obligations, writers and uncovered-constant-forms are unchanged.

The checker no longer assumes callees preserve registers (E-MODAL-CLOSURE-CALL-INVALIDATION-150). After a call or SVC every general register and the flags are unknown. A tracked value held across a call stays tracked as a derived pointer, and a tracked pointer that a callee may read in any of r0-r14, or that is live in any of them at a return, is an escape; lr is excluded only at a BL or BLX, which overwrite it. The current inventory therefore has 6,904 obligations: the 114 of the earlier call model unchanged, 286 stores into the state/event arrays through a pointer held across a call, and 6,504 pointer escapes, 6,061 of them a pointer left in a register at a return (5,873 in r3). Many are conservative and none is waived. Callers, controllers, cells and targets are unchanged. Exactly one store left the resolved writes: the bank's `rb_ui_pop` write of 0 to the modal-75 state cell at `0x00a8654c`, whose address is held across two calls; it is now a computed-index-store obligation.

Byte reads of that store's two calls do not resolve it (E-MODAL-RBPOP-CALLEES-150). `rb_call_stage_open` reaches `0x00d3d858`, whose call tree has no register call, computed jump or published word. It saves r5 at entry, and its only return to the caller restores r5, SP and the return address from that frame; its other stores are lower stack words and five words at `0x0094f64c`. A zero divisor at `0x0004d978` takes the one other exit, `svc #0x1010` followed by a branch to itself at `0x00043610`; no byte there branches back, and the continuation depends on the unexamined SVC handler. The traversal is not a termination proof. If the call returns through that frame, r5 is unchanged only under unproved premises. Every executed instruction must be the pinned, publication-adjusted word, including against the call's own stores, which the unresolved physical map does not separate from code. No exception or other agent may intervene. The saved frame must be physically distinct from those stores, which numeric disjointness does not show under the X-M1 aliases. The 100-byte stack interval below the first-call SP must not wrap. `rb_call_transition` reaches `0x00c12aac`, which never writes r5 but calls the global dispatcher for controller 100, where r5 survives only in a stack slot across a register call to the cell action. With controller 100 in state 0, the 91 event stores of `0x00c12aac` select 61 action functions, and only the event-0 action is a leaf. Decoded with `host/firmware/A32.hs`, their direct closures have at least 122 register-call or computed-jump sites, an SVC and 21 encodings the decoder does not admit, re-enter the dispatcher and reach 32 bank functions through published branches. The store remains a computed-index-store obligation. Resolving it needs callee summaries for those actions, the controller-100 state and table premises, the code-integrity, exception, SVC-handler and alias premises for both calls, and an exclusion of other entries into `rb_ui_pop`.

### 8.3 Capability view

Section 2 holds the evidence behind each category. Gaps that no open child issue covers belong to #127.

| Capability | Owned, qualified | Retained executable and shared data | Open issue for the next action | Not covered by an open child issue (#127) |
| --- | --- | --- | --- | --- |
| Startup and shutdown | Owned first-stage entry and loader-A handoff on the public runtime; retained-menu continuation on the reviewed private runtime (#126), not full startup | Boot ROM (declared retained), retained record-1 helpers, record 2, loaders, application images and later initialization duties | [#126](https://github.com/skeet70/latent-image/issues/126) public controls/menu replay; [#134](https://github.com/skeet70/latent-image/issues/134) peer baseline and held sentinel pass, delayed release fails before its gate; hardware route [#85](https://github.com/skeet70/latent-image/issues/85), [#60](https://github.com/skeet70/latent-image/issues/60) | Ownership of later startup stages, replacement shutdown, power-off persistence |
| Controls and menus | #121 classifiers; #123 field-row binding | `FUN_00bfb028` for every controller; modal-75 actions in 8.1; the snapshot service `FUN_0061ca2c`, `FUN_0061cd14`, `FUN_0061ceac` and accessors for every stock caller (#132 owns the bank's store call in source); key handlers `FUN_00c6f14c`, `FUN_00c70528`; render probe `FUN_00d52684`. Data: cell table `0x00cd1ae4`, action table `0x00d04080` with word 1 interposed at install, SET gates `0x009544f8`, `0x00950474`, `0x00950478`, `0x00954500`, cursor cells `0x009571d8`/`0x009571dc`, current row `0x0095732c`, saved-snapshot tables `0x0093eb74`, `0x009412d4`, `0x00944414` | [#128](https://github.com/skeet70/latent-image/issues/128) dispatcher and editor cancellation; [#132](https://github.com/skeet70/latent-image/issues/132) snapshot-service publication and guard; [#133](https://github.com/skeet70/latent-image/issues/133) SET, confirm and leave; [#124](https://github.com/skeet70/latent-image/issues/124) PLAY and physical shutter inputs | Other controllers behind the global dispatcher; other menus; physical key timing |
| Live view | None | Sensor, processing and display pipeline; the Classic Chrome fragment is a project patch over it | None | Whole category |
| Autofocus and manual focus | None; the contrast policy and AF RAM ring are unqualified | AF orchestration, lens messages, statistics producers | None; [#109](https://github.com/skeet70/latent-image/issues/109) and [#113](https://github.com/skeet70/latent-image/issues/113) keep their own acceptance | AF ownership; manual focus is not inventoried |
| Exposure and capture | None | S2 admission through `0x00c58fc4`, `0x00d0da4c`, `0x00d126f0` and `FUN_00c12aac` ([cardless capture](cardless-capture-feasibility.md)); capture descriptors, JPEG encoder and DMA, sensor services | #124 full-press input, JPEG cursor and DMA, media inspection; [#129](https://github.com/skeet70/latent-image/issues/129) consumes it | Exposure control and capture paths outside #129's selection |
| Image development | None | Develop dispatch and processing services; D3 and Classic Chrome data are project data over them | #129 development-step lifecycle | Other develop paths, RAW conversion |
| Save and playback | None of stock; the recipe-bank storage lane is project C over stock media workers | Ordinary save chain `0x00ea0934` → `0x00e99d4c` → `0x00e518c0`; media open, write, close and completion `0x007115a8`, `0x007116e8`, `0x0071176c`, `0x0071c634`; playback | #129 save facade, phase completion and media permission; #124 PLAY entry and card-sector inspection | Playback rendering, zoom, deletion |
| Persistent settings | #120 validator, native only | Area-3 log with both callbacks; stock getter `0x0061e110` and commit `0x00d4fc74` used by the bank; live settings record `0x0096cc64` | None | Validator integration and the area-3 writer, both gated on a persistence decision; unbound fields and writers |
| Calibration consumption | None | Adjustment cache, `f7` gate and their consumers; calibration data is declared retained resource data | None | Consumer code is not inventoried |
| External interfaces | Packaged emulator RAM-push publication and rollback (#121, #123); the host transport is project code | Firmware USB, PTP, SD host and jig handlers; the installer's CPU1 job channel | #124 read-only card inspection and [#125](https://github.com/skeet70/latent-image/issues/125) native backend are emulator tooling, not firmware ownership | Firmware-side interface handlers |
| Failure recovery | None | Stock recovery, recorder and boot-entry paths; R1 to R3 are stock-firmware evidence | Hardware route #85, #60 | R4 and replacement recovery |
| Runtime services, across categories | None | Scheduler, tasks, queues and mailboxes, interrupts, allocator, render and display, storage drivers | #129 names its retained sensor, processing, storage, scheduler and driver services | Contracted retirement of each service |

The unqualified #129 source in `replacement/save` implements the selected save facade, callbacks and phase completion. Its retained ABI converts flags to typed values, and its guest fault controls use the production validator with exact return-register or memory mismatches. Host compilation and typed-value smoke checks passed. Original-versus-owned execution, guest-control outcomes and lifecycle integration remain pending; no timing, device behavior or runtime retirement is qualified.

The unqualified media-permit source in `replacement/media_permit` retains the selected frame, command and bitmap services. Review found that its defect-control acceptance ignored truncated provenance and writes outside the declared allowance. A throwaway host smoke reproduced both false acceptances; the corrected predicate preserves its positive control and rejects both cases. The poisoned-body control stops after the stock entry veneer and is not a pre-fetch retirement guard. Original-versus-owned execution, guest-control outcomes and integrated capture remain pending.

### 8.4 Milestone blockers

| #117 milestone | Prevented by | Owner |
| --- | --- | --- |
| Own accepted application behaviors | Published qualified ownership covers only bounded decisions and one service lane. The modal controller, capture and save lifecycle and the remaining state machines run stock code. | #128, #129, #124; remaining categories #127 |
| Retire proprietary application/runtime implementations | No published qualified increment retires a stock body or has a forbidden-fetch guard. The #132 snapshot service has a static caller closure and native original/owned qualification, but its stock entries are not published over. Native qualification of tail-call veneers and the media-permission path needs a backend that executes them. | #125, #128, #129, #132; remaining bodies #127 |
| Replacement startup in emulation | The public correct first-stage handoff and peer held-sentinel control pass. Public negative controls/menu replay, delayed-release qualification and ownership of the remaining startup path are open. The latest public-control batch was refused before guest execution at its start-temperature gate. | #126, #130, #134 |
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

[#134](https://github.com/skeet70/latent-image/issues/134) observes a retained boundary, not an owned startup increment or stock-code retirement. The existing first-stage witness observes core 0 only; zero rendezvous cells do not identify their writers. Both the historical observer baseline (`E-PEER-NATURAL-RESET-BASELINE-150`) and the fresh combined-SDK natural-prefix requalification (`E-PEER-COMBINED-NATURAL-RESET-150`) pass the production receipt consumer. The combined held-sentinel control also passes; the delayed-release attempt fails before gate qualification, leaving that control open. The source contract below is supported by `E-PEER-ENTRY-FRAME-HANDOFF-SOURCE-150`; configured engine laws and observation semantics are supported separately by `E-PEER-MODEL-OBSERVATION-SOURCE-150`.

The historical observer SDK passes 70 package harness tests (`E-PEER-OBSERVER-SDK-HARNESS-150`); the combined SDK at `97a6ebe` passes 79 checks against the same qualified public model. Serialized wrong-core/wrong-handoff controls exercise the host consumer, not guest fault behavior. Neither harness result retires retained code.

#### Exact X-M1 1.50 source contract

The original DAT has SHA-256 `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`. The retained record-2 decode and existing re-extraction both hash to `2f1bf3f0e483b892d2890599679c1b515cfc1be8b8c27cbb06d4d913e428abb4`, length `0x4c80`, loaded at `0x10000`. Direct word reads independently agree with the existing listing at record offsets `0x1800`–`0x19a4`, `0x1cc0`–`0x1ccc` and `0x1f40`–`0x1fb8`. These are byte/source findings, not observed execution.

| Observation boundary | Source-bound fact | Qualification limit |
| --- | --- | --- |
| Record-2 entry `0x11800` | First instruction calls execution-state initialization `0x11854`; the vector word at record offset `0x20` supplies `0x11800`. | Entry precedes initialization. Incoming registers, current/banked SP and LR, CPSR/SPSRs and exception history are not established by the vector literal. |
| State and stack setup | `0x11854` selects little endian, masks IRQ/FIQ, clears asynchronous-abort mask and initializes privileged modes/SPSRs. `0x118b8` programs TCM and initializes SP banks from literal `0x70002000`: IRQ `0x70002000`, FIQ `0x70001000`, SYS/USR `0x70000e00`, ABT `0x70000c00`, UND `0x70000a00`, SVC `0x70000800`. | These are the values the instructions request, not measured physical TCM or banked-register readback. A receipt of current registers does not capture every bank. |
| Main-call frame and tail `0x11f40` | `BL 0x11cc0` at `0x11838` supplies LR `0x1183c`. The push saves `{r4,r5,r6,r7,r11,lr}`, **24 bytes**; then `r11=initial_sp-4`, `sp=initial_sp-64`. The saved frame starts at `r11-20` and saved LR is at `r11`. Thus tail `r11=sp+60`. The discriminator saved in r6 is 1 on the configured secondary path. | The saved incoming register values other than LR are observations, not fixed constants. The frame is not 16 bytes. Restored SP means tail SP + 64, not the unconstrained SP sampled at record-2 entry. |
| Rendezvous `0x11f4c`–`0x11f7c` | Store zero to guest `0x40020004`, DSB, then read guest `0x40020000` and repeat only while it equals **exactly `0x22b`**. The retry body has a 100-count busy delay. | Any non-`0x22b` value exits this comparison. The source has no finite wait bound or general readiness check. |
| Target and argument publications | `0x11f98` writes `0x14c80` from segment-id-1 record `+4` → header `+0xc`. `0x11fb0` writes `0x14c84` from original image-4 word `0x61c40c`. Retained loader-B header gives load `0xd0000`, entry `0xd0020`; original image-4 word `+0xc` is `0x0069cdf8`. | Missing segment lookup has no null check at the dereference. Original image-4 argument is distinct from loader-B entry and does not establish application execution. |
| Handoff `0x11850` → loader-B entry `0xd0020` | The epilogue restores the six saved words and SP, returns to `0x1183c`, selects SVC, loads r3 from `0x14c80`, r0 from `0x14c84`, then executes `mov pc,r3`, not `bx`. | Planned receipt requires masked A32 little-endian SVC, r3/target `0xd0020`, r0/argument `0x0069cdf8` and restored SP. Reaching loader-B entry would not establish loader-B wait completion or an application handoff. |

Record 1's source writes `0x22b` to both cells at `0x1e64`/`0x1e68`, then clears its own cell at `0x1ff8`. It clears completion guest `0x400a00c4` at `0x1fec`. The [boot report](update-validation-and-boot.md#second-supplied-boot-record-companion-initialization-and-shared-image-handoff) retains the broader initialization and lookup analysis. Core-qualified runtime observation is still needed to attribute these writes on the selected execution.

#### Configured model laws and unresolved physical facts

The pinned FujiMod source is `3d90035d7d8d651e126f89f94f30765506e6f53f`, with the checked-in Unicorn context-alignment patch. Its EXR II catalog declares two separately scheduled Cortex-R5F proxy contexts, with the adapter's R4F guard. CP14 selector `(opc1,crn,crm,opc2)=(0,2,0,0)` returns configured identities `0x4003`/`0x5003`. The X-M1 discriminator at `0x11aa0` tests equality to `0x4003`, returning 0 for equality and 1 otherwise; it does not prove the other physical identity is `0x5003`.

Core 1 is configured held at PC `0x3fff0000`. Record 1 copies `0x3e8` bytes from `0x10000` through guest alias `0x7fff0000`. The catalog maps `0x3fff0000` and `0x7fff0000` to the same secondary SRAM backing on both cores and additionally maps core 1's `0xffff0000` page there. The declared IRQ1 software pulse for line 120 drives the one-shot `core-release` model; that model deasserts reset without inspecting code or setting PC. These are configured release/mapping laws, not measured silicon reset circuitry. The copied exception literals `0x3fff004c`, `0x3fff0064`, `0x3fff0050`, `0x3fff0058`, `0x3fff0060`, `0x3fff00c0` and `0x3fff0090` remain unresolved physical vector aliases/handlers. They must not be relabeled as captured-ROM handlers.

The model initially aliases low 16 KiB to captured ROM and exposes a decoder choice for low DRAM; guest `0x40000000` aliases DRAM physical zero on both cores. A watch must include current/static maps and decoder choices before either core runs. Relevant physical spans are rendezvous `[0x20000,0x20008)`, completion `[0xa00c4,0xa00c8)` and target/argument `[0x14c80,0x14c88)`. CPU records carry core, PC, guest/physical address, width, value, before bytes, byte order, aggregate instructions and tick. They are **attempted accesses**, not proof a store committed. Handoff/loader-entry readback must independently show both cells zero and the expected target/argument. Completion is observed, not required to remain zero while core 0 runs concurrently.

Transfer records distinguish `model-bus-read`/`model-bus-write` from `host-read`/`host-write`, with null core/PC/instruction/tick fields and no CPU value/before/byte-order metadata. Snapshot physical readback itself adds `host-read` rows. These records cannot prove a guest CPU writer. The watch also collects CPU reads/fetches: polling can exhaust its bound and must retain a violation rather than truncate into a passing receipt.

#### Historical observer baseline and control prerequisites

The [peer-witness plan](../emulation/README.md#secondary-core-peer-witness-pre-execution-plan) was exercised once on the original validated DAT and captured 16 KiB ROM, with no checkpoint, owned candidate, host-seeded rendezvous or forced core release. The receipt stops on core 1 at entry `0x11800` (69,001 aggregate instructions), tail `0x11f40` (5,845,480), handoff `0x11850` (5,845,534) and loader-B entry `0xd0020` (5,845,535), with configured CP14 identities `0x4003/0x5003`. Entry readback is cells `0x22b/0x22b`, completion zero and target/argument `0xffffffff`; tail readback is cells `0/0x22b`, completion/target/argument zero. Handoff and loader-B entry independently read back both cells and completion zero, target `0xd0020` and argument `0x69cdf8`; both capture r0/r2=`0x69cdf8`, r3=`0xd0020` and SP=`0x70000800`. Tail SP=`0x700007c0`, r11=`0x700007fc`, r6=1 and the saved frame `[0x1f,0x11834,0,0x70000000,0,0x1183c]` match the source contract.

The watch retains 5,063 rows: 5,032 CPU reads, nine attempted CPU writes, 20 host reads from stop snapshots and two model-bus writes, with no violation. Attempt metadata attributes the primary clear to core 0/PC `0x1ff8`, guest `0x40020000`/physical `0x20000`, value zero, previous bytes `2b020000`; peer clear to core 1/PC `0x11f4c`, guest `0x40020004`/physical `0x20004`, value zero, previous bytes `2b020000`; and publication to core 1/PC `0x11f98` and `0x11fb0`, physical `0x14c80/0x14c84`, values `0xd0020/0x69cdf8`, previous bytes zero. These are pre-write events, separate from the committed readback snapshots above. Model-bus writes have no CPU identity and supply no guest-writer attribution. The original core-1 equality-load site `0x11f74` appears once, not as a held-sentinel control.

Held-sentinel and delayed-release have source-bound guest-control builders and separate schema-2 observation/acceptance paths, described in the [wait-control interface](../emulation/README.md#source-bound-guest-side-wait-controls). Their gate-only change at `0x1ff4` and delayed 104-byte veneer preserve the original clear instruction, record 2 and loader B. The held control passes with four original equality loads returning `0x22b`, unchanged final core-0 gate registers and no handoff. Delayed release fails before gate qualification and must separately qualify the original core-0 STR attempt, PC transition, unchanged CPSR and immediate committed zero before handoff. Counters measure aggregate entered instructions, not retirement; a completed LDR count may equal its following unconsumed stop count. Stack and active-alias requirements apply at each control's own gate; held observations do not establish delayed bindings. Host memory seeding, forced release and receipt mutations cannot substitute for guest controls. Wrong-core/wrong-handoff synthetic receipts exercise only the host consumer (`E-PEER-WAIT-CONTROL-SOURCE-150`).

The frozen host bounds were active for this baseline: 120 seconds per decoder/driver child, 1 MiB per stdout/stderr stream, process-group cleanup and retained logs through `FujiAF.Host.runBounded`. The existing thermal service reported success, 4.682 seconds runtime, 1.406 seconds CPU and 338.1 MiB peak memory with no swap. A reviewed Haskell resource-only driver launcher verified the guard service and its MainPID ancestry, 4 GiB memory, zero swap, `cpu.max=25000 100000` and CPUs 4,5, then admitted the sole start sample at 55.125°C. It started no second slot or guard and exec'd the immutable SDK with unchanged arguments. No loader-B instruction, completion wait, application entry, physical behavior or held/delayed guest control is qualified by this bounded prefix.

#### Combined-SDK natural-prefix requalification

The fresh natural-reset receipt at SDK source `97a6ebe`, on unchanged public model `3d90035d`, reproduces the four core-1 boundaries and state snapshots above: entry 69,001, tail 5,845,480, handoff 5,845,534 and pre-execution loader-B entry 5,845,535 aggregate entered instructions (`E-PEER-COMBINED-NATURAL-RESET-150`). The original DAT/ROM, schema-1 contract and 20,000,000-entry limit remain unchanged. SDK result and host consumer both pass; machine failure, producer error and watch violation are null. Its 5,063 watch rows comprise 5,032 CPU reads, nine attempted CPU writes, 20 host snapshot reads and two unattributed model-bus writes.

The fresh watch attributes the cell-0 clear attempt to core 0/PC `0x1ff8` at 2,788,426, before core 1 reaches the tail at 5,845,480 and reads cell 0 zero. Core 1 attempts its own cell-1 clear at PC `0x11f4c`/5,845,484, then reads cell 0 zero at `0x11f74`/5,845,490. Later handoff and loader-entry readbacks show both cells zero and the independently derived target/argument `0xd0020/0x69cdf8`. This natural receipt contains no immediate post-STR readback or one-entry clear microstep proof. Entered counts and pre-write rows do not establish retirement or that stronger completion claim.

The run binds the older frozen c546975-era host/guard/shared-module bundle, not PR122's subsequently merged affinity propagation. The private launcher sets process/thread affinity at service creation and checks all 13 tasks across three service-member processes plus its entry thread as CPUs 4,5 before SDK admission. This is a point-in-time affinity check, not hard cgroup cpuset containment. It also verifies the exact guard ancestry/slot owner, 25% CPU quota with 100 ms period, 4 GiB memory, zero swap and a whole-service 120-second SIGKILL/control-group deadline. The sole admission sample is 56.625°C; the canonical 85°C monitor remains active. Service output reports success, 6.238 seconds runtime, 1.569 seconds CPU and 391.1M peak memory with zero swap. Post-exit read-only inspection finds the exact service unloaded, its cgroup and worker/slot-owner processes absent, and no held whole-file slot lock.

The SDK's 79 passed checks and this bounded natural prefix do not themselves qualify guest controls. The separate held result below observes its actual core-0 gate; the delayed attempt fails before those checks. Original `0x3fff00xx` aliases remain unresolved. Record 2, loader B and later startup remain retained dependencies.

#### Combined-SDK guest-control results

The held-sentinel schema-2 guest control passes the SDK and production host consumer at source `97a6ebe` (`E-PEER-COMBINED-HELD-SENTINEL-150`). It independently observes core 0 at `0x1ff4`, aggregate entered count 2,788,424, SP `0x700007f0`, LR `0x33f4`, CPSR `0x600000d3`, ATCM/BTCM `0x70000011/0`, decoder choice 1 and low RAM `0 -> physical 0`, size `0x4000`, permissions 7. Its bindings retain 40 bytes through core 0's private TCM at `0x700007d8`, plus exact virtual/physical code reads at `[0x100,0x168)` and `[0x1fe0,0x2000)`. This does not equate private TCM to Board physical RAM or silicon.

Core 1 reaches entry at 69,001 and tail at 5,845,480. Its four original `0x11f74` loads return `555`, with post-LDR/pre-CMP `0x11f78` stops at 5,845,490, 5,845,794, 5,847,098 and 5,847,402 aggregate entered instructions. Each attempted-load count equals its next stop count, with matching loaded registers and independent snapshots of cell 0 `555`, cell 1 zero and completion/target/argument zero. The fourth comparison/branch is not observed executing. All final primary registers, PC, SP, LR and CPSR equal its gate context; final stack memory was not separately resampled. The watch has six CPU-write attempts, four CPU reads, 35 host reads and two model-bus writes, with no violation. The peer-cell clear attempt is distinct from committed zero at subsequent load snapshots. No primary clear attempt, nonzero publication, handoff or loader-entry stop appears; clear readback remains null. Gate-to-fourth-load count 3,058,978 stays below 4,000,000; total 5,847,402 and 47 rows stay below the unchanged limits.

The delayed-release schema-2 attempt executes but fails before its gate (`E-PEER-COMBINED-DELAYED-RELEASE-REJECTION-150`). At aggregate entered count 2,103 the only stop is core 0/PC `0x14c`, labeled `peer-acknowledged`; the producer reports `control peer acknowledgement out of phase`. Numeric core/address identity does not establish a qualified veneer acknowledgment. No gate, peer entry, tail or wait load is observed; bindings, final primary and clear readback are null. The five watch rows are host snapshot reads only. Both cells, completion and publication cells read zero, without any CPU-writer attribution. The actual process exits 1, machine failure and watch violation are null, and the production host does not report acceptance. This supplies no original-clear completion, publication, handoff or loader-B proof. Delayed release remains unqualified.

Both actual candidate/manifest/contract sets hash-identically to their separately frozen preparations. Both runs retain the older c546975-era host cohort and unchanged 10,000,000-entry, watch/receipt/stream, 120-second service/child, 25% CPU, 4 GiB and zero-swap bounds. Their sole start admissions are 58.250°C held and 54.125°C delayed, under the canonical 85°C monitor; these are not future-start or ending measurements. CPU 4,5 checks are point-in-time process/thread affinity, not hard cpuset containment. The held service reports success, 7.046 seconds runtime, 1.780 seconds CPU and 465.3M peak memory; delayed reports status 1/exit-code, 3.581 seconds runtime, 912 ms CPU and 428M peak memory; both report zero swap. Exact owned units/workers/cgroups/slot owners are absent afterward, and read-only lock queries find no owner. Historical evidence rows and immutable preparation packets/receipts remain unchanged. These controls do not complete #134 or #117 or retire retained startup code.


