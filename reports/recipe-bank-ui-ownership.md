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

The four retargeted modal-75 cells are global state/event registrations, not per-page registrations. The bank interposer owns the LEAVE action of cell `(0x1e, 13)` and delegates the other three cells to their original handlers:

| State, event | Cell | Original action | Original handler |
| --- | --- | --- | --- |
| `0, 8` | `0x00cd20a4` | `0x5a` | `FUN_00c72e10` |
| `0x1e, 8` | `0x00cd211c` | `0x73` | `FUN_00c75ed4` |
| `0, 13` | `0x00cd243c` | `0x81` | `FUN_00c76650` |
| `0x1e, 13` | `0x00cd24b4` | `0x98` | `FUN_00c77794` |

The stock dispatcher `FUN_00bfb028` indexes the cell table by `event * width + state`; modal 75 has width 46. A cell's low halfword selects the action. After that action returns, the signed high halfword selects a transition callback when positive, or a direct state write of its negation otherwise. The action's return value is preserved. The 299-entry function-pointer table at `0x00d04080` is separate from this cell table. The four original handler bindings were checked in the decoded image, including action `0x98` at `0x00d042e0`. The dispatcher instructions are in `private-data/analysis/x-m1-1.50/filmsim/id5.dis` at `0x00bfb028..0x00bfb11c`.

### Owned editor-cancel increment

`replacement/owned_menu/src/lib.rs` now exports `rb_ui_editor_cancel()` for action `0x98` of cell `(0x1e, 13)`. The bank interposer calls it for the LEAVE path in `menu/recipe_bank_actions.c`; the former `rb_call_editor_cancel` trampoline and its `0x00c77794` allowlist entry are removed. The original body remains the oracle in `emulation/OwnedMenuCases.hs`, not a bank fallback.

The export takes no arguments and returns the action word. It calls retained `FUN_0063a0f8` with query `0x53`, reads the retained session cell `0x00954d08`, and tail-calls the retained stage open `0x00d3d858` with kind `0xf` and id `0x1a` before retained `FUN_00c12aac`. A nonzero query with a session value other than `1` returns `0` and performs no stage or transition. Otherwise it returns the transition result; when that result is `-1` or `2`, it writes halfword `0x1d` to the modal state cell `0x0146761e`. The dispatcher's post-action transition, live-frame refill and redraw remain stock and are not owned by this increment.

`//emulation:owned-menu-cases` defines ten differential cases over query values `0`, `1` and `2`, session values `0`, `1` and `2`, transition results `-1`, `2` and `0x5d`, and one read-only session region. It compares every preserved register, `cpsr` control state and the state, session and log regions against the stock oracle. Its negative controls are a changed `r4`, a changed state halfword and a fetch-guarded transfer to the unexecutable stock entry. No natural device route currently enters cell `(0x1e, 13)` and reaches this export in a packaged runtime; the native cases model the retained services at their stock entries and are not device evidence.

The claimed-page SET path still depends on stock activation and mode gates before the generic exit in `FUN_00c72e10`. In `FUN_00d4d7a4` case 1, tokens `0x63..0x70` and `0x73..0x79` set `0x009544f8`; the default taken by bank parent tokens `0x7d`/`0x7e` clears `0x009544fc` and `0x00954500`, not `0x009544f8`. A page class alone therefore does not prove that gate clear. The other SET gates read `0x00950474`, `0x00950478` and `0x00954500`. No device-input scenario establishing a carried set gate is qualified here.

After transition, the SET notification tests use the new live frame: `FUN_0061d1f4` and `FUN_0061d244` refill their local snapshot through `FUN_0061cd14`. Reusing a pre-transition class or row token would change the suppression for class 5/token `0x35` and class `0xe8`. The claimed bank LEAVE path already has its own stage/notification behavior; replacing the controller must preserve that behavior, not silently substitute the generic stock leave path.

The C binding also distinguishes frame-pointer identity `0x0078ea98` from the node id at offset zero. Its row input is absent, stale or present: a null values pointer and an invalid cursor must be handled before reading a token or child class. Physical left/right entry and the registered cell are separate facts from the mutable event and state halfwords.

The existing `card-sel.img` and `emulation/ui-trajectories.json` provide a populated-bank selection baseline. They are not the cardless Rust route or qualification of an owned SET exit. A complete selection/apply/exit cutover needs a matching card-backed FujiMod run, the existing mode-dependent exits and explicit scoped forbidden-execution evidence. Moving more C classification policy alone does not retire these proprietary handlers.

