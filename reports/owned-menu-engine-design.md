# Owned menu engine: design (#128)

Status: revision 2, cleared by MenuEngineDesignReview. Increment 1's host part exists in `replacement/menu_engine` with the generator and case extractor in `replacement/modal_runtime` (`replacement/README.md#owned-menu-engine-increment-1-128`): the owner oracle passes on eight receipts (wrap, push, pop, open, push-restored, push-classifier, scroll, scroll-up) and each of the six negative controls fails at least one of them. It differs from this text in six places. `EngineServices` takes one row's (class, token) for availability, as `0xd52684` does. Notifications and snapshot stores are in-order `EngineServices::perform` calls, not effects returned after `apply`, because `0xbfab98` notifies before `0xbfa800` asks for availability (all.c:331598-331604). `apply` takes the `Tree` and returns `NotOwned` only before its first service call; past that point the engine owns `0xbfa800`'s empty arm and calls the stock kind switch for its nested pop, and reads `values[row]` past a node's terminator through a service. No class with the `0x285` write is owned yet, so there is no property-write effect. The node and token types in `recipe_ui`, `modal_runtime` and `owned_menu/src/lifecycle.rs` are not yet moved into the engine module. No guest code exists. Facts cite the ledger or `private-data/analysis/x-m1-1.50/filmsim/ghidra/all.c`; **[INFERENCE]** marks what the evidence does not show directly.

## Target and acceptance

The menu subsystem's target is the one #128 names: a declarative menu tree, one owned menu engine, the nine generic widgets, a typed settings service and a render layer, with stock modal dispatch reduced to a thin event adapter. This document designs the engine and the part of the tree it consumes. Widgets, the settings service (#127, `replacement/property_service`) and the render layer are consumers or providers at the engine's boundary, not part of this unit.

Acceptance, from #128: for the observed presses (E-MENU-ENGINE-OBSERVE-150) and the picker pairs (E-MODAL75-PICKER-PAIRS-150), the owned engine produces the same globals, snapshots, event-`0x1d` posts and ordered settings writes as stock, and render-compares equal.

## The stock engine this replaces

Facts from E-MENU-ENGINE-OBSERVE-150, `reports/recipe-bank-ui-ownership.md#stock-menu-engine-and-the-owned-target` and the review of revision 1.

