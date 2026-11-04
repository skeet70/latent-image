# Owned menu engine, increment 3: event classes (#128)

Status: design for increment 3 of [the engine design](owned-menu-engine-design.md), not reviewed. Nothing in it is built or run. Increments 1 and 2 own kind 4 and 5 row steps, kind `0xe` push, kind `0xf` pop and kind 1 open on wrapping nodes with queried availability (E-MENU-ENGINE-HOST-ORACLE-150, E-MENU-ENGINE-GUEST-PAIRS-150); every other class falls back to the stock kind switch. This document takes each remaining class in value order and gives its stock branch, its reach on the no-install route, the `--menu-observe` config that records it, the negative control its oracle case must fail, and the engine and checker changes that own it.

Stock facts cite `private-data/analysis/x-m1-1.50/filmsim/ghidra/all.c` (the export of the id5 image `private-data/analysis/x-m1-1.50/lz/id5.dis`), node facts the decoded tables `private-data/scratch/ControllerStrategy/menu-tables.json` (E-MODAL-MENU-TABLES-150), controller cells the manifest `private-data/scratch/ControllerManifest/manifest.json` (E-MODAL-CONTROLLER-MANIFEST-150), and reach the crawl-5 graph (E-MODAL-MENU-OKPATHS-150). **[INFERENCE]** marks what the evidence does not show directly.

## How reach was derived

Crawl-5 records, for each of its 4,584 trials, the from-screen's key path, the 44 bytes of the record at `*0x009571d0` and the cursor `0x009571d8`, the controller dispatches with controller, state and event, and every entry of `0x0063270c` with r0-r3. An entry with r0 = `0x1e` is a stage post (code, kind, node, id); the extra word is not recorded. All 570 screens' records match a decoded node. The first stage post of a trial was classified from the from-screen's record and cursor by the rules of `MenuEngineClass.deriveClass` and the branches below; a later post of the same trial only by its kind and arguments, because the block between posts is not recorded. Counts are trials.

A path is crawl-5's key path from the cardless no-install root to the from-screen, the root `--menu-observe` walks from (`replacement/README.md`, `--menu-observe CONFIG`); the observed key is that trial's key, and each config names its crawl-5 node and trial. Crawl-5 replayed from held snapshots and 7 of its 25 replays diverged, so a path is expected, not known, to reach the same screen from the root **[INFERENCE]**. The extractor change below refuses a receipt whose stage entries differ from the expected events in the table.

Stage posts reach the engine through two consumers in crawl-5: `0x00ca4674` (controller 100, 1,568 dispatches) and `0x00ca4e98` (controller 101, 1,795). Both save the block (`0x0061d384`), call `0xd4dd2c`, restore the block (`0x0061d3a8`) when it returns 0 and otherwise run the special post `0xd42a64` (all.c:457563-457633, 458265-458340). The increment-1 extractor accepts only `0x00ca4674`.

## Class table

Ordered by value: first what the 19 picker pairs and the shooting menu exercise, then by crawl-5 trial count. Crawl counts are first posts of a trial unless noted. Owned paths in crawl-5 are not listed.

| # | Class | Stock branch (all.c) | Crawl-5 trials | Reachable on keys | Path, key (crawl node, trial) | Expected stage event (kind, node, id) | Config `emulation/menu-oracle-*.json` | Negative control |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1a | Pop with the remember write (parent id differs) | 330948-330955, 331582-331605 | 208 | yes; 5 of the 7 guest-pair RememberBranch events | `ok up up up up up up up ok`, `back` (192, 1070) | `0xf, 0x1f, 0x1a` | `pop-remember` | `PopNoRemember` |
| 1b | Pop from a node with Remember off | 330945-330947 | 69 | yes; picker 312's pop | `ok up up up up ok down`, `back` (180, 1003) | `0xf, 0x1e, 0x1a` | `pop-remember-off` | `RememberIgnoresFlag` |
| 1c | Push from a node whose parent has its id | 330953-330955, 331562-331580 | 4 | yes; oracle-push-classifier's fallback push | `ok ok down down`, `ok` (4, 6) | `0xe, 0x1, 0x1d` | `push-same-parent` | `RememberIgnoresParentId` |
| 1d | Push from a node with Remember off | 330945-330947 | 49 | yes, shooting menu | `ok up up up up ok down`, `ok` (180, 980) | `0xe, 0x1e, 0x1d` | `push-remember-off` | `RememberIgnoresFlag` |
| 2 | Close (kind 2) | 331325-331335 | 230 posts | yes, shooting menu | `ok`, `back` (1, 1292) | `2, 0x1, 0x1d` | `close` | `CloseKeepsMenuOpen` |
| 3 | Kinds `0xa`, `0xb`: seek `0x17`, push, move, pop | 331371-331384, 331627-331670 | 31 each | yes, shooting menu (zero-step seek) and node `0xc` (one-step seek) | `ok up left up ok up up ok ok ok`, `up` / `down` (215, 4110 / 4109); `down down ok right ok`, `down` (240, 1323) | `0xa, 0x81, 0x1` / `0xb, 0x81, 0x1`; `0xa, 0xc, 0x1` with extra 1 | `frame-seek-down`, `frame-seek-up`, `frame-seek-step` | `FrameNotifyId`; `NoSeekStep` (step case) |
| 4 | Kinds 6, 7: pop, move, push | 331347-331358 | 70 / 95 | yes | `down`, `down` (146, 1295); `down down`, `up` (237, 1884) | `6, 0x5f, 0x1` / `7, 0x5f, 0x1` | `frame-pop-down`, `frame-pop-up` | `FrameNotifyId` |
| 5 | Fixed availability (node word 5 = 1): open and move | 331435-331438, 331518-331534, 331001 | 116 moves | yes | `ok up left down ok ok down down`, `right` (449, 3275); `ok up left down ok ok down down right`, `down` (450, 3277) | `6, 0x43, 0x2` / `4, 0x43, 0x1` | `frame-fixed-open`, `fixed-wrap` | `QueryFixed`; existing `NoWrap` |
| 6 | Node `0x7a`'s property `0x285` write | 330956-330960 | 30 | yes, close of the screen Q opens | `q down down`, `back` (254, 3217) | `2, 0x7a, 0x1d` | `close-property` | `NoPropertyWrite` |
| 7 | Kinds `0x45`..`0x48` | 331210-331213, 331268-331281, 331018-331180 | 15 each | yes, the screen Q opens | `q down down`, `left` / `right` / `up` / `down` (254, 3216 / 3161 / 3160 / 3159); wrap presses from nodes 262, 256, 259 (trials 3177, 3194, 3183, 3192) | `0x45`..`0x48`, `0x7a`, `0x1` | `group-left`, `group-right`, `group-up`, `group-down`, `group-right-wrap`, `group-left-wrap`, `group-down-wrap`, `group-up-wrap` | `GroupAsRowStep` |
| 8 | No-wrap node (node word 4 = 1) | 330980-331006 | 60 moves | yes | `ok left up ok down down down ok`, `right` / `left` (370, 2529 / 2600) | `4, 0x7d, 0x1` / `5, 0x7d, 0x1` (rejected) | `nowrap`, `nowrap-edge` | `AlwaysWrap` |
| 9 | Seek on open (kind 1, id `0x17`) | 331307-331323, 331649-331670 | 8 (second post) | yes; stays fallback (no case can fail `NoSeekStep`) | `ok left up ok down down down ok`, `back` (370, 2601), window 1 only | `2, 0x7d, 0x1a` (owned close), then `1, 0x3f, 0x17` (fallback) | `open-seek` | `CloseKeepsMenuOpen` (window 1) |
| 10 | Re-open (kind `0x13`) | 331408-331409, 331511-331548 | 1 | yes | `ok left up ok down ok down down ok ok`, `back` (363, 2393) | `0x13, 0x55, 0x1` | `reopen` | `ReopenNotifies` |
| 11 | Row step with id > 1, including the inner moves of kinds 6 and 7 (owned today without a case) | 331215-331266 | 86 | yes | `ok left up ok down ok ok ok down`, `down` / `up` (324, 2067 / 2208) | `4, 0x56, 0xd` / `5, 0x56, 0xd` | `step-rows`, `step-rows-wrap` | `SingleStep`; `NoPassStop` |
| 12 | Kinds 8, 9: push, move, pop | 331359-331370 | 0 | not reached by crawl-5; the key that sends their command codes is not identified | probe: `q down down`, `dial-right` (254, untried) | `8, 0x7a, 0x1` **[INFERENCE]** | `frame-push-probe` | `FrameNotifyId` |
| 13 | Page step (kinds 4, 5, id = rows per page) | 331201-331209, 331219-331266 | 0 | not reached by crawl-5 | none | none | none | none |
| 14 | Multi-level (kinds `0x10`..`0x12`) | 331395-331407 | 0 | not reached by crawl-5 | none | none | none | none |
| 15 | Empty node | 331459-331465 | 0 | not reached by crawl-5 | none | none | none | none |
| 16 | Restored cursor past the rows with no initial row (`token_past_rows`) | 331505-331509, 331545-331548 | 0 | not reached by crawl-5 | none | none | none | none |
| 17 | Kind `0xe` with id `0x17`; kind 1 with id `0x18`; kind 1 with ids `0xe`..`0x12` | 331385-331393, 331627-331670 | 0 | not reached by crawl-5; key-action senders exist, gated on states and properties crawl-5 did not reach | none | none | none | none |

