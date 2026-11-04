# Owned menu engine: design (#128)

Status: design for review. No owned engine code exists yet. Facts cite the ledger or `private-data/analysis/x-m1-1.50/filmsim/ghidra/all.c`; **[INFERENCE]** marks what the evidence does not show directly.

## Target and acceptance

The menu subsystem's target is the one #128 names: a declarative menu tree, one owned menu engine, the nine generic widgets, a typed settings service and a render layer, with stock modal dispatch reduced to a thin event adapter. This document designs the engine and the part of the tree it consumes. Widgets, the settings service (#127, `replacement/property_service`) and the render layer are consumers or providers at the engine's boundary, not part of this unit.

Acceptance, from #128: for the observed presses (E-MENU-ENGINE-OBSERVE-150) and the picker pairs (E-MODAL75-PICKER-PAIRS-150), the owned engine produces the same globals, snapshots, event-`0x1d` posts and ordered settings writes as stock, and render-compares equal.

## The stock engine this replaces

All facts in this section are from E-MENU-ENGINE-OBSERVE-150 and `reports/recipe-bank-ui-ownership.md#stock-menu-engine-and-the-owned-target`.

- Events cross one fixed block `0x0094f64c..0x0094f660` written by `0x0063270c`: code, kind, node, id, extra. Stage events have code `0x1e` and are posted through `0xd3d858(kind, id)`.
- Controller 100's action `0xca4674` consumes the block. It saves the 0x430-byte state block (`0x0061d384`), calls `0xd4dd2c` → engine wrapper `0xbf95c8` → kind switch `0xbfa234` and then `0x0061ceac` (snapshot store), restores the block (`0x0061d3a8`) when the engine returns 0, then runs the special post `0xd42a64` and, for kinds 4, 5 and `0xe`, the value post of event `0x1d` and controller 30.
- Engine state is the block `[0x009571d0,0x00957600)`: open node pointer `0x009571d0`, `0x009571d4` (role not established), cursor `0x009571d8`, visible ordinal `0x009571dc`, visible count `0x009571e0`, scroll top `0x009571e4`, total rows `0x009571e8`, visible count at open `0x009571ec`, row states `0x009571f0[0x4f]` (0 offered, 1 variant, 2 disabled, 3 hidden), saved cursor, ordinal and count from `0x0095732c`, remembered rows `0x0095733c[node]`, cursor stack `0x00957554` indexed by depth `0x009575f4`, menu-open flag `0x009575fc`.
- Kinds (`0xbfa234`, all.c:331301): 1 open, 2 close, 4, 5 and `0x45`..`0x48` move, 6..`0xb` a move wrapped in a pop and push, `0xe`/`0xf` push and pop, `0x10`..`0x12` multi-level push and pop, `0x13` re-open. Kinds 1, 4, `0xe` and `0xf` were observed; the others are read from code only.
- The step `0xbf9b1c` (all.c:330967) wraps unless node word 4 (`+0x10`) is 1 and skips row states 2 and 3 unless node word 5 (`+0x14`) is 1.
- Draw code reads 44-byte copies of the state through `0x0061cd14` (live) or `0x0061ce1c` (snapshots at `0x00944414`, `0x0093eb74+0x2c*node`, `0x009412d4+0x2c*node`), not the globals. The snapshot service is already ported, unwired, as `replacement/owned_menu` module `frame`.
- Writers of the block in the four observed presses were all in `0x00bf977c..0x00bfacb0` plus the clear `0x007221c8`; statically `0x0061d384`/`0x0061d3a8` (rollback bracket) and `0x0061cf14` (clear) also write it.

## Engine boundary

The engine is a `no_std` Rust library with no stock addresses in its core. It takes typed events, reads the declarative tree and a row-availability service, and produces typed state plus effects. Everything stock-specific sits in adapters around it.

```text
            stage event (kind, node, id, extra)
stock 0xca4674 ──► engine-entry adapter (0xbf95c8) ──► Engine::apply(MenuEvent) ──► EngineState
                                                         │            │
                                       Tree (generated)  │            ├─► Effects { snapshot, notify, value_changed }
                                       Availability ◄────┘            └─► state mirror ──► 0x9571d0 block, snapshots
```

- `MenuEvent` is a closed enum decoded from `(kind, node, id, extra)` at the adapter: `Open { node }`, `Close { id }`, `Move { direction, extra }`, `MoveInFrame { direction, pop_first: bool }` for 6..`0xb`, `Push { id }`, `Pop { id }`, `PushPop { levels }` for `0x10`..`0x12`, `Reopen`. A kind outside this set, or one not yet owned, decodes to `Unhandled(kind)`, and the adapter runs the stock kind switch for it.
- `EngineState` is typed: `NodeRef` (generated tree index), `RowIndex`, `VisibleOrdinal`, `RowCount`, `ScrollTop`, `RowState` (closed enum of the four states), a fixed-depth `Frame` stack, a `MenuOpen` flag and a remembered-row table indexed by `NodeId`. It is not stored in the stock layout; the mirror writes that layout.
- `Tree` is a static generated from the decoded menu tables (E-MODAL-MENU-TABLES-150) by a new generator next to `MenuTablesMain.hs`, pinned to the image4 and strings identities. Each node carries its id, type, class or property, rows per page, `no_wrap`, `no_availability`, `remember`, parent, rows `{token, child}` and labels, plus the stock record address the mirror must publish. The three node and token newtype families now spread over `owned_menu`, `recipe_ui` and `modal_runtime` collapse into the engine's.
- `Availability` is a trait the engine calls on open, push, pop and re-open to fill row states. Its first implementation calls stock `0xd52684` (shim below); the declared implementation comes when the tree carries availability rules.

## Placement in the guest