- Events cross one fixed block `0x0094f64c..0x0094f660` written by `0x0063270c`: code, kind, node, id, extra. Stage events have code `0x1e` and are posted through `0xd3d858(kind, id)`.
- Four stage consumers call `0xd4dd2c`: `0xca4674` (controller 100), `0xca0080`, `0xca4e98` and `0xcc45ac`. `0xca4674` saves the 0x430-byte block (`0x0061d384`), calls `0xd4dd2c`, restores the block (`0x0061d3a8`) when the engine returns 0, then runs the special post `0xd42a64` and, for kinds 4, 5 and `0xe`, the value post of event `0x1d`. `0xd4dd2c` does work of its own before and after the engine (`0xd4d7a4`, `0x0063807c`, copies of `0x944580` and the event block).
- `0xd4dd2c` calls the engine wrapper `0xbf95c8`, which calls the kind switch `0xbfa234` and then the snapshot store `0x0061ceac(*0x009571d0)`, also when the switch returns 0. `0xbfa800` (open and re-open, reached by kinds 1, `0x13` and every push and pop through `0xbfab98`) also calls `0xbfa234` directly.
- Kinds (`0xbfa234`, all.c:331301): 1 open, 2 close, 4, 5 and `0x45`..`0x48` move, 6..`0xb` a move wrapped in a pop and push, `0xe`/`0xf` push and pop, `0x10`..`0x12` multi-level push and pop, `0x13` re-open. Each kind branches further on its arguments and node flags: a move whose id equals the node's rows per page is a page move; push and pop with ids `0x17`/`0x18` seek through `0xbfad04`; a parent of type 5 or 7 changes push and pop; kind 1 on an empty node recurses into kind `0xf`. Kinds 1, 4, `0xe` and `0xf` were observed, each in one argument class.
- Engine state, the block `[0x009571d0,0x00957600)`: open node pointer `0x009571d0`; node word 10 (`*(u16 *)(record + 0x28)`, written by `0xbf976c`) at `0x009571d4`; cursor `0x009571d8`; visible ordinal `0x009571dc`; visible count `0x009571e0`; scroll top `0x009571e4`; total rows `0x009571e8`; visible count at open `0x009571ec`; row states `0x009571f0[0x4f]` (0 offered, 1 variant, 2 disabled, 3 hidden); saved cursor, ordinal and count from `0x0095732c`; remembered rows `0x0095733c[node]`; cursor stack `0x00957554` indexed by depth `0x009575f4`, with the visible count saved on pop at `0x0095757c`; the push classifier save `0x009575f8` (`0x0061d0e4`); menu-open flag `0x009575fc`.
- Outside the block, the move `0xbf9f44` stores the moved row's class and token at `0x01467580`/`0x01467584` (all.c:331291); the special post `0xd42a64` posts `0x1d` from them for kinds 8..`0xb`, and `0xcc453c` reads them.
- Snapshot stores: `0xbf9f44` at the event's node index, `0xbfab98` at each new record id at each depth it passes, `0xbf95c8` at `*0x009571d0`. Areas `0x00944414` (root), `0x0093eb74+0x2c*id` (depth 1), `0x009412d4+0x2c*id` (depth 2). Registry index and record id differ for some nodes (`0x78fdd4` is registry 82 with id 63).
- Initial row: `0xbfa800` sets the cursor from `0xd588b8(kind, node, class)` (all.c:331505); its default arm reads the property's current value (`0x0061c93c`) and converts it to a row token (`0x0061d514`), and its class-5 arm reads the block.
- The engine calls `0xd5b350` on open, close, move, push and pop (which calls `0x00637ed0`), `0xd52684` for row states, and `0xbf9a5c` writes property `0x285` through `0x0061c91c` for node `0x7a` (all.c:330959).
- Writers of the block outside the engine: the rollback bracket `0x0061d384`/`0x0061d3a8`; the commit and restore pair `0x006364e8`/`0x006364a4` over the copy at `0x0096b58c`, called from controller actions (all.c:364176-364261, 463715); the initialisers `0x0061cf14`, `0x0061cf84` and `0x0061d038`.
- Draw code reads 44-byte copies through `0x0061cd14` (live) or `0x0061ce1c` (snapshots), not the globals. The redraw `0xd4c8d4` calls `0xbf9a2c` through `0xbf95a8`.

## Engine boundary

The engine is a `no_std` Rust library whose core holds no stock addresses. Its one operation is a pure transition.

```text
apply(state: EngineState, event: MenuEvent, services: &mut impl EngineServices) -> Transition
Transition { result: EngineResult, state: EngineState, effects: [Effect] }
```

