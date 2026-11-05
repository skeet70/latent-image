# Owned menu engine, increment 5: Fn and fallback key gates (#128)

## Scope and sources

The Fn landing is an already owned ordinary open, not a page step or multi-level event. A live-view Fn release opens registry slot `0x63`, with stage arguments `(1, 0x63, 0, 0)`, and lands on ISO `(99, 321, 17)`. Setting `UI_W(0xff78)` to 1 is only one condition of the controller-75 state-21 gate. It does not make that gate unconditional.

Stock source is x70view commit `f659f02b850fcba58de0f9b09ee24ad09a24c2f4`. `//build-support:xm1-decomp lookup` supplies the exactness classifications; `show` supplies the source. The collected lookup/show receipt is `private-data/scratch/ControllerStrategy/inc5/decomp-gates.txt`. NONMATCHING source is a reading aid; byte confirmation below refers to the matched image-5 disassembly `private-data/analysis/x-m1-1.50/lz/id5.dis`. The existing table capture is `private-data/scratch/ControllerStrategy/menu-tables.json`, and the existing ten-key graph is `private-data/scratch/ControllerStrategy/crawl-5/crawl-graph.json`.

This increment uses the stock X-M1 1.50 DAT, SHA-256 `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`, the existing F7 adjustment NAND, the Fn-capable bridge, and the cardless no-install `--menu-observe` route. That observer has no unload phase. Each receipt records its landing rather than claiming that its menu was closed after observation. There is no camera run.

## Gates and concrete paths

### Fn open of `0x63`

The input release is code `0x28`, form `0x80`, dispatched to controller 74 event 5. `ui_m74_action_46` contains the live action at `0xc5a220`: property `0xfc == 0`, cached input `+0x100 == 0`, input `+0xfc == 2`, and nonzero `0xd40208()` select `0xd3d730(0x63)`. The bridge route and the earlier input-gate analysis are recorded in `private-data/scratch/ControllerStrategy/inc5/key-codes.md`; that file predates the sendable Fn route and its missing-route statements are historical.

The concrete sequence from the live view is `fn`. Configuration: `emulation/menu-oracle-fn.json`, retained unchanged from the input-route work. Receipt: `private-data/scratch/ControllerStrategy/inc5/fn/menu-observe.json`. It records stage `(1, 0x63, 0, 0)` and the literal-1 call to exact setter `0x00637f80`.

Registry slot `0x63` points to record `0x0077e54c`, type 9, class/property `0x141`, seven rows per page. It is not a type-5/type-7 framed target. Its ordinary kind-1 open is already `OwnedEvent Open` in `MenuEngineClass.hs`; no new kind, node exception, or class implementation is needed. The only child is at cursor 20, row token `0x1b`, record `0x0078f6a8`, node id `0x24`, class `0x14a`. Its three children also have id `0x24`, classes `0x14b`, `0x14f`, and `0x153`. None has id 8 or `0x6a`.

### Class 5: page step

Exact `ui_m75_action_11`, `0xc6e658`, posts `(5, 4)` at controller 75 state 27 events 0/1. Exact `ui_m75_action_28`, `0xc6ed28`, posts `(4, 4)` at state 27 events 2/3. Their settings guard is `0x0063a0f8(0x53) == 0`. Exact controller-64 actions `0xc47f3c` and `0xc4803c` post the same requests at state 8. The engine's page-step arm additionally requires id equal to the open node's rows per page.

Controller-75 state 27 has predecessors only in states 21, 28, and 29 in the increment-4 gate analysis. `ui_m75_action_274`, `0xc7a528`, is NONMATCHING. The state-21 branch is confirmed by instructions `0xc7a588..0xc7a5d4`: load `0x009544f8`, require 1, call `0x0061d154`, compare the open node id with 8 and `0x6a`, compare the candidate with -1 and `0x12`, then store halfword 21 to controller 75's state. Fn opens id `0x63`, so its flag store alone cannot take that branch.

The ordinary ISO DOWN request uses id 1, not id 4 or the ISO page size 7. The availability-aware ISO-child prefix is `fn down down right`: cursor 19 is hidden, so the two DOWN presses move 17 to 18 to 20. A third DOWN wraps to 0. RIGHT on row token `0x1b` selects the child with id `0x24`, not either state-21 id. The initial three-DOWN probes are retained as negative observations, not classified as child push/pop receipts. Closing and reopening an unrelated menu is not evidence for preserving the flag or entering state 21.

