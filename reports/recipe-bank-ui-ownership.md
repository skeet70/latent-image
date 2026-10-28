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

The page and slot are separate exports on purpose. A packed page and slot made clang emit `UXTAB` in `rb_status_present`; the native runner does not admit that instruction, and the eleven expected-refusal UI cases stopped as `unsupported_instruction`. Splitting the exports removed the `UXTAB` without changing the runner's admission set. This is an engine-support boundary resolved in the design, not a permissive fallback.

`menu/recipe_bank_ui.h` keeps `rb_ui_page` and `rb_ui_decode` as `static inline` wrappers around the exports. `rb_ui_decode` retains its zero-initialized `RbBankItemRef`, so field and padding behavior of the by-value return is unchanged. Rust makes no calls, stores, allocations or mutable-global accesses.

## Evidence

The Rust module's host tests pass 6 of 6: page classes including the adjacent field/sub and sub/edit boundaries, every slot, every verb token, every select token, token gaps and the packed C ABI encoding.

A one-crate smoke with `#![no_std]`, this module included by `#[path]` and one `#[panic_handler]` compiles under the #119 toolchain. The archive exports `rb_ui_route_page`, `rb_ui_route_slot`, `rb_ui_route_item` and one mangled `rust_begin_unwind`; the module owns no handler.

The fixed native UI workload passes 55 of 55 cases with the UI Rust staticlib linked into the bank kernel. The combined `//replacement/bank:bank` archive, which also contains the retained-service module and the probe change, has not been run through the 55 cases yet. The kernel is 32,024 bytes, `__rb_text_end` is `0x00be8c9c` and `__rb_image_end` is `0x00be9c1c`, below `dataBase` `0x00bea200` and the `0x8300` code cap. A control kernel built from `HEAD` also passes 55 of 55; the negative cases `n-select-unset-refused`, `n-save-write-fail`, `n-edit-write-fail`, `n-reset-write-fail`, `n-rename-fail`, `n-refused-dialog` and the media-refresh case remain passing refusals. `//emulation:recipe-bank-ui-plan-tests` and `//emulation:ui-trajectory-tests` pass.

The flash candidate `package-20261024b` with `f7-1.nand` and `bank-selection-stage4/fast-ui/card-sel.img` ran `emulation/recipe-bank-route-trajectories.json` from the card attach checkpoint. Six of seven segments passed: slot-list entry, seven-row slot navigation, the slot verb page and BACK, return to the slot list, SELECT list entry and eight-row navigation, and cancel. The final handback segment was not reached before the host thermal guard stopped the run.

The failed handback expectation was corrected from observation, not fitted to the failure. After the SELECT list cancels, the stock transition returns to shooting menu row 34 `(1,5,34)`, the stock shooting menu row that owns the bank entry. MENU on that row re-enters the bank row, which produced `(1,645,0)`; the trajectory now repeats entry with UP and asserts the shooting-menu handback rather than a live-view state. The final segment remains unvalidated until a compute turn is granted.

An earlier empty-slot run showed SELECT navigation staying on row 0 when the card's slots were unset. That is an observation of the stock list's disabled-row handling, not yet a pinned resource-absence trajectory. Storage failure and refusal paths are covered by the passing host cases; the packaged run has no new resource acquisition of its own.

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

The archive target is an added workflow step relative to the #118 baseline. The flash candidate route needs a DAT rebuilt with the bank archive; `build-recipe-package` does not currently consume it.

## Remaining

The final link uses one bank staticlib from `//replacement/bank:bank` and one panic handler. `replacement/bank/lib.rs` includes this module as `#[path = "../recipe_ui/src/lib.rs"] mod recipe_ui_route;`; a second UI archive or panic handler is not kept. The bank manifest binds `replacement/recipe_ui/src/lib.rs`, the bank archive digest under `rust_object_replacement_bank`, `RB_RUST_BUILD_FACTS` under `rust_toolchain_replacement_bank`, and the #119 toolchain facts. The reviewed relink, evidence re-pin, packaged Rust-candidate run, viewer URL and human visual assessment remain open. The two emulator attempts used one slot and stopped at the 85 C thermal guard; no limit was bypassed.