- `MenuEvent` carries every argument a kind uses: `node: NodeIndex`, `id: StageId`, `extra: StageExtra`, decoded into typed variants (`Open`, `Close`, `Move { direction, step }`, `MoveInFrame`, `Push`, `Pop`, `PushPop { levels }`, `Reopen`), each keeping node, id and extra as the kind reads them. A move's `step` distinguishes a row step from a page step (id equal to rows per page).
- `EngineResult` is `Accepted` or `Rejected`, the value `0xd4dd2c`, the bracket, `0xd42a64` and the value post branch on.
- `EngineState` is the typed form of the block plus the last-move pair `0x01467580`/`0x01467584`: `NodeIndex`, `NodeWord10`, `RowIndex`, `VisibleOrdinal`, `RowCount`, `ScrollTop`, `RowState` per row, the saved triple, the remembered-row table, a `Frame` stack with the pop-saved visible count and push-saved classifier, `MenuOpen`, and `LastMove { class, token }`. The push-saved classifier is computed by the engine from the in-progress parent state, as `0xbfab98` evaluates `0x0061d0e4` after `0xbf9a5c` and before switching nodes (all.c:331566), using the classifier port in `owned_menu::frame`.
- `Effect` is an ordered list: `StoreSnapshot { area, slot, record }` carrying the 44-byte record of the state at that step, `Notify(0xd5b350 arguments)` and `WriteProperty { property, value }` (the `0x285` write). The adapter performs them in order; a snapshot store writes the carried record through the owned frame service, because the stock store `0x0061ceac` reads the live block and would record the final state.
- `EngineServices` is the trait for what the engine asks stock: `availability(state, node) -> [RowState]` (`0xd52684`) and `initial_row(state, kind, node, class) -> Option<RowToken>` (`0xd588b8`, which returns `0x7ffffffe` for no selection, after which `0xbfa800` keeps the restored cursor; all.c:331506, 550703). Each takes the engine's in-progress state, which the adapter writes to the block before calling the stock function, since both read it.
- `Tree` is generated from the decoded menu tables (E-MODAL-MENU-TABLES-150) by a new generator next to `MenuTablesMain.hs`, pinned to the image4 and strings identities. A node carries id, type, class or property, rows per page, `no_wrap` (`+0x10`), `no_availability` (`+0x14`), `remember`, parent, word 10, rows `{token, child}` and labels. Stock record addresses are not in the tree: the generator emits a separate `NodeIndex` to record address table that only the decode and mirror shim reads.
- The tree and engine types are unconditional modules of the bank crate; only the adapter, its seam assembly and its publication are behind `rb_menu_engine_owned`. The node and token newtypes now repeated in `owned_menu`, `recipe_ui` and `modal_runtime` move to the engine's module.

## Placement in the guest

The adapter hooks the engine wrapper `0xbf95c8`, reached by all four stage consumers through `0xd4dd2c`, so stage events from stock and owned key actions both reach it.

- Seam: one published branch at `0xbf95c8`, built like the controller-75 seam (`replacement/modal_runtime/src/entry.S`, one RAM-push publication with rollback).
- On entry the adapter decodes `EngineState` from the block and the last-move pair. The block stays the source of truth while stock code also writes it (the bracket restore, `0x006364a4`, the initialisers, fallback kinds), so every event starts from what stock last left there.
- Coverage is by event class, not kind: a class is (kind, argument class, node flags, node-specific branches). Node flags are `no_wrap`, `no_availability` and `remember`; node-specific branches are the parent-id comparison in `0xbf9a5c`, the `0x285` write for node `0x7a`, the `0xbfad04` case for node `0x38` with class `0x228` (which writes `0x009573e4 = 0x9e`) and a parent of type 5 or 7. For example: kind 4 with a row step on a node with all flags 0 and no node-specific branch. The adapter runs `apply` only for classes with an oracle case (below); any other class runs the stock kind switch and the stock snapshot store, exactly as `0xbf95c8` did.
- After `apply`, the adapter writes the whole block and the last-move pair from the returned state, then performs the effects in order, then returns the result to `0xd4dd2c`.
- Retirement: nothing is retired while any fallback class reaches the stock engine functions, and every function under `0xbfa234` is reachable from some fallback class today. `0xbf9a2c` is a retained interface (the redraw calls it). Instead of a retired-fetch watch, the run's checker checks owned coverage directly: it derives each event's class from the traced event registers and the generated tree, independently of the adapter, and fails the run on any entry to `0xbfa234` from the adapter's fallback call for an owned class. The retired set grows when a function's last fallback class becomes owned; a reference scan over image4 and image5 (pointer tables, instruction-built addresses) must then show no other caller.

## Verification

