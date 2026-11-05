# Owned menu engine, increment 4: framed targets, key reach of the fallback classes, retirement (#128)

Status: increment 4 of [the engine design](owned-menu-engine-design.md), after [increment 3](owned-menu-engine-increment-3.md) (E-MENU-ENGINE-INC3-HOST-ORACLE-150, E-MENU-ENGINE-INC3-GUEST-PAIRS-150); design cleared by MenuEngineIncrement4DesignReview, built at ea3d2e66 (engine), 907de575 (checker) and aebdd8a9 (allowlist fragment). Each of the seven configs below ran once and its receipt matches the class table, so no class was moved back to fallback: `frame-push-up-probe` recorded `9, 0x7a, 1`, so class 4 is owned. All 45 host-oracle cases pass (the 38 of increments 1 and 3 and the seven below, `menu-engine/host-oracle-aebdd8a9/owner-test.log`), each new control fails the cases the class table names, and the picker-table engine build at `0x00c40000` has no allowlist violation with a stack bound of 904 + 164 bytes against 1656. No guest pairs have run. It owns the type 5 and 7 targets that `0xbfad04` re-steps (classes 1 to 3 below) and kind 9 (class 4), gives the key reach of every remaining fallback class from the controller 74 and 75 cells, and reports the retirement scan: at increment 4 no stock engine function under `0xbfa234` can be retired.

Stock facts come from the x70view decomp first, read at the pinned commit `55436fc85c0560dd252b0410af6f2427fe198cd3` (`reports/xm1-decomp.md`; the branch tip `a72c95a357` gives the same status for every engine function below). Exactness is the `//build-support:xm1-decomp -- lookup` status; a NONMATCHING function is a draft and its facts are read from `private-data/analysis/x-m1-1.50/filmsim/ghidra/all.c` (the export of the id5 bytes) or the disassembly `private-data/analysis/x-m1-1.50/lz/id5.dis`. Node facts come from `private-data/scratch/ControllerStrategy/menu-tables.json` (E-MODAL-MENU-TABLES-150), controller cells from `private-data/scratch/ControllerManifest/manifest.json` (E-MODAL-CONTROLLER-MANIFEST-150), reach from crawl-5 (`private-data/scratch/ControllerStrategy/crawl-5/crawl-graph.json`, E-MODAL-MENU-OKPATHS-150). **[INFERENCE]** marks what the evidence does not show directly.

## How reach was derived

The derivations are in `private-data/scratch/ControllerStrategy/inc4/`:

- `framed.jq` classifies crawl-5's first stage post of each trial, as increment 3 did, and keeps the posts whose target `0xbfad04` follows: kind 1 (id not `0x17`/`0x18`) whose registry node has type 5 or 7, kind `0xe` whose cursor row's child has type 5 or 7, kind `0xf` whose source's parent has type 5 or 7. Output `framed.json`, 157 trials.
- `posts.jq` lists every stage post of crawl-5 with its from-screen record (`posts.json`, 3,365 posts).
- `cells-all.jq` flattens the manifest's cells of all 174 controllers into (controller, state, event, action) (`cells-all.json`).
- `reference-scan.js` is the retirement reference scan (below).
- `nochild.jq` lists crawl-5's first stage posts of kind `0xe` from a row without a child and of kind `0xf` from a node without a parent (`nochild.json`, empty).
- `decomp-55436fc85c/` is `git archive` of `decomp/firmwares/xm1-150/app/ui` at the pinned commit.

Controller states change only by an action's store to `record_transition_state[c]` (`UI_STATES`, `M74_STATE`, `M75_STATE` in the decomp; `DAT_0146761c` and `DAT_0146761e` in all.c) or by a cell whose transition word is a direct state: `ui_dispatch_machine_event` (`0xbfb028`, exact, `app/ui/core/ui_machine_dispatch.c:5`) stores `-transition` when the cell's transition is not positive and otherwise runs the handler without a state change. A gating state was traced back by listing every literal store of that state in all.c, mapping the storing function to the cells whose action calls it (callers up to three deep), and repeating from those cells' states until a state crawl-5 reached. The manifest has 1,128 direct-state cells over all controllers; controllers 74 and 75 have five, and one of them enters a state named below (controller 75 state 0, event 41, to state 29). Controller 57's direct-state cells 57/38 events 81 and 82 enter its state 1 (class 7d). All.c has no computed store to either state word of controllers 74 and 75 besides `0xbfb028`'s.

`--menu-observe` presses only the ten keys of `CrawlKey` (`emulation/EmulatorUsb.hs:626`). Crawl-5 maps them to controller events (dispatches recorded per press):

| Key | Controller 74 events | Controller 75 events (forwarded) |
| --- | --- | --- |
| `up` | 30, 32 | 0 |
| `down` | 33 | 2 |
| `left` | 35 | 4 |
| `right` | 37 | 6 |
| `ok` | 22, 23 | 8 |
| `back` | 28, 29 | 13 |
| `q` | 6, 7 | none |
| `dial-right` | 88 | none in crawl-5 |
| `dial-left` | 89 | none in crawl-5 |
| `play` | 102, 103, 107 | none |

Events 113, 136, 139, 142, 145, 148, 151 (controller 74) and 47, 49, 50 (controller 75) are dispatched after many keys and are not key events. The controller 74 event source `0x00c09d24` (all.c:340173) gives event 88 for UI code `0x2d` and 89 for `0x2c` (all.c:341119-341130), so `dial-right` is code `0x2d` and `dial-left` code `0x2c` on this body. The controller 75 event source `0xc0bae8` (`ui_m75_event_dispatch`, NONMATCHING, `app/ui/machines/ui_machine_actions_00c077d0.c:2429`; all.c:341168) gives event 33 for code `0x2d` and 34 for code `0x2c` with a zero press word (all.c:341510-341522).