Classes 1 to 8, 10 and 11 become owned with oracle cases; class 3's one-step seek is owned once `frame-seek-step` passes; class 9 stays fallback and its press gives an owned close case; 12 is owned only if its probe records a kind 8 event; 13 to 17 stay fallback (see "Classes that stay fallback"). "Not reached" means crawl-5 has no trial of the class; it does not mean no key can reach it.

## Changes shared by all classes

### Extractor (`MenuEngineCases.hs`)

- Accept a stage entry from either consumer body, `[0x00ca4674, 0x00ca4834)` or `[0x00ca4e98, 0x00ca5088)`, and take the special post and the rollback restore from the same body. Both consumers are byte-verified in the decomp (x70view `decomp/xm1-coverage` 55436fc85c, `app/ui/machines/ui_machine_actions_00ca4674.c:34, 69`).
- One case per stage entry. Window i runs from stage entry i to the first hit after its wrapper store (`frame-snapshot` with lr `0xbf95ec`). Its before block is the receipt's before block replayed through every block write before stage entry i; the refusal of a block or last-move write before the stage entry applies to window 1 only. Cases are named `NAME-i` when a receipt has more than one window.
- A window's result is decided only from hits between its wrapper store and the next stage entry, or the end of the receipt for the last window: a special post with lr in its consumer body there makes it accepted; otherwise a `rollback-restore` hit (`0x0061d3a8`) with lr in that body there makes it a case with `result: EngineResult::Rejected`, whose after block is the replay to the window's end, as for accepted cases; a window with neither is refused. A later window's special post never counts for an earlier one.
- Replace "replayed engine writes equal the recorded after block" by "replaying every block write of the receipt gives the recorded after block", which holds for rejected receipts whose restore rewrites the block.
- A new call `CallPropertyWrite property value flags` from `property-write` hits (`0x0061c91c`) with lr in the engine body, ordered with the other calls.
- Required sites gain `rollback-restore` (`0x0061d3a8`).
- `menu-engine-cases` takes `NAME=DIR:EVENT[;EVENT...]`, where an event is `KIND,NODE,ID` or `-KIND,NODE,ID`. It refuses a receipt whose stage entries' r0-r2 differ from the listed events, and emits no case for a window marked `-` (a fallback class recorded in the same press).

### Owner oracle (`menu_engine/src/owner.rs`)

- `Case.result` comes from the extractor; `Expected::PropertyWrite { property, value }` answers the new effect.
- Every control below is a `Rules` switch in `engine.rs`, listed in `owner_controls_fail`; each must fail at least one case. A case whose before state makes its control unable to fail (for example `remembered[index]` already equal to the token the write stores) does not count, and its path is changed until the control fails.

### Engine invariant for compound kinds

`apply` returns `NotOwned` only before its first service call. Kinds 6..`0xb` run several stock steps whose targets can depend on availability answers, so the engine checks every target such an event can reach before the first call:

- Kinds 6 and 7: every row child of the source's parent, since the inner move picks the page. The inner pop writes the live frame to `stack[depth]` and the inner push restores that slot (all.c:331591-331594, 331575-331578), so the push restores the live frame at entry; the pre-check bounds the live cursor, and the block's `stack[depth]` at entry is stale and not read.
- Kinds 8 and 9: the cursor row's child, the restored frame `stack[depth + 1]`, and the source as the pop target.
- Kinds `0xa` and `0xb`: the push runs after the seek, so its target is the child of the row the seek stops on. With the zero-step restriction of class 3 that is the cursor row's child; when the one-step seek is owned, every row's child is checked, plus `stack[depth + 1]` and the source.

The check is static over the tree and the block at entry. It can refuse an event stock would complete, never the reverse.

### Checker (`MenuEngineClass.hs`) and pairs