Registry slots 8 and `0x6a` are null in the captured static registry, and none of its 332 decoded nodes has either id. [INFERENCE] A key path composed only of this ISO subtree's row moves, pushes, and pops cannot satisfy the node-id condition. This is not a proof that every possible application context or runtime-created node is unreachable. The controller-64 playback entry gate, `0x009503a0 == 6` at 57/0 event 49 or controller-57 state-38 events 81/82, is unchanged by opening ISO. No physical-key sequence into controller 64 state 8 is established here.

Reassigning Fn does not supply the missing `0x6a` node through the decoded assignment menu. `0xd41844` maps stored property `0xfc == 0xc` to node `0x6a`, but the property's conversion table puts stored value `0xc` at row token 14, which the X-M1 Fn selector does not expose. The selector's row token 12 is labelled INSTANT AF and maps to stored value 8, not `0xc`. `ui_m74_action_46` handles stored 8 through `0xd0d3ac` and execution request `0x62`, not an open of `0x6a`. This distinction is between a menu row token and a stored setting value.

### Class 6: multi-level

The controller-75 state-21 prerequisite is the same one above. State 25 is entered from state 22. NONMATCHING `ui_m75_action_286`, `0xc7abd8`, handles 75/25 event 50: candidate `0x33`, with `UI_W(0x10788) == 0`, posts `(0x10, 0x1d)`; a successful candidate advances to state 29. Instructions `0xc7ac1c..0xc7ac64` confirm these tests and the post. Exact `ui_m75_action_290`, `0xc7ade8`, handles 75/29 event 50: candidate `0x32`, zero `UI_W(0xcea8)` and zero `UI_W(0x10788)` post `(0x11, 0x1d)` and can return to state 21. These are candidate-result gates, not new key codes.

The other increment-4 routes remain distinct: state 4 requires a pushed node of class `0x26`, absent from the decoded nodes; the two settings-gated multi-level arms are dead behind exact constant-1 `0x006394d0`. Fn's ISO child is class `0x14a`, so it does not supply the class-`0x26` route. No concrete ISO-subtree sequence into states 21/22/25/29 is established, and no multi-level oracle is invented from a synthetic state write.

### Other increment-4 fallback classes

| Class | Gate | Effect of the Fn ISO landing |
| --- | --- | --- |
| 7a, kind 1 with seeking id `0x17` | The existing node-`0x7d` BACK path | Unchanged; existing `open-seek` remains the fallback receipt |
| 7b, kind 1 with seeking id `0x18` | Lens state 5 and GPIO0 bit7 low produce code `0x39`, controller 74 event 90 | Open gate, not a button; no lens work in this increment |
| 7c, kind `0xe` with id `0x17` | Controller 74 state 27 or controller 75 state 42 | No entering store or direct-state cell in the increment-4 analysis; Fn supplies neither |
| 7d, kind 1 with ids `0xe`/`0x10` | Dispatch of controller 64 through the playback gates above | Fn's controller-75 ISO path does not dispatch controller 64 |
| 8, kind `0xa`/`0xb` on type 5/7 | A framed open node and one of those posts | ISO and its AUTO ISO children have type 9 |
| 9, `NoChild`/`NoParent` posts | An actual kind-`0xe` leaf push or kind-`0xf` root pop | A key with no stage post is not such a case; the increment-6 injected refusal qualification is separate |

## Observation receipts

Each name below denotes `emulation/menu-oracle-<name>.json` and `private-data/scratch/ControllerStrategy/inc5/<name>/menu-observe.json`. Each observation ran once after acquiring a shared emulator slot; pre-observation `devenv` GC-root activation failures were retried without a guest run.

| Name | Path, then observed key | Prediction and observed result | Oracle case and control |
| --- | --- | --- | --- |
| `fn` | Live view, then `fn` | `(1, 0x63, 0, 0)`, landing `(99, 321, 17)`; matches | `fn:1,63,0`; `NoInitialRow`, `OpenScrollAtZero` |
| `fn-leaf-right` | `fn`, then `right` | No leaf push; no stage entry, still `(99, 321, 17)` | None: no engine event |
| `fn-auto-pop` | `fn down down down right`, then `left` | Initial push/pop prediction failed: hidden cursor 19 makes the third DOWN wrap to 0; no stage entry and still `(99, 321, 0)` | None: no engine event |
| `fn-auto-push` | `fn down down down`, then `right` | Initial push prediction failed at wrapped cursor 0; no stage entry, still `(99, 321, 0)` | None: no engine event |
| `fn-auto-open` | `fn down down`, then `right` | Ordinary push `(0xe, 0x63, 0x1d, 0)`, landing `(36, 330, 0)`; matches corrected path | `fn-auto-open:e,63,1d`; `NoPushClassifier` |
| `fn-auto-return` | `fn down down right`, then `left` | Initial pop prediction failed: the Fn flag and class `0x14a` suppress LEFT; no stage entry, still `(36, 330, 0)` | None: no engine event |
| `fn-instant-af` | `fn back`, existing crawl-5 path to Fn assignment, eight DOWN presses, `ok back`, then `fn` | The assignment's row token 12 is stored value 8, not the mapper's stored `0xc`; no stage entry, unchanged engine block, live-view frame | None: no engine event |