The live-view root is controller 74 in state 4, the predicate of `close_menu` in `emulation/menu-engine-pair-trajectories/oracle.jq` (3e59f908). Each config below names the BACKs that crawl-5's back edges give from its landing to that root; the pair trajectory appends them as its `close-menu` segment before the unload. A `--menu-observe` config itself has no closing field (`emulation/Emulation/MenuObserve.hs:69`): the observation runs on the no-install route and does not unload.

## Class table

Crawl-5 counts are first posts of a trial.

| # | Class | Stock branch (all.c; decomp status) | Crawl-5 trials | Reachable on keys | Path, key (crawl node, trial) | Expected stage event (kind, node, id) | Config `menu-oracle-*.json` | Negative control |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1a | Kind 1 open of a type 7 node, then `0xbfad04`'s push | 331307-331323, 331620-331625; `0xbfa234` and `0xbfad04` NONMATCHING | 22 (node `0x5f`, all `down` on the live view) | yes | `[]`, `down` (0, 1293) | `1, 0x5f, 0` | `framed-open` | `NoFramedFollow`, `FramedNotifyIdSwap` |
| 1b | As 1a with a nonzero initial row | as 1a | 11 of the 22 land on page `0x77d59c` (10 at cursor 2, 1 at cursor 3) | yes | `down down ok`, `down` (238, 1298) | `1, 0x5f, 0` | `framed-open-restored` | `NoFramedFollow`, `FramedNotifyIdSwap` |
| 2a | Kind `0xf` pop onto a type 5 parent, then `0xbfad04`'s pop (group `0x43`) | 331385-331393, 331620-331625; `0xbfa234`, `0xbfad04` and `0xbfab98` NONMATCHING | 97 (47 `back` id `0x1a`, 47 `ok` and 3 `left` id `0x1d`) | yes | `ok up left down ok ok`, `left` (228, 3592); same path, `back` (228, 3593) | `0xf, 0x43, 0x1d`; `0xf, 0x43, 0x1a` | `framed-pop`, `framed-pop-back` | `NoFramedFollow`, `FramedPopAsPush`, `FramedNotifyIdSwap` (back) |
| 2b | As 2a, group `0x41` | as 2a | 34 (16 `back`, 16 `ok`, 2 `left`) | yes | `ok up left down ok down ok down ok`, `back` (486, 3717) | `0xf, 0x41, 0x1a` | `framed-pop-group41` | `NoFramedFollow`, `FramedPopAsPush`, `FramedNotifyIdSwap` |
| 3 | Kind `0xe` push into a type 5 node, then `0xbfad04`'s push | as 2a | 4 (2 into `0x77ff80`, 2 into `0x783d94`) | yes | `ok up left down ok down ok down`, `ok` (485, 3601) | `0xe, 0x3f, 0x1d` | `framed-push` | `NoFramedFollow` |
| 4 | Kind 9: push, move up, pop | 331365-331370; poster `0xc79178` exact | 0 | probe; [INFERENCE] `dial-left` on the screen Q opens | `q down down`, `dial-left` (254, untried) | `9, 0x7a, 1` | `frame-push-up-probe` | `FrameNotifyId`, `PushMoveUpAsDown` |
| 5 | Page step (kinds 4, 5, id = rows per page) | 331201-331209 | 0 | not reached by the ten crawl keys; gate controller 75 state 27 | none | none | none | none |
| 6 | Multi-level (kinds `0x10`..`0x12`) | 331395-331407 | 0 | not reached by the ten crawl keys; the two settings-gated posters are dead code (`0x006394d0` returns 1); gates 75 states 4, 21, 29 | none | none | none | none |
| 7a | Kind 1 with id `0x17` (class 9 of increment 3) | 331307-331323, 331649-331670 | 8 (second post) | yes; stays fallback (increment 3, class 9) | `ok left up ok down down down ok`, `back` (370, 2601) | `2, 0x7d, 0x1a`, then `1, 0x3f, 0x17` | `open-seek` (increment 3) | none |
| 7b | Kind 1 with id `0x18` | 331307-331323, 331649-331670 | 0 | not reached by the ten crawl keys; needs code `0x39` (controller 74 event 90) | none | none | none | none |
| 7c | Kind `0xe` with id `0x17` | 331385-331393, 331649-331670 | 0 | not reached by any key; controller 74 state 27 and controller 75 state 42 have no store or entering cell | none | none | none | none |
| 7d | Kind 1 with ids `0xe`, `0x10` | 331629-331635 | 0 | not reached by the ten crawl keys; needs a dispatch of controller 64 (`0x009503a0` = 6 at 57/0 event 49, or 57/38 events 81/82) | none | none | none | none |
| 8 | Kinds `0xa`, `0xb` on a type 5 or 7 open node | 331371-331384, 331620-331625 | 0 | not reached by crawl-5 | none | none | none | none |
| 9 | Kind `0xe` from a row without a child (`NoChild`), kind `0xf` from a node without a parent (`NoParent`) | 331385-331393, 331563-331565, 331585-331587 | 0 (any post) | not reached by crawl-5 | none | none | none | none |