- `deriveClass` mirrors each class's ownership and the compound pre-checks above. `Fallback` loses `RememberBranch`, `PropertyNode` and `NodeFlags`, gains `SeekSteps` for kinds `0xa` and `0xb` whose cursor row's token differs from the extra word, and `KindNotOwned` stops covering kinds 2, 6, 7, `0xa`, `0xb`, `0x13` and `0x45`..`0x48`. The checker reads the extra word from r3 at the stage entry.
- `oracle.jq` turns each new config into a pair trajectory over crawl-5's landings, as for the ten increment-2 paths, and the pair runs repeat the 29 existing pairs. The stage-entry, kind-switch and seek sites of `picker.jq` are unchanged.
- The engine's static stack bound (808 bytes at increment 2) grows with the compound kinds; the pair check recomputes it from the relinked `kernel.elf` and fails a stage entry whose sp less the bound and the margin falls below task 1's stack base.

### Config base

Every config below uses the same 28 trace sites and six write ranges: the nine sites the extractor requires, `rollback-restore`, `stage-consumer-101` (`0x00ca4e98`), the engine functions `0xbf9f44`, `0xbf9b1c`, `0xbf988c`, `0xbfa800`, `0xbfab98`, `0xbfad04`, `0xbf9a5c`, `0xbf9a2c`, `0x0061d0e4`, the value and commit sites of the increment-1 configs, `rollback-save` and `property-write`; the group configs add `0xbf9c1c`. The write ranges are those of `emulation/menu-oracle-push-classifier.json` (snapshot areas, event block, `0x009503d0`, the engine block, `0x0096cc64`, the last-move pair). Key-specific action sites of the increment-1 configs (`down-action`) and the redraw sites are left out; the redraw and render comparison is the pair runs' job. The JSON blocks are in the appendix.

## Classes

### 1. RememberBranch: push and pop with the other `0xbf9a5c` branches

Stock. `0xbfab98` calls `0xbf9a5c(node)` before switching nodes on push (all.c:331562-331580) and pop (331582-331601). `0xbf9a5c` returns when no node is open or node word 6 (Remember) is not 1 (330942-330947); otherwise it stores the cursor row's token at `0x0095733c[node]` when the node has no parent, or when the parent's id differs from the node's (330948-330955). Increment 2 owns only a push whose write runs (parent id differs) and a pop whose write does not (parent id equal).