The adapter hooks the engine wrapper `0xbf95c8`, the single entry for every stage event whether a stock or an owned key action posted it. This is the report's "Stage-event adapter" placement (b). Placement (a), calling the engine from the owned picker cells, would cover only the frozen picker cells and leave stock-posted events on the stock engine.

- Seam: one published branch at `0xbf95c8`, built like the controller-75 seam (`replacement/modal_runtime/src/entry.S`, one RAM-push publication with rollback).
- Retired code, the end state: the kind switch `0xbfa234` and the engine functions it reaches that write the block (`0xbf976c`, `0xbf985c`, `0xbf988c`, `0xbf9a2c`, `0xbf9a5c`, `0xbf9b1c`, `0xbf9c1c`, `0xbf9f44`, `0xbfa800`, `0xbfab98`, `0xbfad04` and their extents). While some kinds still fall back to stock (below), only functions that no fallback kind reaches are retired. Each retired extent becomes a guard fetch watch, as the controller-75 generic bodies are. Before retirement, a reference scan over image4 and image5 (pointer tables, instruction-built addresses) must show no caller outside the replaced extent set; any other caller becomes a retained interface or blocks retirement of that function.
- Retained around the engine: `0xca4674` (with the rollback bracket, special post and value post), `0xd4dd2c`, `0x0061ceac`, `0xd52684`, the renderer and the commit path. Each is a shim below.
- Build: a `rb_menu_engine_owned` cfg in the bank crate, independent of `rb_modal75_owned`, so that the engine runs against both the stock controller 75 and the owned one.

## The stock-state mirror

Stock code outside the engine reads the block and the snapshots: draw code through `0x0061cd14`/`0x0061ce1c`, the frame accessors `0x61d154`..`0x61d334`, `0xca4674`'s rollback bracket, and controller actions that read the cursor or node. Until each reader is owned, the engine writes the stock layout after every event.

- The mirror writes the whole block image the engine's state implies, in one pass, at the end of `apply`, then calls the snapshot store as `0xbf95c8` does. Writing only changed words would let a stale stock value survive where the owned state has none; a whole-image write makes the block a function of the owned state.
- The open node pointer must be the stock record address, since stock readers dereference it. The tree carries it per node.
- `0x009571d4` and the row-state and saved fields not yet explained are mirrored from the values the decoded semantics produce; the host oracle below compares the whole 0x430 bytes, so an unexplained word that differs fails the oracle instead of passing unnoticed.

## Verification

- Host oracle, before any guest run. The four `--menu-observe` receipts hold the 0x430-byte block before and after one press and the event block arguments (E-MENU-ENGINE-OBSERVE-150). A host test decodes the before block into `EngineState`, applies the event with an availability fake that answers from the recorded after block's row states, and requires the mirrored after block to equal the recorded one byte for byte and the stored snapshot to equal the recorded snapshot words. Each new observation (page moves, no-wrap and no-availability nodes, dialogs) adds a case. A press with no receipt has no oracle and is not owned.
- Kind fallback: kinds without an oracle case decode to `Unhandled` and run the stock kind switch through the adapter. That keeps those kinds unretired, so the retired set grows only with oracle coverage.
- Guest pairs on the picker route. The 20 picker trajectories and the menu-observe paths run once per role, stock against the owned-engine build, under the modal lifetime guard. Each segment adds reads of the block and the three snapshot areas; event posts are counted with a trace site on `0x0063270c` (r0 = `0x1d`). Comparison: the block and snapshot reads equal, the `0x1d` post sequence equal, `modal-settings-compare` equal, render-compare content equal, and no fetch of a retired extent.
- Picker 246's open stall (E-MODAL75-PICKER-PAIRS-150) is excluded until its cause is known.

## Shims for this unit

| Shim | What it does | Removal condition |
| --- | --- | --- |
| Engine-entry adapter at `0xbf95c8` | Decodes stage events from stock `0xd4dd2c` into `MenuEvent` | The owned key layer and an owned stage consumer call the engine directly |
| Stock stage consumer `0xca4674` | Rollback bracket, special post (`0xd42a64`, leave post `0x27d`) and value post of `0x1d` around the engine | The engine reports value changes and leaves to the typed settings service and owns the bracket's decision |
| Stock-state mirror | Writes the 0x430-byte block and the snapshots from owned state | Every reader of the block and snapshots is owned |
| Stock node records as identity | The mirror publishes stock record addresses as the open node | Every reader of `0x009571d0` is owned |
| Stock availability `0xd52684` | Fills row states | The tree declares availability |
| Unhandled kinds through the stock kind switch | Runs kinds without an oracle case in stock code | Every kind has an oracle case and is owned |
| Stock redraw and render path | Redraw requests after the engine | The render layer owns menu drawing |
| Stock settings reload and restore | Copies around each property write and on BACK | The typed settings service owns the image and its backup |
| Picker key cells (frozen) | The seven owned controller-75 cells | The owned key layer replaces them |

## Increments

1. Host: the tree generator and its tests, the engine library for kinds 1, 4, 5, `0xe` and `0xf`, the mirror, and the host oracle over the four receipts. Reviewed before any guest code.
2. Guest: the `0xbf95c8` seam, the adapter with kind fallback, the reference scan and retired set for the owned kinds, and the guest pairs. Reviewed before runs.
3. Observations and kinds: `--menu-observe` runs for page moves (kinds 6..`0xb`), no-wrap and no-availability nodes, multi-level moves and re-open, each followed by its oracle case and the kind's ownership.

## Open questions

- The meaning of `0x009571d4`, node word 10 and the `0x0095757c` slot written on pop.
- Whether the rollback bracket ever restores in practice (the engine returning 0), and for which events.
- The meaning of property `0x27d` in the leave post.
- The cause of picker 246's stall after DOWN on the current emulator model.
