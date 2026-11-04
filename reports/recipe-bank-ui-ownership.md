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

The stock dispatcher `FUN_00bfb028` indexes the cell table by `event * width + state`; modal 75 has width 46. A cell's low halfword selects the action. After that action returns, the signed high halfword selects a transition callback when positive, or a direct state write of its negation otherwise. The action's return value is preserved. The 299-entry function-pointer table at `0x00d04080` is separate from this cell table. The four original handler bindings were checked in the decoded image, including action `0x98` at `0x00d042e0`. The dispatcher instructions are in `private-data/analysis/x-m1-1.50/filmsim/id5.dis` at `0x00bfb028..0x00bfb120`; its complete extent is `[0x00bfb028,0x00bfb124)`.

A hook at `0x00bfb028` changes every controller that calls the dispatcher, not only modal 75. `//replacement/modal_runtime:modal-closure` computes that closure from the RAM-push images, the linked bank ELF and the publication manifest. It exits 0 only when the closure leaves no obligation and no refusal. It does not admit the hook on the current inputs: 3,105 direct callers reach all 174 controllers and 49,738 cells, and 6,904 obligations remain (E-MODAL-CLOSURE-CALL-INVALIDATION-150). Since callees are no longer assumed to preserve registers, 6,790 of them are stores through, or escapes of, an array or table pointer held across a call or left in a register at a return. The other 114 are unresolved caller ids and event-store values, constants reached through ambiguous entries, aligned data words naming reached tables, the initial array contents, and constant forms the checker does not trace (E-MODAL-DISPATCHER-CLOSURE-150). Pointers are traced only from MOVT and PC-relative literal constants and from aligned image words. MOV/MVN/ORR or rotated immediates, arithmetic construction and RAM-loaded pointers are outside its coverage. The bank's own `rb_ui_pop` write of 0 to `0x0146761e` is one of those stores. Byte reads keep r5 across `rb_call_stage_open` only on its normal return and only under code-integrity, exception, SVC-handler and alias premises. `rb_call_transition` reaches the dispatcher's register-called cell actions, so the store stays open (E-MODAL-RBPOP-CALLEES-150).

### Stage-open SVC frame and remaining closure

E-MODAL-SVC-FRAME-LIMIT-150 narrows the exceptional exit left open by E-MODAL-RBPOP-CALLEES-150. Independent reads of the existing loader artifact, SHA-256 `21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882`, match the indexed words at the offsets below. File offset is instruction address minus `0x40000`; the listing is `private-data/analysis/x-m1-1.50/decode/loader-40000.dis`, SHA-256 `b5d3e3195444668751e9b4f866aee314e4ce004f26cee4f7bf45bd6fb19a6c05`. DAT derivation is inherited from the earlier evidence, not independently renewed here. Fresh DAT-to-artifact correspondence remains an unmet proof gate.

The installer at `0x40040..0x40048` constructs offset `0x28`, loads `0x401b4` from literal `0x400e4` and stores it at base + `0x28`. Its base is 0 when `0x427b8` returns 0 and `0xffff0000` otherwise; that helper compares the CP14 discriminator with `0x4003`. These are source publication alternatives, not an active-vector or lifetime observation. Actual core, vector selection, visible vector instruction/target words and their freedom from intervening rewrites remain premises.

Given entry at `0x401b4`, architectural A32 SVC LR = `0x43610`, original SPSR.T = 0 and unchanged instruction words, the wrapper reads the word at LR - 4 and clears its upper byte (`0x401d0` word `051ee004`, `0x401d4` word `03cee4ff`). The `ef001010` at `0x4360c` therefore gives immediate `0x1010`, taking the nonzero branch at `0x401dc`, not `bx r0`. The two `{r12,lr}` pushes and the `{r0-r11}` push allocate exactly 64 bytes. With T = SVC-bank SP at wrapper entry and F = T - 64, the frame passed in r1 is:

| Frame interval or word | Saved value |
| --- | --- |
| `[F,F+48)` = `[T-64,T-16)` | Original r0-r11 in register order |
| `F+48` = `T-16` | Original r12, duplicate |
| `F+52` = `T-12` | SPSR read at `0x401b8` |
| `F+56` = `T-8` | Original r12 |
| `F+60` = `T-4` | Architectural exception LR |

The `ldr r2` at `0x401e8` resolves to literal `0x40410` = `0x40cac`; `0x401f4` calls it with r0 = 2 and r1 = F. Handler `0x40cac` holds F in r4, writes code 2 to `0x4004f118`, and copies all sixteen frame words to `[0x4004f11c,0x4004f15c)` at `0x40ccc..0x40ce8`. It calls reporter `0x41668` with code 2 and `[F+60]`. If word `0x4004f25c` equals 1, that reporter writes the ring selected by halfword `0x4006347e`, calls `0x4118c` (which reaches `0x4cbd8` and conditionally `0x4c620`), records additional memory/device reads, advances the halfword index and clears the next record. The index is read again across that call; a single stable or bounded ring slot is not established. These stores and escaping helper effects are additional to the earlier normal-route stack/global summary.

Next, nonzero word `0x4004f114` selects `blx r3` at `0x40d14`, with code 2 and `[F+60]` as arguments. The setter `0x40c6c..0x40c88` stores its incoming r0 to that word. Startup `0x409ac` captures its incoming function word in r4; `0x40a64..0x40a6c` calls through its then-current r4 with selector 2 and passes the returned r0 to the setter. No null-callback invariant follows. The startup fill at `0x400b4..0x400bc`, through `0x434f0`, requests zeros over `[0x4004dae0,0x40062c54)`, which includes callback and resume words. This initial write is not a lifetime value. The value and every effective writer of resume word `0x4004f10c`, the callback result/targets and their writers/effects, offset-constructed or escaped writes and asynchronous/alias writes remain unclosed. The bounded source inspection is not a complete writer census.

After reporting and the optional callback, nonzero `0x4004f10c` selects `mov sp,r4` at `0x40d2c`, `pop {r0-r12,lr}` at `0x40d30` (word `e8bd5fff`), `pop {r12,lr}` at `0x40d34` (word `e8bd5000`) and `movs pc,lr` at `0x40d38` (word `e1b0f00e`). The first pop places the saved SPSR word in LR only transiently; the second overwrites LR with the saved exception LR. This route bypasses wrapper `msr SPSR_fsxc,lr` at `0x401fc`. Its exception return consumes the then-current SPSR, not a demonstrated reload of the saved SPSR word. Only in the original SVC exception mode/banking, with original r4 = F, intact original frame, unchanged current SPSR and architectural A32 LR = `0x43610`, does it restore r0-r12 and SVC-bank SP = T and resume at `0x43610`. Restoring those SVC-bank values is not a proof of the interrupted caller's deeper frames or other banked state.

Zero resume word instead selects `0x40d3c`, repeatedly calling `0x40df8` while its return is zero, then calling `0x410fc`; the latter has a selfbranch at `0x41104`, and the handler has another at `0x40d4c`. Neither those branches, the selfbranch at `0x43610`, nor the absence of an enumerated return edge establishes termination, no-return or preservation.

The remaining boundary is callback/reporter and writer closure, actual exception routing, original frame/r4/SP and current-SPSR integrity through reporting, callbacks and nested exceptions, and code/publication visibility. All exceptional writes, the copied frame, vectors, executable words, normal and exception stacks and ring destinations need the required physical separation under the X-M1 alias/TCM map, with nonwrapping stack arithmetic and no intervening other-core/DMA writer. The earlier S >= 100 normal-stack, transition action/controller/table/reentry and other-entry premises are unchanged. The computed-index-store at `rb_ui_pop` `0x00a8654c` remains **OPEN**. No caller, cell, target or obligation count changes, checker discharge or runtime acceptance follows.


### Dispatcher register and frame contract

This contract is read from the pinned image-5 bytes that the closure checker uses. The 222-word bank manifest publishes no word of the dispatcher `[0x00bfb028,0x00bfb124)`, the prehook `[0x00d0cc68,0x00d0cc9c)` or its callee `[0x00d3cde8,0x00d3cdfc)` (E-MODAL-DISPATCHER-ABI-150). S is SP at dispatcher entry.

| Point | Stock `FUN_00bfb028` |
| --- | --- |
| Before the prehook | Reads the cell, action and transition table words at `0x00cf58b8`, `0x00cfe2a8`, `0x00d0013c` plus 4 * id and the event, width and state halfwords at `0x014676e4`, `0x00cd5e78`, `0x01467588` plus 2 * id, all zero-extended. A null cell table returns 0 without calling the prehook. |
| Prehook `0x00d0cc68` entry | r0 = id, r1 = state, r2 = event, r3 = 2 * id. |
| Prehook effect | Its only register inputs are the low halfwords of r0-r2; it saves and restores r11 and returns through the saved LR. Stores those halfwords to `0x0095037e`, `0x00950380` and `0x00950382`, then calls `0x00d3cde8`, which changes only r11 and SP and restores both. Returns with r1 = 1, r3 = `0x00944580`, r12 = `0xbe00`, LR = `0x00d0cc98`; r0, r2, r4-r11, SP and NZCV are unchanged. |
| Action entry | r0 = id, r1 = 1, r2 = event, r3 = target, r12 = `0xbe00`, LR = `0x00bfb0cc`, NZCV = `0010` for a target below `0x80000000`. r4 = cell byte offset, r5 = id, r6 = cell table, r7 = event, r8 = width, r9 = action table, r10 = cell address, r11 = S - 4. SP = S - 48; `[S-44]` holds the transition table base, `[S-48]` and `[S-40]` are not written, `[S-36,S)` holds the caller's r4-r11 and return address. |
| After the action | Saves r0 as the result (0 for a null action), then reads the cell's high halfword. |
| Transition entry (high > 0, entry nonzero) | r0, r1 and r12 are the action's residues, or id, 1 and `0xbe00` with no action. r2 = transition base, r3 = target, r4 = result, LR = `0x00bfb0fc`, NZCV = `0010`, the other registers and the frame as at the action. The transition's return is discarded. |
| High <= 0 | Stores halfword -high to `0x01467588` + 2 * id. |
| Return | r0 = result, or 0 for a null cell table. r4-r11 and SP are restored. r1-r3, r12 and NZCV are residues of the path taken. The null-cell path leaves the caller's r1 and r12, r2 = `0x01467588`, r3 = 2 * id and Z set. |

The stock prehook ignores its entry r3, r12 and flags, so the owned prehook call cannot change its effect through those registers.

The source of `rb_modal_dispatch` expresses the reads, their order relative to the calls, the null-cell and null-action results, the post-action high read, the negated state store and the r0 result. As source arguments, it passes the prehook's returned r0 and r1, the captured event as r2, and the target as r3 to the action. The transition gets the action's r0 and r1, or the prehook's with no action, plus the table base and the target. The prehook's returned r2 is not forwarded. The captured event equals it only because the stock prehook leaves r2 unchanged. The hand-written `rb_modal_call` glue in `src/dispatch.S` fixes these differences from stock at every action and transition entry, independent of the compiler: r12 holds the target, LR returns into the glue, `[SP]` holds the r4 of `rb_modal_dispatch` and `[SP+4]` a return address into it, where stock has an unwritten word and the transition table base. The glue itself writes no flags.

An isolated, source-bound qualification composition links `replacement/modal_runtime` into the bank staticlib to observe the compiled code; the integrated default bank does not include it (E-MODAL-DISPATCHER-LINKED-ABI-150). In that composition no bank or UI caller and no publication reaches the module, and the closure inventory records the hook as absent. The compiled `rb_modal_dispatch` is 272 bytes; its instruction words are identical when linked at `0x00c40000` and at `0x00a80ad8`. Compared with stock, it shows these compiler-dependent facts:

- It loads in stock order: the cell, action and transition table words, then the event, width and state halfwords. A null cell table returns 0 before the prehook.
- At prehook entry, r0-r3 and SP = S - 48 equal stock. NZCV also equals stock, because both compare the cell table with 0.
- The prehook's returned r0 and r1 are held in r2 and r12 across the index computation and arrive unchanged as action r0 and r1. The action's r0 and r1 are held the same way and become transition r0 and r1. With no action, the transition gets id and 1, as in stock.
- The frame is `push {r3-r11, lr}` with r11 = S - 8, where stock has r11 = S - 4. SP at every callee entry is S - 48, as in stock, and `[S-36,S)` holds the caller's r4-r11 and return address in stock order. `[S-40]` holds the transition base, `[S-44]` a return address into `rb_modal_dispatch` and `[S-48]` the dispatcher's r4, which is the cell table. Stock leaves `[S-48]` and `[S-40]` unwritten and keeps the transition base at `[S-44]`.
- At action and transition entry, NZCV equals stock because both compare the target with 0, and r8 = width as in stock. The other callee-saved registers have different roles: r4 = cell table, r5 = 2 * id, r6 = event, r7 = cell address, r9 = `0x01467588`, r10 = action table, r11 = S - 8. At transition entry, stock's r4 = result and r12 = the action's r12 residue are not reproduced.
- The high halfword is read after the action, signed, and tested as less than 1. That takes the same branch as stock's test of at most 0. The state store has the same address and halfword value.
- On return, r0, r4-r11 and SP equal stock. After a called transition, r1-r3, r12 and NZCV are the transition's, as in stock. On the other paths the residues differ:
  - Null cell table: r1 = `0x00cd5e78`, r2 = `0x15c` and r3 = the transition base, where stock leaves the caller's r1, `0x01467588` and 2 * id. r12 and NZCV equal stock.
  - Positive high with a null transition entry: r2 = the carried r0, r3 = the transition base and r12 = the carried r1, where stock has the transition base, 0 and the action's r12. r1 and NZCV equal stock.
  - High <= 0: r1 = the state cell address, r2 = the carried r0, r3 = the transition base and r12 = the carried r1, with NZCV from comparing high with 1. Stock has the carried r1, `0x01467588`, -high and the action's r12, with NZCV from comparing high with 0.

The current bank allowlist has separate 4-byte mode-flag stores at `0x00950104` and `0x00950108`. It now declares the concrete retained prehook call to `0x00d0cc68`; that closes only this payload-surface declaration. Checking the actual linked candidate still refuses two sites: the unbounded r12 callback at `0x00a86eb8` in `rb_modal_call` and the unbounded state store at `0x00a87aa0` in `rb_modal_dispatch` (E-MODAL-NATIVE-LIFECYCLE-150). Neither gets an `unproven` exemption. The closure checker refuses the `0x00c40000` link for overlap with image 5. At `0x00a80ad8` it reports no refusal, an absent hook and 6,908 obligations, compared with 6,904 for the earlier 21951a3 bank. The four extra lie in owned-menu functions changed since that bank; the `rb_ui_pop` store obligation remains. No global obligation is waived.

Open premises: whether any admitted action or transition reads r12, LR as a value, the callee-saved registers whose roles differ, or `[S-48,S-36)` at entry; transitive caller consumption of the differing volatile registers and flags; code, saved-frame and return-slot integrity; exception, interrupt, other-core and DMA interference; entries other than the enumerated direct BLs; nested retained-service reentry, termination and the `svc #0x1010` continuation. The bounded native comparisons below do not discharge these global premises. Before any hook, each difference needs reproduction in owned code or a consumer-closure proof. Applicable declarations must not widen an unproved call target or store address. Default-bank composition and its pin changes still wait for admission.

### Bounded native dispatcher and cancellation qualification

E-MODAL-NATIVE-LIFECYCLE-150 qualifies the existing linked candidate's bounded semantics on the #125 native backend: 17 dispatcher original/owned pairs and ten direct editor-cancellation pairs match. The candidate is the source-bound ELF of E-MODAL-DISPATCHER-LINKED-ABI-150, not a new bank composition or a global dispatcher publication. `qualify` pins that ELF, extracts its executable `.text` in process, verifies the supplied text digest and checks required symbol addresses and sizes against the ELF. Both harnesses derive every real stock function extent through the shared `StockInventory.deriveFunction` and reject declaration differences through `checkDeclarations` before execution.

The dispatcher matrix covers null cell/action/transition entries, zero and negative high halfwords including `-32768`, unsigned high-bit event/state inputs, 16-bit index wrap, post-action cell mutation, captured pointer-table roots, action-result preservation and one callback reentry through shared RAM. It compares r0, r4-r11, SP, PC, CPSR control bits, final shared bytes and the ordered non-private-stack write address/width sequence. Bytes from incoming SP through the mapped caller-owned stack tail are protected separately. Callback-register roles, LR identity, NZCV, dispatcher scratch words below incoming SP and arbitrary reentry depth are outside that bounded equivalence.

Dispatcher controls isolate a changed return value, changed high/state halfwords, changed r4, and a real-owned wrapper storing at incoming SP. The last control differs only in caller-owned stack bytes and its additional observable write. A separate body-interior transfer refuses at `0x00bfb02c`. Cancellation's ABI, state and caller-stack controls execute the real owned export before one mutation; its retired-entry control refuses at `0x00c77794`. Ordered effects and unchanged retained query/stage/transition effects are checked, not replaced with a control that returns without executing the candidate.

`CaseSelection` rejects unknown or duplicate case IDs and rejects a selected control without its prerequisite positive scenario. Failure receipts retain the passing outcomes preceding the failure. Qualification executes each corrected cohort once and does not reuse retained runner output.

The caller diagnostic in E-MODAL-CALLER-CONTINUATIONS-150 examines the continuations of all 3,105 direct calls without changing a global obligation:

| Returned register | Every-path overwrite before read | Unresolved continuation |
| --- | ---: | ---: |
| r1 | 0 | 3,105 |
| r2 | 10 | 3,095 |
| r3 | 575 | 2,530 |
| r12 | 0 | 3,105 |
| LR | 0 | 3,105 |

No direct continuation reports a read before its first stop or overwrite. A return is an escape, not a kill: for example, controller-0 calls `0x00bfb254`, `0x00bfb2a8`, `0x00bfb2e8` and `0x00bfb30c` immediately return through `pop {r11,pc}` and forward volatile residues to their outer callers. The analysis stops at calls, returns, indirect transfers, cycles or unmodeled instructions. It does not close outer callers, flag consumption, unknown/alias entries or asynchronous integrity.

The existing candidate does not require a deeper retained-prehook stack extent than stock. Both enter callbacks at S-48, the prehook's own frame reaches S-56 and its shared leaf writes at S-60. The observed differences are frame contents, write history, r11 and callback register roles. The inspected prehook, its leaf and the cancellation body do not directly load dispatcher scratch `[S-48,S-36)`; their saved incoming r11/LR and cancellation's retained callees remain consumers requiring closure.