Classes 1 to 3 become owned; class 4 is owned only if its probe records `9, 0x7a, 1`; classes 5 to 9 stay fallback. "Not reached by the ten crawl keys" names the first state or event on the static path from a crawl-5 state that none of the ten `CrawlKey` keys produced; the codes each gate needs are listed under "Key reach of the fallback classes" for a later key-layer task. No bridge key is added in this increment.

## Changes shared by all owned classes

### Engine invariant

`apply` still returns `NotOwned` only before its first service call. The framed step runs after the open, push or pop that lands on the type 5 or 7 node, so its target is checked before the first call:

- Kind 1 onto node T of type 5 or 7: every row child of T. A `Leaf` child is allowed (stock `0xbfab98` returns 0 with no call, all.c:331563-331565, and the open fails), a `Package` child refuses with `PackageChild`, a `Node` child must pass `check_open` with the restored cursor of `stack[2]`, which the open zeroes (all.c:331311-331312). Depth after the open is 1, so the push needs `STACK_FRAMES > 2`.
- Kind `0xf` from a node whose parent P has type 5 or 7: P's parent G with the restored cursor of `stack[depth - 2]`, and `depth >= 2`. A P without a parent is allowed: `0xbfab98` returns 0 with no call (all.c:331585-331587) and the event is rejected. When P has Remember 1, P's restored cursor `stack[depth - 1].cursor` must be at most P's row count, since the framed pop's `0xbf9a5c` reads P's cursor row; otherwise `RowBounds`.
- Kind `0xe` into a child C of type 5 or 7: every row child of C, as for kind 1, with the restored cursor of `stack[depth + 2]`, and `depth + 2 < STACK_FRAMES`, since C's cursor comes from `0xd588b8`'s answer. C's own restored cursor `stack[depth + 1].cursor` must be at most C's row count, as `check_push_move_pop` requires of its target (engine.rs:1222-1224): with no initial row, `0xbfa800` keeps that cursor, and the framed push reads C's cursor row; otherwise `RowBounds`.
- Kind 1 needs no cursor bound: the open places T's cursor by ordinal (all.c:331314), so it lies inside T's rows.

Each bound refuses with `NotOwned::RowBounds` before any call.

### Engine (`replacement/menu_engine/src/engine.rs`)

- `check_open` loses `FramedTarget`; `FramedNode` stays only for kinds `0xa` and `0xb` on a framed open node (class 8), where `0xbfad04`'s framed arm replaces the seek.
- A new `follow_framed(kind, node_word, notify_id)` replaces `seek_framed` on owned paths. Stock (all.c:331620-331625; id5.dis bfad24-bfad3c, which leaves r3 untouched): the step is `0xbfab98(kind if kind is 0xe or 0xf else 0xe, node, 0x15, r3)` with r3 the fourth argument of `0xbfad04`. So kind 1 pushes with notify id `0x1d` (the call at all.c:331318 passes `0x1d`) and the stage node as `0xbf9a5c`'s index; kinds `0xe` and `0xf` re-push or re-pop with the event's id and the landed record's id (`*0x009571d0`, all.c:331392). `follow_framed` is `switch_node(switch, node_word, Direction::Down, notify_id)` and returns its result; stock's `0xbfad04` returns `0xbfab98`'s result, then runs the node `0x38` tail (all.c:331676-331680), which the engine still refuses on kind 1 (`SEEK_CLASS_NODE`) and which no kind `0xe`/`0xf` reaches.
- `push_pop` and `open` call `follow_framed` when the landed node has type 5 or 7 and the state was not delegated. After a delegated `kind_switch` (the empty-node arm, class 15 of increment 3) the landed node is not known before the first call, so that path keeps the `seek` service.
- `Rules` gains the controls: `NoFramedFollow` (no framed step, result `Accepted`), `FramedPopAsPush` (the framed step always pushes), `FramedNotifyIdSwap` (kind 1 passes the event id, kinds `0xe`/`0xf` pass `0x1d`). Each is listed in `owner_controls_fail` with the cases it must fail.

### Checker (`MenuEngineClass.hs`) and pairs

- `openable FramedRefused` is replaced by the three framed checks above for kinds 1, `0xe` and `0xf`; `FramedTarget` names a framed step whose target fails them; `FramedSource` stays for kinds `0xa` and `0xb`.
- The guest checker fails a run on an entry to the adapter's `seek` call site (`rb_menu_engine_call_seek`, `replacement/menu_engine/adapter/mod.rs:166`) for an event its derivation owns, as it does for the fallback call, except for a `seek` entry that follows a `kind_switch` service entry in the same event (the shim "Stock kind switch and seek as services").
- `oracle.jq` renders each config as for increment 3. The `framed-open` paths start from the live view and dispatch no owned controller-75 action before the key, so they take `--argjson sample` (OK, BACK) like `frame-pop-down` (`replacement/README.md`, increment 3) **[INFERENCE]**.

### Extractor

No change. A framed event is one stage window whose calls include the second `0xbfab98`'s notify, availability, initial row and snapshot store; `menu-engine-cases` takes the expected events of the class table.

### Config base

Every config is `emulation/menu-oracle-frame-push-probe.json` with its `path` and `key` replaced: the 28 trace sites (including `engine-seek` `0x00bfad04` and `engine-push-pop` `0x00bfab98`) and the six write ranges of increment 3. The configs are in `private-data/scratch/ControllerStrategy/inc4/menu-oracle-*.json` and in the appendix.

## Classes

### 1. Kind 1 onto a type 7 node (node `0x5f`)

