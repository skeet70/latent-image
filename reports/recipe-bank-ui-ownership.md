# Recipe-bank UI ownership increment

This report supports [#121](https://github.com/skeet70/latent-image/issues/121) and the [accepted replacement plan](../spec/11-incremental-firmware-replacement.md#user-visible-ownership-pilot). It selects the smallest read-only ownership increment in the existing recipe-bank UI, records its consumed C ABI and evidence, and states what remains open.

## Selection

The bank's page and row-token classifiers were static-inline C in `menu/recipe_bank_ui.h`: `rb_ui_page` maps a frame class to `RbBankPage` and the sub-page slot, and `rb_ui_decode` maps a selected row token to `RbBankItemRef`. Every entry, navigation and leaving decision in the bank consumes them: the shooting-menu rows `0x7d`/`0x7e` in `rb_ui_child_enter`, the SET (`0x8`) and LEAVE (`0xd`) cells of `rb_bank_interpose`, draw-present `0x816`, draw-root, key-left/right, and the edit/dialog routes.

The increment moves those two classifiers to a `no_std` Rust module and keeps the C functions as inline ABI wrappers. No call site, enum, struct layout or `slot_out` contract changes. The old classifier bodies are removed; there is no C fallback.

Read-only scope covers the complete packaged chain. Entering the slot or SELECT CUSTOM SETTING list loads the resident bank image through `rb_slot_load`/`rb_load`; that is retained RAM scratch, not a persistent write. The exercised chain never presses Save Current, Edit, Reset, a dialog OK, a field OK or an OK on a SELECT row, so `rb_bank_action`, selection apply and storage writes are unreachable. No capture or media action occurs.

Retained: the stock modal delegates `FUN_00c72e10`, `FUN_00c75ed4` and `FUN_00c76650`, the transition `FUN_00c12aac`, the redraw/draw paths, storage and probe kernels, the bank ELF allowlist and the C enums and `RbBankItemRef` ABI. Owned: the three pure classification decisions. Retired: the inline C page and token if-chains.

Owned data and resources: the UI module has no static, no initializer, no allocation and no resource acquisition; every value it returns is computed from its arguments on the caller's stack. The case-side owned inputs are `emulation/recipe-bank-route-ram-push-trajectories.json`, the case generator `emulation/RecipeBankUiCases.hs`, and its output under `private-data`. The bank linkage keeps the single composed staticlib and its one panic handler; the C enums, node layout macros and the plan images remain retained data.

## Interface

`replacement/recipe_ui/src/lib.rs` is a plain Rust module for the single bank crate root chosen for the final link. It exports three `#[no_mangle] extern "C"` functions:

| Export | Result |
| --- | --- |
| `rb_ui_route_page(cls)` | `RbBankPage` discriminator, 0 through 7 |
| `rb_ui_route_slot(cls)` | sub-page slot, 0 through 6, otherwise 0 |
| `rb_ui_route_item(token)` | `kind \| slot << 8 \| verb << 16 \| selection << 24` |

The page and slot are separate exports on purpose. A packed page and slot made clang emit `UXTAB` in `rb_status_present`; the native runner does not admit that instruction, and the eleven expected-refusal UI cases stopped as `unsupported_instruction`. Splitting the exports removed the `UXTAB` without changing the runner's admission set. The diagnostic was retained and the candidate changed, so required unsupported execution blocked qualification rather than becoming a pass. This is an engine-support boundary resolved in the design, not a permissive fallback. A `returned` case still has to match its public claims; the harness's `native_case_status: failed` from declared ABI-workspace differences is not treated as a case pass.

The exported seam wraps the raw `u32` class in `MenuClass` and the raw token in `RowToken`; inward results carry `BankPage`, `Slot`, `Selection`, `BankVerb` and the closed `BankItem` enum. Raw ids and the packed word appear only at the three C exports. Unknown classes and tokens still return `NONE`/`0`.

`menu/recipe_bank_ui.h` keeps `rb_ui_page` and `rb_ui_decode` as `static inline` wrappers around the exports. `rb_ui_decode` retains its zero-initialized `RbBankItemRef`, so field and padding behavior of the by-value return is unchanged. Rust makes no calls, no non-stack stores, no allocations and no mutable-global accesses.

A source search for the three export names finds exactly their declarations in `menu/recipe_bank_ui.h` and their definitions and host tests in `replacement/recipe_ui/src/lib.rs`. A search for `rb_ui_page(` finds six call sites, all in `menu/recipe_bank_actions.c` and `menu/recipe_bank_probe.c`, and `rb_ui_decode(` has its single call site in `rb_bank_action`. The retired C if-chains survive only as the historical diff in `reports/replacement-inventory-baseline.md`; no build input references them.

## Retained modal contract

The four retargeted modal-75 cells are global state/event registrations, not per-page registrations. The current bank interposer delegates unclaimed pages to the original handlers:

| State, event | Cell | Original action | Original handler |
| --- | --- | --- | --- |
| `0, 8` | `0x00cd20a4` | `0x5a` | `FUN_00c72e10` |
| `0x1e, 8` | `0x00cd211c` | `0x73` | `FUN_00c75ed4` |
| `0, 13` | `0x00cd243c` | `0x81` | `FUN_00c76650` |
| `0x1e, 13` | `0x00cd24b4` | `0x98` | `FUN_00c77794` |

The stock dispatcher `FUN_00bfb028` indexes the cell table by `event * width + state`; modal 75 has width 46. A cell's low halfword selects the action. After that action returns, the signed high halfword selects a transition callback when positive, or a direct state write of its negation otherwise. The action's return value is preserved. The 299-entry function-pointer table at `0x00d04080` is separate from this cell table. The four original handler bindings were checked in the decoded image, including action `0x98` at `0x00d042e0`. The dispatcher instructions are in `private-data/analysis/x-m1-1.50/filmsim/id5.dis` at `0x00bfb028..0x00bfb11c`.

The claimed-page SET path still depends on stock activation and mode gates before the generic exit in `FUN_00c72e10`. In `FUN_00d4d7a4` case 1, tokens `0x63..0x70` and `0x73..0x79` set `0x009544f8`; the default taken by bank parent tokens `0x7d`/`0x7e` clears `0x009544fc` and `0x00954500`, not `0x009544f8`. A page class alone therefore does not prove that gate clear. The other SET gates read `0x00950474`, `0x00950478` and `0x00954500`. No device-input scenario establishing a carried set gate is qualified here.

After transition, the SET notification tests use the new live frame: `FUN_0061d1f4` and `FUN_0061d244` refill their local snapshot through `FUN_0061cd14`. Reusing a pre-transition class or row token would change the suppression for class 5/token `0x35` and class `0xe8`. The claimed bank LEAVE path already has its own stage/notification behavior; replacing the controller must preserve that behavior, not silently substitute the generic stock leave path.

The C binding also distinguishes frame-pointer identity `0x0078ea98` from the node id at offset zero. Its row input is absent, stale or present: a null values pointer and an invalid cursor must be handled before reading a token or child class. Physical left/right entry and the registered cell are separate facts from the mutable event and state halfwords.

The existing `card-sel.img` and `emulation/ui-trajectories.json` provide a populated-bank selection baseline. They are not the cardless Rust route or qualification of an owned SET exit. A complete selection/apply/exit cutover needs a matching card-backed FujiMod run, the existing mode-dependent exits and explicit scoped forbidden-execution evidence. Moving more C classification policy alone does not retire these proprietary handlers.

## Evidence

The Rust module's host tests are `//replacement/recipe_ui:route-tests`: a checked-in genrule compiles `src/lib.rs` with the pinned `@lab_rustc` and clang under `--test`, and the existing `//emulation:recovery-task-state-tests.sh` wrapper runs the binary. The four cases call only the exported C ABI and cover the page and slot boundaries, all four verb kinds with the first and last slot, select codes 0, 1 and 7, and the enumerated gap tokens returning zero.

The initial UI-only bank link used the #119 toolchain with one panic handler. Its ELF at the `0x00c40000` reference base passed the bank ELF allowlist with 0 violations and 169 declared sites. That archive linked at `0x00be1f00` ended at `__rb_image_end` `0x00be9c24`; its disassembly contained no UXTAB, UXTAH, SXTAB, SXTAH or Thumb form. These are historical UI-only layout observations, not identities for the later composed service package.

The candidate 55-case UI run with the composed archive passed 55/55 under the guarded emulator slot at source `290faba`, with the exact separate model-data workspace; every case reported `returned` with runner exit 0, and all five negative controls were rejected. `//emulation:recipe-bank-ui-plan-tests` passed. The receipt is `private-data/analysis/x-m1-1.50/recipes/issue117-ui-proof/final-native-20261028T111209Z`. This is bounded native evidence, not packaged device or full-system evidence. Final packaged qualification consumes [#120](https://github.com/skeet70/latent-image/issues/120)'s consumed contract and runner convention and [#123](https://github.com/skeet70/latent-image/issues/123)'s shared packaged service qualification; neither is established by this report.

The current Rust candidate completed the 34-input packaged navigation trajectory, all five captures and verification of all 222 rollback words. Its protected-write qualification failed: the watch retained 4,096 accesses, including 1,410 byte-preserving CPU stores, before its record bound was exceeded. That prefix does not establish preservation over the remaining execution. The failed evidence is `private-data/scratch/issue117-ui-final/receipt-primary-envelope-fixed`; the candidate remains unqualified at this effect boundary. Storage failure and refusal paths remain covered at the native boundary.

## Route choice

The device-driven packaged route is the existing cardless RAM push, not a new flash DAT. `emulation/recipe-bank-route-ram-push-trajectories.json` starts from the e2e's focus-area overlay, enters both bank lists, and asserts the empty resident bank's disabled SELECT rows stay on row 0 before cancelling and repeating entry. That is the resource-absence behavior, not a substitute for a populated bank. The flash-candidate trajectory with `card-sel.img` remains baseline evidence for populated slot and select navigation; it is not the packaged Rust route. No flash DAT wiring is added for the automated route.

Human visual assessment needs an interactive surface. The selected cardless route exposes one without new packaging: the pinned FujiMod RAM-push server implements the e2e's `capture` command. `emulation/fujimod-xm1/src/ram_push.rs` writes `output/screenshots/<label>.png` from `machine.frame()`, and `emulation/fujimod-xm1/src/machine.rs` returns a PNG of the first presented graphics scanout. The receipt's `menu_receipts` carry each label, path, SHA-256 and menu state, and `Emulation/Xm1RamPushE2E.hs` requests a capture for every trajectory segment that names one. The retained cardless bridge did the same on the same trajectory shape. Those PNGs are guest-produced frames for a separate human check; they are not interaction or visual acceptance. The e2e exposes no browser URL, so the evidence is the receipt's screenshot paths and hashes, delivered as file references.

Human visual acceptance was recorded on 2026-10-28 for the actual PNGs in `private-data/scratch/issue117-ui-final/receipt-primary-envelope-fixed/screenshots/`: bank-list entry, navigation to CUSTOM 7, return, disabled unset selections and stock-menu handback. The initial and returned bank-list PNGs are byte-identical. This accepts the displayed surfaces, not the failed effect qualification. E-RECIPE-UI-VISUAL-150 records this separate reported claim.

## Qualification status

| Field | Value |
| --- | --- |
| `status` | partial |
| `capability` | false |
| `scope` | Recipe-bank page/slot/token classification behind the unchanged C ABI, bounded native UI cases with the read-only observation rule and effect scoping, and one packaged trajectory |
| `preserved diagnostics` | `returned` for every passing case; one earlier `unsupported_instruction` stop at the `UXTAB` site, retained and fixed; non-returned diagnostics are reported verbatim and cannot pass |
| `transient guard` | The original `media-state` region ended at `0x00910340`; the mapped region is now `0x00910140..0x00910424` (`0x2e4`). Its original `0x200` window remains a `MediaWorkspace`, and the exact linked model image (`0xe4`) at `0x00910340` is a separate `ModelDataWorkspace`. The remaining padding is not mapped. The model tail is a synthetic native-case mapping, not a camera reservation. `span-low` keeps the live settings record and the modeled card files and has no workspace, so the selected read-only cases give it no write permission and the runner's pre-execution memory preflight stops a protected store as `memory_guard`; a mutating case makes it writable only through a changed expected byte. The 4096-event provenance cap is reported and is not used as evidence |
| `coverage_gaps` | complete packaged RAM-effect observation and a discriminating mutation control remain open; human visual acceptance is recorded separately |

The consumer requires the runner's `returned` diagnostic, all of `r0` through `r12`, `sp`, `lr`, `cpsr` and `pc`, every declared region at its exact address and size, and the preserved `r4` through `r11`, `sp` and exit `pc`. It compares the public claims and every byte outside the declared ABI workspaces, and classifies a changed byte as a public claim, a forbidden live-settings or card effect or an unexpected non-workspace effect. A missing or malformed provenance log is `malformed_output`; a truncated log is reported and never becomes a transient claim. Every run also executes five rejection controls and fails if any is accepted: a corrupted public claim, a changed read-only expectation, a missing provenance log, a non-returned diagnostic and a store against a `span-low` with no write permission.

The original `media-state` region ended at `0x00910340`. The media model's `.data` now links at that boundary and occupies `0x00910340..0x00910424`; the mapped `media-state` region is expanded to `0x00910140..0x00910424` (`0x2e4`). The original `0x200` window remains a `MediaWorkspace`, while the exact linked model image (`0xe4`) is a separate `ModelDataWorkspace`; the remaining padding is not mapped. The case generator bounds every model-cell overlay to the linked data length, so the select-draw case keeps the model data within the 16-region case budget without dropping a stock region. This synthetic native-case mapping is not a camera reservation. `span-low` holds only the live settings record at `0x0096cc64`, the field table and the modeled card files at `0x00980000`. It has no workspace: the selected read-only cases leave it without write permission, and a mutating case makes it writable only through an expected byte that differs from the initial slice. `mirror` and `tables-*` remain stock-addressed RAM load.

The `effects` object separates the final-state match from the effect proof. `final_state` is `verified` when the returned diagnostic, registers, ABI workspaces, public claims and every non-workspace byte match; it says nothing about writes that were overwritten. With a complete provenance log every write is classified and a clean run is `verified`. With a truncated log a read-only case can still be `guarded` because `span-low` has no write permission, while a mutating case is `final-only`: its expected final bytes are verified and no complete transient proof is claimed. The verdicts are the closed `EffectVerdict` values `guarded`, `verified`, `final-only`, `failed` and `not evaluated`; guard, transient proof, scope and final state are likewise closed types, not string flags. A required effect is therefore complete or enforced by a guard, never inferred from expected final bytes alone.

The current source implements an aggregate RAM-byte-preservation guard over `0x0096cc64..0x0096ce26`, the live property cache, from before the first UI segment through unload. It counts identity CPU stores, rejects a changed protected byte at the store rather than only at the final snapshot, and refuses unverifiable host/model-bus writes or incomplete observation. SDK compilation and complete-route qualification remain pending. The failed all-access `MemoryWatch` evidence is retained separately. Native no-store checks remain unchanged; no writer-PC exemption or final-snapshot substitute is accepted.

### Packaged watch command

```sh
XM1_EMULATOR_SLOTS=1 devenv shell -- bazel --output_base="$HOME/.cache/bazel/latent-image-issue117-ui-proof" run //emulation:xm1-viewer -- --ram-push-e2e --cardless --fujimod --unreviewed-relink --firmware private-data/analysis/x-m1-1.50/update/prefix/FWUP0002-prefix.DAT --installed-nand private-data/analysis/x-m1-1.50/stored-f7/emulation/inputs/f7-1.nand --trajectories emulation/recipe-bank-route-ram-push-trajectories.json --receipt-output <fresh private path> --output <fresh private runtime path>
```

Run the control with `--trajectories emulation/recipe-bank-route-ram-push-control-trajectories.json` into a separate fresh receipt. Its `ram_preservation` evidence must contain an actual changed protected CPU intersection and a positive changed-write counter; identity stores, an integrity fault or a generic failure do not qualify it. The guest PNG capture and repeated entry, navigation, cancel, handback and resource-failure segments remain in the tracked primary trajectory.

## Measurement

The #118 fixed workload is the behavior-preserving refactor of these two classifiers. Baseline same-invocation samples are 45.29 s total (1.07 s build, 44.22 s case generation and execution) for the unmodified tree, and 42.58 s (0.98 s, 41.59 s) for the C refactor, under warm Nix/Bazel and page caches, two jobs and sibling load.

The guarded repeat used source `290faba`, the exact reviewed C range-helper patch and Rust wrapping-subtraction patch, the fixed `package-20261024b/image` inputs and fresh `RepeatRust`/`RepeatC` output bases and empty disk caches. Rust ran before C. Unrefactored setup was separate and build-only: Rust build log `58.266 s` (`59.900 s` outer), C build log `60.676 s` (`62.644 s` outer), each including the four targets and `//replacement/bank:bank`.

| Guarded same-invocation sample | C | Rust |
| --- | ---: | ---: |
| Incremental build, `--batch` | 12.669 s | 12.778 s |
| UI workload, all 55 + 5 controls | 95.413 s | 97.417 s |
| Plan gate, `--batch` | 14.135 s | 12.824 s |
| Total | 122.224 s | 123.024 s |

Both variants passed 55/55 with five rejected controls and the plan gate. Setup, timed build and plan gate all used Bazel `--batch` inside `thermal-guard-slot-1.service`; no persistent server existed between them. The timed payload recorded the slot cgroup and affinity `0,1`. C peaked at 3,251,122,176 bytes with Tctl 53.5 to 63.6 C; Rust peaked at 4,148,891,648 bytes with Tctl 52.2 to 62.2 C; both had zero swap. The C compiler log has 12 clang invocations including the `menu/recipe_bank_*.c` action units. The Rust execution log records the actual `rustc --crate-name=bank` action with `cacheHit: false`, exit 0 and 0.094 s; the C bank target was cached. No separate Nix bank derivation was found, so no forced Nix bank rebuild was used. Status, registers, ABI validity, effects, mismatches and controls are identical; full projections differ only in `kdata` ABI workspace offsets and provenance totals/retained counts (C 12,571,972/207,059, Rust 12,577,308/207,119). The earlier `98.552 s` C and `97.956 s` Rust samples remain limited measurements, not a matched speedup; the repeat totals are close and no speedup is claimed. Historical two-job samples remain confounded.

## Reproduce

Build the fixed targets first. This is build-only and runs no guest:

```sh
devenv shell -- bazel --output_base="$HOME/.cache/bazel/latent-image-issue117-ui-proof" build --jobs=1 //emulation:native-runner //emulation:recipe-bank-ui-cases //emulation:emulator-slot //replacement/bank:bank
```

Run the 55-case UI workload into fresh directories through the existing emulator-slot wrapper. `build-recipe-bank-ui-cases.sh` invokes the generator and native runner directly, so it must not be run bare:

```sh
XM1_EMULATOR_SLOTS=1 XM1_EMULATOR_MEMORY_MAX=8G \
  bazel-bin/emulation/emulator-slot \
  taskset -c 0,1 \
  nix shell nixpkgs#cpulimit -c cpulimit --include-children --limit=75 -- \
  env RB_RUST_ARCHIVE="$PWD/bazel-bin/replacement/bank/bank.a" BUILD_DIR="$PWD/private-data/analysis/x-m1-1.50/recipes/issue117-ui-proof/ui-build" OUT_DIR="$PWD/private-data/analysis/x-m1-1.50/recipes/issue117-ui-proof/ui-cases" JOBS=1 \
  devenv shell -- bash emulation/build-recipe-bank-ui-cases.sh
```

The plan gate is separate from guest execution:

```sh
devenv shell -- bazel --output_base="$HOME/.cache/bazel/latent-image-issue117-ui-proof" test --jobs=1 --nocache_test_results --test_output=errors //replacement/recipe_ui:route-tests //emulation:recipe-bank-ui-plan-tests //emulation:ui-trajectory-tests
```

The archive target is an added workflow step relative to the #118 baseline, and the UI script now reads it as `RB_RUST_ARCHIVE`. The packaged read-only route runs the pinned FujiMod RAM-push server:

```sh
devenv shell -- bazel --output_base="$HOME/.cache/bazel/latent-image-issue117-ui-proof" build --jobs=2 //emulation:xm1-viewer
XM1_EMULATOR_SLOTS=1 devenv shell -- bazel --output_base="$HOME/.cache/bazel/latent-image-issue117-ui-proof" run //emulation:xm1-viewer -- --ram-push-e2e --cardless --fujimod --unreviewed-relink --firmware private-data/analysis/x-m1-1.50/update/prefix/FWUP0002-prefix.DAT --installed-nand private-data/analysis/x-m1-1.50/stored-f7/emulation/inputs/f7-1.nand --trajectories emulation/recipe-bank-route-ram-push-trajectories.json --receipt-output <fresh private path> --output <fresh private runtime path>
```

`--unreviewed-relink` relinks the composed `//replacement/bank:bank` archive from the viewer's runfiles and runs the bank ELF allowlist check before the push. The receipt's `menu_receipts` hold the captured PNG path, SHA-256 and menu state for each named segment, and `relink.allowlist.violations` must be empty. The generator writes `results.json`, `control.json` and `findings.txt` into `OUT_DIR`; the run fails unless all cases pass and all five controls are rejected.

## Remaining

The final link uses one bank staticlib from `//replacement/bank:bank` and one panic handler. `replacement/bank/lib.rs` includes this module as `#[path = "../recipe_ui/src/lib.rs"] mod recipe_ui_route;`; a second UI archive or panic handler is not kept. The bank manifest binds `replacement/recipe_ui/src/lib.rs`, the bank archive digest under `rust_object_replacement_bank`, `RB_RUST_BUILD_FACTS` under `rust_toolchain_replacement_bank`, and the #119 toolchain facts. The complete packaged effect proof and its mutation control remain open. Human visual acceptance is complete for the displayed candidate frames; it is not automated effect acceptance or reviewed live publication.