The global inventory remains unadmitted with 6,908 obligations across 174 controllers. The [covering BSS fill request](firmware-extraction.md#id-5-bss-initialization-and-image-5-entry-order) does not prove DMA completion, destination visibility, bypass-state initialization or first-use integrity. Declaring coordinate data at `0x0110e770` cannot remove its candidate entry: `0x011442e8/0x011442ec` load before the `index <= 17` check, and the overlapping-base reader at `0x0112e7c4` requires its complete signed-index domain. Stream membership still proves no fetch exclusion.

### Device-driven cancellation boundary

The #133 bank-specific rename cancel/commit path uses `rb_name_edit_cancel`/`rb_name_edit_commit` and `rb_ui_pop`. It does not establish execution of generic `rb_ui_editor_cancel` or the owned dispatcher. Generic cancellation needs a reachable packaged route into modal 75 state `0x1e`, event 13, an observed owned cancellation entry, its retained-service ordering, the dispatcher's subsequent high-halfword decision, repeated entry/edit/cancel/handback, visible output and preserved resource/frame effects. Injecting state or directly invoking the action is not that route.

The existing reviewed `trace_sites` mechanism can observe the retained dispatcher without retargeting it:

| Instruction address | Register observation before the instruction |
| --- | --- |
| `0x00bfb0a0` | r0 = controller, r1 = captured state, r2 = captured event |
| `0x00bfb0c8` | r3 = selected action target |
| `0x00bfb0d8` | r2 = signed high halfword read after the action |
| `0x00bfb0f8` | r2 = captured transition-table base, r3 = transition target |
| `0x00bfb114` | r3 = direct state's negated high halfword |

The retained route can supply bounded caller/frame observations before admission. Global dispatcher retirement would additionally require closed global registration/storage/reference obligations and a both-core retired-fetch guard over the complete replaced extents. The current native and bank-specific results do not provide that cutover.

### Scoped modal-75 diagnostic and fallback design

The entry mechanism selects owned dispatch only for full-word controller ID 75 and retains the original dispatcher for every other ID. The synchronous seam qualification below does not publish or admit the hook. `modal-closure --diagnostic-controller 0x4b` computes a conservative modal-75 diagnostic and always reports `admitted=false` and `fallback_qualified=false` (E-MODAL75-SCOPED-DIAGNOSTIC-150).

The diagnostic retains 6,828 residual obligations, plus the separate pending fallback-boundary proof. It covers 46 resolved modal-75 call sites and eleven unresolved sites that may select 75, 2,208 captured cells, 294 action slots and one transition slot. The residuals are 6,507 pointer escapes, 286 computed-index stores, sixteen analysis limits, eleven unresolved caller IDs, four unresolved state/event values, two data-word references and one each for initial arrays and uncovered constant forms. Scoping removes 79 unrelated table-word references and one known other-controller event-value obligation, not the unattributed provenance obligations.

Both array and table provenance are computed globally before scoping. The global table materialization range and pointer classification remain intact, including an unknown offset derived from another controller's table. Only resolved address extents proved disjoint from admitted storage are dropped. Public regression coverage checks that unrelated known writes can drop while a computed store from another controller's table remains an obligation. The selected caller diagnostic leaves every r1/r2/r12/LR continuation unresolved; three r3 continuations overwrite before a read or unresolved stop, and 54 remain unresolved.

The fallback must account for flags and the replaced first instruction. A bare `cmp r0,#75; bne 0x00bfb028` is invalid: CMP changes NZCV and the branch reenters the hook. The 56-byte A32 entry uses r12 and the eight bytes `[S-8,S)` to save incoming r12/CPSR, compares the complete r0 with 75, then takes one of two separate cleanup paths. Each path restores NZCVQ, r12 and SP without changing the restored flags. The admitted path branches to owned policy with the original incoming context.

The nonadmitted path replays the original first instruction `push {r4-r11,lr}` and branches directly to untouched stock `0x00bfb02c`. At that handoff, r0-r12, LR, flags and SP=S-36 match stock after its first instruction; `[S-36,S)` contains the identical saved words, and caller-owned memory at S and above is unchanged. The replayed push overwrites both temporary words before stock can read them. No table, application-state or peripheral access precedes the controller test.

This is a synchronous handoff relation, not identical pre-handoff write history. The routing adds two temporary stack stores. Its admission proof still needs executable/published-word integrity, nonwrapping original stack capacity, physical separation and exclusion of asynchronous observation or alteration of the temporary interval. It introduces no deeper stack extent than the original 36-byte push. No new callback may be called on the routing path.

For admitted modal 75, cheap callback/return-seam equivalence and consumer closure are complementary. Exact relocated LR identity is not cheap; the 294 captured action slots and single transition slot need an input-use census before selecting repairs. The controller's state/event writer and table-mutation domain remains a separate obligation. The retained dispatcher is not globally retired under this design, and unconditional fetch exclusion of its full body would reject legitimate fallback executions. Guarding must distinguish admitted execution and nested nonadmitted fallback.

### Modal entry seam mechanism

`replacement/modal_runtime/src/entry.S` implements the fourteen-instruction routing mechanism. E-MODAL-ENTRY-SEAM-150 used a standalone 56-byte native-only image bound to the unchanged E-MODAL-DISPATCHER-LINKED-ABI-150 export and stock+4. Independent source review accepted that mechanism and observer before eight stock-prefix oracles, twelve candidates and five controls passed in 25 native invocations. Those artifacts remain historical evidence. The separately selected modal package now links the seam into its actual ELF; the default223 archive does not.

The current `//replacement/modal_runtime:qualify-entry` requires an explicit kernel ELF digest and extracts the actual linked seam and dispatcher addresses from that ELF. It binds the qualified #125 backend and pinned image5, and derives the stock extent with `StockInventory`. Eight nonadmitted controller words cover ordinary IDs, the known upper boundary, out-of-range values and two words whose low bits equal75; four admitted fixtures vary flags and stack alignment. Fallback cases compare every register, complete stack bytes and complete ordered write histories with a stock first-instruction oracle. Admitted cases check the original incoming context before owned policy executes. Both protect the complete incoming-SP-and-up caller interval. Controls remove flag restoration on each route, remove r12 restoration, write caller-owned stack and select the wrong stock continuation. The actual-ELF cohort requires fresh qualification; it does not inherit the historical result.

The native wire profile requires eight-byte initial SP, LR equal to its exit and NZCV-only initial flags. A six-instruction, read-only literal prelude establishes the desired seam SP, LR and NZCVQ/GE without changing stack memory. Only the four-byte original push is declared as the stock prefix function; stock+4 stays mapped. Both handoff endpoints are outside declared functions and stop as `unknown_call`. Qualification requires the exact endpoint, complete register/stack state, zero engine error, canonical instruction count (stock7, fallback17, admitted16) and complete provenance (stock13, fallback17, admitted8 total data accesses, with exact projected ordered writes). The current caller-write control adds an isolated `STR r0,[sp]; B actual_seam` stub, two instructions and one caller-memory store, without changing the seam, SP, registers or flags. The historical control added one instruction.

The original #125 backend excludes all register MSR instructions, including the mechanism's legal user flags restoration. E-NATIVE-APSR-MSR-150 independently reviews and qualifies the extension admitting only CPSR flag-field masks 8 and 12. The seam cohort pins that backend's `b51544c6...75da2860` executable identity. A prior host-only input refusal found the linked ELF's 32,944-byte envelope exceeded the harness's 16,384-byte file bound; raising the bounded envelope to 65,536 leaves the exact 56-byte executable section and symbol checks intact. No guest ran in that refused attempt, and the corrected cohort executed once.

### Explicit native artifact binding

Dispatcher and cancellation drivers require `--kernel-elf-sha256`, `--kernel-text-sha256` and `--kernel-syms-sha256` for the selected build directory. They extract `.text` from that ELF, compare the supplied `kernel-text.bin`, and check symbol addresses, sizes and complete function extents against the same ELF. The relinker writes `kernel-text.bin` to a separate output. `inputs.json` records expected and actual identities; existing receipts remain bound to their original artifacts.

```sh
bazel-bin/replacement/modal_runtime/qualify \
  --runner "$RUNNER" --build "$BUILD" --out "$DISPATCH_OUT" \
  --id5 "$ID5" --id5-base 0x00bf1a00 \
  --kernel-elf-sha256 "$ELF_SHA256" \
  --kernel-text-sha256 "$TEXT_SHA256" --kernel-syms-sha256 "$SYMS_SHA256"
bazel-bin/emulation/owned-menu-cases \
  --runner "$RUNNER" --build "$BUILD" --out "$CANCEL_OUT" \
  --id5 "$ID5" --id5-base 0x00bf1a00 \
  --kernel-elf-sha256 "$ELF_SHA256" \
  --kernel-text-sha256 "$TEXT_SHA256" --kernel-syms-sha256 "$SYMS_SHA256"
bazel-bin/replacement/modal_runtime/qualify-entry \
  --runner "$RUNNER" --id5 "$ID5" --kernel-elf "$BUILD/kernel.elf" \
  --kernel-elf-sha256 "$ELF_SHA256" --out "$ENTRY_OUT"
```

The emitted-code checker admits CPSR reads and register-form APSR_nzcvq writes. Reads invalidate the destination's abstract value; writes invalidate comparison facts. CPSR control writes, SPSR access, immediate MSR forms, other field masks and PC operands remain rejected. The native backend's separately qualified mask12 support does not broaden this bank checker.

### Modal-only 224 requalification

`E-MODAL224-HOST-QUALIFICATION-150` and `E-MODAL224-NATIVE-LIFECYCLE-150` qualify the separately selected modal-only variant's linked ELF. The default remains223 publications; modal adds one final activation publication. Lifecycle selection refuses explicitly until its three publications and stock veneers are supplied. Group destination and physical-order checks reject malformed selected groups, including identically malformed relocation pairs.

The fresh native cohort executes88 invocations:17 dispatcher pairs and five controls, ten cancellation pairs and four controls, then eight stock-prefix oracles,12 entry cases and five entry controls. Every comparison matches and every control refuses at its intended boundary. ELF, extracted text and symbols are bound explicitly; previous candidate receipts remain unchanged. The synchronous stock+4 boundary preserves the complete expected register/stack state, including full-width controller selection and application flags.

The host checks retain15 passing test targets across the corrected runs, including both actual modal link bases and the default archive identity control. The SDK check phase passes164 tests. The emitted-code checker retains commuted pointer roots without inventing bounds; positive and negative controls distinguish exact declarations, absent declarations, different roots and rootless unknowns. Failed host inputs and their corrections remain retained.

This cohort does not execute a packaged guest route or discharge callback-frame, caller continuation, object initialization, asynchronous writer, alias-fetch or task-identity obligations. The producer and selected package interfaces are implemented, but their existence is not route admission. Natural generic state30 cancellation still needs an identified X-M1 route; wireless NAME remains controller76, not that route.

### Packaged modal75 FILMSIM route

`E-MODAL224-PACKAGED-FILMSIM-150` qualifies the selected modal224 package's bounded cardless FILMSIM behavior on the corrected SDK. The matched default223 control retains the stock dispatcher and the already-owned bank actions. Both execute the same16 physical keys: the qualified package prefix, shooting-menu navigation, FILM SIMULATION entry/BACK, repeated entry/exit, live-view handback and post-rollback stock navigation. The candidate installs122040 bytes, completes every segment and verifies all224 original publication words after rollback. The control verifies223. The production default remains223.

The canonical permit interval records254 completed entries:40 owned75 handoffs and214 stock+4 fallback handoffs for controllers0,30,74,100,107. All254 return with no active frame or failure; peak per-core nesting is5. Every observed entry is on core0. The40 captured75 domains have state0 and events2,8,13,50. The complete-array physical writer audit records274 CPU0 write attempts, zero host/model writes and no observer integrity violation. Forty two-byte event75 writes carry exactly2,8,13,50; no state75 write is recorded. Its strict-protected-write classification is retained audit evidence, not a no-write verdict.

`E-MODAL224-RENDER-DIFFERENTIAL-150` compares all ten capture pairs on one SDK with exact artifact bindings. PNGs, native source-plane bytes, complete native frame and scanout states, ordered source spans and pixel metadata all match. Capture clocks differ and are retained, not normalized into equality. Resident1024-byte and live-setting450-byte snapshots remain unchanged at each declared capture; these checks do not exclude transient changes between snapshots. Both final sessions have zero SD commands and accepted writes.

The raw route outcome remains `unqualified-scoped-diagnostic`. Observation begins after installation and ends before rollback. Physical-fetch alias coverage, task identity, whole-installed-lifetime coverage, table/root/width immutability and general caller/private-frame/object closure remain unproved. Same-route migration onto a core already holding a permit is not detected. Generic state30 cancellation remains separately native-qualified without a natural X-M1 route. The two failed inputs and the earlier-backend stock pass remain retained in `E-MODAL224-PACKAGED-ATTEMPTS-150`.

### Published modal storage obligations

Modal75 storage is loaded image5 data, not an established read-only hardware region. Its cell table spans `[0x00cd1ae4,0x00cd3f8c)`, action table `[0x00d04080,0x00d0452c)` and transition table `[0x00d01370,0x00d01378)`; the next controller-table bases match these upper bounds. Root pointer words are `0x00cf59e4`, `0x00cfe3d4` and `0x00d00268`; width is the halfword at `0x00cd5f0e`. Bank publication deliberately changes cell/action words. The static census reports no resolved direct table store but does not establish absence of stores through unresolved pointers. All 286 computed-store obligations concern the state/event arrays.

Route-bounded write observation can constrain escaped table-pointer effects, not eliminate them globally. A reset-to-menu witness needs expected loader writes followed by a post-load immutable phase; a bank editor witness needs the expected publication-adjusted bytes and a postinstall-to-preunload phase. Watches must include roots and width, cover both cores and every memory-write initiator, reject lost view/alias coverage and refuse an actual table-write control. Legitimate state/event writes need a separate value/domain observation rather than a no-write rule. The #133 bank-specific editor route remains distinct from generic owned cancellation.

### Reachable stock modal75 route

E-MODAL-RETAINED-FILM-ROUTE-150 observes a natural stock shooting-menu route with no bank allocation, installation or unload. The reviewed FujiMod cardless no-install path executes `emulation/modal75-stock-route-trajectories.json`: MENU opens `(1,5,2)`, four DOWN inputs and OK open FILM SIMULATION `(1,254,0)`, and BACK returns `(1,5,6)`. All expected states match. Three captures retain the shooting menu, the PROVIA/STANDARD film list and the returned shooting menu. The dispatcher first word is the original `push {r4-r11,lr}` before and after the route. The session has no card and records zero SD commands or accepted writes.

Fixed instruction traces record 25 controller75 contexts, all at state0: event2 four times, event8 once, event13 once and event50 nineteen times. Their action entries are `0x00c6eb44` four times, SET `0x00c72e10` once, BACK `0x00c76650` once and `0x00c7a528` nineteen times. Other observed controllers are 0, 30, 74, 100 and 107. No entry to generic editor cancellation `0x00c77794` occurs. This proves a bounded ordinary-menu route through75, not state30 editor cancellation, callback-frame equivalence or owned dispatch.

The natural wireless NAME menu is not that missing editor route. The [X-M1 manual, page87](https://dl.fujifilm-x.com/support/manual/x/fujifilm_xm1_manual_en.pdf) names WIRELESS SETTINGS → GENERAL SETTINGS → NAME. Its pinned class`0xb5`/row0 action `0x00c7ddac` writes adjacent controller76 state36 at `0x01467620`; its class`0xb6` keyboard uses bufferB and cursor positions9..23. The bufferA state30 producers `0x00c72844` and `0x00c75cdc` instead require class`0x130`/row0 after prior selection classes`0x12e`/`0x12f`. A bounded search of image4/5 literal node headers found no matching stock root. The decoded node scanner excludes kind values above16 and therefore even omits the real wireless NAME node; its omissions do not prove absence. Runtime-built nodes, other images and packed construction remain possible. No natural stock state30 route is established.

### Conditional FILMSIM closure

E-MODAL-FILMSIM-CONDITIONAL-150 runs `modal-closure --diagnostic-filmsim-route` over the pinned resident images, unchanged linked candidate and its published tables. It retains all6,828 controller obligations and reports `admitted=false`. Of46 resolved75 call sites, four have locally known event sets compatible with the route, two have unknown event values and remain possible route entries, and40 are outside only under their local-writer premise. Eleven unresolved controller sites also remain blockers. None of the57 sites proves its captured state/event domain.

| Known compatible call site | Local event values |
| --- | --- |
| `0x00c0bb8c` | 2 |
| `0x00c0bcb0` | 8 |
| `0x00c0bddc` | 13 |
| `0x00c0c538` | 49 or50 |

The two resolved sites with unknown event values are `0x00c0c01c` and `0x00c0c0cc`; four known compatible sites are not the complete may-feed set. Local event stores neither establish captured state zero nor exclude intervening asynchronous, reentrant or alias writers.

For state0 and events2/8/13/50, all four published cells have signed high halfword1 and transition slot1 is null. The published action slots are22,1,1,274, producing target set `{0x00c6eb44,0x00a86144,0x00c7a528}` in this bound candidate. Under the explicit entry-domain, root/width/table use-point immutability and linked-semantic-correspondence premises, the dispatcher transition call and direct state store are unreachable. Callback descendants, caller/frame integrity, initial contents, unknown callers and writer/escape obligations are not discharged. The candidate must be recomputed after the action cutover or relocation.

`emulation/fujimod-xm1/src/modal_permit.rs` implements the pure per-core entry/handoff/return bookkeeping, with fixed-depth stacks, full handoff state, saved stock-frame checks and exact return PC/SP. Baseline policy sends every ID to stock+4; scoped policy sends only fullword75 to owned dispatch. Source review accepts this conditional component; no SDK execution qualifies it yet. It admits downward-growing LIFO observations, but same-route execution migrating onto a core already holding a running permit is not detected. Task/scheduler identity and physical fetch coverage remain separate premises, not fail-closed claims.

### Packaged scoped-hook design

The package selects the modal Rust module and its assembly through the existing bank crate, Bazel data/relink closure and `build-recipe-bank-ram.sh` source loop. `recipe_bank_ram.ld` binds `__rb_modal_stock_resume` to untouched `0x00bfb02c`; the native-only seam linker script is not a package linker. `BuildRecipeBank` resolves `rb_modal_entry`, checks the pinned original push at `0x00bfb028` and adds one ordinary `RamPublication` tail branch. The safe publication order places this activation after its data dependencies, so reverse rollback restores the shared dispatcher entry before restoring dependent table publications. Publication counts, installer/linker assertions, source identities and review pins derive from the joined bank rather than a count-only append. This source selection must join the #133 action cutover; it needs no edit to the action policy.

The seven table/root/width spans above reuse the classified Fixed observer. Both cores and the SDK's CPU, host and model-bus write paths remain covered; unresolved physical mappings, view changes and incomplete observations refuse the run. State/event cells are domain observations, not immutable table ranges. The 450-byte live-setting interval retains per-store byte preservation, while table/root/width ranges reject every unaccounted write, including identity stores. The pending route layer's eight-range combined limit needs a bounded extension when write spans and retired-execution spans are combined; the SDK's larger class limit is not a reason to widen protected addresses over unrelated gaps.

| Phase | Required observation and invariant |
| --- | --- |
| Loader population | Bind legitimate image5 population writes and the exact last-loader-write/handoff boundary. Existing post-attach/install watches do not cover this phase. Fixed-view epochs must be complete before claiming reset-to-menu preservation. |
| Before installation | Arm before allocation/upload and bind original table/root/width bytes to the pinned image. An installed-checkpoint restore qualifies runtime only, not skipped installation. |
| Publication | Account only for the exact ordered manifest intersections, address/width/value and phase, during the installer’s existing parked-peer/interrupt-masked publication window. Every other protected write refuses. |
| Installed route | Require the publication-adjusted table bytes and strict table/root/width immutability. Record state/event values and owned/fallback entry witnesses on the reachable75 route. |
| Rollback | Account only for exact reverse publication writes, then require original table/root/width bytes. End owned-route retirement guards before ordinary unmodified-stock handback. |

A real write control can reuse an existing table restoration store: in a separate control process, withhold the rollback allowance for one declared table publication while retaining the same Fixed ranges and run the real installer rollback. Acceptance requires the actual CPU-write observation at that publication's physical address and width, its observed value and the intended strict no-write refusal. A missing allowance rejected before execution, a generic failure or a fabricated record is not that control. This avoids adding guest mutation bytes or a generic memory-write RPC. Host/model-bus source-class claims still need their corresponding reviewed SDK controls.

An unconditional fetch ban over the original dispatcher body would reject required non75 fallback, including nested fallback from an owned75 callback. Scoped retirement therefore needs an entry/return-context permit, not just physical Fixed ranges: capture full controller/SP/LR at the shared entry, require75 to reach owned dispatch and non75 to reach stock+4, and permit original-body execution only in the matching live fallback frame. Per-core nested contexts must retire at the exact return PC/SP; an interior entry without a fallback permit refuses. The pure component above is not yet wired into the fixed trace producer. Canonical instruction points alone do not establish alias-complete physical fetch exclusion.

The mechanical bank gate also needs narrowly admitted flag-save/restore forms and closed computed-call/state-store domains. Declaring the prehook or selecting the modal module does not discharge those domains. The remaining caller/frame, initialization, object-entry and asynchronous premises are unchanged by the retained FILMSIM observation. No packaged hook or phase-aware table guard is implemented or qualified by this design.



### Owned editor-cancel increment

`replacement/owned_menu/src/lib.rs` now exports `rb_ui_editor_cancel()` for action `0x98` of cell `(0x1e, 13)`. The bank interposer calls it for the LEAVE path in `menu/recipe_bank_actions.c`; the former `rb_call_editor_cancel` trampoline and its `0x00c77794` allowlist entry are removed. The original body remains the oracle in `emulation/OwnedMenuCases.hs`, not a bank fallback.

The export takes no arguments and returns the action word. It calls retained `FUN_0063a0f8` with query `0x53`, reads the retained session cell `0x00954d08`, and tail-calls the retained stage open `0x00d3d858` with kind `0xf` and id `0x1a` before retained `FUN_00c12aac`. A nonzero query with a session value other than `1` returns `0` and performs no stage or transition. Otherwise it returns the transition result; when that result is `-1` or `2`, it writes halfword `0x1d` to the modal state cell `0x0146761e`. The dispatcher's post-action transition, live-frame refill and redraw remain stock and are not owned by this increment.

`//emulation:owned-menu-cases` defines ten differential cases over query values `0`, `1` and `2`, session values `0`, `1` and `2`, transition results `-1`, `2` and `0x5d`, and one read-only session region. It compares preserved registers, CPSR control state, state/session/log bytes, caller-owned stack bytes and ordered observable write addresses/widths against the stock oracle. Its controls change r4, the state halfword or incoming-SP memory after executing owned cancellation, or transfer to the fetch-guarded retired entry. E-MODAL-NATIVE-LIFECYCLE-150 records their passing qualification. No natural packaged route into cell `(0x1e, 13)` establishes this export's device-driven lifecycle; the native cases model retained services and are not device evidence.

The claimed-page SET path still depends on stock activation and mode gates before the generic exit in `FUN_00c72e10`. In `FUN_00d4d7a4` case 1, tokens `0x63..0x70` and `0x73..0x79` set `0x009544f8`; the default taken by bank parent tokens `0x7d`/`0x7e` clears `0x009544fc` and `0x00954500`, not `0x009544f8`. A page class alone therefore does not prove that gate clear. The other SET gates read `0x00950474`, `0x00950478` and `0x00954500`. No device-input scenario establishing a carried set gate is qualified here.

After transition, the SET notification tests use the new live frame: `FUN_0061d1f4` and `FUN_0061d244` refill their local snapshot through `FUN_0061cd14`. Reusing a pre-transition class or row token would change the suppression for class 5/token `0x35` and class `0xe8`. The claimed bank LEAVE path already has its own stage/notification behavior; replacing the controller must preserve that behavior, not silently substitute the generic stock leave path.

The C binding also distinguishes frame-pointer identity `0x0078ea98` from the node id at offset zero. Its row input is absent, stale or present: a null values pointer and an invalid cursor must be handled before reading a token or child class. Physical left/right entry and the registered cell are separate facts from the mutable event and state halfwords.

The existing `card-sel.img` and `emulation/ui-trajectories.json` provide a populated-bank selection baseline. They are not the cardless Rust route or qualification of an owned SET exit. A complete selection/apply/exit cutover needs a matching card-backed FujiMod run, the existing mode-dependent exits and explicit scoped forbidden-execution evidence. Moving more C classification policy alone does not retire these proprietary handlers.

### SET, confirm and leave source increment

Issue [#133](https://github.com/skeet70/latent-image/issues/133) continues candidate B of the [ownership inventory](replacement-inventory-baseline.md#8-current-ownership-and-retirement-view). `replacement/owned_menu/src/lifecycle.rs` implements the complete three action bodies, exporting `rb_ui_set`, `rb_ui_editor_confirm` and `rb_ui_leave`. Bounded original/owned execution and independently reviewed composed editor comparisons pass on the current pinned native backend. The bank now calls those owned entries; the three obsolete stock-call trampolines and their allowlist sites are removed. Native bank/UI qualification is described below. These claims do not retire the modal dispatcher or the application runtime.

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

The return, r4 and state refusal controls are AAPCS-aligned guest wrappers around the real owned generic SET, including its stage `(0xf,0x1d)`, notification `0x930` and blink store. Each wrapper saves r12/LR in an eight-byte frame, calls the real entry, changes only r0, r4 or the state halfword, restores SP/LR and returns. The state wrapper also saves and restores r0/r1. An unwrapped real reference must have no mismatches, and each wrapped run must produce exactly the intended typed register or state-region mismatch, with no other mismatch. Three separate guest transfer controls require both `fetch_guard` and the exact original entry PC. Control reports retain producer status, engine error, PC and mismatches; the shared observer saves each raw `native-result.json` before typed parsing. An earlier producer rejection is reported, not accepted as a broad control success. No pair, reference or control has executed or qualified on either native backend.

The new fixture groups retained ABI bodies in eight immutable synthetic containers. Its exact function extents remain separately declared; the padding is not a declared function or firmware evidence. Original-action bodies are executable only in the original half of a comparison. Owned cases leave those entries unmapped, while each fetch control maps only its own four-byte original target read-only. Emission rejects overlapping model bodies, containers and ordinary regions, rejects bodies/functions that do not fit, and passes every case through the existing loader with its unchanged 16-region/256-function limits. The owned inventory follows compiled ARM branches from the selected export, using ELF function extents; unresolved non-return indirect transfers and undeclared retained targets abort emission. This is a bounded fixture inventory, not stock caller closure.

The final host-only smoke built `//replacement/bank:bank` and `//emulation:controller-lifecycle-cases`, linked with `BUILD_ONLY=1` at native-only base `0x00bd0000`, and emitted 85 original/owned pairs without execution. The 34,104-byte `kernel-text.bin` has SHA-256 `ea41047ab80eb04e0a394c4801a61d61908d28d94c5cb996877b6b834f36e762`; the complete linked `kernel.bin` has SHA-256 `30424e6815e612880bf1e7b6478001e4a658bb685d28b5a63ce9d9e2a5083d38`. Private outputs are under `private-data/scratch/issue117-controller-lifecycle-sol/`. These are build and emission observations, not native qualification (E-MODAL75-LIFECYCLE-SOURCE-150).

The guest-control source follow-up rebuilt `//emulation:controller-lifecycle-cases` and emitted and loader-validated 85 original/owned pairs, one unwrapped real owned reference and six guest controls, 177 cases total, under `private-data/scratch/issue117-controller-lifecycle-sol/cases-guest-controls/`. Five scenario identifiers were corrected to the loader's canonical form before the final successful emission. No native execution or SDK build occurred (E-MODAL75-CONTROLS-SOURCE-150).

The earlier 55-case UI receipts remain historical synthetic-controller evidence. This increment replaces their injected SET, confirm and leave bodies with the owned controllers, removes delegate counters and return echoes, and retains caller-visible return, menu/frame, settings, persistence and editor-buffer assertions. Its per-trajectory function inventory includes only reached owned functions and retained dependencies, within the unchanged 256-function bound.

#### Native lifecycle comparisons

The frozen kernel at native-only base `0x00bd0000` executes on the current pinned `//emulation:native-runner`. The 85 original/owned pairs cover the normal, special-class, confirm, leave and refusal cases described above. The real reference and all six staged guest controls pass. Both halves of `leave-generic` exposed one fixture defect: its transitioned node is `0x61`, which suppresses `0x933`, but its expectation demanded that notification and the blink store. Correcting only those expectations makes both corrected inputs pass. No owned-body or backend defect and no intended behavioral difference was observed (E-MODAL75-LIFECYCLE-NATIVE-150).

`controller-lifecycle-cases --editor` composes SET on class `0x167`/row 1 or 2, confirm or cancellation, then leave in one guest execution with shared RAM. Ten original/owned pairs cover transition results `-1`, `2` and `0x5d`, active-query/session-1 admission and session-0/2 refusal. Literal traces check each action's return, state and editor choice; final RAM and ordered retained-service effects also match. A real composed reference passes; real-body wrappers produce exactly the intended return, r4 or state mismatch, and four transfer controls fetch-guard at the exact original SET, confirm, leave and cancellation entries. Independent review confirms the composition observer after closing its empty-scenario-selection finding with a command-line refusal and behavioral regression test (E-MODAL75-EDITOR-NATIVE-150).

Retained query, frame, editor, buffer, mode, stage/transition, notification and WB operations are modeled ABI boundaries here, not owned service implementations. The composition is direct action invocation, not the modal dispatcher, a bank/UI trajectory or device-driven execution. Native base `0x00bd0000` is not an accepted package placement. The complete frozen image spans `0x94b4` bytes, ending at `0x00bd94b4` there; the unchanged default `0x00be0000` would end at `0x00be94b4`, below name record `0x00beb784`. The full-image/name-record guard remains required.

Reproduction builds `//emulation:controller-lifecycle-cases`, `//emulation:native-runner` and `//emulation:emulator-slot` through devenv/Bazel. Pass `--build <frozen-build> --out <fresh-output> --id5 <original-image-5> --id5-base 0x00bf1a00 --execute --runner <native-runner>` to the comparison driver under one emulator slot. Add `--editor` for the composed corpus. `--scenario <identifier> --no-controls` selects only a corrected action pair; a missing or unknown identifier exits 2 before input loading or emission. `//emulation:controller-lifecycle-cases-tests` exercises that command-line refusal. Preserve a failure and run only corrected inputs once. The evidence ledger binds the retained corpus, results and input identities.

#### Native bank/UI cutover

The migrated 55-case suite executes real owned lifecycle and menu-frame bodies, the retained editor engine, string helpers and loader memory/division helpers. The first full selection passes 54 cases and all eight controls. Its only difference is a harness getter expectation in `n-fields-3-4-5`: `rb_stage_edit_get` returns the stored property code, not the UI row token. COLOR token 0 maps to stored code 3 through the X-M1 table at `0x0077a810`. The corrected case derives getter expectations from the independent literal expected payload and passes once with `--case n-fields-3-4-5 --no-controls`. No other native UI input is repeated (E-MODAL75-UI-CUTOVER-NATIVE-150).

The native UI model supplies the inactive query and stage/transition boundaries. It qualifies the bank consumers of the owned bodies, not the natural retained dispatcher or transition implementation. The packaged device route below separately checks real retained-service inputs, frame/state continuity, text/cursor changes, editor cancellation, bank exit and reentry. Dispatcher/source closure and its integrated route remain #128. Bank-specific pop/cancel helpers do not establish the generic modal cancellation route.

#### Packaged device lifecycle

The current 223-publication recipe-bank, D3 and Classic Chrome RAM package passes a cardless emulator installation, 15 matched lifecycle segments comprising 35 physical-key steps, and rollback with all 223 original publication words verified. Installation validates 131 setup and 92 activation records. No allowlist override is used. The actual allocation is at `0x00a81be0`, with the declared `0x1dc98` extent.

SAVE CURRENT publishes generation 1 with slot mask 1 and no active selection. Typing A then BACK leaves the resident and saved/working payloads unchanged; reopening shows the original empty name. Typing B, navigating to Confirm and accepting publishes generation 2. Reopening reads B with insertion cursor 0; the slot list displays B. Exit hands back to the retained shooting frame, BACK reaches owned leave, and MENU reenters. Fifty-nine literal read assertions and nine cross-segment preservation assertions pass. The natural query inputs, editor session and SET gates are recorded as zero, not host-seeded expectations; this device run covers inactive-query admission.

Entry traces observe owned SET once, owned editor confirm twice and owned leave once. Both-core Fixed-view fetch exclusion records zero fetches in the complete original SET, confirm, leave and cancellation spans while the candidate is active. These traces establish entry identity, not internal helper-call-count qualification. The bank's name-cancel and Confirm-row paths use their custom pop helpers, not generic `rb_ui_editor_cancel`.

The initial device run exposed a guard-policy defect after all lifecycle assertions: strict no-write protection refused 3,807 identity CPU stores. Its immutable failure is retained. The reviewed correction binds an explicit per-store byte-preservation policy, validates complete preimages and endian-decoded values at every protected byte, accepts identity only, and rejects transient changes even if restored. The corrected input passes once. All 3,807 recorded protected CPU stores are identity stores; no host/model-bus writes, observer violations or media operations occur. Preservation and media checks continue through unload (E-MODAL75-DEVICE-GUARD-POLICY-150, E-MODAL75-PACKAGED-DEVICE-150).

The independent refusal input first passes its clean installed interval, then unloads and drives one stock OK action. It observes 272 original-body fetches, beginning exactly at restored SET `0x00c72e10`, and records the expected refusal with a nonzero command exit. Its strict ordinary-settings guard records no writes. This controls the fetch observer without patching an original body or forcing a PC.

The 24-pixel magenta left margin and clipped lower-left OK label in reopened B are shared emulated rendering artifacts, not cutover-introduced redraw differences. Two fresh diagnostic executions use the same viewer/SDK and 35 matched physical keys; the stock variant restores only the three original delegates and their declarations. Both pass installation, all 15 segments and 223-word rollback. Every one of the 15 PNG pairs, native source-plane byte streams, complete native frame states, scanout register states and source spans is identical. Reopened B also exactly reproduces the earlier anomalous PNG. Entry traces prove the stock bodies and the owned entries actually execute. Per-capture plane/rectangle metadata and source bytes are retained (E-MODAL75-RENDER-DIFFERENTIAL-150). Owner visual acceptance and physical rendering/timing remain separate; the declared conversion profile is still an unmeasured UI model. Native active-query/session comparisons remain distinct from this inactive-query device route.

#### Composed placement and stack bounds

The actual volatile recipe-bank plus Classic Chrome image is `0x864c` bytes, within the RAM-push image capacity `0xa300`. Diagnostic source is linked by the normal relinker, but `RB_RAM_DIAGNOSTICS` is undefined. A separate host-only build with that macro enabled measures `0xa66c` bytes, exceeding the unchanged capacity by `0x36c` (876 bytes); the linker refuses it at the existing generated-data boundary. The normal image is the qualified package. The enabled diagnostic variant has no package or runtime qualification. The frozen native comparison image's `0x94b4` bytes exceed the static flash package capacity `0x8300`; that persistent route is not pursued.

Both reference bases, `0x00c40000` and `0x00c4fc88`, reproduce the 122,008-byte package with 223 publications. Each allowlist receipt passes with zero violations, 191 declaration-backed sites, 432 proven stores and 979 proven branches over 7,252 instructions in 261 functions. Independent emitted-code review confirms bank routing to owned entries, lifecycle/capture stock-call ABI, relocation and A32 code without Thumb or SDIV.

The provenance-bound comparison accounts for 7,384 physical differing bytes in 2,392 words. Fifty-seven component mappings include the additional `0x378` offset movement of names and installer data; 467 semantic relocation fields account for the content differences, with zero unclassified fields. Each image validates 470 source-bound pointer fields and 48 publication-pointer fields, including unchanged values, and all 223 actual table records match their manifest. Address-like scalar and stale-pointer controls cover the rejected normalization shortcuts.

The actual table word installed at `0x00e9e038` is `0xeaf6a0aa` at the first base, an unconditional A32 `B` to `rb_capture_develop_wait` at `0x00c462e8`; at the second base it is `0xeaf6dfcc`, targeting `0x00c55f70`. These are decoded table fields, not only manifest assertions. Retaining `--emit-relocs` changes no linked or runtime bytes at either base, established by bytewise comparisons against the pre-metadata packages. The only manifest identity change is the relinker source that requests that metadata (E-MODAL75-PACKAGE-HOST-150).

Retained task-creation source requests 3072 bytes for task entry `0x00c22f60` and 5120 bytes for capture task entry `0x00ea17d0`. `FUN_00725dd8` consumes descriptor word 3 as the allocation size and word 4 as the entry; `FUN_00725c6c` advances the allocation cursor in bytes. The bindings are image-4/5 decompilation `all.c:171145-171260,496198-496224,749300-749326`, not adjacent-constant inference.

| Emitted owned chain | Maximum contiguous owned contribution | Retained task capacity | Capacity less this contribution |
| --- | ---: | ---: | ---: |
| SET/leave → white-balance → row | 40 bytes | 3072 bytes | 3032 bytes |
| Scalar menu accessor → snapshot → classifier | 96 bytes | 3072 bytes | 2976 bytes |
| Capture develop-wait → step-wait → wait veneer | 48 bytes | 5120 bytes | 5072 bytes |

These differences are not available stack headroom. Lifecycle frames allocate no separate locals and save 8 or 16 bytes. Scalar accessors reserve 48 bytes plus an 8-byte save, snapshot saves 24 and classifier saves 16. Capture saves 8, step-wait saves 24, and the wait veneer saves 8 plus an 8-byte output reservation. Tail branches release their caller frames; sequential helper calls are not summed. Both reference-base disassemblies show the same costs and preserve an incoming eight-byte SP alignment.

The larger bank C paths have individual frames of 352 bytes for `rb_select`, 184 for `rb_mode_resume` and 120 for `rb_mode_leave`; the lifecycle bound is not a whole-package bound. Retained caller depth, retained callee depth, callbacks/reentrancy, interrupt/context-switch stack accounting, runtime task allocation/initial SP and complete bank/Classic Chrome nesting are unproved. No live SP high-water mark or whole-task peak is measured. The retained capacities and bounded owned contributions therefore do not establish task-stack safety.


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

On the legacy native runner, all 307 pairs match their literal expectations and each other: 301 return, the 3 zero-row operations both stop at the loader's `SVC` as `unsupported_instruction` after writing words 0 to 8, and the 3 unmapped-cursor operations both stop as `memory_guard` after writing words 0 to 3. Owned stack use is at most 96 bytes below the entry `sp` against the stock bodies' 84. A guest writing the expected words is accepted; a wrong cursor token, a twelfth word, a cleared `r4`, a store to the caller frame and a transfer to `0x0061cd14` in the owned layout are refused. The default kernel also passes all ten editor-cancellation differential cases and rejects their ABI, state and forbidden-fetch controls. Results are in `private-data/analysis/x-m1-1.50/recipes/issue117-model-layout-sol/menu-frame-cases/menu-frame-results.json` and the sibling `owned-menu-cases/owned-menu-results.json`. The SVC continuation, interrupts and concurrent frame changes are outside these cases.

The composed bank image no longer fits between the native suites' former default base `0x00be1f00` and the package plan's name record `0x00beb784`: linked there it ends at `0x00bebbb4`, and 51 of the 55 UI cases stop as `memory_guard` where the plan overwrites bank data. `emulation/build-recipe-bank-cases.sh`, `emulation/build-recipe-bank-ui-cases.sh` and both generators now default to `0x00be0000`, a native-case placement only, and both scripts refuse a kernel image that ends past `RB_NAME_RECORD`. The default UI command passes 55/55 with its controls rejected.

The default SD kernel/model link succeeds with the full model's `0x144` synthetic data bytes in `0x7f000000..0x7f001000`. Link controls reject both the full model in the `0xe4` media tail and a widened tail; the media-only model still links in that tail with byte-identical data. The complete SD workload remains unqualified: its execution stopped before producing aggregate results. Its last intermediate case result is `unknown_call` for `rb_assert_one` in `c-select-iso-manual-after-auto`, not a completed suite verdict. Artifacts are under `private-data/analysis/x-m1-1.50/recipes/issue117-model-layout-sol/kernel-cases-default`. FujiMod native-backend qualification remains separate.

Global retirement stays open. The 3,027 stock call sites still enter the stock bodies; retiring them needs reviewed entry publications at the eleven stock entries, a forbidden-fetch guard on the retired bodies in a packaged menu route, which the packaged FujiMod route does not yet provide, and a device-driven packaged run with that guard.

## Evidence

The Rust module's host tests are `//replacement/recipe_ui:route-tests`: a checked-in genrule compiles `src/lib.rs` with the pinned `@lab_rustc` and clang under `--test`, and the existing `//emulation:recovery-task-state-tests.sh` wrapper runs the binary. The four cases call only the exported C ABI and cover the page and slot boundaries, all four verb kinds with the first and last slot, select codes 0, 1 and 7, and the enumerated gap tokens returning zero.

The initial UI-only bank link used the #119 toolchain with one panic handler. Its ELF at the `0x00c40000` reference base passed the bank ELF allowlist with 0 violations and 169 declared sites. That archive linked at `0x00be1f00` ended at `__rb_image_end` `0x00be9c24`; its disassembly contained no UXTAB, UXTAH, SXTAB, SXTAH or Thumb form. These are historical UI-only layout observations, not identities for the later composed service package.

The native UI suite passes 55/55 with all five negative controls rejected for the exact kernel `f78664` on the retired C `native-runner.c`; every case reported `returned` with runner exit 0. The current proof is `private-data/scratch/issue117-preservation-consumer-tests/final-ui-9893ec0-1/cases/results.json`, with `control.json` and `findings.txt` beside it. This native result does not exercise the FujiMod SDK or the packaged full-system route, and the separate packaged service qualification does not establish native coverage.

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

The original `media-state` region ended at `0x00910340`. The `RBM_MEDIA_ONLY` media model's `.data` now links at that boundary and occupies `0x00910340..0x00910424`; `build-recipe-bank-ui-cases.sh` passes that span to `recipe-bank-model.ld`, and the UI generator refuses a model linked at any other base. The mapped `media-state` region is expanded to `0x00910140..0x00910424` (`0x2e4`). The original `0x200` window remains a `MediaWorkspace`, while the exact linked model image (`0xe4`) is a separate `ModelDataWorkspace`; the remaining padding is not mapped. The case generator bounds every model-cell overlay to the linked data length, so the select-draw case keeps the model data within the 16-region case budget without dropping a stock region. This synthetic native-case mapping is not a camera reservation. The kernel cases link the full model, whose data does not fit this tail, at their own native-case region `0x7f000000..0x7f001000`. `span-low` holds only the live settings record at `0x0096cc64`, the field table and the modeled card files at `0x00980000`. It has no workspace: the selected read-only cases leave it without write permission, and a mutating case makes it writable only through an expected byte that differs from the initial slice. `mirror` and `tables-*` remain stock-addressed RAM load.

The `effects` object separates the final-state match from the effect proof. `final_state` is `verified` when the returned diagnostic, registers, ABI workspaces, public claims and every non-workspace byte match; it says nothing about writes that were overwritten. With a complete provenance log every write is classified and a clean run is `verified`. With a truncated log a read-only case can still be `guarded` because `span-low` has no write permission, while a mutating case is `final-only`: its expected final bytes are verified and no complete transient proof is claimed. The verdicts are the closed `EffectVerdict` values `guarded`, `verified`, `final-only`, `failed` and `not evaluated`; guard, transient proof, scope and final state are likewise closed types, not string flags. A required effect is therefore complete or enforced by a guard, never inferred from expected final bytes alone.

The aggregate API is `Board::preservation_begin(ProtectedRanges)` over one to eight physical ranges. It counts identity and changed CPU stores, observes the differing protected intersection at the CPU store and records a `CpuChange`; it does not block that store, and the E2E verdict refuses the route after observation. That is distinct from the strict native pre-execution `memory_guard`, which stops a protected store before execution. The aggregate guard also records host and model-bus writes, and fails closed on unsupported backends, unresolved mappings, unverifiable writes or incomplete observation. This aggregate byte guard is separate from the legacy all-access `MemoryWatch`; it has no writer-PC allowlist, mapping fallback or final-snapshot substitute. On canonical upstream `9bb8bd9` the 450-byte live-property primary completed the route with the counters and retention in the evidence paragraph. The historical watch overflow remains separate failure evidence.

### Packaged watch command

```sh
XM1_EMULATOR_SLOTS=1 devenv shell -- bazel --output_base="$HOME/.cache/bazel/latent-image-issue117-ui-proof" run //emulation:xm1-viewer -- --ram-push-e2e --cardless --fujimod --unreviewed-relink --firmware private-data/analysis/x-m1-1.50/update/prefix/FWUP0002-prefix.DAT --installed-nand private-data/analysis/x-m1-1.50/stored-f7/emulation/inputs/f7-1.nand --trajectories emulation/recipe-bank-route-ram-push-trajectories.json --receipt-output <fresh private path> --output <fresh private runtime path>
```

Run the control with `--trajectories emulation/recipe-bank-route-ram-push-control-trajectories.json` into a separate fresh receipt. Its `ram_preservation` evidence must contain an actual changed protected CPU intersection and a positive changed-write counter; identity stores, an integrity fault or a generic failure do not qualify it. The guest PNG capture and repeated entry, navigation, cancel, handback and resource-failure segments remain in the tracked primary trajectory.

## Measurement

The #118 fixed workload is the behavior-preserving refactor of these two classifiers. Baseline same-invocation samples are 45.29 s total (1.07 s build, 44.22 s case generation and execution) for the unmodified tree, and 42.58 s (0.98 s, 41.59 s) for the C refactor, under warm Nix/Bazel and page caches, two jobs and sibling load.

The repeat used source `290faba`, the exact reviewed C range-helper patch and Rust wrapping-subtraction patch, the fixed `package-20261024b/image` inputs and fresh `RepeatRust`/`RepeatC` output bases and empty disk caches. Rust ran before C. Unrefactored setup was separate and build-only: Rust build log `58.266 s` (`59.900 s` outer), C build log `60.676 s` (`62.644 s` outer), each including the four targets and `//replacement/bank:bank`.

| Repeat same-invocation sample | C | Rust |
| --- | ---: | ---: |
| Incremental build, `--batch` | 12.669 s | 12.778 s |
| UI workload, all 55 + 5 controls | 95.413 s | 97.417 s |
| Plan gate, `--batch` | 14.135 s | 12.824 s |
| Total | 122.224 s | 123.024 s |

Both variants passed 55/55 with five rejected controls and the plan gate. Setup, timed build and plan gate all used Bazel `--batch` inside emulator slot 1; no persistent server existed between them. The timed payload recorded the slot cgroup. C peaked at 3,251,122,176 bytes and Rust at 4,148,891,648 bytes; both had zero swap. The C compiler log has 12 clang invocations including the `menu/recipe_bank_*.c` action units. The Rust execution log records the actual `rustc --crate-name=bank` action with `cacheHit: false`, exit 0 and 0.094 s; the C bank target was cached. No separate Nix bank derivation was found, so no forced Nix bank rebuild was used. Status, registers, ABI validity, effects, mismatches and controls are identical; full projections differ only in `kdata` ABI workspace offsets and provenance totals/retained counts (C 12,571,972/207,059, Rust 12,577,308/207,119). The earlier `98.552 s` C and `97.956 s` Rust samples remain limited measurements, not a matched speedup; the repeat totals are close and no speedup is claimed. Historical two-job samples remain confounded.

## Reproduce

Build the fixed targets first. This is build-only and runs no guest:

```sh
devenv shell -- bazel --output_base="$HOME/.cache/bazel/latent-image-issue117-ui-proof" build --jobs=1 //emulation:native-runner //emulation:recipe-bank-ui-cases //emulation:emulator-slot //replacement/bank:bank
```

Run the 55-case UI workload into fresh directories through the existing emulator-slot wrapper. `build-recipe-bank-ui-cases.sh` invokes the generator and native runner directly, so it must not be run bare:

```sh
XM1_EMULATOR_SLOTS=1 XM1_EMULATOR_MEMORY_MAX=8G \
  bazel-bin/emulation/emulator-slot \
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

## Modal/controller subsystem project

This is the first-deliverable plan for owning the dispatcher and all174 controllers' decision/state code as one subsystem, folding #128, #132 and #133 into #117/#127. Existing qualification stays scoped: modal224 retains stock+4 fallback for non75 IDs, and neither snapshot nor bank action migration retires every stock caller. Function boundaries may change internally; retained interfaces and accepted capability scenarios remain the qualification boundary. [Sources: [replacement contract](../spec/11-incremental-firmware-replacement.md#goal-and-completion-boundary); E-MODAL224-PACKAGED-FILMSIM-150; E-MENU-FRAME-SNAPSHOT-NATIVE-150; E-MODAL75-PACKAGED-DEVICE-150.]

### Machine-readable inventory and limits

[`//replacement/modal_runtime:controller-manifest`](../replacement/modal_runtime/ControllerManifestMain.hs) consumes pinned image5 bytes, the pinned decompiler function inventory and a receipt projection. Its private output is `private-data/scratch/ControllerManifest/manifest.json`; copied table contents remain private. Every ID records three root words, width, state/event addresses, candidate table intervals, raw contents and decoded cell coordinates. Each distinct nonnull callback records its source/callback-head window, `StockInventory.inspectFunction` structural extent, separately checked return proof, premises, decoded calls, service witnesses and private-ABI leads. `deriveFunction` retains its strict return-admission behavior. A source window alone is not a recovered extent. The helper's opaque-callee SP/r4-r11-preservation assumption is explicit. [Sources: extractor; `replacement/media_permit/StockInventory.hs`; `replacement/media_permit/ReturnProvenance.hs:302-305`; E-MODAL-CONTROLLER-MANIFEST-150.]

| Inventory quantity | Result |
| --- | ---: |
| Controller registrations | 174 |
| Candidate physical cell words | 50,534 |
| Physical action slots | 6,143 |
| Physical transition slots | 348 |
| Unique nonnull action targets | 5,795 |
| Unique nonnull transition targets in these stock table bytes | 0 |
| Structurally inspected callback extents | 5,795 |
| Conditional successful return derivations | 4,844 |
| Structural extents with unresolved return provenance | 951 |
| Sum of callback code-address spans | 710,152 bytes |
| Actually classified callback instruction words | 709,844 bytes |
| Classified literal words | 330 words / 1,320 bytes |

All348 transition words are zero in the pinned stock image; this does not close later writers or exclude a future nonnull transition publication. All5,795 targets have structurally complete instruction/literal classification. The951 return refusals remain ABI/integrity obligations, not failed extent recovery. The232 callbacks missing Ghidra function headers derive from table-entry and neighboring-head evidence; all232 have no direct call, indirect call or external branch in their structural instruction sets, which is not a purity or memory-effect claim. The final callback's explicit `0xcd0c78` code/data fence is listing-bound and independently reviewed. The byte totals exclude dispatcher, transitive retained services, initialization and resources. [Source: E-MODAL-CONTROLLER-MANIFEST-150; private manifest `counts`, `functions[].extent`, `function_fence_evidence`; independent review record.]

Reproduce with owner-supplied inputs and a fresh output path:

```sh
devenv shell -- bazel --batch --output_base="$HOME/.cache/bazel/latent-image-ControllerManifest" build --jobs=2 //replacement/modal_runtime:controller-manifest
bazel-bin/replacement/modal_runtime/controller-manifest \
  --id5 private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin \
  --decompilation private-data/analysis/x-m1-1.50/filmsim/ghidra/all.c \
  --reachability private-data/scratch/ControllerManifest/reachability.json \
  --out private-data/scratch/ControllerManifest/manifest.json
```

The previous 49,738 cells describe the closure checker's captured state/event expansion, not physical table length or distinct functions. Modal75's captured 2,208 cells, 294 action slots and one transition slot similarly differ from its 299-entry physical action table. Root adjacency gives candidate storage fences, not valid index domains or lifetime immutability. Bank publication deliberately changes cell and action words. Unresolved return proofs, unclassified service frontiers and source-window instruction candidates stay unresolved. [Sources: retained modal contract, scoped diagnostic and published storage sections above; `Replacement/ModalRuntime/Closure.hs:1761-1816`; E-MODAL75-SCOPED-DIAGNOSTIC-150.]

### Existing trajectory coverage

The receipt-backed dispatched-ID union is `{0,30,74,75,100,107}`. The retained FILMSIM trace records134 contexts across those IDs, including25 controller75 contexts at state0/events2,8,13,50. The packaged modal224 interval records254 completed contexts,40 owned75 and214 fallback, on core0 with the same75 state/event set. The manifest preserves each trajectory's exact paired state/event domains rather than taking a Cartesian product. These are selected navigation intervals, not a continuous reset-to-menu witness. [Sources: E-MODAL-RETAINED-FILM-ROUTE-150, `retained-film-route/segment-retained-film-route/e2e-result.json` trace hits; E-MODAL224-PACKAGED-FILMSIM-150, `packaged224-owned-corrected/e2e-modal-observation.json` captured domains; private manifest `reachability`.]

The recipe-bank lifecycle observes owned action entries and controller75 RAM snapshots in states0/30, not a full dispatcher-ID/context trace. Direct native editor composition establishes action semantics with modeled services, not natural reachability. Cold first-start records language/date/live-view/shooting endpoints but no complete dispatched-controller set. Therefore168 IDs have no dispatched-ID witness in these receipts; that is missing coverage, not evidence of unreachability. Controller ID30 and controller75 state30 are different coordinates. [Sources: E-MODAL75-PACKAGED-DEVICE-150, `private-data/scratch/modal133/fetch-positive-corrected/e2e-result.json`; E-MODAL75-EDITOR-NATIVE-150; E-APPLICATION-ENTRY-LOOP-SOURCE-150; `private-data/scratch/ControllerManifest/reachability.json`.]

### Retained service categories

Categories overlap. Direct decompiled call expressions and source-window encoded call candidates are recorded separately. Source paths to named services are static evidence, not execution coverage. Unknown callees, indirect calls, memory effects and device descendants remain obligations; a seed names a proposed retained seam, not a closed implementation. In particular transition service `0xc12aac` writes controller100's event cell and reenters `0xbfb028`. [Sources: extractor; `menu/recipe_bank_arm.S:225-275`; `private-data/analysis/x-m1-1.50/filmsim/id5.dis:33797-33834`.]

| Category | Grounded seams | Ownership boundary |
| --- | --- | --- |
| Settings/property gates | Query `0x63a0f8(0x53)`, predicates `0x6394d0/0x6394e8`, bank-declared property read/write/availability/conversion/commit entries | Own branch selection and typed decisions; retain explicit property/mode operations. A commit name does not establish durable output. |
| Frame/editor/buffers | Snapshot11, saved reader `0x61ce1c`, editor `0xd4ad38`, prepare/open `0xd45ce4/0xd45cf8`, copy/clear `0x722a50/0x722178/0x7221c8` | Absorb snapshot early; retain editor engine, frame/value/slot lifetime and buffer primitives until migrated. Read the new frame after transitions. |
| Rendering | Notification `0xd38d98`, redraw `0xd38e1c`, row probe `0xd52684`, text/draw bindings, UILIB/VGLIB | Own controller intent and required ordering; compare acknowledged full-plane results over an explicit renderer/resource/device seam. |
| Persistence | File operations `0x71c648/0x71c6fc/0x71c75c/0x71c7bc`, media worker continuations, settings append `0x6ad3f4` | Separate property changes, RAM bank state and durable effects; preserve file/handle/callback lifetime and errors. |
| Capture/playback/device | Playback mode apply `0x6439ac`, REC actor `0xe00a80`, playback actor `0x1215480` | Keep accepted branches in the inventory; full scenarios depend on their independently grounded device/output laws. |
| Input | Menu left/right `0xc6f14c/0xc70528`, queue1 adapter `0xdb5124` | Normalize typed events; distinguish receive-backed input, pending internal work and unhandled-input handback. |
| Controller/runtime support | Dispatcher, prehook, stage, transition, postlude; queue/task/heap/division | Migrate decision callbacks together while retaining explicit RTOS, allocation, exception and service-reentry interfaces. |

Address meanings are grounded in `menu/recipe_bank_arm.S:225-275`, `menu/recipe_bank_allowlist.json:47-154,219-223`, the lifecycle/snapshot sections above, [file-service recovery](named-recipe-reach.md), [heap survey](stock-heap-survey.md) and [Inventory §8.18](replacement-inventory-baseline.md#818-application-entry-main-ui-actor-and-first-owned-loop-boundary-154). No category is inferred from address proximity.

| Static category witness | Targets calling a seed directly in decompiled source | Targets with a source path to a seed, or themselves a seed |
| --- | ---: | ---: |
| Settings/property gates | 1,397 | 4,525 |
| Frame/editor/buffers | 423 | 3,695 |
| Rendering | 668 | 1,472 |
| Persistence | 0 | 152 |
| Capture/playback/device | 2 | 2 |
| Input | 0 | 2 |
| Controller support | 1,612 | 4,448 |
| Runtime support | 0 | 1,980 |

These counts overlap and do not measure scenario coverage or complete semantic closure. There are 5,338 targets with an unclassified frontier and 380 whose only category is unclassified. The latter include the 232 structurally call-free callbacks missing Ghidra heads. Every target has a record; missing category evidence stays explicit rather than assigning an address-range meaning. Zero direct seed matches does not mean no persistence, input or runtime dependence. [Source: E-MODAL-CONTROLLER-MANIFEST-150; manifest `functions[].direct_service_categories`, `service_categories`, `unclassified_frontier`, `source` and decoded transfers.]

### Private callback ABI blockers

At action entry, stock supplies r4=cell offset, r5=ID, r6=cell table, r7=event, r8=width, r9=action table, r10=cell address, r11=`S-4`, LR=`0xbfb0cc`, SP=`S-48`. Transition changes r4 to the action result and LR to `0xbfb0fc`, and carries action residues in r0/r1/r12. `[S-44]` holds the transition root; `[S-48]` and `[S-40]` are unwritten, not guaranteed zero. Owned dispatch instead uses r11=`S-8`, different register roles/scratch and glue LR/r12. These are actual compatibility obligations wherever retained consumers observe them. [Sources: dispatcher register and frame contract above; E-MODAL-DISPATCHER-ABI-150; E-MODAL-DISPATCHER-LINKED-ABI-150.]

Selected SET/confirm/leave/cancel prologues save incoming r11/LR and establish local r11. Confirm and leave also save/restore entry r3, creating a concrete outgoing volatile-r3 dependency. No incoming-LR-as-data consumer or dispatcher-scratch read is positively established by these excerpts; that is not absence across all callbacks/descendants. Manifest `unaff_r11`, `in_lr` and stack-input tokens are leads only. Decoded literal-pool words are not ABI evidence. [Sources: `private-data/analysis/x-m1-1.50/filmsim/id5.dis:132318-132321,135439-135463,135918-135924,136174-136177,137023-137046,33797-33834`; open premises/caller-continuation sections above.]

**[INFERENCE] Cutover rule:** AAPCS is insufficient while callbacks, saved frames, LR, volatile residues or scratch are externally observable. Internal identities/layouts cease to be comparison fields only after every consumer migrates inside the unit or an actual retained contract excludes those observations. Keep conservative caller/storage/alias/writer obligations; do not accumulate174 routing exemptions.

### Snapshot service folded into the project

Absorb classifier `[0x61ca2c,0x61cd14)`744 bytes, snapshot `[0x61cd14,0x61ce1c)`264, store `[0x61ceac,0x61cf14)`104 and eight80-byte accessors as one11-entry component totaling1,752 original bytes. Compare11 semantic words, result, slot choice and permitted writes, including null destination, detached defaults, zero-row division and invalid values. Preserve live-frame read order. Saved reader, three slot tables, frame/value lifetime and loader divide/SVC remain named retained dependencies. The307 native pairs and bank-store migration are regressions, not global retirement of3,027 stock call sites. [Source: owned menu-frame snapshot section; E-MENU-FRAME-SNAPSHOT-NATIVE-150.]

### One typed interface and internal milestone order

**[INFERENCE] Interface:** one dispatch operation over validated `ControllerId`, controller-specific typed `Event`, logical `ControllerState` and action-specific `ActionResult`. Unknown physical input becomes explicit unhandled input, not an unchecked table index. Keep action/transition/frame/node/row/property IDs and retained statuses typed at adapters. Own selection, post-action state decisions, registration changes and reentry internally. Do not invent a universal error enum: stock returns the action result and discards the transition result. Preserve captured inputs and post-action transition evaluation until every mutation consumer migrates. [Grounding: E-MODAL-DISPATCHER-ABI-150; `replacement/owned_menu/src/lifecycle.rs`; spec §Replacement contract.]

1. **[INFERENCE] Manifest/boundary closure:** resolve table/code fences, source/byte correspondence, index and result domains, service frontiers and actual ABI consumers. Bind static obligations and scenario coverage to the same artifact.
2. **[INFERENCE] Integrate existing pieces:** snapshot11, classifiers, row binding, dispatcher and SET/confirm/leave/cancel become components of one artifact with one property/frame/editor/render/runtime/device service layer. Publish/retire all11 snapshot entries together.
3. **[INFERENCE] Connected menus:** own the decision closure of observed IDs `{0,30,74,75,100,107}`, not only four modal75 cells. Reuse FILMSIM entry/BACK/reentry, nested dispatch, unhandled input, handback and completed rendering.
4. **[INFERENCE] Editor lifecycle:** qualify bank cancel/confirm/leave, ordinary-setting preservation and reentry over explicit editor/buffer/frame services. Keep native generic state30 cancellation separate until a natural route is identified; wireless NAME is controller76/state36.
5. **[INFERENCE] Complete trajectories:** cold first-start/shooting readiness, Q/SETUP and every accepted controller trajectory. Prioritize property/frame/render groups before persistence and capture/playback/device groups without deleting the latter from scope.
6. **[INFERENCE] Global cutover:** migrate every caller/registration/callback to the typed interface, remove stock+4 routing, and qualify continuous retired-code/data exclusion, initialization/publication and the full accepted matrix.

These are milestones inside one174-controller project, not new per-leaf acceptance units. [Grounding: existing FILMSIM/lifecycle/snapshot evidence; Inventory §§8.1-8.5,8.18; #117/#127; spec §§Ownership stages, Automatic verification.]

### Scenario matrix and proposed runs

Every row is **[INFERENCE] proposed acceptance**, not new execution. Extend reviewed trajectories, journals, typed receipts, render comparison and actual-content exclusion; independently review changed mechanisms before use. Each corrected input runs once, with failures preserved.

| Scenario | Reusable evidence | Required observation or gap |
| --- | --- | --- |
| Cold first-start language/date, live view, shooting menu | E-APPLICATION-ENTRY-LOOP-SOURCE-150 | Cold controller trace, executable publication, initialized state/event ownership, actual task1/pump readiness and acknowledged frames; screen endpoints are not controller IDs. |
| Shooting menu, FILMSIM, BACK, repeated reentry | E-MODAL-RETAINED-FILM-ROUTE-150; E-MODAL224-PACKAGED-FILMSIM-150; E-MODAL224-RENDER-DIFFERENTIAL-150 | All connected decisions/results/effects, nested dispatch, handback and next input; extend the selected interval and declare remaining alias/task/lifetime limits. |
| Q and SETUP | Accepted menu capability inventory | Physical navigation/BACK/reentry, gates, property preservation and render completion; collect real controller/state/event domains. |
| Bank Save Current, Rename, A-cancel, B-confirm, leave/reentry | E-MODAL75-PACKAGED-DEVICE-150; E-MODAL75-RENDER-DIFFERENTIAL-150 | Name/buffer/cursor/frame outcomes, retained-service ordering, transient-setting preservation, render output and rollback; RAM success is not durable file persistence. |
| Generic75 state30 cancel/confirm | E-MODAL75-EDITOR-NATIVE-150; E-MODAL-NATIVE-LIFECYCLE-150 | Identify a natural producer, then dispatcher context, effects, post-action transition and repeated handback. Injected state is not reachability. |
| Unhandled/no-op input, pending work, reentry | Native dispatcher boundaries; Inventory §8.18 | Results/public state, no unintended effects, balanced lifetime, task1 handback followed by next input; pending work need not receive a packet. |
| Every accepted device/persistence controller trajectory | Capability inventory and separately admitted device/storage records | Typed errors, completion/cancel/order and final artifacts; unresolved capture/JPEG/media/device laws block broader acceptance, not static inclusion. |
| Wrong artifact/target/context/lifetime, forbidden old execution/data | Existing dispatcher/seam/editor/preservation/render controls | Reject plausible wrong results/state/ordering, caller writes, stale publication, transient setting changes and actual restored-old-body execution. |

**[INFERENCE] First proposed cohort:** retained cold-reset/menu/Q/SETUP coverage capture, then owned connected-menu comparison on identical admitted inputs, expanding the same matrix as editor/service contracts close. No guest was needed for this manifest task. No camera, SD or persistent operation is part of this deliverable. [Grounding: spec §§Repeatable replacement loop, Independent evidence; existing route limits above.]

### Blockers and proposed issue text

Immediate blockers are table/index/mutation lifetime closure; the 951 unresolved callback return proofs; unknown service/computed-call frontiers; actual private-ABI consumers; state/event initialization and nested/asynchronous writers; callers/physical aliases; cold/Q/SETUP/generic-editor reachability; task/queue/pending-work and acknowledged rendering; and independently incomplete storage/capture/playback/device contracts. The 174-ID census is an obligation inventory, not 174 accepted trajectories. [Sources: E-MODAL-CONTROLLER-MANIFEST-150; open premises, scoped diagnostic, published storage, lifecycle and snapshot sections above; Inventory §§8.2,8.18; E-MODAL-CALLER-CONTINUATIONS-150.]

**Proposed #117/#127 project text:** Own modal dispatch and all174 controllers' decision/state code behind one typed event/controller/action interface, absorbing frame snapshots and existing menu/editor implementations. Qualify at retained property, frame/editor/buffer, rendering, input, runtime, storage and device interfaces and through cold first-start/shooting, Q/SETUP, accepted controller trajectories, entry/BACK/reentry, unhandled input and editor cancel/confirm scenarios. Start from the manifest and FILMSIM connected set; keep unobserved controllers and device-dependent scenarios explicit. Completion removes scoped stock fallback and proves caller/callback/data/init/lifetime closure plus continuous original-body/data exclusion within the full accepted scenario/artifact scope.

### Replacement strategy at callback scale

The project is not 5,795 handwritten callback ports. This section measures what the callback population is, how much of it existing trajectories reach, and what each replacement strategy would have to do with it. Shape clusters are syntactic measurements, not behavior equivalence. [Sources: E-MODAL-CONTROLLER-SCALE-150; E-MODAL-CONTROLLER-MANIFEST-150.]

#### Callback structure

[`//replacement/modal_runtime:controller-scale`](../replacement/modal_runtime/ControllerScaleMain.hs) reinspects every manifest callback with `StockInventory.inspectFunction` over the pinned image5 bytes, hashes three normal forms, projects the recorded trajectory domains and scans constant-address controller-cell accesses. It refuses a different manifest or image5 digest. Private outputs, each from one reviewed run, are `private-data/scratch/ControllerManifest/scale.json` (clusters only), `scale-cells.json` (first cell scan, immediate offsets only, superseded) and `scale-cells-regoffset.json` (also resolves constant register offsets, which controllers 128 and above need; authoritative). All three have byte-identical `cluster_summary`. Reproduce with owner-supplied inputs and a fresh output path:

```sh
devenv shell -- bazel --batch --output_base="$HOME/.cache/bazel/latent-image-ControllerManifest" build --jobs=2 //replacement/modal_runtime:controller-scale
bazel-bin/replacement/modal_runtime/controller-scale \
  --manifest private-data/scratch/ControllerManifest/manifest.json \
  --id5 private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin \
  --out <fresh private path>
```

| Code span | Callbacks | Bytes |
| --- | ---: | ---: |
| 16 to 32 bytes | 1,295 | 30,024 |
| 33 to 64 | 1,595 | 77,332 |
| 65 to 128 | 1,497 | 139,520 |
| 129 to 256 | 812 | 142,780 |
| 257 to 512 | 434 | 154,416 |
| 513 to 1,024 | 122 | 80,816 |
| 1,025 to 4,096 | 34 | 46,436 |
| over 4,096 | 6 | 38,828 |

The median callback is 68 bytes (p25 36, p75 128, p90 264, p95 380, p99 808, maximum 12,244). Counts and bytes diverge: the 4,387 callbacks of at most 128 bytes hold 34.8% of the 710,152 code bytes, while the 162 callbacks over 512 bytes hold 23.4% and the 100 largest hold 18.5%. The largest are controller64's `0xc4b588` (12,244) and `0xc486ac` (8,220), controller75's SET delegate `0xc72e10` (5,272) and key-right `0xc70528` (4,536), and controller76's `0xc7e2bc` and `0xc7c338` (4,316 and 4,240). Every callback is registered under exactly one controller: per-controller unique action counts sum to 5,795. Identical bodies occur as separate copies, for example 109 return-`-1` bodies across 103 controllers.

| Normal form | Shapes | Singleton shapes | Callbacks in repeated shapes | Largest shape | Callbacks in top 10 / top 100 shapes |
| --- | ---: | ---: | ---: | ---: | ---: |
| Exact stored words | 5,612 | 5,603 | 192 | 109 | 193 / 283 |
| Helper-bound: immediates, MOVW/MOVT halves, literal values and offset magnitudes parameterized; registers, conditions, operations, internal branch offsets and helper identities exact | 4,058 | 3,471 | 2,324 (40.1%; 176,832 bytes, 24.9%) | 109 | 549 / 1,191 |
| Helper-erased: also parameterizes external call identities, an upper bound | 2,588 | 1,994 | 3,801 (65.6%) | 226 | 1,235 / 2,479 |

Under the helper-bound form, 59.9% of callbacks and 75.1% of bytes are singleton shapes. The representative repeated shapes below are small bodies in one compiler style: an r11 frame prologue, MOVW/MOVT absolute globals, conditional execution and direct BL calls. Listing lines are in `private-data/analysis/x-m1-1.50/filmsim/id5.dis`:

| Members | Shape | Listing |
| ---: | --- | --- |
| 109 (103 controllers) | return `-1` | 227985-227990 |
| 92 (63 controllers) | return a constant | 227979-227984 |
| 100 (25 controllers) | query `0xd38870(0)`, then two `0xd3b198(4,0x130,…)` input-record appends, return 1 or 0 | 227708-227725 |
| 48 | `0xd3a2b8(0)` release-state producer, then router `0xc0dbf8` | 224182-224187 |
| 45 | warning selection `0xd3b64c(0)`, compare, conditional controller-state store | 206745-206752 |
| 226 helper-erased (196 different helpers) | one call, then return a constant | 227254-227258 |
| 158 helper-erased (33 helper-bound, all `0xc0dbf8`) | tail call returning the helper's result | 228131-228134 |

All 5,795 structural instruction sets decode within the closed `host/firmware/A32.hs` decoder; none has an indirect-call or external-branch candidate, so every outgoing transfer is a direct BL. They call 1,926 distinct helpers: 613 are called by one callback and 59 by at least 50; 5,563 callbacks call at least one, and the 20 most shared helpers are called by 3,855. The two most shared are routers. `0xc0dbf8` (1,362 callbacks) and transition service `0xc12aac` (738) read the application mailbox code `u32[0x0094f64c]`, write an event into a fixed controller's event cell and reenter dispatcher `0xbfb028` (id5.dis:28760-28790, 33797-33834); 2,093 callbacks call at least one of them. Next come notification `0xd38d98` (667), postlude `0xd38f34` (665), stage open `0xd3d858` (437) and property read `0x0061c93c` (429). [Sources: E-MODAL-CONTROLLER-SCALE-150 `sizes`, `cluster_summary`, `helper_bound_clusters`, `shared_helpers`, `call_patterns`; manifest `functions[].indirect_call_candidates`, `external_branch_candidates`; `reports/cardless-capture-feasibility.md:248,260` mailbox and UI actor; `reports/af-observation-plan.md:77,153` for `0xd3b64c` and `0xd3b198`; `reports/autofocus-static-map.md:493` for `0xd3a2b8`; `menu/recipe_bank_arm.S:225-275` for the named services.]

#### Next-state encoding and table-driven variants

All 348 transition words are zero, yet 2,609 callbacks (45.0%) contain 4,545 halfword stores to a constant controller state cell. 4,454 store a known constant, 52 distinct values occur, and 1,154 sites are conditional. Every recorded store targets the callback's own controller; 124 controllers' state cells are written this way. No callback stores to an event cell or loads a cell through a constant address. Event writes and cross-controller dispatch go through helpers such as the two routers. Next-state logic therefore lives in callback code as `state[own] := constant`, not in the transition tables. Among repeated helper-bound shapes, 245 (746 callbacks) contain such stores, and 143 have members whose ordered store signatures differ: the same code shape with different next-state constants or store counts. That is the measured table-driven-variant population. The scan resolves only straight-line constant addresses, so absence of a record is not absence of a write. [Sources: E-MODAL-CONTROLLER-SCALE-150 `scale-cells-regoffset.json` `controller_cell_summary`, `callback_index[].controller_cell_accesses`; manifest `controllers[].state_address`.]

The same stores appear in almost all return-proof refusals: 950 of the 951 unresolved derivations list `global-object-stack-separation` at one global store, 932 of them at a recorded state-cell store. That premise was already assumed by the derivation, so it was not the cause. The refusals came from joining a predicated `MOVW`/`MOVT`/store run's not-executed edge with its executed path, which lost the constant store base. See [Controller75 pilot prerequisites](#controller75-pilot-prerequisites). [Sources: manifest `functions[].extent.return_proof.reason` joined to `scale-cells-regoffset.json` store sites; E-MODAL-CONTROLLER-SCALE-150.]

#### Natural coverage and accepted capability demand

| Recorded domain | Paired coordinates | Contexts | Selected pinned-stock action identities | Denominator |
| --- | ---: | ---: | ---: | --- |
| Retained FILMSIM navigation | 29 | 134 | 25 | 5,795 globally, 1,179 registered under its six witnessed IDs |
| Packaged repeated FILMSIM navigation | 41 | 254 | 36 | Same denominators |
| Union | 41 | Separate observations, not additive coverage | 36 | 36/5,795; the retained set is a subset |

The analyzer's static projection reproduces 25 and 36 from the manifest's paired domains. The retained receipt independently records 25 distinct r3 targets at the dispatcher action-call site `0xbfb0c8`, agreeing with the projection. That is selected-target observation, not a callback-body-entry instrument. The packaged 36-target result projects paired entry domains through pinned stock tables: publication and interposition change implementations, so it is not evidence of 36 executed stock callbacks or 36 owned bodies. The bank lifecycle separately records three owned body identities, SET once, confirm twice and leave once. [Sources: E-MODAL-CONTROLLER-SCALE-150 `trajectory_projection`; E-MODAL-RETAINED-FILM-ROUTE-150, `private-data/scratch/modal128/retained-film-route/segment-retained-film-route/e2e-result.json` action trace r3; E-MODAL224-PACKAGED-FILMSIM-150, captured domains; E-MODAL75-PACKAGED-DEVICE-150, labeled owned-entry hits; `private-data/scratch/ControllerManifest/scale-coverage.json`.]

The witnessed set gives almost no template leverage. Its 36 targets fall into 35 helper-bound shapes with 146 members in total, and 109 of those are the shared return-`-1` shape; the other shapes have at most three members. Under the helper-erased upper bound, 635 callbacks share a shape with a witnessed target. Eight of the 36 store controller state and two have unresolved return proofs. Retiring the observed route by template would therefore retire roughly the observed route.

| ID | Unique nonnull registered action targets | Retained-domain selected targets | Packaged-domain selected targets | Evidenced visible area or role |
| --- | ---: | ---: | ---: | --- |
| 0 | 104 | 2 | 2 | Participates in this menu route; no exclusive visible-area identity recovered |
| 30 | 203 | 8 | 15 | Participates; not controller75 state30 |
| 74 | 499 | 6 | 10 | Participates; no exclusive visible-area identity recovered |
| 75 | 297 | 4 | 4 | Shooting-menu/FILMSIM navigation; separately bank list/editor states 0 and 30 |
| 100 | 61 | 4 | 4 | Nested transition dispatch destination, not an exclusive menu area |
| 107 | 15 | 1 | 1 | Participates; no exclusive visible-area identity recovered |
| 76 | Outside the counted route | Not witnessed | Not witnessed | Source-backed Wireless Settings → General Settings → NAME, state36 |

Controller75 participates in a menu family, not only FILMSIM. The source-backed controller76 NAME route is not observed generic75 cancellation. Other controller/menu-area mappings remain unknown; first-start `(69,52,12)`, date `(67,56,0)` and shooting `(1,5,2)` are menu tuples, not controller IDs. [Sources: existing trajectory and reachable stock modal75 sections above; `ghidra/all.c:341168-341273` event75 producer; `id5.dis:33797-33834` nested controller100 dispatch; manifest controller tables; E-APPLICATION-ENTRY-LOOP-SOURCE-150.]

The accepted capability inventory requires controls/menus, startup/shutdown, live view, AF/manual focus, exposure/capture, development, save/playback, persistent settings, calibration consumption, external interfaces and failure recovery. These traces establish a bounded menu/navigation/editor slice and some handback/render outcomes, not the controller sets required by every accepted area. FILMSIM selection is not development execution; bank SAVE CURRENT is not image save or durable persistence; RAM-push/rollback is not ownership of firmware external interfaces or general recovery. Cold/Q/SETUP, other modes, shutdown, capture/development/playback/storage and failures need their own observed domains and effects. No accepted-capability callback count exists, so neither 36 nor 1,179 is a port budget. [Sources: Inventory §§2,8.3; E-MODAL-CONTROLLER-MANIFEST-150; packaged lifecycle and render scopes above.]

#### Strategy comparison

Strategy and effort judgments are **[INFERENCE]** from the measurements above. Effort is stated as the units of work each strategy multiplies, not person-time.

| Measurement | Value | Bears on |
| --- | ---: | --- |
| Callbacks / code bytes | 5,795 / 710,152 | Total retained, translated or re-specified surface |
| Helper-bound shapes / singletons | 4,058 / 3,471 | Template retirement (a) and per-template qualification (b) |
| Helper-erased shapes | 2,588 | Template count once helpers become typed parameters |
| Distinct helpers / single-use | 1,926 / 613 | Service binding surface for every strategy |
| Callbacks storing constant next states | 2,609 | Transitions recoverable as data (c) |
| Callbacks calling dispatch routers | 2,093 | Reentrant dispatch is required by every strategy |
| Unresolved return proofs / citing a state-cell store | 951 / 932 | Callback ABI admission for retained or translated bodies |
| Native cases needed at 256 declared functions each | at least 23 | Differential partitioning for (b) |

| Strategy | Work it multiplies | How qualification scales | Principal risk |
| --- | --- | --- | --- |
| **(a) Owned dispatcher and typed interface; retain stock callbacks, retire clusters by template** | One dispatcher, interface and stock-compatible callback envelope, then one owned implementation per retired shape. Helper-bound templates reach at most 40.1% of callbacks and 24.9% of bytes; 3,471 singleton shapes remain one-offs. | Subsystem matrix once; envelope once; per retired template a contract and per-member binding checks. Retained callbacks are not retired and keep the ABI obligations, including 951 return refusals. | A long mixed period presented as ownership. The envelope must reproduce the stock r4-r11/r11/LR frame for every retained callback, and the routers reenter dispatch from 2,093 callbacks. |
| **(b) Mechanical A32 translation to Rust, qualified per cluster** | Translation rules over the closed decoder's admitted forms (every callback decodes; transfers are direct BL), plus 1,926 helper bindings. Effort grows with rules and bindings, not with callback count. Output keeps absolute globals (`0x00944580` block, state cells) and helper identities. | Rules qualified once with wrong-behavior controls; every member then needs a differential against its original body, at least 23 native cases at the 256-function limit, with helper effects stubbed. Register-only stubs cannot represent helper memory effects. | Ownership of bytes, not of design: translated code stays coupled to stock memory layout and services. A rule bug replicates across members. |
| **(c) Re-specify from tables and strings; generic owned controllers** | Menu content is already data: image4 nodes `{id, type, property, kind, …, parent, values, labels}`, `{row, child}` values arrays, string-id label quadruples, conversion tables and availability matrices; the stock SET delegate `0xc72e10` is a generic leaf handler with frame-class special cases. Next states are constants in 2,609 callbacks and mailbox-to-event routing is switch-coded in routers. Work is extracting those tables and writing a small set of generic primitives. | Generic primitives qualified once over their parameter domains; every extracted instance validated mechanically against stock bytes; behavior compared against stock execution of the same data in the system matrix. | Tables do not hold side effects: 4,525 targets have a property-gate path, 1,472 a render path, and 380 only an unclassified category. No complete menu-node census exists. 168 controllers have no observed domain. |

Strategy (a) is an integration architecture, not permission to use the scoped modal75 adapter globally; `src/dispatch.S` and source-level argument equality do not reproduce the stock frame. Strategy (b) must preserve 32-bit wrapping, signed halfword behavior, predication/NZCV, instruction-visible PC, access widths, volatile/shared read order and call-clobber rules, and must keep the 951 return refusals as exceptions until their premise is proven. Strategy (c) must preserve the lifecycle's repeated queries, post-transition reads, buffer mutations and result-dependent notifications, or specify an accepted difference. No production A32-to-Rust/C translator exists in the repository: the matching pipeline compiles already-recovered x70view C and checks bound bytes. New owned logic defaults to Rust. [Sources: dispatcher ABI sections above; `replacement/modal_runtime/src/dispatch.S`; `replacement/owned_menu/src/lifecycle.rs:141-220`; `matching/README.md:3-28`; `emulation/README.md:558-584`; `reports/filmsim-offer-mechanism.md:22-26`; `reports/recipe-design.md:80,460`; `reports/firmware-extraction.md:116`; spec §§Compatibility and redesign, Automatic verification, Independent evidence; `docs/principles.md:17-29`.]

#### Recommended composition

**[INFERENCE]** No single strategy fits the measured population. Use each where the measurements favor it, inside the one subsystem:

1. **(a) as the frame.** One owned reentrant dispatcher and typed `ControllerId`/`Event`/`ControllerState`/`ActionResult` interface, reached by every router and by the stock entry. This is required by every strategy.
2. **(c) for the state-machine and menu layer.** Recover per-controller next-state constants, router mailbox-to-event maps and menu node/values/labels tables into owned typed data, and drive them with generic controllers and one generic menu engine. The zero transition tables, the 2,609 constant next-state callbacks and the existing node data make this the layer where re-specification is cheapest and most maintainable.
3. **(b) for the residual action bodies.** The 3,471 singleton shapes hold 75.1% of the bytes; template retirement cannot reach them and re-specification needs service contracts that do not yet exist. Translate them to Rust through typed service bindings, cluster by cluster, as a bridge to later re-specification. Keep untranslated bodies explicitly stock until their cluster qualifies.

Qualification then grows with generic primitives, translation rules, helper bindings and accepted scenarios rather than with callbacks; every callback still gets a recorded disposition (generic instance, translated member, exceptional owned body or retained stock) with its binding provenance and open premises. Shape equality never reduces per-member binding checks to one representative.

Next evidence, in order: capture cold, Q and SETUP dispatch domains; then pilot all three layers on controller75 (297 targets, with the most existing route, lifecycle and owned-body evidence) before scaling. The stack-separation, router-map and instruction-form prerequisites are recorded below. The existing native-case format holds at most 256 declared functions and 16 regions per case, so differential qualification is partitioned by execution and service closure. [Source: `emulation/README.md:558-584`.]

Mechanical and contract-based implementations are both eligible for publication under provenance and redistribution review. Neither a generated label nor changed source structure clears copied firmware tables, strings, resources or instruction material. Keep raw extraction and shape artifacts private, publish tools and reproducible owner-input derivation, and review embedded data and upstream notices independently of behavioral qualification. [Sources: spec §Publication; `docs/principles.md:64-74`; `third_party/README.md:23-33`.]

**Proposed #128 strategy text:** Replace the 174-controller subsystem as three layers behind one owned reentrant dispatcher: typed state-machine and menu data recovered from stock tables and constant next-state stores, driven by generic owned controllers; residual action bodies translated to Rust through typed service bindings and qualified per cluster; and explicitly retained stock bodies until their cluster qualifies. Start with the controller-cell stack-separation premise, router maps, an instruction-form census and cold/Q/SETUP domains, then pilot controller75. Completion keeps the subsystem acceptance of the project text above.

#### Controller75 pilot prerequisites

Three host-only tools in `replacement/modal_runtime` read the pinned image5 and the cited manifest (`f27b3562…`). Outputs are private under `private-data/scratch/ControllerPilotPrep/`. Each tool had one independent review, and the findings were fixed.

**Return proofs.** `ReturnProvenance` used to send a failed conditional instruction to pc+4 with its pre-state. A `movweq/movteq/strheq` run then joined the constant base with the unconditional path's register, and the store discarded the saved frame. The not-executed edge now continues after the run of consecutive instructions with the identical condition field. Executed edges are unchanged. A second change models `BFI`/`BFC`: `A32.bitFieldInsert` decodes the fields, and constant operands give the inserted constant. Re-derivation with unchanged inputs only adds results:

| Consumer | Before | Predicated runs | Plus `BFI` |
| --- | --- | --- | --- |
| Media-permit stock inventory (21 windows) | All derive | Byte-identical | Byte-identical |
| RTOS manifest generator (v3 inputs) | 148 derived, 46 unknown | 160 / 34 | 160 / 34, identical |
| Controller manifest | 4,841 conditional, 954 unresolved | 5,776 / 19 | 5,795 / 0 |

No previously derived extent changes its premise list. The baseline at the current head has 954 rather than the cited 951 because the saved-stack exclusion already refuses `0xc6e798`, `0xc6ee04` and `0xc7b1f0`. The 19 that the predicated-run change left refused all contain `BFI`, and with the `BFI` model all 954 derive.

**Stack separation.** `//replacement/modal_runtime:controller-stack-separation` recomputes each derivation and resolves every `global-object-stack-separation` store site with a straight-line predicated constant scan. It then checks the stored interval against the recovered stack regions, folding DRAM aliases onto their physical address. The state and event arrays `[0x1467588,0x1467840)` lie in image5 BSS and are disjoint from every region.

There are 5,646 sites, and 5,622 of them are constant and disjoint from every region. 70 of those depend on a constant held in r4-r11 across a call, which is the same AAPCS callee assumption the derivation already makes. The remaining 24 are indexed from `0x00944580` and keep the premise; they include the stores whose index the scan does not compute through `BFI`. Of the cited 951, 935 have the premise discharged at every site, 931 of them without the callee-preservation assumption. The proof itself clears none of the 951, because the derivation was already assuming the premise. It removes the premise from 2,944 derivations.

The discharge rests on one named assumption, `recovered-stack-allocation-completeness`: every stack pointer installed while a controller callback or anything it interrupts runs lies in a recovered stack region. The output records each region with its evidence:

| Recovered stack source | Physical extent | Evidence |
| --- | --- | --- |
| Task reservation `0x725c6c` pool | `[0xa10200,0xa5b200)` | E-KERNEL-PHASE0-RESOURCE-ORDER-SOURCE-150; all 84 hardware-observed task stacks lie here (E-RAM-CALIBRATION-150) |
| Separate-storage reservation `0x723470` pool | `[0x9c4460,0xa0f460)` | E-KERNEL-PHASE0-RESOURCE-ORDER-SOURCE-150 |
| Loader A and B boot stacks | `[0x62c54,0x63460)`, `[0xf2c14,0xf3420)` | Inventory 8.13; E-STARTUP-TASK-RETAINED-STACK-SOURCE-150 |
| Kernel A and B runtime below the idle tops `0xa0000`, `0x130000` | `[0x60000,0xa0000)`, `[0xf0000,0x130000)` | `classic-chrome-patch.md` stacks; `ram-calibration.md` regions |
| TCM exception-mode stacks | `[0x70000000,0x70002000)` | E-PEER-ENTRY-FRAME-HANDOFF-SOURCE-150; loader IRQ top reload |
| Boot ROM stack | `[0xfff00000,0xfff01000)` | `bootrom-window.md` |

Not closed: task-creation callers that pass stack memory not obtained from either reservation, stacks installed by code outside the recovered loader, record-2, scheduler and exception-entry sources, and exception entries that copy SP from a mode whose SP no recovered source set.

**Router maps.** `//replacement/modal_runtime:controller-routes` explores every path through both routers with interval domains over loaded words. Both routers ignore their arguments. `0xc0dbf8` always dispatches controller 80 from 81 sites, and the mailbox `u32[0x94f64c]` selects event values 0..81 (for example, 358 gives event 0 and 359..361 give event 1). `0xc12aac` always dispatches controller 100 from 91 sites, with events 0..92 except 26. Its selectors are the mailbox, `u32[0x944580]`, query results `0x632a3c(0x2380/0x20c0)`, `0x63a0e0`, `0xd35fbc` and `0xd1740c(mailbox)`, plus 28 routes that compare `u32[0x944584]` with halfword fields near `0x94f6de`. Two routes, c0dbf8 event 80 and c12aac event 7, require `0x63a0e0` to return something other than 1, while its decompilation returns 1. The 3,412 B/BL caller sites lie in 2,093 callbacks of 38 controllers, which gives 40 typed controller-to-controller edges. These results depend on two premises: the routers' callees preserve the compared words, and repeated queries with the same constant arguments return the same value.

**Instruction forms.** `//replacement/modal_runtime:controller-forms` classifies the 177,461 code words of the 5,795 callbacks into 56 forms in 33 families. `A32.decode` decodes the operands of 55 forms, used by all callbacks. `BFI` (19 instructions, 19 callbacks) decodes as `Compute`; `A32.bitFieldInsert` supplies its operands to the return derivation. The top 30 forms cover 5,391 callbacks completely, and the top 50 cover 5,785. 28,395 instructions are predicated.

#### Controller75 pilot design

[`//replacement/modal_runtime:controller75-inventory`](../replacement/modal_runtime/Controller75InventoryMain.hs) inventories controller 75 from pinned inputs: the cited manifest (`f27b3562…`) for tables and trajectory domains, the current derivation `controller-manifest-bfi.json` (`2190bc89…`) for extents, return proofs, calls and service witnesses, `scale-cells-regoffset.json`, `routes.json`, `separation.json`, the modal75 closure diagnostic (`b7c71e0c…`), image5 and the decompilation. It sweeps every aligned image5 word with straight-line value-set propagation to resolve dispatcher r0, same-segment event-cell writes and every constant-address access to controller 75's state and event cells, and refuses unless it reproduces every `scale-cells-regoffset` record for 75. It reuses the instruction-form definition and the service-seed closure, now shared modules (`Replacement/ModalRuntime/InstructionForm.hs`, `ServiceCategories.hs`); `controller-forms` and `controller-manifest` outputs stay byte-identical to `forms.json` and `controller-manifest-bfi.json`. One independent review found one blocking and four should-fix items, all fixed before the single cited run. The private output is `private-data/scratch/ControllerPilot75/inventory.json`, SHA-256 `153bfd3c9f960e9b888cfc6991edf157ccc85a09bce8de1f36ff59ec4265d0e8`.

```sh
devenv shell -- bazel --batch --output_base="$HOME/.cache/bazel/latent-image-ControllerPilotPrep" build --jobs=2 //replacement/modal_runtime:controller75-inventory
bazel-bin/replacement/modal_runtime/controller75-inventory \
  --manifest private-data/scratch/ControllerManifest/manifest.json \
  --derivation private-data/scratch/ControllerPilotPrep/controller-manifest-bfi.json \
  --cells private-data/scratch/ControllerManifest/scale-cells-regoffset.json \
  --routes private-data/scratch/ControllerPilotPrep/routes.json \
  --separation private-data/scratch/ControllerPilotPrep/separation.json \
  --closure-diagnostic private-data/scratch/modal128/closure-modal75-diagnostic.json \
  --id5 private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin \
  --decompilation private-data/analysis/x-m1-1.50/filmsim/ghidra/all.c \
  --out <fresh private path>
```

**Tables.** The dispatcher indexes `event * 46 + state`, so the 2,346 cell words are 51 event rows (0..50) over 46 states (0..45).

| Controller 75 quantity | Result |
| --- | ---: |
| Cells with a non-null action | 1,137 |
| No-op cells: null action and null transition slot 1 | 1,209 |
| Cells with high halfword 1 / direct-state cells | 2,342 / 4 |
| Transition words | 2, both zero |
| Action words: null (slots 0, 1) / non-null | 2 / 297 |
| Unique callbacks; callbacks in more than one slot | 297; none |
| Callback code bytes | 52,968 |
| Slots referenced by cells | 298, every slot except 1 |
| Constant state-store sites / callbacks / conditional sites | 252 / 119 / 51 |
| Stored state values | every value 0..45, none unknown |

The action slot count is 297, not 294. The 294 is the closure diagnostic's capture: 48 locally resolved events (all but 24, 28 and 29) by 46 states, 2,208 cells, read from the publication-adjusted candidate. The stock tables reference 296 slots over the same pairs. The candidate publishes bank targets into slots 1, 37 and 66 (`0x00a86144`, `0x00a86544`, `0x00a86048` for stock null, `0xc6f14c` and `0xc70528`), and its published cells no longer reach stock slots 90, 115 and 129.

Four cells carry a direct state: `(13,4)` and `(13,5)` run `0xc6f994` then set state 0, `(0,41)` runs `0xc79a40` then sets 29, and `(14,47)` runs `0xc7a3f0` then sets 0. On those cells the dispatcher's post-action store overrides any store the action made; on every other cell the action's own store stands. Event row 29 has no non-null cell. Outside callbacks, the only constant-address writer of state 75 is the state-array initializer `0xc22bd0` (store of 0 at `0xc22d10`), and the only writer of event 75 is router `0xc0bae8`. No code reads either cell through a constant address.

**Routes in.** Router `0xc0bae8` holds all 49 image5 dispatcher sites for 75 and writes events 0..50 unconditionally before each; two sites select between `{47,48}` and `{49,50}`. Its selectors are `u32[0x944580]` and `u16[0x944588]` (listing read, `ghidra/all.c:341168-341273`), and its other calls `0x6394d0`, `0x63a0e0`, `0xd35fbc` and `0xd36090` leave the frontier `{0x63a0e0, 0xd35fbc, 0xd36090}`. 23 BL sites in 22 controller-74 callbacks call it; no controller-75 callback does. Routers `0xc0dbf8` and `0xc12aac` never dispatch 75. Of the 3,105 dispatcher call sites, three have no straight-line controller: `0xc0e03c` inside `0xc0dbf8` (path-resolved to 80 by `controller-routes`), and `0xc19c90` and `0xc229ac` after heads `0xc19b50` and `0xc22600`, which stay may-dispatch-75 obligations.

**Routes out.** 266 of the 297 callbacks call `0xc12aac`, which dispatches controller 100 with events 0..92 except 26. None calls `0xc0dbf8`, `0xc0bae8`, another event router or the dispatcher directly. `0xc12aac`'s own bindings leave the frontier `{0x63a0e0, 0xd16e2c, 0xd16f00, 0xd16f24, 0xd1734c, 0xd1740c, 0xd35fbc, 0xd35ffc}`.

**Layers.** The rules are mechanical and recorded in the output (`layer_rules`). Generic primitives are the dispatcher, the two `routes.json` routers and event routers: non-callback functions whose every dispatcher site carries one constant controller and an unconditional known event write. (c) is decided first from the closures of each callback's non-generic direct callees, its private-ABI source tokens and its undischarged stack-separation sites. (a) requires calls only to generic primitives, stores only to the stack frame or controller 75's state cell with known values, and loads only from literals, the frame, 75's cells or image data. Everything else is (b). A primitive's own bindings are one router-level obligation, reported per callback but not counted as a caller's reason. Owned Rust bodies follow the same rules.

| Layer | Callbacks | Code bytes |
| --- | ---: | ---: |
| (a) generic instance: constant route through `0xc12aac`, seven with constant state stores (values 0, 22, 29, 33), some selected by the router result | 8 | 548 |
| (a) generic instance: constant or argument result (`0xc79a40`, absent from the decompilation) | 1 | 24 |
| (b) residual translation | 148 | 21,156 |
| (b) of which only seed or primitive calls and no global or computed memory access | 44 | 3,404 |
| (c) retained: unclassified frontier only | 135 | 30,584 |
| (c) retained: unresolved computed reference (undischarged index from `0x944580`) | 4 | 400 |
| (c) retained: persistence service, also frontier (`0xc74e5c`) | 1 | 256 |
| Total | 297 | 52,968 |

No callback reaches a capture/playback/device seed or carries a private-ABI source token. If `0xc12aac`'s binding frontier counted against callers, 8 of the 9 (a) and 139 of the 148 (b) callbacks would move to (c). The (c) frontier has 141 leaves: `0x722054` alone reaches 93 retained callbacks and is the only leaf of 13. Classifying the top 20 leaves clears 27 of the 135 frontier-only callbacks (3,544 bytes). At the binding level, direct helpers `0xd3a654` (38 retained callbacks, one leaf), `0xd3c9d8` (30, 41 leaves), `0x6361b4` (18, one leaf), `0xd20a94` (18), `0xd19cfc` (17) and `0xd3b198` (14) carry most of it. **[INFERENCE]** `0x722054` is a call-free copy loop over halfword and word pointers (`ghidra/all.c:168602`); its category is not established.

The owner has since replaced the frontier reason with typed stock-service bindings, kept nested dispatch to 100 through stock+4, accepted build-time table derivation and put `0xc0bae8` out of scope. Under that rule no callback moves to (a): every frontier-only callback calls a non-primitive helper. The 135 frontier-only callbacks (30,584 bytes) move to (b), giving (a) 9/572, (b) 283/51,740 and (c) 5/656 (E-MODAL75-INVENTORY-DESIGN-150).

The existing trajectories select six callbacks. The four retained and packaged FILMSIM coordinates `(0,2)`, `(0,8)`, `(0,13)`, `(0,50)` select `0xc6eb44` (c), SET `0xc72e10` (c), BACK/leave `0xc76650` (b) and `0xc7a528` (b). The bank editor contract cells `(30,8)` and `(30,13)` select confirm `0xc75ed4` and cancel `0xc77794`, both (b). SET lands in (c) through its callees' frontier although `rb_ui_set` already replaces it in the bank, so the frontier rule is stricter than the bindings the owned lifecycle bodies already use.

**[INFERENCE] Typed interface.** For the pilot only:

| Type | Definition | Source of the domain |
| --- | --- | --- |
| `ControllerId` | Full 32-bit word compared with 75 at the seam; the owned path takes only `Modal75` | `entry.S`, scoped seam qualification |
| `Event75` | `u8` newtype 0..=50 parsed from the event halfword; any other value is `UnhandledInput(u16)` | 51 rows; router `0xc0bae8` writes exactly 0..50 |
| `State75` | `u8` newtype 0..=45; any other halfword is `UnknownState(u16)` | width 46; every constant store is 0..45 |
| `Cell75` | `{ action: Option<Action75>, after: AfterAction }` with `AfterAction::{KeepActionState, SetState(State75)}` | 2,342 high-1 cells over two zero transition words; 4 direct-state cells |
| `Action75` | `Generic(GenericAction)`, `Translated(fn() -> ActionResult)`, `Retained(StockTarget)` per callback disposition | layer inventory |
| `GenericAction` | `RouteToTransition { on_result: [(u32, State75)] }` or `Result(ResultForm)` | the 9 (a) bodies |
| `ActionResult` | `u32` newtype returned unchanged as the dispatcher r0 | stock keeps the action r0; no error enum |

Unknown event or state is not an owned-table index. Stock indexes another row or past the cell table for those, so the owned dispatcher reproduces the stock raw-word lookup for them and reports `UnhandledInput` or `UnknownState` until a recorded trajectory shows such a value. The `[[Cell75; 46]; 51]` table is derived from owner-supplied image5 bytes at build time and checked against the stock words, not checked in. Bank publications into slots 1, 37 and 66 become `Action75` entries instead of word publications.

**[INFERENCE] Dispatcher and seam.** `entry.S` stays as it is: full-word 75 goes to `rb_modal_dispatch`, every other ID to stock+4 with the replayed push. What changes is the 75 path inside `rb_modal_dispatch`. It still reads the shared event and state halfwords, because stock router `0xc0bae8` (from controller 74) and the initializer `0xc22bd0` write them, and still calls the retained prehook `0xd0cc68` first. It then parses `Event75`/`State75`, looks up the owned `Cell75` instead of the stock cell, action and transition pointers, runs the action by disposition, and applies `AfterAction` with the stock post-action ordering. `Generic` and `Translated` actions run in Rust. `Retained` actions keep `rb_modal_call` from `dispatch.S` and inherit every open private-ABI obligation of the dispatcher frame contract above. Nested dispatch from `0xc12aac` reenters the seam and reaches controller 100 through stock+4. In the pilot `0xc0bae8` and `0xc12aac` stay stock; their recovered route maps become typed expectations for the comparison, not owned code. Owned calls to `0xc12aac` rest on the named premise `transition-router-register-inputs` (E-MODAL75-TRANSITION-PREMISE-150). The owned path also reads the cell word and transition entry before the action, which stock does not; the modal guard watches writes and fetches only.

**[INFERENCE] Translation rules for (b).** The 148 residual bodies use 28 forms, all with `A32.decode` operand semantics except one `BFI`. 1,010 of their instructions are predicated, 462 of them branches. Each rule needs a wrong-behavior control before any member is accepted:

1. `bl` (1,010): call through a typed binding; r0-r3 arguments, r0 result; r0-r3, r12, LR and flags clobbered; r4-r11 preserved.
2. `mov imm`, `mvn imm`, `movw`, `movt`, `ldr imm offset base=pc`: constant materialization. An absolute address pair becomes a named typed global: `State75` for the state cell, a typed `0x944580` block for the mailbox words, a named retained global otherwise.
3. `cmp imm`, `cmn imm`, `cmp reg`, `sub.s imm`: exact 32-bit NZCV; conditions read NZCV, never a source-level signed-compare shortcut.
4. Predication on any form: `if cond(nzcv) { op }`, with flags changed only by flag-setting forms.
5. `b`: structured control flow within the callback extent; every target is internal.
6. `stmdb! base=sp +lr`, `ldmia! base=sp +pc`, `add imm rn=sp`, `sub imm rd=sp`, `sub imm rn=sp rd=sp`, `str imm offset base=sp`: frame and locals become Rust locals; r11 is unobservable only while no private-ABI consumer exists.
7. `mov reg`, `add imm`, `sub imm`: wrapping `u32` arithmetic.
8. `strh imm offset` (93): state-cell stores become `State75` writes; other halfword stores truncate to 16 bits.
9. `str reg offset`, `ldr reg offset`, `ldrb reg offset`, `ldr imm offset`, `ldrb imm offset`, `strb reg offset`: volatile accesses of the exact width, in program order relative to calls.
10. `bfi`: `A32.bitFieldInsert` semantics.

**[INFERENCE] Service bindings for (b).** 37 distinct direct callees. Seeds: transition router `0xc12aac` (139 callbacks), property query `0x63a0f8` (118), stage `0xd3d858` (104), snapshot accessors `0x61d1f4` (50), `0x61d244` (25) and `0x61d154` (9), notification `0xd38d98` (44), postlude `0xd38f34` (12), property access `0x61c93c` (9), gates `0x6394e8` (9), `0x63270c` (8), `0x6394d0` (4) and `0x632a3c` (3), editor `0xd45ce4`/`0xd45cf8` (4 each) and `0xd4ad38` (2), buffer copy `0x722a50` (1). Non-seed helpers whose closures are fully classified: `0xd3d81c` (10), `0xd48ab4` (8), `0xd3d78c` (6), `0xd1150c`, `0xd1f228`, `0xd21808`, `0xd48a34`, `0xd493f4`, `0xd4e1b4` (2 each) and nine single-use settings helpers. The snapshot accessors bind to the owned #132 frame module; the rest bind to retained stock through typed wrappers.

**[INFERENCE] Qualification plan.** Comparisons per dispatcher entry: `(controller, state, event)`, action result r0, post-dispatch state halfword, ordered binding calls with arguments, nested controller-100 contexts, protected-table writes and acknowledged render captures.

| Trajectory | Controller 75 selection | What it qualifies |
| --- | --- | --- |
| Retained FILMSIM route, E-MODAL-RETAINED-FILM-ROUTE-150 | state 0, events 2, 8, 13, 50: `0xc6eb44` (c), `0xc72e10` (c), `0xc76650` (b), `0xc7a528` (b) | Stock baseline for the same keys |
| Packaged modal224 and lifetime guard pair, E-MODAL224-PACKAGED-FILMSIM-150, E-MODAL-LIFETIME-GUARD-150 | same four coordinates, with events 8 and 13 on published slot 1 | Owned dispatch with whole-lifetime fetch guard, 16 keys, core 0 |
| Native lifecycle, E-MODAL-NATIVE-LIFECYCLE-150 and E-MODAL75-EDITOR-NATIVE-150 | dispatcher fixtures; direct pairs on cancel `0xc77794`, confirm `0xc75ed4`, leave `0xc76650`, SET `0xc72e10` | Dispatcher semantics and owned bodies with modeled services |

None of these selects any (a) callback. The menu crawler must still find: events reaching the (a) cells (`0xc7985c` on 99 cells in events 39, 45, 48, 49; the seven state-selective event-50 routes; `0xc79a40` at `(0,41)`), the four direct-state cells, states other than 0 and 30, the controller-74 callers that feed `0xc0bae8`, and a natural state-30 editor route.

Discriminating controls, each expected to refuse:

- Wrong next state: one (a) instance or the `(0,41)` direct-state entry stores a different `State75`; the post-dispatch state comparison must differ.
- Wrong route event: the typed `0xc0bae8` expectation maps one admitted key to a neighboring event; the `(75, state, event)` context comparison must differ.
- Wrong action result: one translated (b) body returns its result plus one; the dispatcher r0 comparison must differ.
- Retained-body fetch guard: one owned (a) or (b) callback is routed to its stock body; the continuous fetch guard must refuse, while (c) bodies stay permitted only inside an owned-dispatch action frame.

**Open questions for the owner.** Whether the frontier rule should stay as specified (135 callbacks and 30,584 bytes stay stock) or admit a callee bound as a retained typed service, as the owned lifecycle bodies already do. Whether `0xc12aac`'s binding frontier blocks its 266 callers or is closed once with the router. Whether the pilot owns `0xc0bae8` and its 23 controller-74 call sites, or keeps it stock with a typed expectation. Whether build-time derivation of the owned table from owner bytes satisfies the publication review. Whether nested dispatch to controller 100 through stock+4 stays admitted inside owned 75 actions for the pilot.

#### Controller75 generic controller, host evidence

This implements the strategy-independent part of the design under the owner's decisions above. `replacement/modal_runtime/src/modal75.rs` defines `Event75` (0..=50), `State75` (0..=45), `GenericAction` (`RouteToTransition` with at most four router-result to state pairs, or `ConstantResult`), `OwnedCell`, `plan`, `run_generic` and `post_action_state`. `plan` returns an owned plan only while the live cell word, the action target and, for a positive high halfword, the selected transition entry equal the table's. Every other cell, and every out-of-domain event or state, takes the unchanged stock path.

`rb_modal_dispatch` uses these only under `--cfg rb_modal75_owned`. Owned generic actions call the existing bank trampoline `rb_call_transition` to `0xc12aac`, so nested dispatch to controller 100 still goes through stock+4. Retained actions keep `rb_modal_call`. The four direct-state cells use a typed `State75` store that equals the stock negated-high store by construction, not new behavior. Without the cfg, the stock action match and state store are textually unchanged. The default `rb_modal_scoped` bank crate, cross-compiled at HEAD and with this change using the same flags, disassembles identically apart from archive member names. No bank variant or package selects the owned path yet.

[`//replacement/modal_runtime:controller75-table`](../replacement/modal_runtime/Controller75TableMain.hs) generates the table from owner-supplied image5 and the inventory; neither the table nor its vectors are checked in. It derives each layer-(a) action by concrete A32 execution of the stock body with `0xc12aac` stubbed, over every compared constant plus uncompared fillers. It refuses any post-call read of r0 other than `cmp/cmn r0,#imm` tested by EQ or NE, any non-stack store other than a halfword to the state cell, and any unrestored SP, LR or r4-r11. It then re-verifies every action and every cell word, target and transition entry against stock, and writes the stock outcome of each owned cell for each router candidate as test vectors.

| Output (private, `private-data/scratch/ControllerPilot75/`) | SHA-256 | Content |
| --- | --- | --- |
| `modal75-table.rs` | `31f4d8ecffd5a830140c8680a759181f940a3589d8adda12d912fca2e0bd363a` | 9 generic actions, 130 owned cells, 4 direct-state |
| `modal75-vectors.rs` | `7249669136cbc73378d4a87872ad0e56d55820609113a579603acccaa79ca67a` | 682 stock vectors |
| `generator-runs.log` | `28f7309cacabcc5f66201ef0e9146615d8f522a9206ec338bc4877bce46ee1d8` | table run and three refused controls |
| `host-checks.log` | `c7eeb6f6fcef0f771c8c244e0eb083250e173279caa11a42293ae7f73d5e5129` | owner vector tests, owned cross-compile, default-code identity |

The 130 cells are 127 cells of the nine layer-(a) callbacks plus three direct-state cells with retained actions; `(0,41)` is both. Controls, all refusing: the generator's wrong next state (`0xc7a6ac`, router result `0x40`), wrong action result (`0xc79a40`) and wrong route event (cell `(0,40)` word differs). `check_vectors` runs the real `plan`, `run_generic` and `post_action_state` against the vectors and refuses mutated tables for wrong next state, wrong direct state, wrong constant result, a shifted event and a stale cell word. The stale cell word is the retained-body fetch guard: a generic cell planned as stock. `//replacement/modal_runtime:modal75-tests` runs these on synthetic data (9 tests). The owner run, `rustc --test --cfg rb_modal75_vectors` with `RB_MODAL75_TABLE` and `RB_MODAL75_VECTORS`, passes 11 tests, including `owner_table_matches_stock_vectors` and `owner_controls_refuse`. The composition inside `rb_modal_dispatch` is review-checked, not test-checked: carried residues, arm order and how `LiveCell` is built.

Owned generic actions reach `0xc12aac` through `rb_call_transition` from Rust rather than from the stock callback frame, as `rb_ui_editor_confirm` and the other owned lifecycle bodies already do (`owned_menu/src/lifecycle.rs:117`). That rests on the named premise `transition-router-register-inputs`: `0xc12aac` and its callees read no incoming r0-r3 or r11 beyond their declared arguments (E-MODAL75-TRANSITION-PREMISE-150). Static evidence:

- `0xc12aac` opens with `push {r3,r4,r11,lr}`, which only saves, then `add r11,sp,#12`, and writes r3 and r2 before any other use (`id5.dis` at `0xc12aac..0xc12abc`).
- The decompilation infers no undeclared register input (`in_rN`, `unaff_*`, `in_lr`, `in_stack_*`) in `0xc12aac`, in its direct callees `0x63a0e0(void)`, `0xd1740c(int)`, `0xd35fbc(void)`, `0xd176f4(void)` and `0x632a3c(uint)`, or in the six callees of `0xd176f4`.
- In the router-map sample paths, incoming r0 and r1 are still in the argument registers only at `0xc12c24` (call to `0x63a0e0(void)`) and incoming r1 only at `0xc12adc` (call to `0xd1740c(int)`, which reads r0 only: `id5.dis` at `0xd1740c`). Neither callee declares those registers.
- Nested dispatch from `0xc12aac` passes only r0 = 100 to the dispatcher, which re-establishes every register role itself.

Not proven: decompiler signatures are inference, not a read-before-write proof. The image4 bodies of `0x63a0e0` and `0x632a3c` and the callees below the second level are not inspected instruction by instruction. The router-map arguments come from sample paths, not every path.

The owned path reads the full cell word, and for a positive high halfword the selected transition entry, before the action; stock reads neither at that point. This difference is intentional: the modal lifetime guard watches writes and fetches only, not reads.

No layer-(a) cell or direct-state cell lies on the recorded FILMSIM or packaged guard-pair domains (state 0, events 2, 8, 13, 50). Their four cells select `0xc6eb44`, `0xc72e10`, `0xc76650` and `0xc7a528`, which all stay stock in this increment. No guest was run; the evidence is host-only (E-MODAL75-GENERIC-HOST-150).

#### Controller75 owned package and retired-body guard

The continuous lifetime guard now takes the nine layer-(a) stock body extents as retired CPU-fetch classes. In a watch with instruction guards, an accepted fetch outside every guard records a `cpu-fetch` row when a class selects it (`build-support/fujimod-fetch-attribution.patch`); before, every fetch in a guarded watch was silent. The monitor refuses a fetch starting in a retired extent with `retired_fetch` from the install entry through the last reverse publication write, through any mapped or decoder alias on either core; outside that window the restored stock dispatcher may fetch them, and the completed Haskell check accepts those rows only before the first or after the last publication-writer row. Retired extents must avoid the runtime, every write-watched extent and every guarded instruction site. SDK tests show canonical, aliased and decoder-alias fetches on both cores refusing, and the phase window with adjacent extents (`0xc7aef4`, `0xc7af6c`, `0xc7afc8`).

`--modal75-owned-archive` and `--modal75-retired` link an owner-built bank archive (`--cfg rb_modal_scoped --cfg rb_modal75_owned`, `RB_MODAL75_TABLE` = the private table) and the generator's retired document, which names schema 1, the pinned image5 and the table digest. The first owned archive pulled `core::slice::index::slice_index_fail` and `__aeabi_memcpy` into `rb_modal_dispatch`; the RAM-push link has no section GC, so `.text` overlapped `.ram_push.text`. `modal75.rs` now references table actions and iterates route pairs without slicing. Relinked at the reference base `0x00c40000`, the owned archive passes the bank allowlist with 0 violations (443 proven stores, 987 proven branches against stock's 439 and 986), and its bank object has stock's undefined-symbol set. Eight independent review rounds covered the guard, package, trajectories and settings rule (E-MODAL75-OWNED-PACKAGE-HOST-150).

Under the original guard the stock223 control refused on both crawl-3 routes to layer-(a) cells, because the stock property setter changes a guarded live-setting byte (E-MODAL75-ROUTE-STOCK-REFUSAL-150).

| Run | Route | Reached | Refusing write |
| --- | --- | --- | --- |
| `run-stock223` | MENU, OK (live view), then OK into `(1,321,17)` | shooting menu `(1,5,2)` | `0x0096ccd2` `0x89`→`0xc9`, pc `0x0062ae3c`, instruction 1,522,403,269 |
| `run-stock223b` | MENU, MENU, then OK into `(1,321,17)` | same | identical row stream (duplicate input) |
| `run-q-stock223` | MENU, Q, then BACK | Q menu `(122,5,1)`, `retired-c7a7e4` hit 3 times | `0x0096ccca` `0x20`→`0x21`, pc `0x0062fed8`, instruction 1,520,318,916 |

Both refusing writes fall in the key after the last completed segment, inside the stock property setter. The shooting-menu runs match the passing packaged stock run row for row through instruction 1,522,242,231.

**Settings rule.** A value-changing store to the 450-byte live-settings block is now admitted only from a declared settings writer; both roles bind the stock property setter `[0x624694, 0x6309c4)`, its jump-table arms fenced by the next function. Each admitted store is counted in the receipt, re-derived from the final watch and written with its instruction count, PC, address and old and new bytes to `e2e-modal-settings-writes.json`. `//emulation:modal-settings-compare` re-derives both roles' stores from their bound receipts and requires the same ordered (address, width, value) sequence. Any other writer, and any resident-bank change, still refuses. The comparison covers the watched block at bank slot 0, not the setter's mirror at `0x0089ec00` or other slots. Owned code declares no settings binding.

**Q-route pair.** Under the new mechanism the Q route ran once per role (E-MODAL75-OWNED-Q-ROUTE-150). Both runs pass install, trajectory and unload with the completed lifetime check and one admitted setter store, `0x0096ccca` `0x20`→`0x21`. The stock control fetched `0xc7a7e4` four times at controller 75 `(43,50)`. The owned candidate ran the owned generic action instead (`owned-transition-call` 4 times, 6 owned handoffs among 1,521 balanced entries), with no retired-body trace hit and no CPU-fetch row in its 573,333 guard rows. The settings sequences and all five captures compare equal. Only the `(43,50)` cell is exercised; the other eight retired bodies are not reached on this route.

### Behavioural census

The maintainer scoped #128 as functional replacement: the camera's menu behaviour, not every stock callback. This section measures that behaviour by crawling the emulated stock UI and maps what it reaches onto the callback measurements above. [Sources: E-MODAL-MENU-CRAWL-150; E-MODAL-CONTROLLER-SCALE-150.]

#### Crawl mechanism

`//emulation:xm1-viewer -- --ram-push-e2e --cardless --fujimod --flash-candidate --menu-crawl emulation/menu-crawl.json` boots or restores the stock no-install attach checkpoint and crawls depth-first from live view. The FujiMod RAM-push bridge holds up to 24 in-memory guest snapshots (`snapshot-save`, `snapshot-restore`, `snapshot-key`, `snapshot-drop`). Every key press restores the snapshot of its source screen, so no press sees state a sibling branch left behind, and a strict stop is returned as data and cleared by the next restore together with a fresh R4F guard. After a press the bridge advances until a scanout instance presents graphics. [Sources: `emulation/fujimod-xm1/src/ram_push.rs` snapshot requests; `emulation/Emulation/MenuCrawl.hs`; `emulation/Xm1RamPushE2E.hs` `runMenuCrawlE2E`.]

Each screen records its frame, the menu reading (node id and setting id at the pointer `0x009571d0`, cursor at `+8`), the open node's 48-byte descriptor and all 174 state and event cells. Each press records the bridge settle result and every entry of eight traced sites: dispatcher context `0xbfb0a0` (r0 controller, r1 state, r2 event), action call `0xbfb0c8` (r3 target, paired per core with the latest context), direct-state store `0xbfb114`, property write `0x0061c91c`, setter `0x00624694`, commit dispatcher `0x00d4fc74`, mailbox writer `0x0063270c` and application sender `0x006334dc`. A screen is new unless its frame was seen before or its menu reading and state cells match a known screen. The keys are OK (also MENU), DOWN, UP, RIGHT, LEFT, BACK (also DISP), Q, both sub-command dial turns and PLAY; screens with controller 74 in state `0x18` use only the first six, because in the first run every Q, PLAY and dial press from such a screen reached a known screen and DIAL-RIGHT equalled DOWN in 194 of 195 cases. [Sources: `emulation/menu-crawl.json`; first-run journal `private-data/scratch/ControllerStrategy/crawl-1/crawl-trials.jsonl`.]

Depth-first discovery order fixes a screen's recorded depth, so a long list walked early pushes everything beneath it to the depth limit. After each root's depth-first pass the crawl computes the fewest-press recorded path to every depth-limited screen, replays it from the root, accepts the landing by the same frame-first rule and expands the screen from there. A replay that lands elsewhere is recorded as diverged and leaves the screen depth-limited.

The pointer at `0x009571d0` is not cleared when a menu closes: live view keeps the first-start date node's reading `(67,56,0)`. Controller 74 (modal `0x4a`) separates the cases: `0x04` on the live-view root, `0x11`, `0x19` and `0x1c` on live-view overlays such as drive and white balance, `0x01` on the LCD BRIGHTNESS slider, `0x18` on the menus MENU and Q open, and `0x00` under the SET-UP pages. The census keys menu behaviour by (node id, setting id) on screens in `0x18` or `0x00` and lists all other screens separately.

Limits: the attach route consumes the first-start language and date screens with fixed keys before the crawl starts; there is no card, lens, sensor image or S1/S2 press; the mode dial stays at its boot position; live view is the synthetic scene. These screens and paths are left to the camera-side harness below.

#### Runs

| Run | Mechanism | Result | Receipt under `private-data/scratch/ControllerStrategy/` |
| --- | --- | --- | --- |
| crawl-1 | Snapshot crawl, all ten keys on every screen | Stopped at trial 1,996 after about 230 screens: BACK from SET-UP page 1 settled with no display plane and the capture error ended the session. Fix: a press advances in 2M-instruction steps, up to 60M, until a plane is presented, and a missing plane is a stopped edge. | `crawl-1/`, `crawl-1.log` |
| crawl-2 | Plus the plane wait and menu-state keys | Killed at trial 388 by an orchestrator restart, not by its input | `crawl-2/` |
| crawl-3 | Same input as crawl-2 | 309 screens, 1,948 trials, 72 min, 5 plane stops; 25 screens depth-limited at 16 whose recorded shortest paths are 3 to 10 presses, so the SET-UP subtree was unexplored | `crawl-3/` |
| crawl-4 | Plus shortest-path replay | 564 screens (1,089 frames), 4,544 trials and 250 replay presses in 177 min; 25 replays expanded and 6 diverged; 14 screens depth-limited; 16 stopped presses: 7 with no plane (a live-view overlay `(16,472)` and GEOTAGGING SET-UP `(73,71)`) and 9 on an unsupported Wi-Fi SDIO command (PC AUTO SAVE SIMPLE SETUP `(83,177)`) | `crawl-4/`, `census-4-menus.json` |

The six diverged replays took a shorter path through state the frame does not show, such as a menu reopened at its remembered cursor. The 14 remaining depth-limited screens are positions deep in long value lists plus those six; two of them, Fn BUTTON SETTING `(40,252)` and DISPLAY CUSTOM SETTING `(24,389)`, were seen on one screen each.

#### Behaviour counts

The census groups crawl-4's 495 menu screens into 87 keys and leaves 69 live-view and overlay screens outside menus. Presses dispatched 371 distinct actions of 38 controllers; those actions fall in 295 helper-bound shapes whose members are 753 of the 5,795 table callbacks. An action can serve keys of several classes, so the per-class action counts overlap. [Sources: E-MODAL-MENU-CRAWL-150; `private-data/scratch/ControllerStrategy/census-4-menus.json`.]

| Class (census rule) | Keys | Examples | Actions | Shapes |
| --- | --- | --- | --- | --- |
| Navigation list: opens keys that are not its ancestors | 18 | shooting-menu pages `(1,5)`, SET-UP pages `(63,5)`, tab strip `(0,5)`, ISO AUTO CONTROL `(36,330)`, SCREEN SET-UP `(78,7)` | 235 | 197 |
| Value list: cursor moves commit a property | 38 | ISO `(1,321)`, WHITE BALANCE `(11,237)`, DATE/TIME fields `(67,56..59)`, language `(68,52)`, VOLUME `(72,45)` | 111 | 96 |
| Toggle: a value list with two positions | 25 | SILENT MODE `(63,525)`, sensor-cleaning timing `(127,209)`, `(127,210)` | 95 | 83 |
| Other | 6 | Q menu `(122,5)`, name keyboard `(86,182)`, password digits `(126,201)`, previous-connection details `(85,183)`, `(40,252)`, `(24,389)` | 58 | 42 |
| Confirmation: commits its own setting | 0 | see below | 0 | 0 |
| Outside menus (69 screens) | | live view, drive, focus-area, white-balance and other arrow-key overlays | 131 | 105 |

Value-list and toggle moves commit through `0x00d4fc74`; editors such as DATE/TIME commit their field (`0x39`, `0x3a`, `0x3b`) without a property write. Frames show five OK/CANCEL confirmation dialogs: SET OK?, CUSTOM RESET, SHOOTING MENU RESET, SET-UP RESET and RESET WIRELESS SETTINGS. Each keeps its parent's menu reading and opens on CANCEL, and its cursor is in neither the reading nor the state cells, so the crawl treats OK and CANCEL as frame variants of one screen and never pressed OK. The census therefore counts them inside their parents.

Twenty-one actions, of controllers 0, 30, 41, 42, 74, 75, 76, 100, 101 and 107, run on at least 10% of presses (454 of 4,544); presses average 15.1 dispatches. **[INFERENCE]** These form the per-press dispatch path rather than menu behaviour.

**[INFERENCE]** Generic behaviours that cover every reached screen, each driven by recovered menu data (node, setting, property id, value array, row targets) rather than by the stock action bodies:

| Generic behaviour | Covers in crawl-4 |
| --- | --- |
| List navigation with page tabs | 18 navigation keys |
| Value picker committing on each move; a toggle is the two-value case | 63 value-list and toggle keys |
| Multi-field editor: LEFT/RIGHT between fields, each field a picker | DATE/TIME `(67,56..59)`, TIME DIFFERENCE `(65,64..66)` |
| Character and digit entry | name keyboard `(86,182)`, SSID `(125,192)`, password `(126,201)` |
| Slider | VOLUME `(72,45)`, LCD BRIGHTNESS (state `0x01`) |
| Confirmation dialog defaulting to CANCEL | 5 dialogs |
| Q menu grid | `(122,5)` |
| Live-view key overlay strip | drive, focus area, white balance and the other outside screens |
| Information or progress screen | sensor cleaning run, previous-connection details, PC auto-save connection |

**[INFERENCE]** Nine generic behaviours therefore cover the 87 menu keys, five dialogs and the live-view overlays the emulator reaches, where the stock firmware spends 371 actions in 295 shapes across 38 controllers. For #128 this moves the functional unit from the callback to the menu table: an owned dispatcher plus these behaviours replaces the reached actions once the menu, property and value tables are recovered as data, which the next section does. The scope still open is what the emulator cannot reach: card, lens, mode-dial, S1/S2 and Wi-Fi paths, and the depth-limited screens.

#### Menu tables, confirmation paths and the behaviour map

`//replacement/modal_runtime:menu-tables` decodes the stock menu data from the pinned image-4 decode and the English string slot. The node registry at `0x0081babc` has 0x86 slots (bound from the walk in `FUN_0061d038`), 111 of them filled. With parents and row children the decode holds 332 node records of 11 words: id, type, property or class, kind, two zero words, flag, parent, values, labels and a last word of unknown use. Values are `{row, child}` pairs ending at `{0x7ffffffe, 0}` (`FUN_00bf9794`); labels are `{preview icon, list icon, help, name}` string-id records; 142 property conversion tables come from `*(0x0081b09c + 4*property)` for properties up to `0x286` (`FUN_00631a00`). Most row labels resolve to the camera's private-use glyph font rather than plain English text. [Sources: E-MODAL-MENU-TABLES-150; `private-data/scratch/ControllerStrategy/menu-tables.json`.]

Every menu key of crawl-5 matches exactly one stock node by its 44-byte record. The crawl reached 88 of the 332 nodes; 63 of the 244 it did not reach are row children of reached nodes, and which paths open the other 181 is not established. **[INFERENCE]** The decoded tables carry the menu data the nine generic behaviours need, including for menus the emulator does not reach.

The confirmation cursor is the word `0x009503c0`. Two `--ram-diff` runs sampled `[0x00600000,0x03300000)`: in the SHOOTING MENU RESET dialog, DOWN changes it from 0 to 1, a second DOWN or UP restores it, and Q leaves it; DOWN on the RESET list behind the dialog does not change it. Of the small words with that pattern it is the only one in image-4 data; the others lie in repeated structures at `0x00a4d9xx..0x00a87dxx`, `0x0165xxxx` and `0x0207xxxx..0x0283xxxx`. Crawl-5 adds it to screen identity and marks OK from a dialog as a terminal press, so each OK is pressed once from its dialog's own snapshot and the state after the confirmation is never explored. With it, the OK positions of all five dialogs became separate screens. Dialogs are recognised by controller state: controller 75 in `0x0d` for SET OK?, controller 76 in `0x21` for CUSTOM RESET, SHOOTING MENU RESET and SET-UP RESET, and controller 76 in `0x25` for RESET WIRELESS SETTINGS.

| OK path (crawl-5) | Distinct properties set | Landing |
| --- | --- | --- |
| RESET WIRELESS SETTINGS | 10 | SET-UP tab column, known screen |
| CUSTOM RESET | 92 | SET-UP tab column, known screen |
| SHOOTING MENU RESET | 304 | SET-UP tab column, known screen |
| SET-UP RESET | 69 | START MENU language page `(69,52,12)`, new screen, not expanded |
| SET OK? | not pressed: its OK position stayed depth-limited after its replay diverged | |

None of the four confirmations goes through the commit dispatcher `0x00d4fc74`; each writes its properties through the setter `0x00624694` and sends application message `0x816` with `0xa05`, `0xa0e` or both. Crawl-5 otherwise matches crawl-4: 570 screens, 4,584 trials, 88 menu keys (the new key is the START MENU page), 380 distinct actions in 301 shapes, 15 screens depth-limited. [Sources: E-MODAL-MENU-OKPATHS-150.]

`census-5.json` maps each of the 88 keys to its stock node (address, row count, row names), its controllers and every dispatched `(controller, state, event, action)` with the manifest cell index and action slot; the 21 per-press actions are flagged. Grouping keys by the set of actions they dispatch, other than per-press ones:

| Class | Keys | Distinct action sets |
| --- | --- | --- |
| Navigation list | 18 | 18 |
| Value list | 38 | 17 |
| Toggle | 25 | 12 |
| Other | 7 | 5 |

The 63 pickers use 117 distinct actions. Twenty-one shooting-menu pickers dispatch the identical set of ten actions of controllers 30, 74, 75 and 100, so the stock firmware already runs them through one generic picker path; the SET-UP pickers fall into smaller groups (at most five keys) with setting-specific controller-76 actions. **[INFERENCE]** A generic owned picker driven by the decoded node, values and conversion table reproduces both groups; the setting-specific controller-76 actions are the residual whose side effects (properties written, application messages) the census lists per key and a replacement must keep.

#### Owned generic picker, increment 1

The 21 shooting-menu pickers that share one stock action set do not change a value in those actions. Each of the ten actions gates on the busy query `0x63a0f8(0x53)` and posts a stage operation through `0xd3d858` (UP `(5,1)`, DOWN `(4,1)`, LEFT and OK `(0xf,0x1d)`, BACK `(0xf,0x1a)`) before calling the transition router `0xc12aac`. The value step runs downstream: the stock menu engine moves its cursor and posts mailbox `0x1d` with the property and the row token of the new cursor; controller 30 event 2 (`0xc2d8ec`, run on every press) passes them to the commit dispatcher `0xd4fc74`; and the property's commit arm converts the token and calls the write `0x0061c91c`, which reaches the setter. Only the arms of properties 163 and 214 reduce to "convert, write, exit"; the other 18 make further stock calls. [Sources: E-MODAL-MENU-OKPATHS-150; id5 `0xd4fc74..0xd4fcd0`, `0xd52664`.]

Increment 1 owns the seven controller-75 key actions (cells 75/0/0, 2, 4, 6, 8, 13 and 49) for 20 of those pickers; FILMSIM (setting 254, node `0x7823a4`) and every other menu stay stock. `replacement/modal_runtime/src/picker.rs` runs each owned action through stock services: the busy query, the node, class and token accessors, stage open, the router, the notification `0xd38d98` and the flag `0x009504a8`. The action runs only while the open node `*0x009571d0` is a picker node, the live cell word and action target equal the generated table, and the cell selects no transition entry. An owned action that would reach a non-generic stock branch (special classes, mode cells `0x009544f8`, `0x00950474`, `0x00950478` or `0x00954500`) declines before any side effect, and the dispatcher runs the stock action instead. `//replacement/modal_runtime:picker-table` generates the private table from pinned image4 and image5, the decoded menu tables, the census and the controller manifest.

The build is opt-in: `--cfg rb_menu_picker_owned` (which requires `rb_modal_scoped`), with `RB_PICKER_TABLE` naming the generated table. It uses the existing 56-byte controller-75 seam, so the default 223/224 packages are unchanged. The picker cells move into the controller-75 table as a picker cell kind once agent/ControllerPilotPrep merges. Runs wait for that branch's settings-write comparison rule.

**Shelved: owning controllers 30, 74 and 100.** Routing those ids through `rb_modal_dispatch` (a 68-byte seam testing `{30, 74, 75, 100}`) would need:
- one typed seam layout (owned id set, byte length, fixed words, resume and owned-branch offsets) used by the four consumers that now require the 56-byte, 75-only seam: `host/firmware/BuildRecipeBank.hs` (seam size check), `replacement/modal_runtime/Replacement/ModalRuntime/EntryBinding.hs`, `host/firmware/RuntimeRelocationCompare.hs` and `emulation/fujimod-xm1/src/modal_permit.rs`;
- an owned-set policy in the lifetime guard and scope contract (`modal_permit.rs`, `emulation/Emulation/ModalObservation.hs`, `modal_trace.rs`, a `controllers` array in `modal-scope.json`);
- qualification of the dispatcher port for those ids (`Qualify.hs` takes controller 75 as fixed; entry fixtures; the closure diagnostic per id; a register and frame census of their action bodies).

**[INFERENCE]** The table-driven value step belongs to the stock menu engine, which runs inside modal dispatch as controller 100's stage consumer (next section), so the next increment investigates that engine.

#### Stock menu engine and the owned target

Four single-press `--menu-observe` runs fix the stock menu engine's shape together with the decompilation; none of their watches reported a violation. [Sources: E-MENU-ENGINE-OBSERVE-150; `private-data/analysis/x-m1-1.50/filmsim/ghidra/all.c`.]

| Run | Configuration | Press | Trace hits | Recorded writes |
| --- | --- | --- | --- | --- |
| 1 | `emulation/menu-observe-picker-down.json` | DOWN at the last row of the setting-246 picker | 174 | 1,270 |
| 2 | `emulation/menu-observe-picker-open.json` | OK on that picker's row in its parent list | 285 | 13,258 |
| 3 | `emulation/menu-observe-picker-back.json` | BACK from the picker | 233 | 12,663 |
| 4 | `emulation/menu-observe-menu-open.json` | OK at the attach root | 148 | 6,473 |

**Event path.** All events cross one fixed block, not a queue:
1. In run 1, controller 75's DOWN action `0xc6eb44` calls stage open `0xd3d858(4, 1)`, which writes event `0x1e` (kind 4, node 1, id 1) into the fixed event block `0x0094f64c..0x0094f660` through `0x0063270c`.
2. The stock dispatcher then runs controller 100 (state 1, event `0x15`), whose action `0xca4674` consumes the block.
3. `0xca4674` enters `0xd4dd2c`, the engine wrapper `0xbf95c8` and the kind switch `0xbfa234` (all.c:331301). Kind 1 opens a node at its remembered row; 2 closes and remembers the row; 4, 5 and `0x45` to `0x48` move within the open node; 6 to `0xb` wrap a move between a pop and a push (6, 7) or a push and a pop (8 to `0xb`, with 10 and `0xb` first calling `0xbfad04`); `0xe` and `0xf` push and pop through `0xbfab98`; `0x10` to `0x12` push or pop two or three levels; `0x13` re-opens.
4. The move `0xbf9f44` steps through `0xbf9b1c` (all.c:330967; code `0x16` up, anything else down). The step wraps unless node word 4 is 1, skips rows whose state is 2 or 3 unless node word 5 is 1, and then sets the visible ordinal from the row's value token and the visible count.
5. The move then recomputes the scroll top (`0xbf988c`) and stores 44-byte snapshots through `0x0061ceac`.

**Value step.** For kinds 4, 5 and `0xe`, `0xca4674` (all.c:457594-457621) asks `0xd586bc` with modes 3, 2 and 1 about the node class `0x0061d1f4` and the cursor token `0x0061d244`, and when one of them is nonzero posts event `0x1d` (class, token, `0xf000`) through `0xd3d620` and dispatches controller 30 (`0xc0028c`). In run 1 mode 3 answered: the post carried property `0xf6` (246) with token 1. Controller 30's action `0xc2d8ec` called the commit dispatcher `0xd4fc74(0xf6, 1)`, and the property writer `0x0061c91c` was then entered for properties `0xf6`, `0xf9`, `0x161` and `0x1e5`, in that order. On kind `0xf` with id `0x1a` (BACK), the special-post function `0xd42a64`, which `0xca4674` calls after the engine (all.c:457585), asks `0xd586bc` with modes 3, 2 and 1 about the previous node's property and token, read through `0x0061d494`/`0x0061d4b8`, and posts `0x1d` for property `0x27d` with token 0 when one of them is nonzero (all.c:537689-537735). What property `0x27d` means is not established.

**State.** The engine's state is a set of globals:
- the open node pointer `0x009571d0`;
- cursor `0x009571d8`, visible ordinal `0x009571dc`, visible count `0x009571e0`, scroll top `0x009571e4`, total rows `0x009571e8`;
- per-row states `0x009571f0` (0 offered, 1 variant, 2 disabled, 3 hidden), saved-row copies `0x0095732c..`;
- a cursor stack `0x00957554` indexed by depth `0x009575f4`;
- remembered rows `0x0095733c[node]`.

Run 1 wrapped the cursor, the visible ordinal and the scroll top from 8, 8 and 2 to 0 (writers `0x00bf9bec`, `0x00bf9c04`, `0x00bf9c10` and `0x00bfa1c8`). `0x0061cd14` (all.c:10050) then rewrote the depth-2 snapshot of node 1 at `0x00941300` as `{depth 2, node words 1, 0, 246, cursor-row token 1, saved-row token 1, cursor 0, ordinal 0, visible 9, page word 1, top 0}`.

Run 2 posted kind `0xe` (push) with node 1 and id `0x1d` through `0xd3d858`, called from `0x00c738ec`. The push changed the node pointer from `0x0078ea98` to the picker record `0x00784014` and the depth `0x009575f4` from 1 to 2. It saved the parent's cursor, ordinal and visible count (3, 2, `0x20`) at `0x00957564`, cleared the row states, and set the total rows from `0x21` and the visible count from `0x20` to 9. Kind `0xe` takes the same value-post branch as run 1 (return address `0xca4800` in both), so `0x1d` and the commit ran for property `0xf6` with token 1: entering the picker re-commits its current value.

Run 3 posted kind `0xf` (pop) with node 1 and id `0x1a` from `0x00c769f4`. The pop restored the parent node `0x0078ea98`, cursor 3, ordinal 2, visible count `0x20`, total `0x21` and depth 1, and recomputed the row states (rows 0 and 1 in states 3 and 2). `0xd42a64` then asked `0xd586bc(3, 0xf6, 1)` from `0xd430dc`, posted `0x1d` for property `0x27d` with token 0 from `0x00d43170`, and the commit dispatcher ran for `0x27d`.

Run 4 posted kind 1 (open) with node 1 and id 0 from `0x00d3d754`, without `0xd3d858`. The open set the menu-open flag `0x009575fc`, the node pointer `0x0078ea98` and depth 1, and called the step `0xbf9b1c(0x15)` from `0x00bfaac0`; the cursor landed on row 2 with ordinal 1, past row 0 (state 3) and row 1 (state 2). `0xd42a64` then posted event `0x20` from `0x00d42da4`. Before the press the globals still held a depth-2 node `0x0078ecd8` from an earlier menu. The node index 1 that runs 2 to 4 post names both `0x00784014` and `0x0078ea98`; kind 1 loads the record from `(&0x0081babc)[node]`.

In these four runs every write to `[0x009571d0,0x00957600)` came from `0x00bf977c..0x00bfacb0` or, in run 4, from `0x007221b0` inside the clear `0x007221c8` that kind 1 calls. Statically the block has other writers: `0xca4674` saves the whole 0x430-byte block to `0x0096b58c` through `0x0061d384` before `0xd4dd2c` and restores it through `0x0061d3a8` when `0xd4dd2c` returns 0 (all.c:10520-10535), and `0x0061cf14` clears it through `0x7221c8`.

**Rendering.** The engine requests a redraw through `0xd4c8d4` and `0xd3d894` (codes `0x837`, then `0x816`). `0xd38d98` (all.c:531642) stores codes below `0x900` to `0x009503d0` (run 1: `0x50b`, then `0x400`) and sets `0x009503d4` to 1 only for codes `0x900` and above (runs 2 to 4: `0x92b`, `0x933`, `0x919`). Draw functions in `0x0116xxxx` read 44-byte copies of the engine state through `0x0061cd14` (live) or `0x0061ce1c` (saved) rather than the globals. After the redraw requests of run 1, the row-availability function `0xd52684` was entered 106 times, 53 of them from `0x00bf8dbc` and 16 from `0x00bf924c`. In run 1, 142 of the 162 writes to `0x00a85000..0x00a88000` came from `0x008409bc`, `0x00840a48` and `0x006d8518..0x006d85f8`; in runs 2 to 4, `0x00043594` and `0x00043564` wrote most of that range.

**Target and shims.** **[INFERENCE]** The target for the menu subsystem is:
- a declarative menu tree built from the decoded tables (nodes, rows, labels, availability rules);
- one owned menu engine implementing these kinds over typed state;
- the nine generic widgets;
- a typed settings service;
- a render layer.

The stock modal-dispatch hook becomes a thin event adapter. The stock code the owned engine still meets is wrapped in named shims, each removed when its condition holds:

| Shim | What it does | Removal condition |
| --- | --- | --- |
| Stage-event adapter | Takes the `0x1e` stage events that stock key actions post and feeds them to the owned engine, at the single engine entry `0xbf95c8` | The owned key layer delivers key events to the engine directly |
| Stock-state mirror | The owned engine writes the stock globals above and the `0x0061ceac` snapshots, so stock readers and the stock renderer stay consistent | Every reader of those globals and snapshots is owned |
| Value-event adapter | Posts `0x1d (P, V)` for kinds 4, 5 and `0xe` and dispatches controller 30, so stock commit arms apply the value | The typed settings service owns the commit arm of every property the tree uses |
| Leave post | Reproduces `0xd42a64`'s `0x1d (0x27d, 0)` post on BACK when `0xd586bc` modes 3, 2 or 1 report the previous node's property | The settings service owns property `0x27d` and the conditions that request it |
| Stock engine rollback | `0xca4674` saves the 0x430-byte state block to `0x0096b58c` before the engine call and restores it when the call returns 0; with the stage-event adapter inside this bracket, a rejected event restores stock state over the owned engine's mirror | The owned engine is entered from the owned key layer outside `0xca4674`, so the bracket no longer runs |
| Stock redraw path | Redraw requests `0x837`/`0x816` and notifications through `0xd38d98` | The owned render layer draws menus |
| Stock availability | Row states from `0xd52684` | Availability is declared in the menu tree |
| Picker key cells | Increment 1's seven owned controller-75 cells | The owned key layer replaces them (no further per-cell ownership is added) |

Next observations, each a single `--menu-observe` run: page moves, a move with no-wrap or no-availability nodes, value pickers whose commit has a plain arm (properties 163 and 214), and a confirmation dialog.

#### Camera-side validation harness (design)

**[INFERENCE]** Purpose: check the emulator graph against the body for the screens the emulator cannot reach (first-start language and date, card-backed playback and storage dialogs, lens and focus menus, other mode-dial positions, wireless, flash, movie, Fn assignments, S1/S2 flows) and for screens it may render differently (live view, timing-dependent overlays). The owner's webcam records the LCD while keys are pressed and the body's own state is read back.

| Part | Design | Existing support | Missing |
| --- | --- | --- | --- |
| Key script | Generate per-screen key sequences from the emulator graph: a path from a known start to the target screen, then the edges to check, each with its expected next screen. Exclude every edge whose emulator activity commits durable state (storage, format, reset, save) or that leads to one. | Graph edges with their traced commits, mailbox codes and dispatches | Script generator |
| Phase 0 input, no camera code | The owner presses the scripted keys by hand at a host prompt; the webcam records. | Physical key codes and release forms bound by the act-logged capture: OK `0x01`, UP `0x02`, DOWN `0x03`, LEFT `0x04`, RIGHT `0x05`, DISP/BACK `0x09`, PLAY `0x0b`, Q `0x32`, Fn `0x28`, sub-command push `0x2b`, release `code \| 0x80` (E-PRESS-MAP-150) | Prompt tool |
| Phase 1 input, RAM-side hook | A RAM-push payload extends the stock key-scan callback `0x006a33f0` (task 37, period 25; E-XM1-KEY-SAMPLING-PATH-150): on a scan tick it takes one pending key record from a mailbox in its allocation and publishes it through the stock publisher `0x006a1c50`, the GPIO handler's route into the queue-1 key message, so injected presses take the stock debounce and dispatch path. The host writes the mailbox with the existing function-3 RAM write. | RAM-push install and rollback on the body (E-RAM-PUSH-INSTALL-150 and the RAM-push 253 runs, E-RAM-PUSH253-INSTALL-150); publisher route `0x006a1c50 -> 0x006a1b40` and sender `0x006334dc` from static analysis only | Hook bytes; proof that calling `0x006a1c50` from the scan callback's task context matches the GPIO handler's; queue-1 message layout; physical debounce and release timing (#53 is open with no measurements; #111 covers emulator release caps only) |
| State readback | After each press, `read-ram` the menu record (`0x009571d0`, 16 bytes, already the first planned live window), the controller state and event cells `[0x01467588,0x01467840)` in three windows and the 48-byte node descriptor. This is the emulator graph's own screen identity, independent of video. | Standing read grant for `read-ram` and `read-config` (E-CAMERA-READ-GRANT-150; spec 07 section 3.4); `transport/README.md` `read-ram` plans, at most 2,032 requests per run | Plan generator; the cells have not been read on the body |
| Video | Record the webcam with ffmpeg through v4l2; rectify each frame by a homography fitted once on a calibration screen whose emulator image is known (SHOOTING MENU page 1); compare rectified frames with the expected node's emulator PNG and with its alternatives. | Pinned ffmpeg and ffprobe; `//af-session` presentation-timestamp listing and frame extraction; phone-video precedent E-RAM-PUSH253-SELECT-ENTRY-FAIL-150 | Capture command, rectification, image comparator |
| Alignment | No subtraction across host, camera and video clocks (spec 08). Order suffices: each press is followed by one readback and by the first video frame that differs from the previous settled screen. A step passes when the readback matches the expected node's menu reading and cells and the frame matches its image; otherwise the observed screen becomes a new node. | Graph identity rules above | Step matcher |
| Procedure | Phase 0 is owner operation plus H1 reads under the standing grant. Phase 1 is H2: independent review of the hook bytes, the same payload exercised first in the emulator with the crawl driving it through the hook and its edges compared with the physical-key edges, then the owner's go. Every session ends by removing the battery, so picker changes are not committed by an orderly switch-off. | `spec/07-governance-and-handoff.md` sections 3.1 to 3.4 | Review and go |

**[INFERENCE]** Order: phase 0 on the unreachable areas first, since it needs no camera code and decides which emulator gaps matter; phase 1 only if the scripted volume makes manual presses impractical. The attach-time `0xff` fill of `0x43360000..0x45a9ffff` overlaps display buffers (`transport/README.md`), so each session's comparison starts after a full redraw.