Stock. `0xbfa234` case 1 opens registry node `param_2`, notifies `(1, node, 0x1d, class)` and calls `0xbfad04(1, node, type, 0x1d, id, extra)` (all.c:331307-331323). With the open node's type 5 or 7, `0xbfad04` takes the framed arm before the seek mask (all.c:331620-331627; decomp `menu_cursor_handle_selection`, NONMATCHING, `app/ui/core/tu_00bf8d20.c:1333`, same branch order): `0xbfab98(0xe, node, 0x15, 0x1d)` pushes into the cursor row's child, with `0xbf9a5c(node)`, `0x0061d0e4`, `0xd5b350(0xe, page id, 0x1d, class of 0x787cbc)`, `0xbfa800(0xe, page id, 0x15)` and the snapshot store at the page id (all.c:331552-331607). If the push returns 0, the open clears `0x009575fc` and returns 0 (all.c:331320-331322).

Reach. Node `0x5f` is registry `0x5f`, record `0x00787cbc` (type 7, no parent, Remember 0, six rows, every child a type 7 page of id `0x5f`). `down` on the live view posts `1, 0x5f, 0` (22 trials). The landing page follows the initial row: 7 trials land on `0x00791294` (cursor 0), 11 on `0x0077d59c` (10 at cursor 2, trial 1888 at cursor 3) and one each on the other four pages.

Paths. `framed-open`: `[]`, `down` (crawl node 0 to 146, trial 1293; lands on `0x00791294`). `framed-open-restored`: `down down ok`, `down` (node 238 to 237, trial 1298; lands on `0x0077d59c` at cursor 2, so the push takes row 1, not row 0). Both end with one BACK (146 or 237 to node 143, the live view).

Controls. `NoFramedFollow` leaves `0x787cbc` open and fails both. `FramedNotifyIdSwap` notifies the push with id 0 and fails both.

Engine. `open` replaces `seek_framed` with `follow_framed(StageKind::Push, node_word, StageId::VALUE)` after the pre-check of every row child of `0x787cbc`.

### 2. Kind `0xf` onto a type 5 parent

Stock. `0xbfa234` case `0xf` with an id other than `0x17`/`0x18` runs `0xbfab98(0xf, node, 0x15, id)`, then `0xbfad04(0xf, *0x009571d0, type, id, 0, 0)` (all.c:331385-331393). Landing on a type 5 parent, the framed arm keeps kind `0xf` and pops again with notify id `id` (all.c:331620-331625).

Reach. Group `0x43`: pages of `0x00783d94` (type 5, id `0x43`, parent `0x00789228`, node `0x3f`). Group `0x41`: pages of `0x0077ff80` (type 5, id `0x41`, parent `0x00783d68`, type 0). The two parents have Remember 0, so neither framed pop writes `0x0095733c`. Crawl-5 has 97 pops onto `0x00783d94` (landing on `0x00789228`) and 34 onto `0x0077ff80` (landing on `0x00783d68` at cursor 1). The 131 pops of increment 3's decision are these two groups together, not group `0x43` alone.

Paths. `framed-pop`: `ok up left down ok ok`, `left` (228 to 227, trial 3592, id `0x1d`). `framed-pop-back`: same path, `back` (trial 3593, id `0x1a`). `framed-pop-group41`: `ok up left down ok down ok down ok`, `back` (486 to 485, trial 3717). Closing BACKs: one (227 to 231) for the group `0x43` cases, two (485, 229, 231) for group `0x41`.

Controls. `NoFramedFollow` fails all three. `FramedPopAsPush` pushes into the parent's cursor row child instead and fails all three. `FramedNotifyIdSwap` notifies the second pop with `0x1d`, which fails `framed-pop-back` and `framed-pop-group41` (id `0x1a`) and cannot fail `framed-pop`.

Engine. `push_pop` for `Switch::Pop` calls `follow_framed(StageKind::Pop, landed id, id)` when the landed node has type 5 or 7.

### 3. Kind `0xe` into a type 5 node

Stock. As class 2 with kind `0xe`: the framed arm pushes again into the cursor row's child of the node the push opened.

Reach. 4 trials: `ok` and `right` from `0x00783d68` row 1 into `0x0077ff80` (landing on page `0x0077e9a8`), and from `0x00789228` row 0 into `0x00783d94` (landing on `0x0078ecd8`).

Path. `framed-push`: `ok up left down ok down ok down`, `ok` (485 to 486, trial 3601). Three closing BACKs (486, 485, 229, 231).

Control. `NoFramedFollow` leaves `0x0077ff80` open. `FramedNotifyIdSwap` cannot fail it (the event id is `0x1d`).

Engine. `push_pop` for `Switch::Push` calls `follow_framed(StageKind::Push, landed id, id)` after the pre-check of every row child of the pushed node.

### 4. Kind 9 (probe)

Stock. `0xbfa234` case 9: `0xbfab98(0xe, node, 0x15, 0)`, `0xbf9f44(5, node, id)`, `0xbfab98(0xf, node, 0x16, 0)` (all.c:331365-331370). The only poster is `ui_m75_action_217` (`0xc79178`, exact, `app/ui/machines/ui_m75_actions_c7838c.c:745`), cells 75/43 events 31 and 34; its body equals kind 8's poster `ui_m75_action_212` (`0xc7900c`, exact, `:669`) except for posting `(9, 1)` instead of `(8, 1)`.