### Owned menu-frame snapshot service

Issue: [#132](https://github.com/skeet70/latent-image/issues/132). `replacement/owned_menu/src/lib.rs` module `frame` implements the X-M1 1.50 menu-frame snapshot service from the image-4 bytes: classifier `FUN_0061ca2c` (744 bytes), snapshot `FUN_0061cd14` (264), store `FUN_0061ceac` (104) and the eight 80-byte accessors. Expectations come from those bytes and the image-4 node data, not from the x70view C.

| Word | Source | Accessor |
| --- | --- | --- |
| 0 depth | `FUN_0061ca2c`: no frame 1; frame id 0 gives 0; otherwise 1 or 2 by the class rule below | `0x0061d0e4` |
| 1 node id | frame `+0x00` | `0x0061d154` |
| 2 node type | frame `+0x04` | `0x0061d1a4` |
| 3 class | frame `+0x08` | `0x0061d1f4` |
| 4 cursor token | `values[*0x009571d8].token`, only when frame `+0x20` is nonzero | `0x0061d244` |
| 5 current-value token | `values[*0x0095732c].token`, values reloaded, same condition | `0x0061d294` |
| 6 raw cursor | `*0x009571d8` | none |
| 7 visible ordinal | `*0x009571dc` | none |
| 8 scroll state | `*0x009571e0` | none |
| 9 page | loader `FUN_0004d978((rows + word7), rows)`, rows at frame `+0x0c` | `0x0061d2e4` |
| 10 navigation word | `*0x009571e4` | `0x0061d334` |

The writer first stores the detached defaults `{1, 0x86, 0xc, 0, ...}` and returns 1 when `*0x009571d0` is null; a null destination returns 0 with no store. The class rule gives 1 for classes `5`, `0x47`, `0xd0`, `0xdf`, `0xe0`, `0x185`, `0x1b9`, `0x1bb`, `0x220`, `0x240`, `0x245`, `0x246`, `0x247`, class 7 on token 1 and class `0x166` on token 0; 2 for `0x2d`, `0x43`, `0xad`, `0xdc`, `0xe6`, `0xfc`, `0x100`..`0x102`, class `0xed` on token 3, `0xe5` on token 0, `0x141` on token `0x1b` and `0x27` on token 0. Any other case follows the parent at `+0x1c`: none or a class-5 parent with another id reads the cursor row's child word, a parent with the same id gives 2, and a parent of another class gives 1. Values indexes have no bound. A zero row count reaches the loader's divide-by-zero `SVC 0x1010` after word 8 is stored. The store writes depth 0 to `0x00944414`, depth 1 to `0x0093eb74 + root * 0x2c` and depth 2 to `0x009412d4 + root * 0x2c`, with 32-bit wrap; a slot address of 0 returns 0 without a store. Every service result is 0 or 1.

The owned exports are `rb_menu_frame_classify`, `rb_menu_frame_snapshot`, `rb_menu_frame_store` and `rb_menu_frame_{depth,node,node_type,class,cursor_token,current_token,page,navigation}`, with the same AAPCS signatures. Every frame, values and cell read is a volatile load in the stock order, so a slot or destination is written exactly as the stock body writes it. The module has no static, allocation or copy beyond the caller's snapshot destination and its own stack frames. The divide stays the retained loader routine through the `rb_call_loader_uidiv` trampoline, which keeps the zero-divisor fatal path.

Bank cutover: `rb_ui_child_enter` refreshes SELECT's saved snapshot through `rb_menu_frame_store`. The `rb_call_snapshot_store` trampoline and its `0x0061ceac` allowlist entry are removed; the allowlist adds stock call `0x0004d978`, the 44-byte depth-0 slot as a stock store and argument/index declarations for the snapshot writer and the per-table store helpers.

Static caller closure, X-M1 1.50 stock and package `id4-final`/`id5-final` bytes, scanning every aligned ARM word as `B`/`BL`/`BLX`, every aligned and unaligned word as data and every `movw`/`movt` pair and PC-relative `add`/`sub`:

| Entry | Image 4 | Image 5 | Plain image 6 |
| --- | --- | --- | --- |
| `FUN_0061ca2c` | `FUN_0061cd14`, `FUN_0061ceac` only | 0 | 0 |
| `FUN_0061cd14` | `FUN_0061ceac` (3), the eight accessors | 0 | 16 |
| `FUN_0061ceac` | 0 | `0x00bf95e8`, `0x00bfa204`, `0x00bfacfc` | 0 |
| `0x0061d0e4` | 0 | 2 | 0 |
| `0x0061d154` | 4 | 574 | 1 |
| `0x0061d1a4` | 0 | 1 | 0 |
| `0x0061d1f4` | 3 | 1,361 | 0 |
| `0x0061d244` | 0 | 1,057 | 0 |
| `0x0061d294` | 0 | 2 | 0 |
| `0x0061d2e4` | 0 | 1 | 0 |
| `0x0061d334` | 0 | 2 | 0 |

Every reference is a direct `BL` to an entry. No data word, formed address or branch reaches an interior address; the only matches inside the bodies are unaligned byte coincidences inside instructions. Loaders A and B, the `0x40048` boot block, image 2 and the `0x50048` boot block (data words only, its base being unsettled) have none. Thumb code and register-computed transfers without a data word are outside the scan. The saved-snapshot reader `FUN_0061ce1c` (10 image-6 callers) and the three slot tables stay stock.

`//emulation:menu-frame-cases` runs the original bodies and the owned exports once each over 30 scenarios: 19 on real image-4 menu nodes (`id4-final` of `package-20261024b`; its snapshot code is byte-identical to stock and the chosen nodes give the same words on stock image 4) and 11 on synthetic frames for classes and boundaries absent from the node data. Each scenario's eleven words, return value and slot address are literals in the generator. Completing scenarios run the classifier, snapshot, store and all eight accessors; the boundaries add a null destination, a store root whose depth-2 slot wraps to 0, a zero row count and a cursor past the values array. Each run checks status, `r0`, `r4`-`r11`, `sp`, exit `pc` and `cpsr` control bits, the full destination region with guard bytes, the full slot-table region and the caller frame above `sp`. The owned layout maps the stock bodies without execute permission.

On the legacy native runner, all 307 pairs match their literal expectations and each other: 301 return, the 3 zero-row operations both stop at the loader's `SVC` as `unsupported_instruction` after writing words 0 to 8, and the 3 unmapped-cursor operations both stop as `memory_guard` after writing words 0 to 3. Owned stack use is at most 96 bytes below the entry `sp` against the stock bodies' 84. A guest writing the expected words is accepted; a wrong cursor token, a twelfth word, a cleared `r4`, a store to the caller frame and a transfer to `0x0061cd14` in the owned layout are refused. Results are in `private-data/analysis/x-m1-1.50/recipes/issue117-menu-frame/menu-frame-cases/menu-frame-results.json`. The SVC continuation, interrupts and concurrent frame changes are outside these cases.

Global retirement stays open. The 3,027 stock call sites still enter the stock bodies; retiring them needs reviewed entry publications at the eleven stock entries, a forbidden-fetch guard on the retired bodies in a packaged menu route, which the packaged FujiMod route does not yet provide, and a device-driven packaged run with that guard.

## Evidence

The Rust module's host tests are `//replacement/recipe_ui:route-tests`: a checked-in genrule compiles `src/lib.rs` with the pinned `@lab_rustc` and clang under `--test`, and the existing `//emulation:recovery-task-state-tests.sh` wrapper runs the binary. The four cases call only the exported C ABI and cover the page and slot boundaries, all four verb kinds with the first and last slot, select codes 0, 1 and 7, and the enumerated gap tokens returning zero.

The initial UI-only bank link used the #119 toolchain with one panic handler. Its ELF at the `0x00c40000` reference base passed the bank ELF allowlist with 0 violations and 169 declared sites. That archive linked at `0x00be1f00` ended at `__rb_image_end` `0x00be9c24`; its disassembly contained no UXTAB, UXTAH, SXTAB, SXTAH or Thumb form. These are historical UI-only layout observations, not identities for the later composed service package.

The native UI suite passes 55/55 with all five negative controls rejected for the exact kernel `f78664` on the existing `native-runner.c`; every case reported `returned` with runner exit 0. The current proof is `private-data/scratch/issue117-preservation-consumer-tests/final-ui-9893ec0-1/cases/results.json`, with `control.json` and `findings.txt` beside it. This native result does not exercise the FujiMod SDK or the packaged full-system route, and the separate packaged service qualification does not establish native coverage.

Canonical upstream `9bb8bd9` passed 171 `fujimod-emulator` library tests, 157 cpu-unicorn tests, 28 xm1 library tests and 3 r4f-guard tests. On that SDK the 34-input packaged primary was candidate-qualified with aggregate RAM-byte preservation: 1,974 identity CPU stores, 0 changed, 0 host and 0 model-bus writes, a null first violation, and the same 450-byte physical range. It restored 222 publication words, retained the 122,008-byte allocation, used zero SD counters and produced five captures byte-identical to the prior accepted frames. The evidence is `private-data/scratch/issue117-ui-final/receipt-primary-sdk-9bb8bd9/e2e-result.json` and `e2e-preservation.json`. The current packaged service report `private-data/scratch/issue117-service-final/receipt-sdk9bb8-1/service-packaged-qualification.json` records 5/5 rows, 75 sequences, 15 entries and 30 nested calls per row, and 2,949 retained hits with the same archive, facts, reference and retention. The historical all-access `MemoryWatch` run overflowed at 4,096 records after 1,410 byte-preserving CPU stores; it remains cited failure evidence, not the primary verdict. The accepted control, source `712442b`, produced `control-refused` with 6,626 identity CPU stores, 1 changed, 0 host and 0 model-bus writes, and a null problem. Its first CPU change is core 0 at PC `0x629454`, physical and guest address `0x96ccee`, size 1, before `00`, after `30`, at instruction `1631821794`. That is an actual witness, not an OK attribution. The control restored 222 rollback words, retained the 122,008-byte allocation, and left cardless media counters at zero. The control omits the primary's initial focus-overlay-dismiss step after the `mode_dial M` prelude; expected root and film states are unchanged, and this does not characterize all S/M overlay states. The scoped #121 pilot is qualified; full application/runtime/startup milestones remain open under #117.

## Route choice

The device-driven packaged route is the existing cardless RAM push, not a new flash DAT. `emulation/recipe-bank-route-ram-push-trajectories.json` starts from the e2e's focus-area overlay, enters both bank lists, and asserts the empty resident bank's disabled SELECT rows stay on row 0 before cancelling and repeating entry. That is the resource-absence behavior, not a substitute for a populated bank. The flash-candidate trajectory with `card-sel.img` remains baseline evidence for populated slot and select navigation; it is not the packaged Rust route. No flash DAT wiring is added for the automated route.

Human visual assessment needs an interactive surface. The selected cardless route exposes one without new packaging: the pinned FujiMod RAM-push server implements the e2e's `capture` command. `emulation/fujimod-xm1/src/ram_push.rs` writes `output/screenshots/<label>.png` from `machine.frame()`, and `emulation/fujimod-xm1/src/machine.rs` returns a PNG of the first presented graphics scanout. The receipt's `menu_receipts` carry each label, path, SHA-256 and menu state, and `Emulation/Xm1RamPushE2E.hs` requests a capture for every trajectory segment that names one. The retained cardless bridge did the same on the same trajectory shape. Those PNGs are guest-produced frames for a separate human check; they are not interaction or visual acceptance. The e2e exposes no browser URL, so the evidence is the receipt's screenshot paths and hashes, delivered as file references.

Human visual acceptance was recorded on 2026-10-28 for the actual PNGs in `private-data/scratch/issue117-ui-final/receipt-primary-envelope-fixed/screenshots/`: bank-list entry, navigation to CUSTOM 7, return, disabled unset selections and stock-menu handback. The initial and returned bank-list PNGs are byte-identical. The new SDK primary produced all five capture PNGs byte-identical to those accepted files, so that verdict carries to the primary; this is not a new human approval. It accepts the displayed surfaces, not the aggregate effect qualification. E-RECIPE-UI-VISUAL-150 records this separate reported claim.

## Qualification status

| Field | Value |
| --- | --- |
| `status` | qualified (scoped pilot) |
| `capability` | true (scoped pilot: bounded toolchain, validator, service and UI paths) |
| `scope` | Recipe-bank page/slot/token classification behind the unchanged C ABI, bounded native UI cases with the read-only observation rule and effect scoping, one packaged primary with aggregate 450-byte RAM preservation, and its accepted changed-store control |
| `preserved diagnostics` | `returned` for every passing case; one earlier `unsupported_instruction` stop at the `UXTAB` site, retained and fixed; non-returned diagnostics are reported verbatim and cannot pass |
| `transient guard` | The original `media-state` region ended at `0x00910340`; the mapped region is now `0x00910140..0x00910424` (`0x2e4`). Its original `0x200` window remains a `MediaWorkspace`, and the exact linked model image (`0xe4`) at `0x00910340` is a separate `ModelDataWorkspace`. The remaining padding is not mapped. The model tail is a synthetic native-case mapping, not a camera reservation. `span-low` keeps the live settings record and the modeled card files and has no workspace, so the selected read-only cases give it no write permission and the runner's pre-execution memory preflight stops a protected store as `memory_guard`; a mutating case makes it writable only through a changed expected byte. The 4096-event provenance cap is reported and is not used as evidence |
| `coverage_gaps` | The scoped #121 pilot is qualified; #117 full application/runtime/startup milestones remain open; #85 is hardware blocked and persistence is unpursued; visual acceptance is carried only by byte-identical PNGs |

The consumer requires the runner's `returned` diagnostic, all of `r0` through `r12`, `sp`, `lr`, `cpsr` and `pc`, every declared region at its exact address and size, and the preserved `r4` through `r11`, `sp` and exit `pc`. It compares the public claims and every byte outside the declared ABI workspaces, and classifies a changed byte as a public claim, a forbidden live-settings or card effect or an unexpected non-workspace effect. A missing or malformed provenance log is `malformed_output`; a truncated log is reported and never becomes a transient claim. Every run also executes five rejection controls and fails if any is accepted: a corrupted public claim, a changed read-only expectation, a missing provenance log, a non-returned diagnostic and a store against a `span-low` with no write permission.

The original `media-state` region ended at `0x00910340`. The media model's `.data` now links at that boundary and occupies `0x00910340..0x00910424`; the mapped `media-state` region is expanded to `0x00910140..0x00910424` (`0x2e4`). The original `0x200` window remains a `MediaWorkspace`, while the exact linked model image (`0xe4`) is a separate `ModelDataWorkspace`; the remaining padding is not mapped. The case generator bounds every model-cell overlay to the linked data length, so the select-draw case keeps the model data within the 16-region case budget without dropping a stock region. This synthetic native-case mapping is not a camera reservation. `span-low` holds only the live settings record at `0x0096cc64`, the field table and the modeled card files at `0x00980000`. It has no workspace: the selected read-only cases leave it without write permission, and a mutating case makes it writable only through an expected byte that differs from the initial slice. `mirror` and `tables-*` remain stock-addressed RAM load.

The `effects` object separates the final-state match from the effect proof. `final_state` is `verified` when the returned diagnostic, registers, ABI workspaces, public claims and every non-workspace byte match; it says nothing about writes that were overwritten. With a complete provenance log every write is classified and a clean run is `verified`. With a truncated log a read-only case can still be `guarded` because `span-low` has no write permission, while a mutating case is `final-only`: its expected final bytes are verified and no complete transient proof is claimed. The verdicts are the closed `EffectVerdict` values `guarded`, `verified`, `final-only`, `failed` and `not evaluated`; guard, transient proof, scope and final state are likewise closed types, not string flags. A required effect is therefore complete or enforced by a guard, never inferred from expected final bytes alone.

The aggregate API is `Board::preservation_begin(ProtectedRanges)` over one to eight physical ranges. It counts identity and changed CPU stores, observes the differing protected intersection at the CPU store and records a `CpuChange`; it does not block that store, and the E2E verdict refuses the route after observation. That is distinct from the strict native pre-execution `memory_guard`, which stops a protected store before execution. The aggregate guard also records host and model-bus writes, and fails closed on unsupported backends, unresolved mappings, unverifiable writes or incomplete observation. This aggregate byte guard is separate from the legacy all-access `MemoryWatch`; it has no writer-PC allowlist, mapping fallback or final-snapshot substitute. On canonical upstream `9bb8bd9` the 450-byte live-property primary completed the route with the counters and retention in the evidence paragraph. The historical watch overflow remains separate failure evidence.

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

The final link uses one bank staticlib from `//replacement/bank:bank` and one panic handler. `replacement/bank/lib.rs` includes this module as `#[path = "../recipe_ui/src/lib.rs"] mod recipe_ui_route;`; a second UI archive or panic handler is not kept. The bank manifest binds `replacement/recipe_ui/src/lib.rs`, the bank archive digest under `rust_object_replacement_bank`, `RB_RUST_BUILD_FACTS` under `rust_toolchain_replacement_bank`, and the #119 toolchain facts. Primary packaged aggregate effect observation and its accepted changed-store control qualify the scoped #121 pilot for the 450-byte range on one route. #117 full application/runtime/startup milestones remain open; #85 is hardware blocked and persistence is unpursued. Visual acceptance is carried only by byte-identical PNGs, not a new human approval or reviewed live publication. Existing modal-75 and source-retirement limits and the sourceMode94 correction are unchanged; no application/runtime retirement, startup, hardware or migration default cutover is claimed.
