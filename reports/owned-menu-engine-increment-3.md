# Owned menu engine, increment 3: event classes (#128)

Status: design for increment 3 of [the engine design](owned-menu-engine-design.md), cleared by MenuEngineIncrement3DesignReview and built at 330a5c3c. All 28 configs (checked in as `emulation/menu-oracle-*.json`, named in the class table) ran once; each yields an oracle case that passes and each control fails cases of its class (E-MENU-ENGINE-INC3-HOST-ORACLE-150, [host-oracle workflow](../replacement/menus.md#engine-inputs-and-host-oracle)). 56 of 57 guest pairs pass, and oracle-nowrap-edge fails in both roles at the package unload (E-MENU-ENGINE-INC3-GUEST-PAIRS-150). Increments 1 and 2 owned kind 4 and 5 row steps, kind `0xe` push, kind `0xf` pop and kind 1 open on wrapping nodes with queried availability (E-MENU-ENGINE-HOST-ORACLE-150, E-MENU-ENGINE-GUEST-PAIRS-150). [Increment 4](owned-menu-engine-increment-4.md) takes the framed `0xbfad04` targets and kind 9.

Host results at 330a5c3c (`private-data/scratch/ControllerStrategy/menu-engine/host-oracle-330a5c3c/owner-test.log`): every case passes. `frame-seek-step` passes and `NoSeekStep` fails it, so the one-step seek of class 3 is owned, with the pre-check over every row's child. `frame-push-probe`'s stage entry is `(8, 0x7a, 1)` and its case passes, so kind 8 of class 12 is owned with class 4's change and `FrameNotifyId` fails it; kind 9 has no case and falls back. No receipt contradicts the design. `GroupAsRowStep` cannot fail `group-left` and `group-right`, which land where a one-row step lands; it fails the other six group cases, including both wrap cases of kinds `0x45` and `0x46`. The extractor requires the `rollback-restore` site only for a window without a special post, so the increment-1 configs, whose windows are all accepted, keep extracting.

Stock facts cite `private-data/analysis/x-m1-1.50/filmsim/ghidra/all.c` (the export of the id5 image `private-data/analysis/x-m1-1.50/lz/id5.dis`), node facts the decoded tables `private-data/scratch/ControllerStrategy/menu-tables.json` (E-MODAL-MENU-TABLES-150), controller cells the manifest `private-data/scratch/ControllerManifest/manifest.json` (E-MODAL-CONTROLLER-MANIFEST-150), and reach the crawl-5 graph (E-MODAL-MENU-OKPATHS-150). **[INFERENCE]** marks what the evidence does not show directly.

## How reach was derived

Crawl-5 records, for each of its 4,584 trials, the from-screen's key path, the 44 bytes of the record at `*0x009571d0` and the cursor `0x009571d8`, the controller dispatches with controller, state and event, and every entry of `0x0063270c` with r0-r3. An entry with r0 = `0x1e` is a stage post (code, kind, node, id); the extra word is not recorded. All 570 screens' records match a decoded node. The first stage post of a trial was classified from the from-screen's record and cursor by the rules of `MenuEngineClass.deriveClass` and the branches below; a later post of the same trial only by its kind and arguments, because the block between posts is not recorded. Counts are trials.

A path is crawl-5's key path from the cardless no-install root to the from-screen, the root `--menu-observe` walks from ([menu observation](../emulation/README.md)); the observed key is that trial's key, and each config names its crawl-5 node and trial. Crawl-5 replayed from held snapshots and 7 of its 25 replays diverged, so a path is expected, not known, to reach the same screen from the root **[INFERENCE]**. The extractor refuses a receipt whose stage entries differ from the expected events in the table.

Stage posts reach the engine through two consumers in crawl-5: `0x00ca4674` (controller 100, 1,568 dispatches) and `0x00ca4e98` (controller 101, 1,795). Both save the block (`0x0061d384`), call `0xd4dd2c`, restore the block (`0x0061d3a8`) when it returns 0 and otherwise run the special post `0xd42a64` (all.c:457563-457633, 458265-458340).

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

Owned by this increment: 1a-1d, 2, 3 (including the one-step seek), 4, 5, 6, 7, 8, 10, 11 and kind 8 of class 12. Kind 9 of class 12 and classes 13 to 17 stay fallback (see "Classes that stay fallback"); [increment 4](owned-menu-engine-increment-4.md) owns kind 9 and the framed targets. "Not reached" means crawl-5 has no trial of the class; it does not mean no key can reach it.

## Shared boundaries

- Compound pre-check: `apply` returns `NotOwned` only before its first service call. Kinds 6..`0xb` run several stock steps whose targets can depend on availability answers, so the engine checks every target such an event can reach before the first call. Kinds 6 and 7 check every row child of the source's parent, since the inner move picks the page; the inner pop writes the live frame to `stack[depth]` and the inner push restores that slot (all.c:331591-331594, 331575-331578), so the push restores the live frame at entry and the block's `stack[depth]` at entry is stale and not read. Kinds 8 and 9 check the cursor row's child, the restored frame `stack[depth + 1]` and the source as the pop target. Kinds `0xa` and `0xb` check every row's child, `stack[depth + 1]` and the source. The check is static over the tree and the block at entry. It can refuse an event stock would complete, never the reverse.
- Config base: the 28 configs are checked in as `emulation/menu-oracle-<name>.json`, named in the class table, and share `emulation/menu-oracle-frame-push-probe.json`'s 28 trace sites and six write ranges; the group configs add `0xbf9c1c`. The write ranges are those of `emulation/menu-oracle-push-classifier.json`.
- Increment-specific shims: the stock rollback restore for rejected events, where a rejected owned event relies on the consumer's `0x0061d3a8` restoring the block, and the fixed-availability row-state clear, where the engine sets the `0x4f` row-state words to 0 as `fill_u32(0x009571f0, 0, 0x4f)` does. The design's [shim table](owned-menu-engine-design.md#shims-for-this-unit) owns the rest.

## Classes

### 1. RememberBranch: push and pop with the other `0xbf9a5c` branches

Stock. `0xbfab98` calls `0xbf9a5c(node)` before switching nodes on push (all.c:331562-331580) and pop (331582-331601). `0xbf9a5c` returns when no node is open or node word 6 (Remember) is not 1 (330942-330947); otherwise it stores the cursor row's token at `0x0095733c[node]` when the node has no parent, or when the parent's id differs from the node's (330948-330955). Increment 2 owns only a push whose write runs (parent id differs) and a pop whose write does not (parent id equal).

Value. All 7 RememberBranch events of the guest pairs: 5 pops in the unloaded-picker segments of pickers 26, 163, 214, 312 and 314, and in oracle-push-classifier one push with the same parent id and one pop with another parent id (E-MENU-ENGINE-GUEST-PAIRS-150; classified from each pair's stage-entry registers and block). Picker 312's pop is from node `0x1e` (record `0x0078c790`, Remember 0); the other four pop with the write. Crawl-5: 208 pops with the write (including `left` on the shooting root, `ok`, `left`), 69 pops and 49 pushes with Remember off, 4 pushes with the same parent id.

Paths. `pop-remember` pops node `0x1f` (record `0x0078cf64`, the node of picker 26's pop, cursor 1) to the shooting root. `pop-remember-off` pops node `0x1e` (`0x0078c790`, picker 312's node) from cursor 1 (crawl node 180, trial 1003), and `push-remember-off` pushes from the same screen into `0x0077dd34` (trial 980). `push-same-parent` pushes from `0x00783de0` (id 1, parent `0x0078ea98` with id 1, cursor 20) into `0x0077c890`, the push oracle-push-classifier falls back on.

Controls. `PopNoRemember` (a pop never writes) fails `pop-remember`; `RememberIgnoresFlag` (writes with Remember 0) fails `pop-remember-off` and `push-remember-off`; `RememberIgnoresParentId` (writes when the parent has the node's id) fails `push-same-parent`. Each needs `remembered[node]` before the event to differ from the token written. For node `0x1e` that holds at cursor 1 and not at cursor 0: `0x0061cf14` sets `remembered[i]` to the row-0 token of registry entry i (id4.dis 61cf14-61cf64), registry `0x1e` is `0x0078c790` with row-0 token 0 and row-1 token 1, and no `0xbf9a5c(0x1e)` writes the slot, because `0x0078c790` has Remember 0 and its two children have parent id `0x1e`. For the other two cases the receipt decides; a case whose control does not fail is replaced.

### 2. Close (kind 2)

Stock. For id `0x1a`..`0x1d`: notify `0xd5b350(2, node, id, class of the open node)`, `0xbf9a5c(node)`, menu-open flag `0x009575fc` = 0, result 1; any other id gives 0 with no call (all.c:331325-331335). The wrapper then stores the snapshot at the open node's id as for every kind.

Value. Crawl-5 has 230 close posts, ids `0x1d` (169 first posts), `0x1a` (23), `0x1b` (2, each followed by a kind 1 open of node 0 in the same press). `back` on the shooting root closes it (node 1, id `0x1d`).

Path. `close`: `ok`, then `back`.

Control. `CloseKeepsMenuOpen` leaves `0x009575fc` at 1.

### 3. Kinds `0xa` and `0xb`: seek, push, move, pop

Stock. `0xbfad04(kind, node, type of the open node, 0x1d, 0x17, extra)`, then `0xbfab98(0xe, node, 0x15, 0)`, `0xbf9f44(4 or 5, node, id)` and `0xbfab98(0xf, node, 0x15, 0)`, each run only when the previous returned 1 (all.c:331371-331384). In `0xbfad04` kinds 1, `0xa`, `0xb` and `0xe` (mask `0x4c02`) take the switch on the seek id (331627); id `0x17` steps the cursor down with `0xbf9f44(4, node, 1)` until the cursor row's token equals `extra`, at most total-rows times (331649-331670), and id `0x18` pushes when it is found. An open node of type 5 or 7 pushes instead (331620-331625).

Value. Shooting menu: node `0x81` (record `0x0078cf38`, three rows whose children `0x007912c0`, `0x0078ecac`, `0x00789f24` have 10 rows each), under node `0x80`, under the shooting root, with controller 75 in state 11. Node `0xc` (`0x0078231c`, row tokens 0 and 1) with controller 75 in state 9. Crawl-5: 31 trials each.

Extra word. The posters call `0xd3d81c(kind, id, extra)`, which posts `(0x1e, kind, node id, id, extra)` (all.c:534853-534860). In state 11, `up` is controller 75 slot 8 (`FUN_00c6e5a8`) and `down` slot 26 (`FUN_00c6ecb4`), which pass `0x0061d244()`, the cursor row's token, as the extra word (decomp `ui_m75_action_8`, byte-verified, `app/ui/machines/ui_m75_actions_c6e378.c:127`; id4.dis 61d244-61d290), so `0xbfad04`'s loop matches on its first comparison and makes no step. In state 9 the slots post constants (all byte-verified in the same decomp): `up` slot 7 `(0xb, 1, 1)` (`ui_m75_actions_c6e378.c:113`), `down` slot 25 `(0xa, 1, 1)` (`:383`), `left` slot 44 `(0xa, 1, 0)` (`ui_m75_actions_c6f814.c:29`), `right` slot 73 `(0xb, 1, 0)` (all.c:420574). On node 240 at cursor 0, `down` (trial 1323) seeks token 1 from token 0, one step, and crawl-5 lands at cursor 1.

Paths. `frame-seek-down` and `frame-seek-up` from node 215 (zero-step); `frame-seek-step` from node 240 (one step).

Controls. `FrameNotifyId` (the inner notifications pass the event id instead of 0) fails the zero-step cases. `NoSeekStep` (skip the seek loop) cannot fail a zero-step case; it must fail `frame-seek-step`.

Ownership. The seek that steps is owned: `frame-seek-step` passes and `NoSeekStep` fails it, so the pre-check covers every row's child, `stack[depth + 1]` and the source (see "Shared boundaries").

### 4. Kinds 6 and 7: pop, move, push

Stock. Kind 6: `0xbfab98(0xf, node, 0x15, 0)`, `0xbf9f44(4, node, id)`, `0xbfab98(0xe, node, 0x15, 0)`; kind 7 the same with the pop's direction `0x16` and move kind 5 (all.c:331347-331358). No `0xbfad04` runs, so the parent of type 5 or 7 that these kinds move in is not re-pushed.

Value. The framed groups: node `0x5f` (type 7, record `0x00787cbc`, six pages of type 7, opened from crawl-5's root screen by `down` as a kind 1 open of a type 7 node), and node `0x43` (type 5, `0x00783d94`, no-wrap, six pages, four of them with fixed availability, class 5 below). Crawl-5: kind 6 70 trials, kind 7 95, ids 1, 2 and 3. Ids 2 and 3 make the inner `0xbf9f44` take several steps, which class 11 owns.

Paths. `frame-pop-down` moves from the one-row page `0x00791294` to `0x0077d59c` (cursor 2 after). `frame-pop-up` moves from `0x0077d59c` at cursor 2 back into the one-row page `0x00791294`, so `0xbfa800` opens a one-row node with restored cursor 2. Had `0xd588b8` returned `0x7ffffffe`, the cursor would stay at 2, whose row state is 3, and `0xbf9b1c` returns 0 for a total of 1 (all.c:330974-330977), so the push and the event would fail; crawl-5 shows the event landing on `0x00791294` at cursor 0, so `0xd588b8` returned a row token and this case does not reach class 16.

Control. `FrameNotifyId` passes the event id instead of 0 to the inner notifications.

### 5. Fixed availability (node word 5 = 1)

Stock. `0xbfa800` on such a node sets the total, clears the row states with `FUN_007221a0(0x009571f0, 0, 0x4f)`, leaves the visible count at open `0x009571ec` unwritten (331436-331437; the queried arm writes it at 331502), queries no availability, has no empty arm (331435-331438) and keeps the initial or restored cursor without a step (331514-331534). The step `0xbf9b1c` accepts every row (331001) and the count and ordinal functions get no row-state array (`0xbf985c`, `0xbf9a2c`).

Value. 16 nodes have node word 5 = 1: four of framed group `0x43`'s six pages (`0x00784afc`, `0x00789b8c`, `0x00790790`, `0x0078d2fc`), all eight children of node `0x7d` and four children of node `0x7e`. Crawl-5 has 116 moves on `0x00784afc` and `0x00789b8c`, and kinds 6 and 7 enter them.

Paths. `frame-fixed-open`: kind 6 with id 2 from `0x0078ecd8` (cursor 2) into `0x00789b8c`, which opens a fixed node and moves the no-wrap parent two pages. `fixed-wrap`: kind 4 at the last row (11) of `0x00789b8c`, which wraps to 0.

Controls. `QueryFixed` asks availability on a fixed node and fails `frame-fixed-open`; the existing `NoWrap` fails `fixed-wrap`.

### 6. Node `0x7a`: property `0x285`

Stock. After the remember branches, `0xbf9a5c(0x7a)` writes property `0x285` through `0x0061c91c(0x285, 0xbf9a2c(0x0095733c[0x7a], 0), 0)`, the row index of the remembered token in the open node (all.c:330956-330960). It runs on close, push and pop with node `0x7a`.

Value. Node `0x7a` is registry slot `0x7a`, record `0x0078e4a0` (16 rows, Remember 1, no parent), opened by `q` on crawl-5's root screen (trial 1934 posted `1, 0x7a, 0`). Crawl-5: 30 closes of node `0x7a` (`back` or `ok`), no push or pop with it.

Path. `close-property`: `q`, `down`, `down` (two kind `0x48` moves to cursor 9), then `back`.

Control. `NoPropertyWrite` omits the effect. A control that writes the cursor's row index instead does not discriminate here, because the remember write just stored the cursor's token.

### 7. Kinds `0x45`..`0x48`

Stock. `0xbf9f44` calls `0xbf9c1c(0x45, 0x16)`, `(0x46, 0x15)`, `(0x47, 0x16)` or `(0x48, 0x15)` (all.c:331210-331213, 331268-331281). Kinds `0x47` and `0x48` step one row at a time with wrap, count rows not hidden, and accept at the fourth when its state is below 2, at most total-1 steps (331034-331072). Kinds `0x45` and `0x46` move within a group of four by the row's visible ordinal modulo 4 (331074-331180). On success the shared tail scrolls with `0xbf988c(..., kind)`, which for these kinds keeps the top unless the total is below rows per page (330827-330878), notifies, writes the last-move pair and stores (331283-331296).

Value. The screen Q opens (node `0x7a`, 16 rows, controller 75 state 43): `left` `0x45`, `right` `0x46`, `up` `0x47`, `down` `0x48`. From cursor 9 crawl-5 lands at 8, 10, 5 and 13.

Paths. `group-left`, `group-right`, `group-up`, `group-down`, all from node 254 (cursor 9; crawl-5 lands at 8, 10, 5, 13), stay inside the rows and inside a group of four. The wrap set takes the other code: `group-right-wrap` from cursor 7 (node 262, lands at 4), `group-left-wrap` and `group-up-wrap` from cursor 4 (node 256, land at 7 and 12), `group-down-wrap` from cursor 12 (node 259, lands at 4, past row 0, which no crawl-5 screen of this node has as its cursor).

Control. `GroupAsRowStep` runs a kind 4 or 5 one-row step.

### 8. No-wrap nodes (node word 4 = 1)

Stock. `0xbf9b1c` returns 0 at the first or last row instead of wrapping (all.c:330980-331006); `0xbf9f44` then returns 0 and the consumer restores the block (457579-457584, 458281-458286).

Value. Nodes `0x7d` (`0x007830d0`, class `0xc0`), `0x7e` (`0x00787b78`, class `0xc9`) and `0x48` (`0x00786d50`), and framed group `0x43`'s parent. Crawl-5: 60 moves, 6 at an edge.

Paths. `nowrap`: kind 4 at cursor 0 of `0x007830d0` (lands at 1). `nowrap-edge`: kind 5 at cursor 0, rejected.

Control. `AlwaysWrap` ignores node word 4 and fails `nowrap-edge`.

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

### 11. Row steps with id above 1

Stock. Kinds 4 and 5 repeat the step until `id` steps are made or the cursor passes its start (all.c:331215-331266). The engine and checker own every nonzero non-page id, but the ten oracle cases all have id 1. The inner moves of kinds 6 and 7 run the same loop with the event's id (class 4), so they are owned by this class, not restricted to id 1.

Value. Node `0x56` (`0x007894e0`, type 6, 43 rows, 42 per page) posts ids 4 to `0xd` from `FUN_00d48ab4` (540908); crawl-5 has 86 such trials, and 27 kind 7 events with ids 2 or 3.

Paths. `step-rows`: `down` at cursor 0 posts id `0xd` and stops after 13 steps at 13 (trial 2067). `step-rows-wrap`: `up` at cursor 0 posts id `0xd`; the first step wraps to 42, past the start, and the loop stops there (trial 2208, crawl-5 lands at 42).

Controls. `SingleStep` makes one step and fails `step-rows`. `NoPassStop` keeps stepping after the cursor passes its start and fails `step-rows-wrap`.

### 12. Kinds 8 and 9: push, move, pop

Stock. `0xbfab98(0xe, node, 0x15, 0)`, `0xbf9f44(4 or 5, node, id)`, `0xbfab98(0xf, node, 0x15 or 0x16, 0)` (all.c:331359-331370). The only posters are `FUN_00c7900c` (8) and `FUN_00c79178` (9), controller 75 action slots 212 and 217 at state 43, events 30 and 33, and 31 and 34 (manifest; all.c:425583, 425679).

Value. Controller 75 is in state 43 on the screen Q opens. Crawl-5 tried only `ok`, `down`, `up`, `right`, `left` and `back` there (controller 74 state 24 is its context), and controller 75 received events 0, 2, 4, 6, 8, 13, 47, 49 and 50 only.

Key layer. In controller 74 state 24, events 88 (crawl-5's `dial-right`) and 89 go to slot 7, `FUN_00c58f58`, which calls `0x00c0bae8` (all.c:400582-400586; manifest). The decomp names `0x00c0bae8` `ui_m75_event_dispatch` (NONMATCHING draft, `app/ui/machines/ui_machine_actions_00c077d0.c:2429`); it chooses controller 75's event from the command code at `0x0094f64c`: event 30 for codes `0x1d` and `0x207`, 31 for `0x1e` and `0x208`, 33 for `0x2d`, 34 for `0x2c`. Neither the decomp nor crawl-5 records which key produces those codes; crawl-5's `dial-right` never dispatched controller 75 on any screen. So crawl-5 does not reach class 12, and the decomp alone does not identify the key; the probe below supplies `dial-right` for kind 8.

Path. `frame-push-probe`: `q`, `down`, `down`, then `dial-right`. The receipt's stage entry is `(8, 0x7a, 1)`, so kind 8 is owned with class 4's change and `FrameNotifyId` fails it. Kind 9 has no receipt here and falls back; [increment 4](owned-menu-engine-increment-4.md) owns it with the `dial-left` probe.

### 13 to 17

See "Classes that stay fallback".

## Classes that stay fallback

Each stays behind the shim "Fallback classes through the stock kind switch" ([design](owned-menu-engine-design.md#shims-for-this-unit)), whose removal condition is that every class is owned; the class row in the checker stays `FallbackEvent`.

- Page step (kinds 4 and 5 with id equal to rows per page and a nonzero node, all.c:331201-331209). The posters with an id that can equal a node's rows per page are `0xd3d858(4, 4)` and `(5, 4)` in `FUN_00c6ed28` and `FUN_00c6e658`, controller 75 slots 28 and 11 at state 27, events 0 to 3 (all.c:418670, 418276), and in `FUN_00c4803c` and `FUN_00c47f3c`, controller 64 slots 11 and 6 at state 8 (387662, 387590). The six nodes with 4 rows per page include `0x007823a4`, the film-simulation offer list of `emulation/ClassicChromeSiteCases.hs`, which crawl-5 visited at controller 75 state 0. Crawl-5 never reached controller 75 state 27, and controller 64 is in state 0 on all 570 screens. State 27 is set by `FUN_00c6fe98`, `FUN_00c726bc`, `FUN_00c75914`, `FUN_00c75c70`, `FUN_00c77670`, `FUN_00c7ad48` and `FUN_00c7ade8` (all.c:419544-427359); two of them follow a push whose last-move class is 299 with token 2 or 3. Observation needs a route into either state, which no crawl records. The `0xd48ab4` steps (4 to `0xd`) are row steps on node `0x56` (class 11).
- Multi-level (kinds `0x10`..`0x12`, all.c:331395-331407). Posters: controller 74 state 26, events 22, 28, 37 (`FUN_00c5c3a0`, `FUN_00c5d9bc`, `FUN_00c60e8c`; the first two post only for last-move classes `0x213` and `0x21a`..`0x21d`, all.c:403506-403510, 404516-404529); controller 75 state 4 event 8 (`FUN_00c74424`, class `0xee`), state 9 event 8 (`FUN_00c74b34`), states 25 and 29 event 50 (`FUN_00c7abd8`, `FUN_00c7ade8`). Crawl-5 never reached controller 74 state 26 or controller 75 states 4, 25 and 29; it reached state 9 on node `0xc` twice (trials 1322, 1324), where `FUN_00c74b34` posted kind 2 because `0x009544f8` = 1 or `0x00956864` is 8 or 9 (all.c:422188-422194). Kind 1 with ids `0xe`..`0x12` (the same `0xbfad04` arms, 331633-331645) was not posted. Observation needs a state where `FUN_00c74b34` reaches 422206.
- Empty node (`0xbfa800`'s arm at all.c:331459-331465). No crawl-5 push or open landed elsewhere because its target had no selectable row: every push or open whose landing record differs from the target is a type 5 or 7 target re-pushed by `0xbfad04`. OK on rows whose child has no rows posted kind 2 or no stage event. The engine keeps its owned empty arm (synthetic tests only) with the `kind_switch` service, and `NotOwned::EmptyNode` for a node without rows.
- Restored cursor past the rows with `0xd588b8` returning `0x7ffffffe` (`token_past_rows`). No crawl-5 screen has a cursor at or past its node's rows; for a node with queried availability the open's step moves such a cursor unless the node has one row, in which case the step fails and the open with it, and a fixed node keeps it. `frame-pop-up` (class 4) enters a one-row page with cursor 2 and is accepted, so it does not reach this class. The shim "Values past the rows" stays.
- Kind `0xe` with id `0x17`, kind 1 with id `0x18`, kind 1 with ids `0xe`..`0x12`: not reached by crawl-5, whose kind `0xe` posts all carry id `0x1d` and kind 1 posts ids 0 and `0x17`. Key-action senders exist, posting through `0x0063270c` directly rather than `0xd3d858`:
  - `FUN_00d3d78c(kind, extra)` posts `(kind, node id, 0x17, extra)` (all.c:534820-534828); its 18 calls all pass kind `0xe`, from controller 74 slots 71, 130, 147, 163, 179 and 379 in state 27 and controller 75 slots 19, 34, 64, 88, 201 and 202 in state 42 (manifest). Slot 19, `FUN_00c6e9fc`, posts only when the last-move class is `0x1bb` (all.c:418457-418475).
  - `FUN_00d3d7f0(node, extra)` posts kind 1 with id `0x18` (534843-534847); its one call, `FUN_00c67d78(1, 8)` (412778), is controller 74 slot 322 at state 4, event 90.
  - `FUN_00d3d75c(node, id)` posts kind 1 with the given id (534810-534814): ids `0x10` and `0xe` for node `0x2d` from controller 64 slots 25, 38, 75, 76 and 89 at state 0 (all.c:389239-392628), and id `0x13` for node `0x2a` (392595).
  Crawl-5 never reached controller 74 state 27 or controller 75 state 42, never dispatched controller 74 event 90, and never dispatched controller 64. Kind `0xf` is outside `0xbfad04`'s seek mask, so a pop with id `0x17` or `0x18` runs only its `0xbfab98` (331385-331393, 331627). Kind 1 with id `0x17` is class 9.

## Decomp cross-check

Source: x70view branch `decomp/xm1-coverage` at 55436fc85c0560dd252b0410af6f2427fe198cd3 (`private-data/scratch/xm1-decomp/guide.md`), read from committed refs only. Names in it are inferences; the facts below come from the code.

Byte-verified (no NONMATCHING marker), so source-inspected: the engine wrapper `0xbf95c8` (`app/ui/core/ui_menu_tree_accessors.c:60`), `0xbf976c` and `0xbf9a2c` (`app/ui/core/tu_00bf8d20_exact.c:336, 352`); `0x007221a0`, a word fill (`app/runtime/byte_strings.c:187`), which settles the class 5 question; `0x0061d384` and `0x0061d3a8`, which copy the whole 0x430-byte block to and from a backup (`app/native_recovery/xm1_menu_state.c:138-144`), so a rejected event's restore rewrites the entire block; both stage consumers, `0x00ca4674` and `0x00ca4e98` (`app/ui/machines/ui_machine_actions_00ca4674.c:34, 69`), with the bracket, special post and value post as read above; `0x00c74b34` (`app/ui/machines/ui_m75_actions_c746fc.c:186`), which gives kind 2 when the `0xff78` UI word is 1, the `0x2c` input word is 8 or 9, or a status call returns nonzero, otherwise `0x12` when the `0x2c` word is 10, else `0x11`, and is class 14's poster as read from Ghidra.

Drafts (NONMATCHING, "instruction selection and ordering differ"): `0xbfa234`, `0xbfa800`, `0xbfab98`, `0xbfad04`, `0xbf9f44`, `0xbf9c1c`, `0xbf9b1c`, `0xbf9a5c`, `0xbf988c` and `0xbf9794` (`app/ui/core/tu_00bf8d20.c`). `0xd588b8`, `0xd4dd2c`, `0xd5b350` and `0xd48ab4` are untuned Ghidra drafts. Every engine function a class exercises is a draft, so each class still needs its oracle receipt for byte qualification; the engine's branches are confirmed by those receipts, not cited from the C. The decomp's `Xm1MenuState` (`app/native_recovery/xm1_menu_state.h`) leaves most of the block as reserved bytes and adds no field the block decode lacks.

## Limits of the oracle

A wrong engine still passes the cases in these places:

- Group-move scroll: node `0x7a` is one 16-row page, so `ScrollRule::Keep` is indistinguishable from any other scroll rule in the group cases.
- Property value: a value equal to the cursor's row index passes `close-property`, because the remember write just stored the cursor's token.
- `group-left` and `group-right`: node `0x7a` has no hidden or disabled rows, so `0xbf9c1c`'s arms away from a group boundary land where a one-row step lands; the wrap cases cover the boundary arms.

Kind 9 (push, move up, pop) has no receipt, so `apply` and the checker leave it to stock.

## Decisions

- The compound pre-check refuses kinds 6..`0xb` when any reachable target fails the open conditions, where stock would complete. The alternative, letting `apply` refuse after a call, breaks the design's rule that the event completes once committed.
- The fallback classes 13 to 17 stay stock-backed; #128's acceptance covers the observed presses and the picker pairs, which none of them reach.

## Configs

The 28 configs are checked in under `emulation/` with the names in the class table, and their expected events are that table's. `emulation/menu-oracle-frame-push-probe.json` is the base; the others differ only in `path` and `key`, except `close-property` (key only) and the group configs, which add the `engine-group-move` site `0x00bf9c1c`.