Reach. On the screen Q opens (controller 74 state 24, controller 75 state 43) `dial-left` dispatches controller 74 event 89 (crawl-5), which runs slot 7 `ui_m74_action_7` (`0xc58f58`, exact, `app/ui/machines/ui_m74_actions_c58bec.c:279`), a call of `0xc0bae8`. `0xc0bae8` gives controller 75 event 34 for code `0x2c` with a zero press word (all.c:341518-341522). `frame-push-probe` (E-MENU-ENGINE-INC3-HOST-ORACLE-150) shows the same chain for `dial-right` (event 88, code `0x2d`, event 33, kind 8 at node `0x7a`). **[INFERENCE]**: `dial-left` posts `9, 0x7a, 1` there, since `0xc0bae8` is NONMATCHING and crawl-5 never pressed `dial-left` on that screen.

Path. `frame-push-up-probe`: `q down down`, `dial-left`. One closing BACK (254 to 143), on the assumption that the press lands back on node 254 as `frame-push-probe` did.

Ownership. If the receipt's stage entry is `9, 0x7a, 1` and its case passes, `frame_move` stops refusing `PushMoveUpPop`: kind 8's steps with `move_rows(Direction::Up)` and the pop's open direction `Direction::Up`, under the existing `check_push_move_pop`. Otherwise the class stays fallback and the config is dropped.

Controls. The existing `FrameNotifyId`. `PushMoveUpAsDown` moves down instead. A control on the pop's direction cannot fail this case: the pop reopens the parent `0x0078e4a0` (crawl node 254) on its restored row 9, which is selectable in the receipt's before block **[INFERENCE]**, so `0xbfa800`'s step direction is unused.

### 5 to 9. Fallback classes

Each stays behind "Fallback classes through the stock kind switch". Their key reach is the next section. Class 9 (`NoChild`, `NoParent`) would be owned as `Rejected`, since stock's `0xbfab98` returns 0 with no call (all.c:331563-331565, 331585-331587), but crawl-5 has no case: `nochild.jq` finds no such first post, and `posts.json` has no kind `0xe` or `0xf` post after the first of a press. It stays fallback until a receipt shows one.

## Key reach of the fallback classes

Posters are the callers of `0xd3d858` (`ui_execute_record_mode_request`, exact), `0xd3d75c`, `0xd3d78c`, `0xd3d7c4` and `0xd3d7f0` (all exact, `tu_00d3c898.c:478-499`), with their cells. "Reached" states are crawl-5's states on any screen or in any dispatch: controller 74 states 0, 1, 4, 17, 24, 25, 28; controller 75 states 0, 1, 2, 3, 9, 10, 11, 13, 14, 18, 19, 41, 43; controller 64 state 0 only, never dispatched.

Codes each gate needs, for a later key-layer task (no bridge key is added in this increment):

| Class | Gate | What enters it | Code or event needed |
| --- | --- | --- | --- |
| 5 page step | 75 state 27 (or 64 state 8) | 75 state 21 from 75/0 event 50 with `UI_W(0xff78)` = 1 and open node id 8 or `0x6a`; 75 state 29 from 75/0 event 41 | a kind 1 open of nodes `0x63`..`0x79` (the only live literal one: 74/4 event 5, UI code `0x28` with press word `0x80`; `0xc5e9f0` at 74/4 event 32 is dead behind `0x006394e8`, which returns 0); record event `0xa46` for event 41, not a key code; for controller 64, as for 7d |
| 6 multi-level | 75 states 4, 21, 29 | 75 state 4 after a kind `0xe` post on a node of class `0x26`; 21 and 29 as for class 5 | no decoded node has class `0x26`, so none; otherwise as for class 5 |
| 7b kind 1, id `0x18` | 74/4 event 90 | live view | UI code `0x39` |
| 7c kind `0xe`, id `0x17` | 74 state 27, 75 state 42 | nothing | none exists |
| 7d kind 1, ids `0xe`, `0x10` | dispatch of controller 64 | `0xc06b7c`, called by `0xc3f3dc` at 57/0 event 49 only when `0x009503a0` = 6 (all.c:378367-378371); controller 57 state 1, entered by the direct-state cells 57/38 events 81 and 82 | `0x009503a0` = 6 when `play` dispatches 57/0 event 49 (crawl-5 trial 2444 dispatched it with another value), or controller 57 events 81/82 in state 38 (codes not traced) |

### Page step (class 5)

Posters: `ui_m75_action_11` (`0xc6e658`, exact, `ui_m75_actions_c6e378.c:158`) posts `(5, 4)` at cells 75/27 events 0 and 1, and `ui_m75_action_28` (`0xc6ed28`, exact, `:426`) posts `(4, 4)` at 75/27 events 2 and 3; both only test `0x0063a0f8(0x53) == 0`. `ui_m64_action_6` and `_11` (`0xc47f3c`, `0xc4803c`, exact) post the same at controller 64 state 8. A page step also needs the open node's rows per page to be 4; the six such nodes include the two pages of node `0x7a` (`0x0077d908`, `0x0078b000`). The variable-id posters pass `0xd48ab4`, which returns 1 except for classes `0x131` and `0xb6`; the one class `0xb6` node (`0x007894e0`, node `0x56`) has 42 rows per page and the ids are 4..13, so none is a page step.

