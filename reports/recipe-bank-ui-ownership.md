# Recipe-bank UI ownership increment

This report supports [#121](https://github.com/skeet70/latent-image/issues/121) and the [accepted replacement plan](../spec/11-incremental-firmware-replacement.md#user-visible-ownership-pilot). It selects the smallest read-only ownership increment in the existing recipe-bank UI, records its consumed C ABI and evidence, and states what remains open.

## Selection

The bank's page and row-token classifiers were static-inline C in `menu/recipe_bank_ui.h`: `rb_ui_page` maps a frame class to `RbBankPage` and the sub-page slot, and `rb_ui_decode` maps a selected row token to `RbBankItemRef`. Every entry, navigation and leaving decision in the bank consumes them: the shooting-menu rows `0x7d`/`0x7e` in `rb_ui_child_enter`, the SET (`0x8`) and LEAVE (`0xd`) cells of `rb_bank_interpose`, draw-present `0x816`, draw-root, key-left/right, and the edit/dialog routes.

The increment moves those two classifiers to a `no_std` Rust module and keeps the C functions as inline ABI wrappers. No call site, enum, struct layout or `slot_out` contract changes. The old classifier bodies are removed; there is no C fallback.

Read-only scope covers the complete packaged chain. Entering the slot or SELECT CUSTOM SETTING list loads the resident bank image through `rb_slot_load`/`rb_load`; that is retained RAM scratch, not a persistent write. The exercised chain never presses Save Current, Edit, Reset, a dialog OK, a field OK or an OK on a SELECT row, so `rb_bank_action`, selection apply and storage writes are unreachable. No capture or media action occurs.

Retained: the stock modal delegates `FUN_00c72e10`, `FUN_00c75ed4` and `FUN_00c76650`, the transition `FUN_00c12aac`, the redraw/draw paths, storage and probe kernels, the bank ELF allowlist and the C enums and `RbBankItemRef` ABI. Owned: the three pure classification decisions. Retired: the inline C page and token if-chains.

## Interface

`replacement/recipe_ui/src/lib.rs` is a plain Rust module for the single bank crate root chosen for the final link. It exports three `#[no_mangle] extern "C"` functions:

| Export | Result |
| --- | --- |
| `rb_ui_route_page(cls)` | `RbBankPage` discriminator, 0 through 7 |
| `rb_ui_route_slot(cls)` | sub-page slot, 0 through 6, otherwise 0 |
| `rb_ui_route_item(token)` | `kind \| slot << 8 \| verb << 16 \| selection << 24` |

The page and slot are separate exports on purpose. A packed page and slot made clang emit `UXTAB` in `rb_status_present`; the native runner does not admit that instruction, and the eleven expected-refusal UI cases stopped as `unsupported_instruction`. Splitting the exports removed the `UXTAB` without changing the runner's admission set. The diagnostic was retained and the candidate changed, so required unsupported execution blocked qualification rather than becoming a pass. This is an engine-support boundary resolved in the design, not a permissive fallback. A `returned` case still has to match its public claims; the harness's `native_case_status: failed` from declared ABI-workspace differences is not treated as a case pass.

The exported seam wraps the raw `u32` class in `MenuClass` and the raw token in `RowToken`; inward results carry `BankPage`, `Slot`, `Selection`, `BankVerb` and the closed `BankItem` enum. Raw ids and the packed word appear only at the three C exports. Unknown classes and tokens still return `NONE`/`0`.

`menu/recipe_bank_ui.h` keeps `rb_ui_page` and `rb_ui_decode` as `static inline` wrappers around the exports. `rb_ui_decode` retains its zero-initialized `RbBankItemRef`, so field and padding behavior of the by-value return is unchanged. Rust makes no calls, stores, allocations or mutable-global accesses.

## Evidence

The Rust module's host tests pass 4 of 4 and call only the exported C ABI: page and slot boundaries including the adjacent field/sub and sub/edit classes, every verb slot and verb, every select code, and unknown or gap tokens returning zero. No test reads internal `PageRoute`, `BankItem` or `ItemRef` fields.

A one-crate smoke with `#![no_std]`, this module included by `#[path]` and one `#[panic_handler]` compiles under the #119 toolchain. The archive exports `rb_ui_route_page`, `rb_ui_route_slot`, `rb_ui_route_item` and one mangled `rust_begin_unwind`; the module owns no handler. Re-linking the typed module into the bank kernel leaves `__rb_image_end` at `0x00be9c0c`, and the disassembly contains no UXTAB, UXTAH, SXTAB or SXTAH. The composed bank crate with the retained-service module and one handler compiles and links against the bank C objects with the service trampoline: `__rb_text_end` is `0x00be8ca4`, `__rb_image_end` is `0x00be9c24`, and the disassembly contains no extend-add or Thumb form. That is a link and ISA check, not a case run.

The fixed native UI workload passes 55 of 55 cases with the UI Rust staticlib linked into the bank kernel. Those cases are the modeled native boundary, not the full read-only lifecycle. Final qualification consumes [#120](https://github.com/skeet70/latent-image/issues/120)'s consumed contract and runner convention and [#123](https://github.com/skeet70/latent-image/issues/123)'s shared packaged service qualification; neither is established by the 55 synthetic cases or by this report. The combined `//replacement/bank:bank` archive, which also contains the retained-service module and the probe change, has not been run through the 55 cases yet. The kernel is 32,024 bytes, `__rb_text_end` is `0x00be8c9c` and `__rb_image_end` is `0x00be9c1c`, below `dataBase` `0x00bea200` and the `0x8300` code cap. A control kernel built from `HEAD` also passes 55 of 55; the negative cases `n-select-unset-refused`, `n-save-write-fail`, `n-edit-write-fail`, `n-reset-write-fail`, `n-rename-fail`, `n-refused-dialog` and the media-refresh case remain passing refusals. `//emulation:recipe-bank-ui-plan-tests` and `//emulation:ui-trajectory-tests` pass. The 55-case result predates the typed seam commit `a2786b9`; that commit is covered by the exported-ABI host tests and the kernel link/disassembly check, not by a repeated 55-case run.

The flash candidate `package-20261024b` with `f7-1.nand` and `bank-selection-stage4/fast-ui/card-sel.img` ran `emulation/recipe-bank-route-trajectories.json` from the card attach checkpoint. Six of seven segments passed: slot-list entry, seven-row slot navigation, the slot verb page and BACK, return to the slot list, SELECT list entry and eight-row navigation, and cancel. The final handback segment was not reached before the host thermal guard stopped the run.

The failed handback expectation was corrected from observation, not fitted to the failure. After the SELECT list cancels, the stock transition returns to shooting menu row 34 `(1,5,34)`, the stock shooting menu row that owns the bank entry. MENU on that row re-enters the bank row, which produced `(1,645,0)`; the trajectory now repeats entry with UP and asserts the shooting-menu handback rather than a live-view state. The final segment remains unvalidated until a compute turn is granted.

An earlier empty-slot run showed SELECT navigation staying on row 0 when the card's slots were unset. That is an observation of the stock list's disabled-row handling, not yet a pinned resource-absence trajectory. Storage failure and refusal paths are covered by the passing host cases; the packaged run has no new resource acquisition of its own.

## Route choice

The device-driven packaged route is the existing cardless RAM push, not a new flash DAT. `emulation/recipe-bank-route-ram-push-trajectories.json` starts from the e2e's focus-area overlay, enters both bank lists, and asserts the empty resident bank's disabled SELECT rows stay on row 0 before cancelling and repeating entry. That is the resource-absence behavior, not a substitute for a populated bank. The flash-candidate trajectory with `card-sel.img` remains baseline evidence for populated slot and select navigation; it is not the packaged Rust route. No flash DAT wiring is added for the automated route.

Human visual assessment needs an interactive surface. The RAM-push e2e terminates after its trajectories and exposes no viewer URL; the existing interactive bank surface is a flash DAT. The smallest same-payload packaging change is to have `build-recipe-package` link the same `//replacement/bank:bank` archive into its kernel, reusing the existing container chain. That is a packaging prerequisite for visual evidence, not a second build system or a hardware publication.

## Qualification status

| Field | Value |
| --- | --- |
| `status` | partial |
| `capability` | false |
| `scope` | Recipe-bank page/slot/token classification behind the unchanged C ABI, bounded native UI cases and one packaged trajectory; no full read-only lifecycle claim |
| `preserved diagnostics` | `returned` for every passing case; one earlier `unsupported_instruction` stop at the `UXTAB` site, retained and fixed; no diagnostic reclassified as success |
| `coverage_gaps` | combined `//replacement/bank:bank` archive not run through the 55 cases; typed seam not rerun through the 55 cases; no packaged Rust-candidate completion; final handback/repeat segment unvalidated; resource failure covered at the native boundary only |

The observation projection follows the native UI harness rather than the area-3 solver: `status` must be `returned` and every public claim must match; declared effect regions are confined to the case's ABI workspaces and public claims; `r4` through `r11` and `sp` must be preserved with `pc` at the exit; memory provenance must be present and complete. The harness's `native_case_status: failed` from declared workspace differences is a projection detail, not a pass. Precedence for a mixed result is failed over blocked over unknown over qualified, and a partial domain never sets `capability`.

## Measurement

The #118 fixed workload is the behavior-preserving refactor of these two classifiers. Baseline same-invocation samples are 45.29 s total (1.07 s build, 44.22 s case generation and execution) for the unmodified tree, and 42.58 s (0.98 s, 41.59 s) for the C refactor, under warm Nix/Bazel and page caches, two jobs and sibling load.

The Rust repeat is partial because the single bank crate root and the fixed-command wiring are not merged. With a prebuilt Rust staticlib and a manual `KERNEL_DIR`, the staticlib compiled in 0.24 s and the UI build plus 55 cases took 42.33 s to 43.02 s. That is comparable to the baseline range but is not the fixed command: it omits the Bazel archive target, uses a prebuilt kernel directory and has unknown thermal/load state. It is recorded as a changed condition, not as an improvement. No stock hook, call-site ABI or plan layout had to be rediscovered. The one investigation item was the `UXTAB` admission boundary and the page/slot split.

## Reproduce

Build the bank archive with the fixed targets, then run the 55-case UI workload into fresh directories:

```sh
devenv shell -- bazel --output_base="$HOME/.cache/bazel/latent-image-issue117-recipe-ui" build --jobs=2 //emulation:native-runner //emulation:recipe-bank-ui-cases //host/firmware:build-recipe-bank //replacement/bank:bank
SERVICE_ARCHIVE="$PWD/bazel-bin/replacement/bank/bank.a" BUILD_DIR="$PWD/private-data/analysis/x-m1-1.50/recipes/issue117-recipe-ui/ui-repeat-build" OUT_DIR="$PWD/private-data/analysis/x-m1-1.50/recipes/issue117-recipe-ui/ui-repeat-cases" JOBS=2 devenv shell -- bash emulation/build-recipe-bank-ui-cases.sh
devenv shell -- bazel --output_base="$HOME/.cache/bazel/latent-image-issue117-recipe-ui" test --jobs=2 --nocache_test_results --test_output=errors //emulation:recipe-bank-ui-plan-tests //emulation:ui-trajectory-tests
```

The archive target is an added workflow step relative to the #118 baseline. The packaged read-only route is:

```sh
XM1_EMULATOR_SLOTS=1 devenv shell -- bazel run //emulation:xm1-viewer -- --ram-push-e2e --cardless --unreviewed-relink --firmware private-data/analysis/x-m1-1.50/update/prefix/FWUP0002-prefix.DAT --installed-nand private-data/analysis/x-m1-1.50/stored-f7/emulation/inputs/f7-1.nand --trajectories emulation/recipe-bank-route-ram-push-trajectories.json --receipt-output <fresh private path> --output <fresh private runtime path>
```

The flash candidate route needs a DAT rebuilt with the bank archive; `build-recipe-package` does not currently consume it.

## Remaining

The final link uses one bank staticlib from `//replacement/bank:bank` and one panic handler. `replacement/bank/lib.rs` includes this module as `#[path = "../recipe_ui/src/lib.rs"] mod recipe_ui_route;`; a second UI archive or panic handler is not kept. The bank manifest binds `replacement/recipe_ui/src/lib.rs`, the bank archive digest under `rust_object_replacement_bank`, `RB_RUST_BUILD_FACTS` under `rust_toolchain_replacement_bank`, and the #119 toolchain facts. The reviewed relink, evidence re-pin, packaged Rust-candidate run, viewer URL and human visual assessment remain open. The two emulator attempts used one slot and stopped at the 85 C thermal guard; no limit was bypassed.