The assignment path in `fn-instant-af` is `fn back ok`, fourteen `down`, `ok`, eight `down`, `ok back`, then the observed `fn`. Its before PNG is the live view. Its menu tuple `(1, 5, 16)` is the retained engine cursor after menu close, not evidence that the menu remains visible. The original ISO receipt marks cursor 19 Hidden (`RowState` 3); that observed availability corrects the initial AUTO ISO path prediction.

The LEFT guard at `ui_m75_action_38`, `0xc6f410`, rejects class `0x14a` when the Fn flag is 1. `ui_m75_action_130`, `0xc76a64`, handles BACK differently: class `0x14a` with the flag set posts a close `(2, 0x1d)`, whereas its non-Fn branch posts a pop. Neither key supplies a multi-level post. These source gates explain the observed `fn-auto-return` no-op without treating it as `NoParent`.

## Owned boundary and controls

The `fn` oracle passes the full owned-engine block comparison against stock. It records 24 retained service calls, 22 snapshot writes, and no last-move write. There is no engine-block divergence for this kind-1 open. `NoInitialRow` fails `fn` with cursor 0 instead of stock's 17; `OpenScrollAtZero` fails it with scroll top 0 instead of stock's 11. `owner_controls_fail` requires both failures.

`0xd4dd2c` calls the engine before the kind-1 arm of `0xd4d7a4`. The latter's Fn flag write is outside the engine block `[0x009571d0, 0x00957600)`. It remains the named record-mode/application shim around the engine, not an effect the engine should reproduce. Its removal requires owning those record-mode and settings duties, not a special case for ISO in the generic engine. The pinned stage-source receipt is `private-data/scratch/ControllerStrategy/inc5/decomp-stage.txt`.

The first fresh host qualification, `private-data/scratch/ControllerStrategy/inc5/host-oracle-05c77716/owner-test.log`, reports four passing tests, no failures, and `owner exit=0`: all 46 receipt cases, the 25 controls, the existing three injected refusals, and receipt-block round trips. It confirms the two Fn control failures before both are made mandatory. `MenuEngineClass.hs` and the production engine remain intentionally unchanged for an already owned ordinary open.

The 47-case host qualification at `private-data/scratch/ControllerStrategy/inc5/host-oracle-59860071/owner-test.log` adds `fn-auto-open`; all four tests pass with `owner exit=0`. That push has seven service calls, 44 snapshot writes, and no last-move write. `NoPushClassifier` fails it at block offset `+0x428`, returning classifier 1 instead of stock's 2. The control is now required to fail this case. This remains the existing owned `Push` class, not new ownership.

## Reproduction

For a configuration named `NAME`, run the existing viewer under the project environment, with one host-wide emulator slot:

```sh
XM1_EMULATOR_SLOTS=1 devenv shell -- bazel-bin/emulation/xm1-viewer \
  --workspace "$PWD" --ram-push-e2e --cardless --fujimod --flash-candidate \
  --menu-observe "emulation/menu-oracle-NAME.json" \
  --firmware private-data/firmware/x-m1-1.50/FWUP0002.DAT \
  --installed-nand private-data/analysis/x-m1-1.50/stored-f7/emulation/inputs/f7-1.nand \
  --output private-data/scratch/ControllerStrategy/inc5/runtime-NAME \
  --receipt-output "$PWD/private-data/scratch/ControllerStrategy/inc5/NAME"
```

The host generator accepts `fn=private-data/scratch/ControllerStrategy/inc5/fn:1,63,0`. `private-data/scratch/ControllerStrategy/inc5/host-cases.sh` adds it to all 45 existing receipt cases, regenerates the private tree and cases, compiles the owned engine's receipt test, and runs its owner tests. It writes only into a fresh `host-oracle-<sha>` directory.