Gate: controller 75 state 27. Stores: `0xc6fe98`, `0xc6fee0`, `0xc726bc`, `0xc75914`, `0xc75c70`, `0xc77670`, `0xc7ad48`, `0xc7ade8` (all.c:419544-427359), in cells of states 21, 28 and 29 only. State 21 is entered from 75/0 event 50 by `ui_m75_action_274` (`0xc7a528`, NONMATCHING; all.c:426872-426877) only when `0x009544f8` (`UI_W(0xff78)`) is 1, the open node id is 8 or `0x6a`, and the dispatch candidate is -1 or `0x12`. `0x009544f8` has one writer, `0x637f80` (exact), which `0xd4d7a4` (NONMATCHING; all.c:543401-543423) calls with 1 on a kind 1 open of nodes `0x63`..`0x79` other than `0x71`, `0x72`. Crawl-5 opened no such node; the one key-cell poster with a literal node in that range is `ui_m74_action_46` (`0xc59f38`, node `0x63`) at 74/4 event 5, which `0xc09d24` produces from code `0x28` with press word `0x80` (all.c:340220-340227) and no bridge key produced. State 29 is entered by the direct cell 75/0 event 41, which `0xc0bae8` produces only for record event `0xa46` (all.c:341469-341470), and from 75/27. Controller 64 never dispatched. Verdict: not reached by the ten crawl keys.

### Multi-level (class 6)

The two settings-gated posters were traced to their writers. Both writers are reachable through the ten keys, and both posters are dead code on this body, because the stub `0x006394d0` (decomp `FUN_009042bc`, exact, `app/ui/core/ui_extension_stubs.c:21`) returns 1.

