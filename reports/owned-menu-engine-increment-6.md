# Owned menu engine, increment 6 (#128)

## Scope and contract

This increment owns kind `0xe` from a row with no child and kind `0xf` from a node with no parent as `Rejected`. The stock-byte probes and owned cases below use prepared state. They are not key-reachable acceptance or a packaged/device qualification. Crawl-5 has no such stage post; the increment-4 analysis records no first post and no later kind `0xe` or `0xf` post in a key trial ([increment 4, class 9](owned-menu-engine-increment-4.md#fallback-classes)). No new key reach is claimed.

The contract was read first through `xm1-decomp lookup` and `show` at x70view commit `f659f02b850fcba58de0f9b09ee24ad09a24c2f4`. The three functions are NONMATCHING, so their C is a reading aid, not byte-exact evidence. The lookup records and C were confirmed against `private-data/analysis/x-m1-1.50/lz/id5.dis` and the native execution below. The source is `decomp/firmwares/xm1-150/app/ui/core/tu_00bf8d20.c`: dispatcher lines 1032-1143, rebuild lines 1145-1275, change-level lines 1277-1330. [Decomp evidence rules](xm1-decomp.md#evidence-rules).

| Function | Lookup extent | Stock extent SHA-256 |
| --- | --- | --- |
| `0xbfa234` | `0xbfa234..0xbfa800` | `a9a7515285d23a833bbffd2e966307c976075391c26ba54e7e2666e1d8baa677` |
| `0xbfab98` | `0xbfab98..0xbfad04` | `0b0010980f6d7baa0721e2143f730a61ad5d3cc07ac2620fe9da783d6317f28d` |
| `0xbfa800` | `0xbfa800..0xbfab98` | `a07b9f197913235262a9d081860fc3c7efaa7a14f2b4607995d450f6db81bb19` |

For an ordinary id, outside `0x17` and `0x18`, `0xbfa234` calls `0xbfab98(kind, node, 0x15, id)`. If the result is not 1 it returns immediately; it does not call `0xbfad04`. The kind-switch jump table at `0xbfa294` and `0xbfa298` selects the common arm at `0xbfa6b8`. Seeking ids retain their existing fallback and are not part of this increment.

| Condition | Byte path in `0xbfab98` | Result and effects |
| --- | --- | --- |
| Push, `values[cursor].child == 0` | `0xbfac30..0xbfac50`, then `0xbfacb8..0xbfacbc` | Return 0. No calls, block writes, last-move writes, snapshot store or notification inside change-level. |
| Pop, `node.parent == 0` | `0xbfabd0..0xbfabdc`, then `0xbfacb8..0xbfacbc` | Return 0. No calls, block writes, last-move writes, snapshot store or notification inside change-level. |

Both paths read the source class at node `+8`; push reads the block cursor and the child word of its row; pop reads the parent at node `+0x1c`. Only the call stack is written. Neither path calls the remember service `0xbf9a5c`, node setter `0xbf976c`, classifier `0x61d0e4`, notifier `0xd5b350`, rebuild `0xbfa800` or inner snapshot store `0x61ceac`. The latter three calls are below the successful-switch join at `0xbfacc0`, at `0xbface0`, `0xbfacf0` and `0xbfacfc`. Thus `0xbfa800` performs no availability query, initial-row query or block update for these refusals because it is not entered.

The engine replaces `0xbf95c8`, not only `0xbfa234`. That wrapper preserves the kind-switch result and unconditionally calls `0x61ceac(current_node.id)` before returning it: `id5.dis`, `0xbf95c8..0xbf95f0`, specifically the call at `0xbf95e8`. Therefore the complete owned event returns `Rejected`, leaves the block and last-move pair unchanged, emits no notification, and makes exactly one outer snapshot store. Removing that outer store would be a compatibility error. The retained snapshot contract is already qualified in E-MENU-FRAME-SNAPSHOT-NATIVE-150 ([snapshot service](recipe-bank-ui-ownership.md#owned-menu-frame-snapshot-service)).

## Prepared cases and implementation

`//emulation:menu-engine-refusal-cases` loads the original image-5 functions and original image-4 node data. Its node is `0x77c0f0`, id `0x10`, type 4, class `0x1d8`, rows-per-page 8, null parent, with four rows. Row 0 has token 8 and null child; row 4 is the terminator pair with null child. It checks the parent and child words before running. Each case starts with block node `0x77c0f0`, word10 `0x15b`, total 4, depth 1 and menu-open 1; the other block words are zero.

| Case | Kind | Cursor | Dispatcher arguments | Change-level arguments |
| --- | --- | --- | --- | --- |
| `no-child` | `0xe` | 0 | `(0xe, 0x10, 0x1d, 0)` | `(0xe, 0x10, 0x15, 0x1d)` |
| `no-child-terminator` | `0xe` | 4 | `(0xe, 0x10, 0x1d, 0)` | `(0xe, 0x10, 0x15, 0x1d)` |
| `no-parent` | `0xf` | 0 | `(0xf, 0x10, 0x1d, 0)` | `(0xf, 0x10, 0x15, 0x1d)` |

Every row runs once at each seam, six native probes. Only the dispatcher and change-level functions are admitted, with no stubs; an unexpected service call fails the run. The checker requires `Returned`, r0 = 0, an identical final block and a complete write-provenance log containing only stack writes. Case JSON, hashed input artifacts and `native-result.json` are retained per probe.

`check_switch` now permits a leaf/terminator push and a parentless pop to reach `switch_node`'s existing false result. `push_pop` returns `Rejected` and the existing outer store remains in `run`. `MenuEngineClass` independently derives `OwnedEvent Rejected` for the same conditions, before target or restored-frame checks. Package children, seeking ids, invalid registry slots and the existing block/cursor bounds remain fallback. `NotOwned::NoChild` and the checker's unused `NoChild` reason are removed; `NoParent` remains for compound frame-move prechecks.

`owner_injected_refusals_match_stock` applies the same three blocks and events to the owned engine. It expects no block or last-move change, one store to slot `0x10`, and 22 literal snapshot word writes at `0x941594`: eleven detached defaults followed by `[2, 0x10, 4, 0x1d8, cursor_token, 8, cursor, 0, 0, 1, 0]`. The cursor token is 8 or the terminator `0x7ffffffe`. Snapshot addresses, tokens and the one-based page are expectations, not values read back from the candidate. `RefusalAccepted` changes only the refusal result to `Accepted`; each of the three cases must fail it. The injected cases are a separate test from the 45 receipt-derived cases.

## Runs and results

All paths below are relative to the MenuEngine246 worktree. Build commands used `devenv shell -- bazel --batch --output_base=/home/murph/.cache/bazel/latent-image-MenuEngine246 ... --jobs=2`.

- Built `//emulation:menu-engine-refusal-cases`, `//emulation:native-runner` and `//replacement/modal_runtime:menu-engine-pair-tests`. The final refusal/checker build completed successfully with no Haskell warning. The checker binary was built, not executed by this increment.
- `inc6-native-1` timed out before its first observation. The new Haskell executable lacked `-threaded`; after adding it, `inc6-native-2` completed all six probes in 2.62 seconds. Only `inc6-native-2` supports the native behavior claims.
- Native command: `bazel-bin/emulation/menu-engine-refusal-cases bazel-bin/emulation/native-runner private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin private-data/scratch/ControllerStrategy/menu-engine/inc6-native-2`. All six returned 0 with unchanged block and stack-only write provenance, with no external call.
- The initial fresh `host-oracle-82d943b4` owner run passed the receipt and existing-control tests but failed the new injected test: its page expectation was 0 rather than stock's one-based 1. Commit `f28b97c7` corrects that expectation. This was a test expectation change, not a production behavior change.
- `bash private-data/scratch/ControllerStrategy/menu-engine/inc3-cases.sh private-data/scratch/ControllerStrategy/menu-engine/host-oracle-f28b97c7` regenerated the tree and all 45 receipt cases, compiled the owner test, and ran it once. `owner-test.log` reports **4 passed, 0 failed**, `owner exit=0`: all 45 receipt cases pass, all 25 existing controls fail their required cases, all three injected refusals pass, and `RefusalAccepted` fails each injected case.

The complete result is a host-qualified refusal increment. No new key-driven guest pair, camera run, global caller-retirement claim or reachability claim is included.
