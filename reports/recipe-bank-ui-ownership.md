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

### SET, confirm and leave source increment

Issue [#133](https://github.com/skeet70/latent-image/issues/133) continues candidate B of the [ownership inventory](replacement-inventory-baseline.md#8-current-ownership-and-retirement-view). `replacement/owned_menu/src/lifecycle.rs` implements the complete three action bodies, exporting `rb_ui_set`, `rb_ui_editor_confirm` and `rb_ui_leave`. This is an unqualified source increment. The bank still calls the original delegates through `rb_call_set_dispatch`, `rb_call_editor_confirm` and `rb_call_leave_edge`; their retirement and the integrated lifecycle gate remain open.

The source binding is X-M1 1.50 image 5 at `0x00bf1a00`, SHA-256 `3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a`. The inspected bodies and literal pools are SET `[0x00c72e10,0x00c742a8)`, confirm `[0x00c75ed4,0x00c75f38)` and leave `[0x00c76650,0x00c76a64)`. Private provenance is `private-data/analysis/x-m1-1.50/filmsim/ghidra/all.c:421262-421844,422923-422941,423307-423456`, checked against the original image and confirm disassembly. The report's earlier `retained-modal-contract.md` name resolves to this report's retained-modal-contract section, not a separate file. No sibling-camera evidence is used.

All three query `FUN_0063a0f8(0x53)`. SET and leave refuse every nonzero answer. Confirm admits a nonzero answer only with session word `0x00954d08 == 1`, passes the original query answer to retained `FUN_00d4ad38`, then calls transition `FUN_00c12aac`; results `-1` and `2` invoke postlude `FUN_00d38f34(9)`. Other transition results are returned unchanged.

| SET input, after admission | Owned behavior |
| --- | --- |
| Node 0 | Stage `(0xe,0x1d)` and transition, with no notification |
| Class 5 | Preserve the ordered row and mode decisions below |
| Nonzero `FUN_006394d0`, node `0x5b` | Class `0xdf`/row 0 uses stage `(2,0x1d)`; rows 1 and 2 additionally call `FUN_00d350c4(2)`, `FUN_006357dc(1)`, `FUN_00635800(0 or 1)` and `FUN_00635824(1)` after result `-1` or `2`, returning 1; other classes/rows return 0 |
| Class `0x184` | Stage `(0xe,0x1d)`; successful exit selects state 3 and notification `0x92a` |
| Class `0x141`/row `0x1b`, with `FUN_006394e8 == 0` or word `0x00956934 != 2` | Same stage; successful exit selects state 1 and notification `0x92a` |
| Class `0xf5` | Stage `(0xf,0x1d)` with no notification |
| Class `0x167`/row 0 | Clear `0x00954cf4`, stage `(0xe,0x1d)`; only result `-1` selects state `0x20`, re-reads the editor choice, copies or clears the 25-byte buffer at `0x009543ae`, clears the retained area at `0x00952690` with argument `0xc`, then notifies `0x92a` |
| Class `0x167`/row 1 or 2 | State `0x26`, editor choice 1 or 2, retained prepare/open with id `0x28`, notification `0x92a`, return `-1` |
| Class `0x26` | Stage `(0xe,0x1d)`; successful exit selects state 4 and notification `0x92a` |
| Class `0x19`/row 0 or 1 | Store the full word 0 or 1 at `0x0095047c`, stage `(0xe,0x1d)`; successful exit selects state `0x29` and notification `0x92a` |
| Class `0xed`/row 1 or 2, `FUN_0064257c == 1` | State `0xe`, postlude 9, retained WB enable/prepare/refresh and notification `0x135`, return 1 |
| Class `0xed`/row 3 | Stage `(0xe,0x1d)` without notification |
| Other class `0xed` or class `0xee` | First nonzero `FUN_006394d0` gives stage `(0xe,0x1d)` and state 9 on exit; otherwise a second zero answer gives `(0xf,0x1d)` and notification `0x930` on exit; a changed second answer gives `(2,0x1d)` |
| Other non-class-5 frame | Preserve activation/mode gates at `0x009544f8`, `0x00950474`, `0x00950478` and `0x00954500`, including class `0x47`/row 0's retained mode operation; otherwise stage `(0xf,0x1d)` and, after transition, suppress `0x930` only for the new class-5/row-`0x35` frame or new class `0xe8` |

An exit in this table means result `-1` or `2`, except the explicit class-`0x167`/row-0 branch. Notifications `0x92a`, `0x92b`, `0x92c`, `0x930` and `0x933` also write word 1 at `0x009504a8`; `0x135` and `0x13d` do not.

For a stable class-5 frame, SET rows `0x32,0x33,0x36,0x37,0x38` and row `0x7c` with `FUN_006394e8 == 0` enter through `(0xe,0x1d)` and reset through `FUN_00635b60(0)` on exit. Row `0x7c` with that mode active selects state `0x2d`/notification `0x92c`; rows `0x1c,0x13,0x27` notify `0x92c`; row `0x14` enters without notification. State/`0x92a` rows are `0x1e -> 9`, `8 -> 10`, `0x17 -> 2`, `0x46,0x68,0xa1 -> 0x10`, `0x28 -> 0x15`, `0x2d -> 3`, `0xd -> 0x14`, `0x25 -> 0x12` and `0x7b -> 0x13`. Row `0x2c` opens state `0xd` through prepare/refresh, clears `0x009503c0`, sends `0x13d` and postlude 9, returning 1. Active-mode rows `0x99,0x83` perform the retained enable/refresh/mode-1 sequence and set words `0x0095201c` and `0x00950104`. Row `0x58` with `FUN_006394d0` active and row `0x5a` with `FUN_006394e8` active write only the low byte of `0x0095047c`, then use state `0x29`; active row `0x5b` uses state `0x2a`. Rows `0x29` and `0x30` call their retained selector with 1 and return 2. Row `0x51` with property `0x20d == 1` prepares `0x2a` and uses stage `(2,0x1d)`. Remaining rows enter and notify `0x92b` on exit. The implementation retains the source's repeated frame reads and nested non-class-5 comparisons rather than treating the initial class read as an atomic snapshot.

Leave uses `(2,0x1d)` for node 0, class 5 or node `0x5b`, before the mode gates. With activation or either `0x00950474/78` gate set, class `0xee` uses `(0xf,0x1d)`; other frames use `(2,0x1a)`, except white-balance frames with either repeated `FUN_006394d0` read nonzero, which use `(2,0x1d)`. Without those gates, class `0x167` uses `(0xf,0x1d)` and notification `0x933` on exit, class `0xf5` uses `(0xf,0x1a)`, mode-active white balance uses `(2,0x1d)` and class `0xee` uses `(0xf,0x1d)`. Other frames pop through `(0xf,0x1a)`; class `0xfe` suppresses the notification, while the generic path suppresses `0x933` only when the new node is `0x60`, `0x37` or `0x61`. The remaining white-balance tail preserves its two repeated mode reads and source stages/`0x930` notification.

The implementation owns decisions and the named scalar stores, with typed frame classes, row tokens, node/state/operation/editor ids, stages, flags, property ids/indexes and byte counts. The new code has no heap, initializer or owned global resource. The three frame accessors, mode queries, editor operation, stage/transition, notifications, WB/mode operations and buffer primitives remain retained through the declared trampolines. In particular this cut does not retire the frame accessors or their initializer/data dependencies. The new allowlist declarations describe source dependencies, not permission to execute or deploy this source.

`//emulation:controller-lifecycle-cases` emits original-versus-owned cases without executing unless explicitly passed `--execute`. Each pair uses the supplied original action bytes, the owned linked export, literal return/state/scalar-memory/service-effect expectations, preserved registers, stack pointer, exit PC and CPSR control bits. The transition model changes the frame before later reads and can change the editor choice to reach both copy branches. Retained frame reads are unlogged. Synthetic service code records only the arguments in each retained ABI, not calls between owned helpers.

The return, r4 and state refusal controls operate on the typed observation of the real owned generic SET, including its stage `(0xf,0x1d)`, notification `0x930` and blink store. The reference must have no mismatches. Each control then changes only r0, r4 or one state byte and must produce exactly the intended register or state-region mismatch, with no other mismatch. These are observation-checker controls, not substituted guest implementations. Three separate guest transfer controls require both `fetch_guard` and the exact original entry PC. This source has not run on either native backend; no pair, reference or control is qualified.

The final host-only smoke built `//replacement/bank:bank` and `//emulation:controller-lifecycle-cases`, linked with `BUILD_ONLY=1` at native-only base `0x00bd0000`, and emitted 85 original/owned pairs without execution. The 34,104-byte `kernel-text.bin` has SHA-256 `ea41047ab80eb04e0a394c4801a61d61908d28d94c5cb996877b6b834f36e762`; the complete linked `kernel.bin` has SHA-256 `30424e6815e612880bf1e7b6478001e4a658bb685d28b5a63ce9d9e2a5083d38`. Private outputs are under `private-data/scratch/issue117-controller-lifecycle-sol/`. These are build and emission observations, not native qualification (E-MODAL75-LIFECYCLE-SOURCE-150).

The control-review follow-up rebuilt `//emulation:controller-lifecycle-cases` and emitted the 85 pairs, one real owned reference and three guest fetch controls under `private-data/scratch/issue117-controller-lifecycle-sol/cases-observation-controls/`. The three observation mutations are applied only after a later real reference execution. No native execution or SDK build occurred (E-MODAL75-CONTROLS-SOURCE-150).

The existing 55 UI cases remain historical synthetic-controller evidence. They are unchanged here: their injected SET/confirm returns and delegate counters are not original-binary comparisons or #133 evidence. After the new comparisons qualify, the bank cutover must migrate all three `rb_ui_stock` delegate calls, remove their trampolines and allowlist entries, and migrate the affected UI trajectories to real owned controllers. Delete fake delegate-log/return-echo assertions rather than replacing them with owned-call wiring assertions; retain consumer-visible returns, menu position, settings and storage outcomes. The larger composed image also requires a freshly reviewed native/package placement before any existing layout proof carries forward.

#### Frozen later native plan

The host-only preparation is `devenv shell -- bazel --batch --output_base=/home/murph/.cache/bazel/latent-image-Issue117ControllerLifecycle build --jobs=2 //replacement/bank:bank //emulation:controller-lifecycle-cases`. A kernel-only link uses `BUILD_ONLY=1 RB_BASE=0x00bd0000 BUILD_DIR=<frozen-build> RB_RUST_ARCHIVE=<bank.a> devenv shell -- bash emulation/build-recipe-bank-ui-cases.sh`; this stops before any model generation or native execution. The native-only base is not a proposed package placement.

`0x00bd0000` was selected for source-growth headroom, not because the current permanent default overlaps. The frozen complete image spans `0x94b4` bytes, including data and zero BSS: it ends at `0x00bd94b4` there and would end at `0x00be94b4` at the unchanged `0x00be0000` default, below name record `0x00beb784`. Further growth could exhaust that gap; the full `__rb_image_end`/name-record guard remains in the builder. Linker capacity checks, model text/data assertions, function ranges and case-region bounds are unchanged. This arithmetic does not qualify a package placement or a device-driven route.

After a separate reviewed frozen plan, authenticated qualified native backend and one emulator-slot grant, emit the cases with `controller-lifecycle-cases --build <frozen-build> --out <frozen-cases> --id5 <owner-supplied-original-id5> --id5-base 0x00bf1a00`, then use the identical inputs with `--execute --runner <qualified-native-runner>`. The input identity check binds the complete original image, not generated substitutes. Bind source commit, toolchain, kernel/image digest, case digest, backend and output directory before the slot; run one job, preserve every result, require all literal comparisons and all negative controls, and stop on unsupported execution or guard/ABI/effect differences. An engine limitation is not a candidate pass. Complete bank callsite/trajectory migration only after this bounded gate, then separately review and run a device-driven integrated lifecycle route with forbidden original-action execution. Keep #128 global closure, #133 integrated lifecycle, application ownership, runtime retirement, camera and persistence gates open.

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

The composed bank image no longer fits between the native suites' former default base `0x00be1f00` and the package plan's name record `0x00beb784`: linked there it ends at `0x00bebbb4`, and 51 of the 55 UI cases stop as `memory_guard` where the plan overwrites bank data. `emulation/build-recipe-bank-cases.sh`, `emulation/build-recipe-bank-ui-cases.sh` and both generators now default to `0x00be0000`, a native-case placement only, and both scripts refuse a kernel image that ends past `RB_NAME_RECORD`. The default UI command passes 55/55 with its controls rejected.

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