Value. All 7 RememberBranch events of the guest pairs: 5 pops in the unloaded-picker segments of pickers 26, 163, 214, 312 and 314, and in oracle-push-classifier one push with the same parent id and one pop with another parent id (E-MENU-ENGINE-GUEST-PAIRS-150; classified from each pair's stage-entry registers and block). Picker 312's pop is from node `0x1e` (record `0x0078c790`, Remember 0); the other four pop with the write. Crawl-5: 208 pops with the write (including `left` on the shooting root, `ok`, `left`), 69 pops and 49 pushes with Remember off, 4 pushes with the same parent id.

Paths. `pop-remember` pops node `0x1f` (record `0x0078cf64`, the node of picker 26's pop, cursor 1) to the shooting root. `pop-remember-off` pops node `0x1e` (`0x0078c790`, picker 312's node) from cursor 1 (crawl node 180, trial 1003), and `push-remember-off` pushes from the same screen into `0x0077dd34` (trial 980). `push-same-parent` pushes from `0x00783de0` (id 1, parent `0x0078ea98` with id 1, cursor 20) into `0x0077c890`, the push oracle-push-classifier falls back on.

Controls. `PopNoRemember` (a pop never writes) fails `pop-remember`; `RememberIgnoresFlag` (writes with Remember 0) fails `pop-remember-off` and `push-remember-off`; `RememberIgnoresParentId` (writes when the parent has the node's id) fails `push-same-parent`. Each needs `remembered[node]` before the event to differ from the token written. For node `0x1e` that holds at cursor 1 and not at cursor 0: `0x0061cf14` sets `remembered[i]` to the row-0 token of registry entry i (id4.dis 61cf14-61cf64), registry `0x1e` is `0x0078c790` with row-0 token 0 and row-1 token 1, and no `0xbf9a5c(0x1e)` writes the slot, because `0x0078c790` has Remember 0 and its two children have parent id `0x1e`. For the other two cases the receipt decides; a case whose control does not fail is replaced as the shared rules say.

Engine. Remove the two gates in `push_pop`; `remember()` already has all four branches. Remove `NotOwned::RememberBranch`. `stage_index` keeps refusing node `0x7a` until class 6 lands.

Checker. `pushPop`, `push` and `pop` lose their Remember and parent-id conditions.

### 2. Close (kind 2)

Stock. For id `0x1a`..`0x1d`: notify `0xd5b350(2, node, id, class of the open node)`, `0xbf9a5c(node)`, menu-open flag `0x009575fc` = 0, result 1; any other id gives 0 with no call (all.c:331325-331335). The wrapper then stores the snapshot at the open node's id as for every kind.

Value. Crawl-5 has 230 close posts, ids `0x1d` (169 first posts), `0x1a` (23), `0x1b` (2, each followed by a kind 1 open of node 0 in the same press). `back` on the shooting root closes it (node 1, id `0x1d`).

Path. `close`: `ok`, then `back`.

Control. `CloseKeepsMenuOpen` leaves `0x009575fc` at 1.

Engine. `MenuEvent::Close` gets an arm: refuse with `NoOpenNode` when no node is open and `StageNode` for a node at or above `0x86`; for an id outside `0x1a..=0x1d` return `Rejected` with no call; otherwise notify, `remember(index)`, clear `menu_open`, `Accepted`. Close on node `0x7a` is class 6.

Checker. Kind 2 is owned for an open node and a node below `0x86` other than `0x7a`.

### 3. Kinds `0xa` and `0xb`: seek, push, move, pop

Stock. `0xbfad04(kind, node, type of the open node, 0x1d, 0x17, extra)`, then `0xbfab98(0xe, node, 0x15, 0)`, `0xbf9f44(4 or 5, node, id)` and `0xbfab98(0xf, node, 0x15, 0)`, each run only when the previous returned 1 (all.c:331371-331384). In `0xbfad04` kinds 1, `0xa`, `0xb` and `0xe` (mask `0x4c02`) take the switch on the seek id (331627); id `0x17` steps the cursor down with `0xbf9f44(4, node, 1)` until the cursor row's token equals `extra`, at most total-rows times (331649-331670), and id `0x18` pushes when it is found. An open node of type 5 or 7 pushes instead (331620-331625).

Value. Shooting menu: node `0x81` (record `0x0078cf38`, three rows whose children `0x007912c0`, `0x0078ecac`, `0x00789f24` have 10 rows each), under node `0x80`, under the shooting root, with controller 75 in state 11. Node `0xc` (`0x0078231c`, row tokens 0 and 1) with controller 75 in state 9. Crawl-5: 31 trials each.

Extra word. The posters call `0xd3d81c(kind, id, extra)`, which posts `(0x1e, kind, node id, id, extra)` (all.c:534853-534860). In state 11, `up` is controller 75 slot 8 (`FUN_00c6e5a8`) and `down` slot 26 (`FUN_00c6ecb4`), which pass `0x0061d244()`, the cursor row's token, as the extra word (decomp `ui_m75_action_8`, byte-verified, `app/ui/machines/ui_m75_actions_c6e378.c:127`; id4.dis 61d244-61d290), so `0xbfad04`'s loop matches on its first comparison and makes no step. In state 9 the slots post constants (all byte-verified in the same decomp): `up` slot 7 `(0xb, 1, 1)` (`ui_m75_actions_c6e378.c:113`), `down` slot 25 `(0xa, 1, 1)` (`:383`), `left` slot 44 `(0xa, 1, 0)` (`ui_m75_actions_c6f814.c:29`), `right` slot 73 `(0xb, 1, 0)` (all.c:420574). On node 240 at cursor 0, `down` (trial 1323) seeks token 1 from token 0, one step, and crawl-5 lands at cursor 1.

Paths. `frame-seek-down` and `frame-seek-up` from node 215 (zero-step); `frame-seek-step` from node 240 (one step).

Controls. `FrameNotifyId` (the inner notifications pass the event id instead of 0) fails the zero-step cases. `NoSeekStep` (skip the seek loop) cannot fail a zero-step case; it must fail `frame-seek-step`.

Ownership. This increment owns the zero-step seek only: `apply` refuses with `NotOwned::SeekSteps` before any call when the open node has no values or `values[cursor].token != extra`. The one-step seek is owned when `frame-seek-step`'s case passes and `NoSeekStep` fails it; the pre-check then widens to the targets listed under the compound invariant.

Engine. `MenuEvent::FrameMove` for kinds `0xa` and `0xb`: the seek on an open node of type other than 5 and 7 (refuse `FramedNode` otherwise), then an owned `0xbfab98` step without `0xbfad04` (`push_pop` split into the node switch plus `open_rows`, and the `0xbfad04` tail), `move_rows`, and the pop. The inner steps pass notify id 0 (the fourth argument of `0xbfab98`, 331374-331383), and `open_rows` takes `0xbfab98`'s third argument as the direction of its step. Each step's 0 result ends the event with `Rejected`; the steps already made stand and the consumer's rollback restores the block.

Checker. Kinds `0xa`, `0xb` owned under the compound pre-check.

### 4. Kinds 6 and 7: pop, move, push

Stock. Kind 6: `0xbfab98(0xf, node, 0x15, 0)`, `0xbf9f44(4, node, id)`, `0xbfab98(0xe, node, 0x15, 0)`; kind 7 the same with the pop's direction `0x16` and move kind 5 (all.c:331347-331358). No `0xbfad04` runs, so the parent of type 5 or 7 that these kinds move in is not re-pushed.

Value. The framed groups: node `0x5f` (type 7, record `0x00787cbc`, six pages of type 7, opened from crawl-5's root screen by `down` as a kind 1 open of a type 7 node), and node `0x43` (type 5, `0x00783d94`, no-wrap, six pages, four of them with fixed availability, class 5 below). Crawl-5: kind 6 70 trials, kind 7 95, ids 1, 2 and 3. Ids 2 and 3 make the inner `0xbf9f44` take several steps, which class 11 owns.

Paths. `frame-pop-down` moves from the one-row page `0x00791294` to `0x0077d59c` (cursor 2 after). `frame-pop-up` moves from `0x0077d59c` at cursor 2 back into the one-row page `0x00791294`, so `0xbfa800` opens a one-row node with restored cursor 2. Had `0xd588b8` returned `0x7ffffffe`, the cursor would stay at 2, whose row state is 3, and `0xbf9b1c` returns 0 for a total of 1 (all.c:330974-330977), so the push and the event would fail; crawl-5 shows the event landing on `0x00791294` at cursor 0, so `0xd588b8` returned a row token and this case does not reach class 16.

Control. `FrameNotifyId` passes the event id instead of 0 to the inner notifications.

Engine. As class 3 without the seek; the pop's direction is `Direction::Up` for kind 7. Inner steps may open nodes of type 5 or 7, since `0xbfad04` does not follow them: `check_open` loses `FramedNode` for these steps and keeps it where `0xbfad04` follows.

Checker. Kinds 6 and 7 owned under the compound pre-check.

### 5. Fixed availability (node word 5 = 1)

Stock. `0xbfa800` on such a node sets the total, clears the row states with `FUN_007221a0(0x009571f0, 0, 0x4f)`, leaves the visible count at open `0x009571ec` unwritten (331436-331437; the queried arm writes it at 331502), queries no availability, has no empty arm (331435-331438) and keeps the initial or restored cursor without a step (331514-331534). The step `0xbf9b1c` accepts every row (331001) and the count and ordinal functions get no row-state array (`0xbf985c`, `0xbf9a2c`).

Value. 16 nodes have node word 5 = 1: four of framed group `0x43`'s six pages (`0x00784afc`, `0x00789b8c`, `0x00790790`, `0x0078d2fc`), all eight children of node `0x7d` and four children of node `0x7e`. Crawl-5 has 116 moves on `0x00784afc` and `0x00789b8c`, and kinds 6 and 7 enter them.

Paths. `frame-fixed-open`: kind 6 with id 2 from `0x0078ecd8` (cursor 2) into `0x00789b8c`, which opens a fixed node and moves the no-wrap parent two pages. `fixed-wrap`: kind 4 at the last row (11) of `0x00789b8c`, which wraps to 0.

Controls. `QueryFixed` asks availability on a fixed node and fails `frame-fixed-open`; the existing `NoWrap` fails `fixed-wrap`.

Engine. `move_rows` and `check_open` lose the availability condition; `open_rows` gets the fixed arm. `0x007221a0` is `fill_u32(destination, value, count)`, byte-verified, storing `count` words (x70view `decomp/xm1-coverage` 55436fc85c, `app/runtime/byte_strings.c:187`), so the fixed arm sets all `0x4f` row states to 0 and leaves `visible_at_open` as it was.

Checker. `wrapsAndQueried` loses its availability half.

### 6. Node `0x7a`: property `0x285`

Stock. After the remember branches, `0xbf9a5c(0x7a)` writes property `0x285` through `0x0061c91c(0x285, 0xbf9a2c(0x0095733c[0x7a], 0), 0)`, the row index of the remembered token in the open node (all.c:330956-330960). It runs on close, push and pop with node `0x7a`.

Value. Node `0x7a` is registry slot `0x7a`, record `0x0078e4a0` (16 rows, Remember 1, no parent), opened by `q` on crawl-5's root screen (trial 1934 posted `1, 0x7a, 0`). Crawl-5: 30 closes of node `0x7a` (`back` or `ok`), no push or pop with it.

Path. `close-property`: `q`, `down`, `down` (two kind `0x48` moves to cursor 9), then `back`.

Control. `NoPropertyWrite` omits the effect. A control that writes the cursor's row index instead does not discriminate here, because the remember write just stored the cursor's token.

Engine. `Effect::PropertyWrite { property: PropertyId::new(0x285), value }` performed through `0x0061c91c` at stock's point; the shim "Engine property write `0x285`" of the design takes effect. `stage_index` stops refusing `0x7a`. Design line 51 ("No owned class has the `0x285` write") is replaced.

Checker. `PropertyNode` is removed.

### 7. Kinds `0x45`..`0x48`

Stock. `0xbf9f44` calls `0xbf9c1c(0x45, 0x16)`, `(0x46, 0x15)`, `(0x47, 0x16)` or `(0x48, 0x15)` (all.c:331210-331213, 331268-331281). Kinds `0x47` and `0x48` step one row at a time with wrap, count rows not hidden, and accept at the fourth when its state is below 2, at most total-1 steps (331034-331072). Kinds `0x45` and `0x46` move within a group of four by the row's visible ordinal modulo 4 (331074-331180). On success the shared tail scrolls with `0xbf988c(..., kind)`, which for these kinds keeps the top unless the total is below rows per page (330827-330878), notifies, writes the last-move pair and stores (331283-331296).

Value. The screen Q opens (node `0x7a`, 16 rows, controller 75 state 43): `left` `0x45`, `right` `0x46`, `up` `0x47`, `down` `0x48`. From cursor 9 crawl-5 lands at 8, 10, 5 and 13.

Paths. `group-left`, `group-right`, `group-up`, `group-down`, all from node 254 (cursor 9; crawl-5 lands at 8, 10, 5, 13), stay inside the rows and inside a group of four. The wrap set takes the other code: `group-right-wrap` from cursor 7 (node 262, lands at 4), `group-left-wrap` and `group-up-wrap` from cursor 4 (node 256, land at 7 and 12), `group-down-wrap` from cursor 12 (node 259, lands at 4, past row 0, which no crawl-5 screen of this node has as its cursor).

Control. `GroupAsRowStep` runs a kind 4 or 5 one-row step.

Engine. `MenuEvent::GroupMove` gets the arms of `0xbf9c1c` and the move tail with a new `ScrollRule::Keep`. The arms read row states, so the node needs queried availability or the fixed rule of class 5.

Checker. Kinds `0x45`..`0x48` owned on the conditions of a row move.

### 8. No-wrap nodes (node word 4 = 1)

Stock. `0xbf9b1c` returns 0 at the first or last row instead of wrapping (all.c:330980-331006); `0xbf9f44` then returns 0 and the consumer restores the block (457579-457584, 458281-458286).

Value. Nodes `0x7d` (`0x007830d0`, class `0xc0`), `0x7e` (`0x00787b78`, class `0xc9`) and `0x48` (`0x00786d50`), and framed group `0x43`'s parent. Crawl-5: 60 moves, 6 at an edge.

Paths. `nowrap`: kind 4 at cursor 0 of `0x007830d0` (lands at 1). `nowrap-edge`: kind 5 at cursor 0, rejected.

Control. `AlwaysWrap` ignores node word 4 and fails `nowrap-edge`.

Engine. `move_rows` and `check_open` lose the wrap condition; `step` already has the rule. `nowrap-edge` is the first rejected oracle case.

Checker. `wrapsAndQueried` loses its wrap half.

### 9. Seek on open (kind 1 with id `0x17`): stays fallback

Stock. Kind 1 calls `0xbfad04(1, node, type, 0x1d, id, extra)` after the open (all.c:331307-331323); with id `0x17` it runs class 3's seek loop (331649-331670). The tail for node `0x38` and class `0x228` (331676-331679) is a separate branch the engine keeps refusing.

Value. `back` on node `0x7d` posts a close of `0x7d` with id `0x1a`, then `FUN_00d3d7c4(0x3f, 0xc)` (all.c:432304) posts an open of node `0x3f` (registry `0x3f`, `0x00789228`) with id `0x17` and extra `0xc`, through `0x00ca4e98`. Crawl-5: 8 trials, always as the second post.

Why it stays fallback. The path reaches `0x7d` by a push from row 17 of `0x00789228`, whose token is `0xc`, so `remembered[0x3f]` is `0xc`. Kind 1 places the cursor from `remembered[0x3f]` (331311-331316) and `0xd588b8` returns `FUN_0061cff4(0x3f)`, which reads the same slot (550650-550693; id4.dis 61cff4), so the seek stops at once and no case of this path can fail `NoSeekStep`. Unlike class 3, the landing cursor depends on the `initial_row` answer, so a zero-step restriction cannot be decided before the first call. It stays fallback until a case exists where `remembered[0x3f]` differs from `0xc`.

Path. `open-seek` still runs: its first window, the close of node `0x7d` with id `0x1a` through `0x00ca4e98`, is a class 2 case; its second window is marked `-` in the extractor's event list.

### 10. Re-open (kind `0x13`)

Stock. `0xbfa800(0x13, node, 0x15)` (all.c:331408-331409): availability or the fixed clear, initial row (`0xd588b8` returns `0x7ffffffe` for kind `0x13` on class 5, 550689-550690), the row-state test and step, then skips the scroll and the saved frame (331511, 331535) and recomputes the ordinal and count (331545-331548). Kind `0x13` has no notification.

Value. Crawl-5: one trial, `back` on node `0x55` (`0x0078c29c`) through `0x00ca4e98`.

Path. `reopen`.

Control. `ReopenNotifies` notifies as kind 1 does after a successful open.

Engine. `MenuEvent::Reopen` runs `open_rows` with a `Reopen` mode that skips the scroll and saved frame. `Effect::PageEntry` runs before it, since the bank hook is on `0xbfa800`.

Checker. Kind `0x13` owned on the open-target conditions.

### 11. Row steps with id above 1

Stock. Kinds 4 and 5 repeat the step until `id` steps are made or the cursor passes its start (all.c:331215-331266). The engine and checker own every nonzero non-page id, but the ten oracle cases all have id 1. The inner moves of kinds 6 and 7 run the same loop with the event's id (class 4), so they are owned by this class, not restricted to id 1.

Value. Node `0x56` (`0x007894e0`, type 6, 43 rows, 42 per page) posts ids 4 to `0xd` from `FUN_00d48ab4` (540908); crawl-5 has 86 such trials, and 27 kind 7 events with ids 2 or 3.

Paths. `step-rows`: `down` at cursor 0 posts id `0xd` and stops after 13 steps at 13 (trial 2067). `step-rows-wrap`: `up` at cursor 0 posts id `0xd`; the first step wraps to 42, past the start, and the loop stops there (trial 2208, crawl-5 lands at 42).

Controls. `SingleStep` makes one step and fails `step-rows`. `NoPassStop` keeps stepping after the cursor passes its start and fails `step-rows-wrap`.

Engine and checker. None beyond the cases.

### 12. Kinds 8 and 9: push, move, pop

Stock. `0xbfab98(0xe, node, 0x15, 0)`, `0xbf9f44(4 or 5, node, id)`, `0xbfab98(0xf, node, 0x15 or 0x16, 0)` (all.c:331359-331370). The only posters are `FUN_00c7900c` (8) and `FUN_00c79178` (9), controller 75 action slots 212 and 217 at state 43, events 30 and 33, and 31 and 34 (manifest; all.c:425583, 425679).

Value. Controller 75 is in state 43 on the screen Q opens. Crawl-5 tried only `ok`, `down`, `up`, `right`, `left` and `back` there (controller 74 state 24 is its context), and controller 75 received events 0, 2, 4, 6, 8, 13, 47, 49 and 50 only.

Key layer. In controller 74 state 24, events 88 (crawl-5's `dial-right`) and 89 go to slot 7, `FUN_00c58f58`, which calls `0x00c0bae8` (all.c:400582-400586; manifest). The decomp names `0x00c0bae8` `ui_m75_event_dispatch` (NONMATCHING draft, `app/ui/machines/ui_machine_actions_00c077d0.c:2429`); it chooses controller 75's event from the command code at `0x0094f64c`: event 30 for codes `0x1d` and `0x207`, 31 for `0x1e` and `0x208`, 33 for `0x2d`, 34 for `0x2c`. Neither the decomp nor crawl-5 records which key produces those codes; crawl-5's `dial-right` never dispatched controller 75 on any screen. So class 12 is not reached by crawl-5, and the key is not identified.

Path. `frame-push-probe`: `q`, `down`, `down`, then `dial-right` **[INFERENCE: that `dial-right` posts code `0x1d` or `0x207` here]**. If its stage entry is kind 8, the class is owned with class 4's engine change and control. Otherwise the class is recorded as not reached and the probe config is dropped.

### 13 to 17

See "Classes that stay fallback".

## Classes that stay fallback

Each stays behind the shim "Fallback classes through the stock kind switch" (design, "Shims for this unit"), whose removal condition is that every class is owned; the class row in the checker stays `FallbackEvent`.

- Page step (kinds 4 and 5 with id equal to rows per page and a nonzero node, all.c:331201-331209). The posters with an id that can equal a node's rows per page are `0xd3d858(4, 4)` and `(5, 4)` in `FUN_00c6ed28` and `FUN_00c6e658`, controller 75 slots 28 and 11 at state 27, events 0 to 3 (all.c:418670, 418276), and in `FUN_00c4803c` and `FUN_00c47f3c`, controller 64 slots 11 and 6 at state 8 (387662, 387590). The six nodes with 4 rows per page include `0x007823a4`, the film-simulation offer list of `emulation/ClassicChromeSiteCases.hs`, which crawl-5 visited at controller 75 state 0. Crawl-5 never reached controller 75 state 27, and controller 64 is in state 0 on all 570 screens. State 27 is set by `FUN_00c6fe98`, `FUN_00c726bc`, `FUN_00c75914`, `FUN_00c75c70`, `FUN_00c77670`, `FUN_00c7ad48` and `FUN_00c7ade8` (all.c:419544-427359); two of them follow a push whose last-move class is 299 with token 2 or 3. Observation needs a route into either state, which no crawl records. The `0xd48ab4` steps (4 to `0xd`) are row steps on node `0x56` (class 11).
- Multi-level (kinds `0x10`..`0x12`, all.c:331395-331407). Posters: controller 74 state 26, events 22, 28, 37 (`FUN_00c5c3a0`, `FUN_00c5d9bc`, `FUN_00c60e8c`; the first two post only for last-move classes `0x213` and `0x21a`..`0x21d`, all.c:403506-403510, 404516-404529); controller 75 state 4 event 8 (`FUN_00c74424`, class `0xee`), state 9 event 8 (`FUN_00c74b34`), states 25 and 29 event 50 (`FUN_00c7abd8`, `FUN_00c7ade8`). Crawl-5 never reached controller 74 state 26 or controller 75 states 4, 25 and 29; it reached state 9 on node `0xc` twice (trials 1322, 1324), where `FUN_00c74b34` posted kind 2 because `0x009544f8` = 1 or `0x00956864` is 8 or 9 (all.c:422188-422194). Kind 1 with ids `0xe`..`0x12` (the same `0xbfad04` arms, 331633-331645) was not posted. Observation needs a state where `FUN_00c74b34` reaches 422206.
- Empty node (`0xbfa800`'s arm at all.c:331459-331465). No crawl-5 push or open landed elsewhere because its target had no selectable row: every push or open whose landing record differs from the target is a type 5 or 7 target re-pushed by `0xbfad04`. OK on rows whose child has no rows posted kind 2 or no stage event. The engine keeps its owned empty arm (synthetic tests only) with the `kind_switch` service, and `NotOwned::EmptyNode` for a node without rows.
- Restored cursor past the rows with `0xd588b8` returning `0x7ffffffe` (`token_past_rows`). No crawl-5 screen has a cursor at or past its node's rows; for a node with queried availability the open's step moves such a cursor unless the node has one row, in which case the step fails and the open with it, and a fixed node keeps it. `frame-pop-up` (class 4) enters a one-row page with cursor 2 and is accepted, so it does not reach this class. The shim "Values past the rows" stays.
- Kind `0xe` with id `0x17`, kind 1 with id `0x18`, kind 1 with ids `0xe`..`0x12`: not reached by crawl-5, whose kind `0xe` posts all carry id `0x1d` and kind 1 posts ids 0 and `0x17`. Key-action senders exist, posting through `0x0063270c` directly rather than `0xd3d858`:
  - `FUN_00d3d78c(kind, extra)` posts `(kind, node id, 0x17, extra)` (all.c:534820-534828); its 18 calls all pass kind `0xe`, from controller 74 slots 71, 130, 147, 163, 179 and 379 in state 27 and controller 75 slots 19, 34, 64, 88, 201 and 202 in state 42 (manifest). Slot 19, `FUN_00c6e9fc`, posts only when the last-move class is `0x1bb` (all.c:418457-418475).
  - `FUN_00d3d7f0(node, extra)` posts kind 1 with id `0x18` (534843-534847); its one call, `FUN_00c67d78(1, 8)` (412778), is controller 74 slot 322 at state 4, event 90.
  - `FUN_00d3d75c(node, id)` posts kind 1 with the given id (534810-534814): ids `0x10` and `0xe` for node `0x2d` from controller 64 slots 25, 38, 75, 76 and 89 at state 0 (all.c:389239-392628), and id `0x13` for node `0x2a` (392595).
  Crawl-5 never reached controller 74 state 27 or controller 75 state 42, never dispatched controller 74 event 90, and never dispatched controller 64. Kind `0xf` is outside `0xbfad04`'s seek mask, so a pop with id `0x17` or `0x18` runs only its `0xbfab98` (331385-331393, 331627). Kind 1 with id `0x17` is class 9.

## Shims

New or changed by this increment; the rest of the design's table stands.

| Shim | What it does | Removal condition |
| --- | --- | --- |
| Engine property write `0x285` (changed: now performed) | `Effect::PropertyWrite` for node `0x7a` through `0x0061c91c` | The settings service owns property `0x285` |
| Stock rollback restore for rejected events (new use) | Rejected owned events rely on the consumer's `0x0061d3a8` restoring the block | Owned stage consumers decide the result without a byte restore |
| Two stock stage consumers (changed) | Owned events arrive through `0x00ca4674` and `0x00ca4e98` | Owned stage consumers |
| Fixed-availability row-state clear | The engine sets the `0x4f` row-state words to 0 as `fill_u32(0x009571f0, 0, 0x4f)` does | The tree declares availability and nothing reads row states of a fixed node |
| Fallback classes through the stock kind switch (narrowed) | Class 9, class 3's stepping seek until its case passes, classes 13 to 17 and the framed `0xbfad04` re-push | Every class is owned |

No class adds an owned key cell; the seven frozen picker cells stay as they are.

## Decomp cross-check

Source: x70view branch `decomp/xm1-coverage` at 55436fc85c0560dd252b0410af6f2427fe198cd3 (`private-data/scratch/xm1-decomp/guide.md`), read from committed refs only. Names in it are inferences; the facts below come from the code.

Byte-verified (no NONMATCHING marker), so source-inspected:

- The engine wrapper `0xbf95c8` (`app/ui/core/ui_menu_tree_accessors.c:60`), `0xbf976c` and `0xbf9a2c` (`app/ui/core/tu_00bf8d20_exact.c:336, 352`).
- `0x007221a0`, a word fill (`app/runtime/byte_strings.c:187`); this settles the class 5 question.
- `0x0061d384` and `0x0061d3a8` copy the whole 0x430-byte block to and from a backup (`app/native_recovery/xm1_menu_state.c:138-144`), so a rejected event's restore rewrites the entire block. The extractor's rule for rejected receipts relies on this.
- Both stage consumers, `0x00ca4674` and `0x00ca4e98` (`app/ui/machines/ui_machine_actions_00ca4674.c:34, 69`), with the bracket, special post and value post as read above.
- `0x00c74b34` (`app/ui/machines/ui_m75_actions_c746fc.c:186`): kind 2 when the `0xff78` UI word is 1, the `0x2c` input word is 8 or 9, or a status call returns nonzero; otherwise `0x12` when the `0x2c` word is 10, else `0x11`. This is class 14's poster as read from Ghidra.

Drafts (NONMATCHING, "instruction selection and ordering differ"): `0xbfa234`, `0xbfa800`, `0xbfab98`, `0xbfad04`, `0xbf9f44`, `0xbf9c1c`, `0xbf9b1c`, `0xbf9a5c`, `0xbf988c` and `0xbf9794` (`app/ui/core/tu_00bf8d20.c`). `0xd588b8`, `0xd4dd2c`, `0xd5b350` and `0xd48ab4` are untuned Ghidra drafts. The engine's branches therefore stay confirmed by the oracle receipts, not cited from the C. The decomp's `Xm1MenuState` (`app/native_recovery/xm1_menu_state.h`) leaves most of the block as reserved bytes and adds no field the block decode lacks.

Effect on the plan:

- Removed: the open question on `0x007221a0`'s width, and any trace run to settle the rollback restore's extent.
- Shortened: the per-class runs stop being discovery. Each class still needs its oracle receipt, because every engine function it exercises is a draft. The case's purpose is byte qualification of the owned engine against stock, which a draft cannot give.
- Used in this revision: the byte-verified controller 75 actions 7, 8, 25 and 44 fix class 3's extra words and give the one-step seek case; `ui_m75_event_dispatch` (a draft) maps command codes to controller 75 events for class 12, though not codes to keys.
- Still possible: finding which actions enter controller 75 state 27 (class 13) and 42 and controller 74 state 27 (class 17) from the C, before any run.

## Decisions

- Framed `0xbfad04` targets (`NotOwned::FramedNode`) are outside the assignment's list but frequent: crawl-5 has 131 pops from a page of group `0x43` onto its type 5 parent, 22 opens of node `0x5f` by `down` on the root screen, and 4 pushes into type 5 or 7 nodes. Owning them replaces the `seek` service; it needs `0xbfad04`'s framed arm (all.c:331620-331625) and cases such as `ok up left down ok ok`, `back` (crawl-5 trial 3593). Include in this increment or the next.
- The compound pre-check refuses kinds 6..`0xb` when any reachable target fails the open conditions, where stock would complete. The alternative, letting `apply` refuse after a call, breaks the design's rule that the event completes once committed.
- `frame-push-probe` is a run whose outcome decides whether class 12 is owned; the key layer read in class 12 leaves the key unidentified.
- Class 3's one-step seek is owned only after `frame-seek-step` passes with `NoSeekStep` failing it; until then `NotOwned::SeekSteps` refuses it.
- The fallback classes 13 to 17 stay stock-backed; #128's acceptance covers the observed presses and the picker pairs, which none of them reach.

## Appendix: configs

Each block is ready to commit as `emulation/menu-oracle-<name>.json`. The expected events are those of the class table.

### `menu-oracle-pop-remember.json`

```json
{
  "schema": 1,
  "path": ["ok", "up", "up", "up", "up", "up", "up", "up", "ok"],
  "key": "back",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-pop-remember-off.json`

```json
{
  "schema": 1,
  "path": ["ok", "up", "up", "up", "up", "ok", "down"],
  "key": "back",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-push-same-parent.json`

```json
{
  "schema": 1,
  "path": ["ok", "ok", "down", "down"],
  "key": "ok",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-push-remember-off.json`

```json
{
  "schema": 1,
  "path": ["ok", "up", "up", "up", "up", "ok", "down"],
  "key": "ok",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-close.json`

```json
{
  "schema": 1,
  "path": ["ok"],
  "key": "back",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-frame-seek-down.json`

```json
{
  "schema": 1,
  "path": ["ok", "up", "left", "up", "ok", "up", "up", "ok", "ok", "ok"],
  "key": "up",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-frame-seek-up.json`

```json
{
  "schema": 1,
  "path": ["ok", "up", "left", "up", "ok", "up", "up", "ok", "ok", "ok"],
  "key": "down",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-frame-seek-step.json`

```json
{
  "schema": 1,
  "path": ["down", "down", "ok", "right", "ok"],
  "key": "down",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-frame-pop-down.json`

```json
{
  "schema": 1,
  "path": ["down"],
  "key": "down",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-frame-pop-up.json`

```json
{
  "schema": 1,
  "path": ["down", "down"],
  "key": "up",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-frame-fixed-open.json`

```json
{
  "schema": 1,
  "path": ["ok", "up", "left", "down", "ok", "ok", "down", "down"],
  "key": "right",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-fixed-wrap.json`

```json
{
  "schema": 1,
  "path": ["ok", "up", "left", "down", "ok", "ok", "down", "down", "right"],
  "key": "down",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-close-property.json`

```json
{
  "schema": 1,
  "path": ["q", "down", "down"],
  "key": "back",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-group-left.json`

```json
{
  "schema": 1,
  "path": ["q", "down", "down"],
  "key": "left",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"},
    {"address": "0x00bf9c1c", "label": "engine-group-move"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-group-right.json`

```json
{
  "schema": 1,
  "path": ["q", "down", "down"],
  "key": "right",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"},
    {"address": "0x00bf9c1c", "label": "engine-group-move"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-group-up.json`

```json
{
  "schema": 1,
  "path": ["q", "down", "down"],
  "key": "up",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"},
    {"address": "0x00bf9c1c", "label": "engine-group-move"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-group-down.json`

```json
{
  "schema": 1,
  "path": ["q", "down", "down"],
  "key": "down",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"},
    {"address": "0x00bf9c1c", "label": "engine-group-move"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-group-right-wrap.json`

```json
{
  "schema": 1,
  "path": ["q", "down", "down", "right", "down", "right", "up", "up"],
  "key": "right",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"},
    {"address": "0x00bf9c1c", "label": "engine-group-move"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-group-left-wrap.json`

```json
{
  "schema": 1,
  "path": ["q", "down", "down", "right", "down", "right", "up", "up", "right"],
  "key": "left",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"},
    {"address": "0x00bf9c1c", "label": "engine-group-move"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-group-down-wrap.json`

```json
{
  "schema": 1,
  "path": ["q", "down", "down", "right", "down", "right", "up", "up", "right", "down", "down"],
  "key": "down",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"},
    {"address": "0x00bf9c1c", "label": "engine-group-move"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-group-up-wrap.json`

```json
{
  "schema": 1,
  "path": ["q", "down", "down", "right", "down", "right", "up", "up", "right"],
  "key": "up",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"},
    {"address": "0x00bf9c1c", "label": "engine-group-move"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-nowrap.json`

```json
{
  "schema": 1,
  "path": ["ok", "left", "up", "ok", "down", "down", "down", "ok"],
  "key": "right",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-nowrap-edge.json`

```json
{
  "schema": 1,
  "path": ["ok", "left", "up", "ok", "down", "down", "down", "ok"],
  "key": "left",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-open-seek.json`

```json
{
  "schema": 1,
  "path": ["ok", "left", "up", "ok", "down", "down", "down", "ok"],
  "key": "back",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-reopen.json`

```json
{
  "schema": 1,
  "path": ["ok", "left", "up", "ok", "down", "ok", "down", "down", "ok", "ok"],
  "key": "back",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-step-rows.json`

```json
{
  "schema": 1,
  "path": ["ok", "left", "up", "ok", "down", "ok", "ok", "ok", "down"],
  "key": "down",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-step-rows-wrap.json`

```json
{
  "schema": 1,
  "path": ["ok", "left", "up", "ok", "down", "ok", "ok", "ok", "down"],
  "key": "up",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```

### `menu-oracle-frame-push-probe.json`

```json
{
  "schema": 1,
  "path": ["q", "down", "down"],
  "key": "dial-right",
  "trace_sites": [
    {"address": "0x00d3d858", "label": "stage-open"},
    {"address": "0x0063270c", "label": "event-post"},
    {"address": "0x00ca4674", "label": "stage-consumer"},
    {"address": "0x00ca4e98", "label": "stage-consumer-101"},
    {"address": "0x0061d384", "label": "rollback-save"},
    {"address": "0x0061d3a8", "label": "rollback-restore"},
    {"address": "0x00d4dd2c", "label": "stage-entry"},
    {"address": "0x00bfa234", "label": "engine-kind"},
    {"address": "0x00bf9f44", "label": "engine-move"},
    {"address": "0x00bf9b1c", "label": "engine-step"},
    {"address": "0x00bf988c", "label": "engine-scroll"},
    {"address": "0x00bfa800", "label": "engine-open"},
    {"address": "0x00bfab98", "label": "engine-push-pop"},
    {"address": "0x00bfad04", "label": "engine-seek"},
    {"address": "0x00bf9a5c", "label": "engine-remember"},
    {"address": "0x0061d0e4", "label": "push-classifier"},
    {"address": "0x00bf9a2c", "label": "row-of-token"},
    {"address": "0x0061ceac", "label": "frame-snapshot"},
    {"address": "0x00d52684", "label": "row-availability"},
    {"address": "0x00bfa914", "label": "availability-return"},
    {"address": "0x00d588b8", "label": "initial-row"},
    {"address": "0x00bfaa3c", "label": "initial-row-return"},
    {"address": "0x00d5b350", "label": "engine-notify"},
    {"address": "0x00d42a64", "label": "special-post"},
    {"address": "0x00d586bc", "label": "value-mode"},
    {"address": "0x00d3d620", "label": "value-post"},
    {"address": "0x00d4fc74", "label": "commit"},
    {"address": "0x0061c91c", "label": "property-write"}
  ],
  "write_ranges": [
    {"start": "0x0093eb74", "end": "0x00944440"},
    {"start": "0x0094f64c", "end": "0x0094f660"},
    {"start": "0x009503d0", "end": "0x009503d8"},
    {"start": "0x009571d0", "end": "0x00957600"},
    {"start": "0x0096cc64", "end": "0x0096ce28"},
    {"start": "0x01467580", "end": "0x01467588"}
  ],
  "limit": 65536
}
```