- 74/26 events 22 (`ok`, `ui_m74_action_93`, NONMATCHING), 28 (`back`, `ui_m74_action_115`, exact) and 37 (`right`, `ui_m74_action_178`, NONMATCHING), which post only for last-move classes `0x213` and `0x21a`..`0x21d` (increment 3). Gate: controller 74 state 26, stored only by `ui_m74_action_476` (`0xc6c298`, NONMATCHING; all.c:416782-416849) at 74/4 event 151 when the dispatch candidate `0xc12aac()` is `0x3f` and the open node id is `0x5c`. The only literal open of node `0x5c` (registry `0x5c`, `0x0078ed04`) is in `ui_m74_action_170` (`0xc606c4`, NONMATCHING) at 74/4 event 37 (`right` on the live view), behind the test of live cell `0x00956844` = 7 (all.c:406971-406983). That test follows `if (FUN_006394d0() != 0)` (all.c:406936-406951), which always returns: it opens node `0xb` (crawl-5's only `right` open on the live view) or returns 0. Writer of `0x00956844`: `0x0063386c` (exact, `app/native_recovery/xm1_cached_settings.c:34`), called by the commit `0xd4fc74` (NONMATCHING) for property `0x1b1` (the value) and properties `0x139`, `0x183`, `0x1b2`, `0x1b5`, `0x1b7`, `0x1b8`, `0x214` (constants 4, 8, 0, 7, 5, 6, 1; all.c:545512-545778), the classes of node `0x5f` and its pages. Verdict: not reached; the gate is unreachable code, not a setting.
- 75/9 event 8 (`ok`, `ui_m75_action_98`, `0xc74b34`, exact, `ui_m75_actions_c746fc.c:186`): kind 2 when `UI_W(0xff78)` is 1 or `INPUT_W(0x2c)` (`0x00956864`) is 8 or 9; otherwise kind 2 again when `FUN_009042bc()` (`0x006394d0`, all.c:422198) is nonzero, which it always is; the `0x11`/`0x12` arm is unreachable. Writer of `0x00956864`: `0x0063396c` (exact, `xm1_cached_settings.c:74`), called by the commit `0xd4fc74` for property `0xed` (all.c:545282-545289), the property of node `0xb` (`0x007866f8`), which `right` opens on the live view; node `0xc`, the screen of 75 state 9, is that node's child. A ten-key path sets the cell, but the poster still posts kind 2, as crawl-5's trials 1322 and 1324 show. Verdict: not reached; no prefix or config is added.
- 75/4 event 8 (`ui_m75_action_93`, NONMATCHING, class `0xee`): state 4 is stored by `ui_m75_action_66` (75/0 `right`) and `ui_m75_action_90` (75/0 `ok`) after their kind `0xe` post on a node of class `0x26` (all.c:419942-419950), and by two actions of unreached states 5 and 6. No decoded node has class `0x26` or `0xee`. Verdict: not reached by the ten crawl keys; gate controller 75 state 4.
- 75/25 and 75/29 event 50 (`ui_m75_action_286`, NONMATCHING; `_290`, exact): state 25 is entered only from state 22, which is entered from states 21, 23, 24, 25, 26 and 29. None was reached, and every static chain from a reached state enters 21 by 75/0 event 50 or 29 by the direct cell (above). Verdict: not reached by the ten crawl keys; gate controller 75 state 21 or 29.

### Seeking ids (class 7)

- Kind 1 with id `0x17`: `ui_m76_action_130` (`0xc81e30`, NONMATCHING) at 76/46 event 12 posts `(1, 0x3f, 0x17, 0xc)`; reached by `back` on node `0x7d` (increment 3, class 9). It stays fallback for increment 3's reason.
- Kind 1 with id `0x18`: one poster, `ui_m74_action_322` (`0xc67d78`, exact, `ui_m74_actions_c67b90.c:143`), at 74/4 event 90, posting kind 1 on node 1 with id `0x18` and extra 8 (`0xd3d7f0(1, 8)`) when `FUN_00d401c8()` and `FUN_00d1147c()` are nonzero. `0xc09d24` produces event 90 from code `0x39` (all.c:340762-340763); no bridge key dispatched controller 74 event 90 in crawl-5. Verdict: not reached by the ten crawl keys; needs code `0x39`.
- Kind `0xe` with id `0x17`: the 18 calls of `0xd3d78c` are in 74/27 (events 13, 30, 33, 35, 37, 102) and 75/42 (events 0, 2, 4, 6, 22, 23). All.c stores neither controller 74 state 27 nor controller 75 state 42, and no direct-state cell enters them. Verdict: not reached; no static entry into either gate.
- Kind 1 with ids `0xe` and `0x10` (all.c:331629-331635): `0xd3d75c` from `ui_m64_action_25` (`0xc4b2f8`, exact) and four other controller 64 actions at 64/0 events 7, 12, 27, 28, 36. Controller 64 is dispatched only by `0xc06b7c` (`0xbfb028(0x40)`). Crawl-5 dispatched 57/0 event 49 (`play`, trial 2444, action `0xc3f3dc`), which calls `0xc06b7c` only when `0x009503a0` is 6 (all.c:378367-378371); the other entry is controller 57 state 1, which the direct-state cells 57/38 events 81 and 82 enter, and crawl-5 reached state 38 but never dispatched those events. Crawl-5 never dispatched controller 64. Verdict: not reached by the ten crawl keys; gate `0x009503a0` = 6 at 57/0 event 49, or 57/38 events 81/82. Id `0x13` (the open of node `0x2a`) is not a fallback class: `0xbfad04`'s default arm returns 1 with no call (all.c:331645-331647), and the engine's `StageId::seeks` (`0xe..=0x12`, `0x17`, `0x18`) does not refuse it, so it is owned with kind 1.

### Kinds `0xa`, `0xb` on a framed node (class 8)

`0xbfad04` takes the framed arm, a push, instead of the seek when the open node has type 5 or 7. Crawl-5 has no kind `0xa` or `0xb` post on a type 5 or 7 screen. Not reached; the engine keeps refusing it with `FramedNode`.

## Retirement scan

Functions under `0xbfa234`: the callee closure of `0xbfa234` in all.c restricted to the engine unit (`0xbf8d20`..`0xbfb028`). For each, its callers in image4, image5 and overlays 44, 46 and 47, its decomp status, and what still reaches it after increment 4.

| Function | Status (55436fc85c) | Callers (scan) | Reached after increment 4 by |
| --- | --- | --- | --- |
| `0xbfa234` kind switch | NONMATCHING | bl from `0xbf95c8`, `0xbfa800` | the adapter's fallback call for every fallback class; the `kind_switch` service (empty node) |
| `0xbfad04` | NONMATCHING | `0xbfa234` only | classes 7, 8; node `0x38`; kind 1 or `0xe`/`0xf` refused before the call; the `seek` service after a delegated pop |
| `0xbfab98` | NONMATCHING | `0xbfa234`, `0xbfad04` | class 6; class 4 until owned; push or pop refusals (`PackageChild`, `NoChild`, `NoParent`, `StackBounds`) |
| `0xbf9f44` | NONMATCHING | `0xbfa234`, `0xbfad04` | class 5; class 7 seeks that step |
| `0xbf9c1c` | NONMATCHING | `0xbf9f44` | kinds `0x45`..`0x48` under a bounds refusal (`RowTotal`, `RowBounds`) |
| `0xbfa800` | NONMATCHING | `0xbfa234`, `0xbfab98` | every fallback open, push or pop |
| `0xbf9b1c`, `0xbf988c` | NONMATCHING | `0xbf9f44`, `0xbfa800` | class 5; every fallback open |
| `0xbf9a5c` | NONMATCHING | `0xbfa234`, `0xbfab98` | every fallback close, push or pop |
| `0xbf976c` | exact | `0xbfa234`, `0xbfab98` | every fallback open, push or pop |
| `0xbf985c` | exact | 9 bl in the engine | every fallback open |
| `0xbf9990` | NONMATCHING | `0xbf9a2c` | `0xbf9a2c` |
| `0xbf9a2c` | exact | 16 bl, including `0xbf95a8` | retained interface (the redraw `0xd4c8d4` through `0xbf95a8`) |
| `0xbf9794` | NONMATCHING | 12 bl from `0xbf8d20`..`0xbf93e0` and `0xbf985c` | shared with tree code outside the kind switch |
| `0xbf95a8`, `0xbf95b8`, `0xbf90f0` | exact, exact, NONMATCHING | `0xd4c8d4`, `0xd588b8`; `0xbf90f0` also from overlay 44 | the `initial_row` service (`0xd588b8`), the redraw, and overlay 44 |
| `0xbfaf84` | NONMATCHING | `0xbf95b8` | the `initial_row` service through `0xbf95b8` |

Result: no function under `0xbfa234` is retired at increment 4. Classes 5 to 9, the empty node, the restored cursor past the rows and the generic refusals keep the whole kind switch reachable, and every function reached only from the engine is reached through `0xbfa234` or a service.

Reference scan (`reference-scan.js`, run over `id4-0061c400.bin` sha256 `f2e2acab…dcff16`, `id5-00bf1a00.bin` sha256 `3cd501c0…fd892a`, and overlays 44 (`0x01100000`, `0xac5a0` bytes), 46 (`0x011c0000`, `0x1cca0`) and 47 (`0x011e0000`, `0x26a5e0`) of `plain-01100000.bin` sha256 `ee211bda…e2d889d`, extents from `reports/xm1-decomp.md`): for the 18 functions it looks for word-aligned and halfword-aligned 32-bit words equal to the address or the address with bit 0 set (pointer tables, literal pools), A32 `b`/`bl`/`blx`, Thumb `bl`/`blx`, A32 `movw`/`movt` pairs into one register within 64 bytes, and A32 `add`/`sub` from `pc`. It found no pointer word, no A32 `movw`/`movt` pair, no `pc`-relative address and no Thumb `bl`/`blx` in any of the five. Image4, overlay 46 and overlay 47 have no reference. Image5's references are the A32 `bl` sites in the table, each inside the engine unit, `0xd4c8d4` or `0xd588b8`. Overlay 44 has seven A32 `bl` to `0xbf90f0` (at `0x01164104`, `0x01164280`, `0x01165758`, `0x011660e4`, `0x011668a8`, `0x0116bbd0`, `0x0116bc7c`; all.c places them in `0x01163f2c`, `0x01164e1c`, `0x01165aa0`, `0x011662a0`, `0x0116b610`) and none to another function under `0xbfa234`. This is the check a later retirement must repeat: a function retires only when the scan shows callers only in already-retired code. Limits of the method: it does not decode Thumb `b.w`, Thumb `movw`/`movt` or Thumb `adr`, and it does not see a register-indirect call through an address computed other than by these patterns. A later retirement that relies on the scan must add the Thumb decodings first.

## Shims

New or changed by this increment; the rest of the design's and increment 3's tables stand.

| Shim | What it does | Removal condition |
| --- | --- | --- |
| Stock kind switch and seek as services (narrowed) | `kind_switch` for `0xbfa800`'s empty arm; `seek` only after that delegated pop, when the landed node may have type 5 or 7 | The empty-node arm is owned |
| Fallback classes through the stock kind switch (narrowed) | Classes 5 to 9, class 4 until its case passes, the empty node, the restored cursor past the rows, node `0x38`, and the bounds refusals | Every class is owned |

No class adds an owned key cell; the seven frozen picker cells stay. No per-cell owned dispatch is introduced: classes 1 to 4 are reached through the stock cells and stage consumers.

## Limits of the oracle

- The framed parents and node `0x787cbc` have Remember 0, so no case shows which index (stage node or landed id) the framed step passes to `0xbf9a5c`.
- `framed-pop` has id `0x1d`, so `FramedNotifyIdSwap` cannot fail it; `framed-pop-back` and `framed-pop-group41` carry that control.
- The open direction of the framed step (`0x15`) is visible only if the restored cursor's row is not selectable; every target here has fixed or all-offered rows **[INFERENCE]**.

## Decisions

Decided by ControllerStrategy on this design:

- Keys: no bridge key is added in this increment. Gates outside the ten `CrawlKey` keys are recorded as "not reached by the ten crawl keys" with the codes they need (table under "Key reach of the fallback classes").
- Multi-level settings prefixes: the writers of `0x00956844` and `0x00956864` are traced (class 6). Both posters behind those cells are dead code because `0x006394d0` returns 1, so no prefix and no class 6 config is added.
- `NoChild` and `NoParent`: to be owned as `Rejected`, but crawl-5 has no case, so they stay fallback as class 9.
- Class 4 is owned only if its probe records `9, 0x7a, 1`.
- The `seek` service stays for a framed landing after a delegated pop.

Open:

- Retirement needs the generic refusals gone, not only the classes: while `RowTotal`, `RowBounds`, `StackBounds`, `PackageChild`, `NoChild` and `NoParent` fall back, every function under `0xbfa234` stays reachable, and overlay 44 keeps `0xbf90f0` live regardless.
- A case for class 9 needs a receipt with such a post: an observation that searches for one, or the guest pairs' traced stage entries (620 events in E-MENU-ENGINE-INC3-GUEST-PAIRS-150, not classified for it here).

## Appendix: configs

Each block is `emulation/menu-oracle-frame-push-probe.json` with `path` and `key` replaced, ready to commit as `emulation/menu-oracle-<name>.json` (copies in `private-data/scratch/ControllerStrategy/inc4/`). The trace sites and write ranges are the same in every block and are listed once:

```json
{
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
  ]
}
```

| Config | `path` | `key` | Expected events for `menu-engine-cases` | Closing BACKs (crawl-5 nodes) |
| --- | --- | --- | --- | --- |
| `menu-oracle-framed-open.json` | `[]` | `"down"` | `1,0x5f,0` | 1 (146, 143) |
| `menu-oracle-framed-open-restored.json` | `["down", "down", "ok"]` | `"down"` | `1,0x5f,0` | 1 (237, 143) |
| `menu-oracle-framed-pop.json` | `["ok", "up", "left", "down", "ok", "ok"]` | `"left"` | `0xf,0x43,0x1d` | 1 (227, 231) |
| `menu-oracle-framed-pop-back.json` | `["ok", "up", "left", "down", "ok", "ok"]` | `"back"` | `0xf,0x43,0x1a` | 1 (227, 231) |
| `menu-oracle-framed-pop-group41.json` | `["ok", "up", "left", "down", "ok", "down", "ok", "down", "ok"]` | `"back"` | `0xf,0x41,0x1a` | 2 (485, 229, 231) |
| `menu-oracle-framed-push.json` | `["ok", "up", "left", "down", "ok", "down", "ok", "down"]` | `"ok"` | `0xe,0x3f,0x1d` | 3 (486, 485, 229, 231) |
| `menu-oracle-frame-push-up-probe.json` | `["q", "down", "down"]` | `"dial-left"` | `9,0x7a,1` | 1 (254, 143) |

Each file is `{"schema": 1, "path": PATH, "key": KEY, "trace_sites": [as above], "write_ranges": [as above], "limit": 65536}`. For example:

```json
{
  "schema": 1,
  "path": ["ok", "up", "left", "down", "ok", "ok"],
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