- Host decode and mirror round trip: decoding each of the eight before and after images in the four receipts and mirroring it back gives the same 0x430 bytes.
- Host oracle per event class: the receipt's before block and event, through `apply` with services answered from the receipt, must give the recorded after block byte for byte, the recorded writes to `0x01467580`/`0x01467584`, every recorded snapshot word write in order (the receipts' watch over `[0x0093eb74,0x00944540)`), the `0x285` write when present, and the recorded result (the special post's presence). Receipts so far record neither `0xd588b8`'s nor `0xd52684`'s return value or the last-move pair, so new `--menu-observe` receipts add trace sites at the returns of `0xd588b8` and `0xd52684` and a watch on `0x01467580..0x01467588`. The four existing receipts are re-observed with those additions before they serve as cases.
- Negative controls: deliberately wrong engines must fail the oracle: one that ignores `no_wrap`, one with the skip rule inverted, one without initial-row selection, one that drops the last-move pair. A picker case whose current value is not at the restored row is needed for the initial-row control to discriminate.
- Guest pairs: the 20 picker trajectories (except picker 246 while its stall is open) and the `--menu-observe` paths run once per role, stock against the owned-engine build, under the modal lifetime guard. Each segment reads the block, the last-move pair and the three snapshot areas; event posts are counted with a trace site on `0x0063270c` (r0 = `0x1d`). Comparison: those reads equal, the `0x1d` post sequence equal, `modal-settings-compare` equal (it already covers the `0x285` write and the settings copies), render-compare content equal, and no fallback entry for an owned class.

## Shims for this unit

| Shim | What it does | Removal condition |
| --- | --- | --- |
| Engine-entry adapter at `0xbf95c8` | Decodes stage events from `0xd4dd2c` into `MenuEvent` and returns `EngineResult` | The owned key layer and owned stage consumers call the engine directly |
| Stock stage consumers `0xca4674`, `0xca0080`, `0xca4e98`, `0xcc45ac` | Post events and handle the result: bracket, special post (leave post `0x27d`, last-move posts), value post `0x1d` | The engine reports value changes and leaves to the typed settings service and owned consumers decide the result |
| `0xd4dd2c` work around the engine | `0xd4d7a4`, `0x0063807c`, copies of `0x944580` and the event block | Owned stage consumers |
| Block as source of truth | Decode from and mirror to the 0x430-byte block and `0x01467580/84` on every event | No stock code writes or reads the block: owned bracket, owned commit and restore, owned initialisers, no fallback classes, owned readers |
| Stock commit and restore `0x006364e8`/`0x006364a4` | Copies the block to and from `0x0096b58c` for controller actions | Those controller actions are owned |
| Stock node records as identity | The mirror publishes record addresses from the side table | Every reader of `0x009571d0` and the snapshots is owned |
| Initial row `0xd588b8` | Places the cursor on open, push and pop from the property's value | The tree and the settings service give the initial row |
| Stock availability `0xd52684` | Row states | The tree declares availability |
| Notification `0xd5b350` | Effect the adapter performs | Its consumers are owned |
| Engine property write `0x285` | Effect performed through `0x0061c91c` | The settings service owns property `0x285` |
| Fallback classes through the stock kind switch | Runs event classes without an oracle case | Every class is owned |
| Stock redraw and render path | Redraw requests after the engine | The render layer owns menu drawing |
| Stock settings reload and restore | Copies around each property write and on BACK | The typed settings service owns the image and its backup |
| Picker key cells (frozen) | The seven owned controller-75 cells | The owned key layer replaces them |

## Increments

1. Host: the tree generator with the record side table, the engine library, decode and mirror, and the oracle with negative controls, for the classes with receipts: kind 4 and kind 5 row steps, push, pop and open. Before that, one `--menu-observe` run per existing case with the added return sites and the last-move watch, so that each case records initial row, availability and the last-move pair, plus one picker case whose current value is not at the restored row, which the initial-row control needs. Reviewed before any guest code.
2. Guest: the `0xbf95c8` seam, the adapter with class coverage and fallback, the fallback-entry guard check, and the guest pairs. Reviewed before runs.
3. Observations and classes: `--menu-observe` runs for page moves (kind 4 and 5 page steps and kinds 6..`0xb`), no-wrap and no-availability nodes, seeks (ids `0x17`/`0x18`), multi-level moves, re-open and an empty node, each followed by its oracle case and the class's ownership.

## Open questions

- Whether the rollback bracket restores in practice, and for which events.
- The meaning of property `0x27d` in the leave post.
- The cause of picker 246's stall after DOWN on the current emulator model (E-MODAL75-PICKER-PAIRS-150).
