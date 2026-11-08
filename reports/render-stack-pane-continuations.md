# Render stack entry ingress and retention contracts

## Entry ingress

The diagnostic admits a render entry only when the LR is a direct-call continuation of that entry. The word at `LR - 4` must be a direct `bl` whose target is the entry address. The caller check binds the continuation to the tree, picker or pane entry, so a shared returns list cannot admit a tree caller at the pane entry.

The complete direct-call ingress sets are pinned from X-M1 1.50 image6.

| Entry | Call site | Continuation | Function |
| --- | --- | --- | --- |
| tree `0x1164e1c` | `0x1167a5c` | `0x1167a60` | `FUN_0116768c` |
| tree | `0x116c054` | `0x116c058` | `FUN_0116bff4` |
| picker `0x1163f2c` | `0x1168ce0` | `0x1168ce4` | `FUN_01168bb8` |
| picker | `0x1169304` | `0x1169308` | `FUN_01169160` |
| picker | `0x1169a14` | `0x1169a18` | `FUN_01169914` |
| picker | `0x116a1b8` | `0x116a1bc` | `overlay_draw_button_assignment_panel` |
| picker | `0x116a8b0` | `0x116a8b4` | `FUN_0116a710` |
| picker | `0x116b52c` | `0x116b530` | `FUN_0116b434` |
| picker | `0x116b568` | `0x116b56c` | `FUN_0116b434` |
| picker | `0x116c584` | `0x116c588` | `FUN_0116c4f0` |
| pane `0x11662a0` | `0x1158180` | `0x1158184` | `FUN_0115815c` |
| pane | `0x11586e4` | `0x11586e8` | `FUN_011586cc` |
| pane | `0x1158808` | `0x115880c` | `FUN_011587f4` |
| pane | `0x1167a10` | `0x1167a14` | `FUN_0116768c` |
| pane | `0x1168c68` | `0x1168c6c` | `FUN_01168bb8` |
| pane | `0x116a784` | `0x116a788` | `FUN_0116a710` |

`FUN_011587f4` calls `FUN_011662a0(input_1, *(int *)(blk_015e46c4 + 0x1d8))` and returns through `0x115880c`. The caller's eight-byte `{r11, lr}` frame and the pane's observed SP are inside the task33 stack descriptor. The entry must be primary task33 in SVC mode with the admitted descriptor, and the SP must remain inside the descriptor's 4,096 bytes. The inner emulator scope remains 2G with zero swap and one slot.

The arbitrary-LR, wrong-frame, stale-return and nesting refusals remain in force. The caller binding rejects a legal continuation of a different entry.

## Why the earlier list failed

The original five-site returns list was route-observed, not the complete declared ingress set. The partial route reached only the pane callers in `FUN_0115815c` and `FUN_0116768c`. The complete route reached `FUN_011587f4` at `0x1158808`, whose continuation `0x115880c` was absent, and the other pane and tree/picker callers were also absent. The stock config now carries the complete 16-site ingress set and the source binds each caller to its entry.

## Watch and lifetime retention

The watch producer retains a bounded window of at most `2 * 4096 + 1` rows and spills older rows to an append-only spool in row order. `RowCursor` types row positions; `Records::for_each_since` streams spool rows then window rows, and `rows_since` serves only the retained window. The modal guard row digest, row cursor and final `WatchEvidence` export use `for_each_since`; the `record_count`, `records_sha256`, completion fields and lifetime/diagnostic evidence are unchanged. A spool write or read failure marks the stream incomplete and the row stream returns an error, so no successful count or digest is emitted for lost rows. The spool uses the job's `TMPDIR` scratch with create-new/PID/serial collision handling and drop cleanup. The scoped watch uses the same limit and spool through `at_limit()`.

The legacy `Records::state()` still materializes all rows for the non-modal small/native consumers that require the rows in a value (`application_observation`, `composed_startup`, `peer_witness*`, and the raw `watch-state` memory_watch); that path is the remaining unbounded memory source. The modal final watch does not use it. The 189,430-row prefix produced a 36,672,975-byte final watch; the full 469,409-row worst case projects to about 90,875,914 bytes of JSON. The prefix at a rounded 2G peak is not a full memory-fit proof. A full-length synthetic smoke exercises the spool and the production `Rows`/`RowDigest` serialization with real watch-schema rows, record count/hash/order and late rows under 2G, without another firmware rerun. No cap increase or trace exclusion is proposed.

## Source and request status

The caller-binding and bounded watch producer pass host-only qualification at source `194e19bf`. The five production tools and viewer test bind the same checked SDK package. The 469,409-row producer smoke and the SDK serializer boundary/full-length smokes each execute their selected tests and pass under 2 GiB/no swap; the two earlier producer boundary checks are explicitly reused. Proof: `private-data/scratch/RenderCutover/runs/watch-spool-host-b8242d63/main-host-review-packet.json`. Test selection uses exact test presence, not a filename-prefix assumption: the package has an empty library harness and a binary harness. This is synthetic host evidence, not complete-camera memory-fit or full-67 acceptance.

The complete stock `oracle-framed-push` resource case passes at `2ef924c6` using that unchanged SDK and trajectory. Installation, trajectory and rollback pass, and the final watch contains all 463,469 reported rows. The exact inner emulator scope peaks at 2,147,483,648 bytes with zero swap, reaching the configured cap. The diagnostic finishes with continuous coverage and 18 pane invocations, but remains diagnostic-only: whole-route stack-minimum proof and modal task-identity verification are false. This accepts the complete stock resource case, not the owned comparison or full matrix. Proof: `private-data/scratch/RenderCutover/runs/largest-stock-2ef924c6/review-packet.json`.

The owner visually approved the targeted owned-renderer shooting menu with DYNAMIC RANGE selected and its flyout, with no notes. The reviewed images are `oracle-path.png` and `oracle-key.png` under `private-data/scratch/RenderCutover/runs/stack-semantic-63fb7154/runs/oracle-push-classifier-owned-minimum-context/screenshots/`. This review covers those two pane/tree captures, not the full menu matrix, picker controller or hardware.
