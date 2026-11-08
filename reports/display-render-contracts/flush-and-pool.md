# Draw flush and resource-object pool contract

## Scope and evidence

This is the stock-facing contract for the future owned renderer, target R2 and shims S1, S4 and S7 in [display-render-target-design.md](../display-render-target-design.md). It specifies software state transitions and service calls, not rasterization, display timing or a new publication implementation. No firmware case, candidate, emulator session or camera operation was run for this report.

The source pin is x70view commit `a72c95a3572562fc05f359b4f35915f3a0243b19`. Source paths below are relative to `decomp/firmwares/xm1-150/`. **Exact** means source-inspected exact C according to [xm1-decomp.md](../xm1-decomp.md), including its translation-unit marker and building-tree qualifications. **Bytes** means a NONMATCHING definition used only as a reading aid, with each established behavior confirmed in `private-data/analysis/x-m1-1.50/decode/img6.dis` at the cited instruction addresses. The listing is the plain image based at `0x01100000`; the pinned overlay44 slice has SHA-256 `2dfaa5d4bd025d68e991a329cc1377d5da16de656148dc4027d9a6bc0c349f5c`. Names are labels, not evidence of semantics. Byte ranges below are inclusive instruction addresses.

Unless a row states otherwise, inputs are AAPCS arguments, pointer inputs must name suitably aligned readable or writable objects for the full accessed extent, callback targets must remain callable until consumed, and the surrounding UILIB serialization is a precondition. These functions do not establish thread safety for concurrent producers. A void return or an incidental r0 value is not an error result. Uninspected transitive callees retain their existing service contract; recording a call to them does not establish their implementation.

## Shared state and representation

Let `D = 0x00957aa0`. These offsets, rather than the decomp's overlapping provisional structure views, define the S4 boundary.

| Location | Contract-visible representation and use | Evidence |
| --- | --- | --- |
| `D+0` | Wake-limit word, raised to at least 8 during flush and refreshed afterward | Exact `update_requests.c:14-22`, `animation.c:128-139` |
| `D+8`, `D+0xc` | Previous/current replay-mode words; replay has a special `current == 1 && previous == 0` branch | Bytes `117bf18-117bf48` |
| `D+0x10` | Byte cleared by draw-request reset; no fuller meaning established here | Exact `tu_0117b3fc.c:22-27` |
| `D+0x11` | Global opacity byte, overwritten by each admitted draw-event enqueue and read by apply for all events | Bytes `117aee4-117aee8`, `117c588-117c598` |
| `D+0x12` | Global event-count byte; enqueue refuses when above 29, pool reset clears it | Bytes `117ae08-117ae10`, `117af60-117af68`; exact pool reset |
| `D+0x14`, `+0x16`, `+0x18` | Three u16 next-index counters, returned as signed 16-bit values; reset clears all six bytes | Exact `draw_event_submission.c:14-31`, `tu_0117b3fc.c:22-27` |
| `D+0x1c` | Applied event array, five planes, 30 slots per plane, 0x20 bytes per slot | Bytes `117c4c8-117c4fc`, `117c7cc-117c820` |
| `D+0x12dc` | Requested event array of the same geometry, exactly 0x12c0 bytes after applied | Bytes `117ae24-117ae3c`, `117c4ec-117c4fc` |
| `D+0x259c = 0x95a03c` | Applied 0x28-byte plane descriptor | Bytes `117cd60-117cd7c`; exact comparator `FUN_0117cac8` |
| `D+0x25c4 = 0x95a064` | Requested 0x28-byte plane descriptor; draw preamble clears it | Bytes `117caa8-117cac4` |
| `D+0x2bf0` | Output selector read by event application; value 2 selects the 1920x1080 bounds, other values the 640x480 bounds in this function | Bytes `117c5fc-117c6c0`, literals `117c838-117c844` |
| `D+0x2bf8` | Word unconditionally cleared near flush end | Exact flush; bytes `11783c0-11783d0` |
| `D+0x2bfc` | `batch_busy`: waits test equality with 1, not general nonzero | Exact queue and clear |
| `D+0x2c00` | `batch_callback`: nullable callback word | Exact queue and dispatch |
| `D+0x2c08` | `commands_dirty`: nonzero causes VGLIB scheduling; cleared after the schedule call | Exact queue |
| `D+0x2c0c` | `display_pending`: nonzero causes selected-display event 3; cleared after that service returns | Exact take-pending |
| `0x01665388` | Command-cache population byte, cleared before drawing by `FUN_0117c948`; replay branches on it | Exact reset; bytes `117bee4-117bef4` |
| `0x01665660` | Saved replay-cache count byte; entry `i` has a 0x48-byte stride and payload begins at base `+4+0x48*i` | Bytes `117bf50-117bfa4`, `117c094-117c0bc` |
| `0x015e748c+8*bank` | Object-list `{head, tail}` pointer pair | Exact append and reset |
| `0x015e7538+0x6a00*bank+0xd4*slot` | Resource object; 128 slots per bank; bank stride 0x6a00 | Exact pool functions; literal base `11797b0` |

The pool extent relevant here is `[0x015e7538, 0x01665338)`, 19 banks numbered 0 through 18. Reset and commit operate on **banks 1 through 18 only**, starting at `0x015edf38`, not at the pool base. The reset literals are `1179700/1179704`; the commit literals are `117dc00/117dc04`; both are `{0x015edf38, 0x01665338}`. The list commit explicitly starts bank 1 with an 18-bank countdown (`117db5c-117db74`, `117dbec-117dbf4`). The decomp header's 20-bank array declaration is not a license to reset bank 0 or access bank 19.

The established object prefix is `next +0`, `handle +4`, `identifier +8`, text/argument pointer word `+0xc`, allocation state `+0x10`, kind `+0x14`, and lookup coordinates signed halfwords at `+0x34/+0x36`. The 0xd4-byte object includes inline key bytes starting at `+0x6e`. See exact `native_resource_pool.c`, `native_resource_pool.h`, `resource_object_allocate.c` and `native_resource_clear.c`. Do not extend the provisional public structure layout to unconfirmed offsets.

An event slot has identifier word `+0`, signed index `+4`, signed coordinate halfwords `+6/+8/+0xa/+0xc`, endpoint halfwords `+0xe/+0x10`, stroke/palette halfword `+0x12`, three color bytes `+0x14..+0x16`, layer halfword `+0x18`, flags byte `+0x1a`, palette halfword `+0x1c`, and option bytes `+0x1e/+0x1f`. The names of the two halfword roles at `+0x12` differ across decomp views; the byte offset is authoritative. Bytes `117ae30-117af58` establish the enqueue writes, and exact `draw_cache.c:15-28` establishes which fields the slot comparator reads. Slot byte `+0x17` is not the opacity source used by apply.

## Draw-event entry and flush

### `ov44_handle_draw_event` at `0x0110871c`: bytes

Inputs are a nominal u16 code and family/kind in r1. The handler obtains its effective event by calling `0x1102098`; it does not simply use incoming r0 as the table index (`1108724-1108730`). The effective event must resolve to an admitted screen-table row before the table branch; this function supplies no bounds check at the indirect load. Return is 1, including the suppression early exit (`1108a20-1108a24`). Source aid: `app/native_overlay44/ov44_event_core.c:180-257`, NONMATCHING.

The preamble handles effective events `0x174` and `0x175` by calling `0x1101f24` and storing its low halfword at `0x15e46c4+0x3c` (`1108730-1108798`). Otherwise family `0x4000` copies two 12-byte state blocks into `0x15e46c4+0x30/+0x3c`, sets `+0x38` to the latter block's address, and makes the profile reads for events `0x116/0x117` (`110879c-110889c`). The exact settings policy outside these branches is not redefined by the renderer seam.

Required reset order before the screen draw is:

1. `0x11120a0`, then `0x1112130` (`11088a0-11088a4`). These exact wrappers call `FUN_0117b3fc` and `FUN_0117caa8`, respectively.
2. Event-specific preparation through `0x1163534`, `0x1108160`, `0x110219c`, followed by storing the effective event halfword at `0x15e46c4+0x480` (`11088a8-11088e0`).
3. Calls `0x112612c`, `0x1113058`, `0x11267dc`, `0x110226c(event)`, `0x1126840`, `0x11130b0`, `0x1113a7c`, `0x1113c1c`, `0x1106e78` in order (`11088e4-1108908`). Exact `overlay_reset_renderer_cache_entries` at `0x1113a7c` calls the pool reset below. Other callees remain retained preparation services.

If UI byte `0x15e46c4+0x5e` is nonzero, call `0x1108350(u16(event+0x4000))` and return without screen draw or flush (`110890c-1108924`). Otherwise clear the command population byte through exact `FUN_0117c948`, call `0x1162738(event)` and `0x11627ec(event)` (`1108928-1108938`), then choose the detail-view branch or suppression branch from the UI/settings fields (`110893c-11089a4`). On the ordinary branch, call `0x1127a5c`, bounds service `0x1112d64(0,0,0,0)`, font service `0x1112b2c(15)`, then the handler word at `0x110f9f8+8*(event-1)` (`11089a8-11089d8`). Clear UI word `+0x1d0` after the ordinary handler (`11089dc-11089e8`). The alternate settings-suppression branch also clears that word without calling a table handler.

After `0x1110c50`, call `ov44_deferred_event_pending` (`0x11086f4`). Nonzero calls exact `overlay_complete_display_update(ov44_post_deferred_event)` at `0x1112170`, forwarding callback address `0x1108638` into the flush; zero calls `0x1108638` directly and does **not** flush (`11089ec-1108a0c`). Then call `0x1110b04` and `0x1108350(u16(event+0x4000))` (`1108a10-1108a1c`). A replacement may not equate finished-screen notification with an unconditional frame submission.

### Reset helpers

| Function and exactness | Inputs, outputs, state effects, calls and preconditions |
| --- | --- |
| `FUN_0117b3fc`, exact, `tu_0117b3fc.c:22-27` | No arguments or result. Clear `D+0x10`, palette-cache dirty word, all requested event slots `[D+0x12dc,D+0x259c)`, and the six index-counter bytes at `D+0x14`. Calls the memory-zero service. Applied slots and the four completion-tail words are not cleared by this function. |
| `FUN_0117caa8`, bytes, NONMATCHING `tu_0117caa8.c:13-17` | No arguments or result. Call `0x7221c8(0x95a064,0,0x28)`; it clears requested plane descriptor, not applied descriptor. Bytes `117caa8-117cac4`. |
| `FUN_0117c948`, exact, `app/native_overlay44/xm1_overlay44_cache.c:11-14` | No arguments or result. Set byte `0x1665388` to zero; no calls. |
| `ov48b_reset_draw_mappings`, `0x117bd8c`, exact, `draw_cache.c:5-13` | Input preserve flag. Set each of ten 8-byte mapping rows to `{state: preserve == 0 ? 2 : 0, index: -1}`; no calls or result. This reset is separate from the pool reset and requested-event reset. |

### `FUN_01178378` at `0x01178378`: exact

Source: `app/ui/render/tu_011764c0.c:816-833`. Input is a nullable completion callback, despite the provisional C integer argument. Return is always 0. All intermediate return values, including scheduling and apply errors, are discarded. It preserves this order, also directly visible in bytes `1178378-11783e0`:

1. `ov48b_raise_wake_limit(8)` at `0x1177818`.
2. `FUN_006ddab4()`.
3. `FUN_0117cb74()` to reconcile the staged plane descriptor.
4. `FUN_0117db18()` to commit resource objects.
5. `ov48b_advance_animation()` at `0x1176f24`.
6. `FUN_01178358(0xff)`, which writes palette/control byte `0x15e6680+0x2b`.
7. `FUN_0117bedc()` to replay the command cache.
8. `FUN_01178358(0)`.
9. `FUN_0117c460()` to apply requested draw events.
10. `ov48b_queue_completion(callback)`.
11. `ov48b_take_pending_result()`.
12. Clear word `D+0x2bf8`.
13. Call `0x6ddacc`.
14. `ov48b_refresh_wake_limit()` at `0x1177848`, then return 0.

The wake-limit raise only stores/calls `service_request_channel_option` when the requested limit exceeds the current word. Refresh leaves the word alone when callback requests are enabled; otherwise an enabled callback forces limit 8, or a nonzero old limit is reset to 0. Those helpers are exact (`update_requests.c:14-22`, `animation.c:128-139`). The names assigned to `0x6ddab4/0x6ddacc` do not establish locking or a hardware barrier here. Callback invocation can occur synchronously at step 10, before pending-display handling and flush-tail cleanup. Precondition: prior batch completion must eventually clear a busy value of 1 if this flush is to terminate.

## Completion state machine

All three entry contracts are exact. Source: `tu_011764c0.c:498-507,535-553` and `tu_011764c0_exact.c:37-48`. Their bytes additionally confirm the tail offsets (`1177760-11777a0`, `1177c9c-1177d98`).

| Function | Required behavior |
| --- | --- |
| `ov48b_queue_completion(callback)`, `0x1177c9c` | While `batch_busy == 1`, call `dispatch_delay_ticks(3)` at `0x723ce8` and re-read busy. If dirty is zero, invoke the input callback if nonnull and return 0 without storing it or scheduling. Otherwise set busy to 1, store callback, call `vglib_schedule_animation(3,0x1177760)` at `0x6dcb60`, clear dirty, and return that service's result. A scheduling failure does not roll back busy or callback. Null callback still schedules a dirty batch. Values of busy other than 1 do not cause a wait. |
| `ov48b_dispatch_completion()`, `0x1177760` | Return no value. First clear busy. Call exact `ov48b_dispatch_enabled_callback` at `0x1176eec`. Then re-read the stored batch callback; if null return, otherwise invoke it and clear the callback word **after** it returns. The enabled callback can therefore change which stored callback is read. A reentrant stored callback can write the slot, but the final clear still overwrites it. There is no dirty or pending clear here. |
| `ov48b_take_pending_result()`, `0x1177d54` | If pending is zero, return 0 without a call. Otherwise call `display_selected_callback_event3(0)` at `0x6cfd84`, clear pending after the call and return its result, including a failure result. It does not wait for VGLIB or clear busy/dirty. |

Exact `callback_gate.c:12-19` establishes that `ov48b_dispatch_enabled_callback()` calls its separate gate callback only when both enabled and callback are nonzero; it does not consume the gate callback. The batch callback is a separate cell. These are software completion edges. None alone establishes that a newer frame is scanning or that a buffer is free to overwrite.

## Object-pool lifetime

### Exact pool functions

| Function | Inputs and output | Reads, writes, calls and preconditions |
| --- | --- | --- |
| `ov48b_reset_object_banks()`, `0x117967c` | No arguments; return 1 | Set allocation state to 2 for all 128 objects in each bank 1..18; clear each corresponding list head and tail; clear text-cache count byte `0x15e67a4` and event-count byte `D+0x12`. Do not clear handles, payloads, bank 0 or applied events. No calls. Source `resource_object_bank_reset.c:7-57`; byte boundary literals `1179700/1179704`. |
| `ov48b_find_object(bank,identifier,x,y,argument)`, `0x1179528` | Return first slot index 0..127 or -1 | Reject identifier family `0xfd000000` immediately. Scan the selected bank in ascending slot order for allocation state 2, matching identifier, stored signed coordinates, and text/argument pointer equality. For identifier exactly `0xfe000000`, additionally require `string_mismatch(object.key_bytes,argument) == 0`. No writes. Source `native_resource_pool.c:5-21`. Bank bounds and pointer validity are caller preconditions; this is not a content-only text interning API. |
| `ov48b_mark_object(identifier,bank,x,y,argument)`, `0x117987c` | Return object pointer or null | Narrow x/y to signed 16-bit before find. A match gets state 1. Otherwise call allocate and, if nonnull, set state 0. Do not initialize other object fields or append. Source `native_resource_pool.c:23-36`. |
| `ov48b_allocate_object(bank)`, `0x11797b4` | Return object pointer or null | First scan for state 2 and handle 0, ascending slot order. If absent, call `ov48b_first_available_object(bank)` and clear a nonnull result through `ov48b_clear_resource_object`. If neither exists, copy the stock limit-message prefix at `0x117ad4c`, terminate its local message, copy it to error record `0x15e6748`, and call `ui_send_notification(error,0)`. Calls `string_length` and `app_copy_text` on that error path. Does not reserve the returned free slot by itself. Source `resource_object_allocate.c:15-51`; mark supplies the reservation state. |
| `ov48b_append_object(bank,object)`, `0x1179708` | No result | If tail null, set head to object; otherwise set old tail's next to object. Set object's next null and tail to object. No calls, capacity check, duplicate check or cycle detection. The object must be nonnull and not already in this event's list; caller must maintain a valid list. Source `resource_pool.c:19-25`. |
| `ov48b_clear_resource_object(object)`, `0x117da88` | Null returns 0; nonnull returns 1 | For nonnull input, wait while busy equals 1 with `dispatch_delay_ticks(3)`. For kind 4,5,6 call `ov48b_request_display_update(handle)` at `0x117cd90`; otherwise call `ov48b_request_display_refresh(handle)` at `0x117cdbc`. Then zero all 0xd4 bytes through `0x7221c8`, regardless of the deletion service's result. Source `native_resource_clear.c:5-18`; raw kind/handle offsets also visible at `117dad4-117db04`. No unlinking occurs. |

Allocation state is an event-relative mark, not an unconditional free/used enum. State 2 means eligible for reuse lookup and, if left unmarked, retirement at commit. A state-2 object with handle 0 is the preferred allocation candidate; a state-2 object with nonzero handle is the eviction candidate. State 0 is a newly allocated object needing creation, state 1 a matched object needing update, and other states skip both creation/update branches in commit. Do not replace this protocol with clearing the pool at event start.

### `ov48b_first_available_object(bank)` at `0x1179758`: bytes

Source aid: `tu_01178f6c.c:275-296`, NONMATCHING. Read base literal `0x15e7538`, multiply bank by 0x6a00, scan exactly 128 state words at stride 0xd4, return the first state-2 object regardless of handle, or null (`1179758-11797b0`). No calls or writes. Valid bank is a precondition; no bounds check is present. It does not inspect or remove objects from the event list.

### `FUN_0117db18()` at `0x117db18`: bytes

Source aid: `tu_0117caa8.c`, NONMATCHING. No arguments; return 1. This is a two-phase commit, with no transaction rollback:

1. Sweep banks 1..18, ascending physical slot order. Call `ov48b_clear_resource_object(object)` for every state-2 object, including a zero-handle object. Stride 0xd4, bank extent 0x6a00, literals described above (`117db20-117db58`). All retirements precede creation/update.
2. Visit linked lists for banks 1..18 in bank order (`117db5c-117db7c`). For each node: state 0 calls create `0x117d390`; if it returns 0, zero 0xd0 bytes starting at object+4, preserving next. State 1 calls update `0x117d844`, whose result is ignored. Other states skip both. Then follow next. Stop on null or after 128 nodes (`117db84-117dbe8`). Return 1 (`117dbf8-117dbfc`).

The commit does not clear list heads/tails or set every surviving allocation state to 2. The next event reset establishes the next reuse set. The 128-node cap bounds traversal but does not make malformed lists valid. Create failure preserves the chain while wiping handle and payload. Neither a create failure nor an update failure changes the function's successful return of 1. Full creation, texture ownership and update semantics below `0x117d390/0x117d844` are not established here and remain retained services at this boundary.

## Draw-event enqueue and application

### `ov48b_next_event_index(kind)` at `0x117ad6c`: exact

Source `draw_event_submission.c:14-31`. Kind 1 consumes/increments counter `D+0x16`; kinds 2,3,4 consume/increment `D+0x18`; every other kind consumes/increments `D+0x14`. Return the old u16 counter sign-extended as i16. There is no capacity check or call. Normal enqueue requires counters freshly reset and fewer than 30 admitted events, not arbitrary stale counter values.

### `ov48b_queue_draw_event` at `0x117ade0`: bytes

Source aid `tu_0117ab64.c:96-146`, NONMATCHING. Established inputs are `(plane,identifier,left,top,right,bottom,endpoint_x,endpoint_y,stroke,color_pointer,opacity)`. The promoted declaration's extra flags argument is not an established input: this body writes constant flags 0x12. It returns no defined value. Preconditions are plane 0..4, the selected index below 30, and three readable color bytes. The total-count test is not a plane/index validator.

If unsigned event-count byte `D+0x12 > 29`, return without allocation or state writes (`117ae08-117ae10`). Otherwise call `ov48b_next_event_index` with incoming r0, the plane, not the draft's constant 0 (`117ade8-117ae14`). Compute requested slot `E = D+0x12dc+32*(30*plane+index)`, write index and identifier (`117ae24-117ae3c`). When halfword `0x15e6680+0x1c == 1`, write `(right,bottom,left,top)` into the four coordinate fields and call `0x1178484` on each endpoint pair; otherwise write `(left,top,right,bottom)` (`117ae40-117aed8`). The coordinate-transform callee's full law is not established here.

Write opacity to global `D+0x11`, endpoints to slot `+0xe/+0x10`, stroke to `+0x12`, copy exactly three color bytes to `+0x14`, plane to `+0x18`, constant 0x12 to `+0x1a`, palette from halfword `0x15e6680+0x28` to `+0x1c`, and options from bytes `+0x2a/+0x2b` to `+0x1e/+0x1f`; increment total-count byte (`117aedc-117af68`). Unwritten slot bytes retain their initial contents. Most importantly, opacity is global and last-writer-wins within the batch.

### `ov48b_draw_event_changed(current,previous)` at `0x117c0c4`: exact

Source `draw_cache.c:15-28`. Return 1 on any difference in identifier, left/top/right/bottom, stroke, three color bytes, flags or index, otherwise 0. Read only; no calls. Endpoint extents, slot opacity, layer, palette, option bytes and padding are not compared by this function. It is not a full 32-byte equality predicate.

### `FUN_0117c460()` at `0x117c460`: bytes

Source aid `tu_0117b3fc.c:611-791`, NONMATCHING. No arguments. It reads both event arrays, global opacity and output selector; writes successfully applied slots and, through the setters, pending/dirty words. Calls global-change gate `FUN_0117c1b0`; if its result is not exactly 1, return 0 without applying (`117c4b8-117c4c4`). That gate only scans planes 0..3, 30 slots each (`117c328-117c350`), while apply itself scans planes 0..4 (`117c7dc-117c820`). A plane-4-only change therefore does not by itself open this gate. The gate compares an offset set different from the exact per-slot comparator, including endpoints and palette (`117c23c-117c324`); the two must not be merged into one generic equality test.

When the gate is open, visit plane-major, slot-major order. For each pair call exact `ov48b_draw_event_changed(applied,requested)`, skip unchanged slots (`117c4ec-117c504`). An identifier-zero requested slot calls `FUN_0117cf14(plane,slot,0)`, then copies requested to applied unless the setter returns -10 (`117c790-117c7b8`). A nonzero identifier builds a 0x30-byte packet: signed coordinates converted to float, endpoint halfwords copied, slot `+0x12` converted to float, colors divided by 255.0, global opacity divided by 255.0, slot flags copied, and an enabled byte set to 1 (`117c518-117c5d8`, divisor literal `117c834`).

Validation rejects a rectangle wholly left of zero, wholly beyond the selected horizontal bound, wholly above zero, or with top at/beyond the vertical bound. Endpoint x/y must have their sign bits clear and be below width/height. Selector 2 uses 1920x1080, otherwise 640x480. Invalid cases set local result -30 and skip that slot without copying requested to applied (`117c5e0-117c6c0`, `117c7c0-117c7c8`). This is the function's 640-wide validation, not a claim that the panel OSD buffer is 640 pixels wide.

For accepted geometry, compute width/height from the coordinate differences, call transform `0x1177a08(option_secondary,&x,&y,&width,&height)` using requested slot byte `+0x1f`, rebuild right/bottom, then call `FUN_0117cf14(plane,slot,0)`, `FUN_0117ce14(plane,slot,&packet)`, `FUN_0117cf14(plane,slot,1)` in order (`117c6c4-117c77c`). A -10 at any step returns immediately, leaving earlier changes intact and the current slot uncopied. Other errors do not abort that sequence. Copy all 32 requested bytes to applied after the sequence, even when its final non--10 result is nonzero (`117c780-117c7b8`). Return the accumulated local status, initially 0; unchanged slots do not reset it and later processed slots can overwrite an earlier -30. Full transform mathematics and lower setter effects are not specified by this packet-level contract.

### `FUN_0117ce14(plane,index,packet)` at `0x117ce14`: bytes

Source aid `tu_0117caa8.c`, NONMATCHING and misleadingly declared void. Bytes return the underlying service's r0. For plane 4, construct a zeroed 0x34-byte local packet: copy source words `+0,+4,+8,+0xc`; halfwords `+0x10/+0x12`; source word `+0x14`; words `+0x18/+0x1c/+0x20/+0x24`; copy bytes source `+0x2d/+0x2e/+0x2f` to destination `+0x30/+0x31/+0x32`. Call `0x6dd4d4(0,index,&local)`. Other planes call `0x6d0a20(plane,index,packet)` with the original inputs, not zeros (`117ce20-117cee4`).

If and only if service result is 0, set `D+0x2c0c = 1` for unsigned plane <=1, otherwise `D+0x2c08 = 1`; leave both alone on nonzero result (`117cee8-117cf10`). No bounds check is supplied; plane 0..4 and valid index are caller preconditions. This wrapper does not clear either flag.

### `FUN_0117cf14(plane,index,enabled)` at `0x117cf14`: bytes

Source aid `tu_0117caa8.c`, NONMATCHING, with missing arguments and void result in its draft. Plane 4 calls `0x6dd700(0,index,enabled)`; other planes call `0x6d0d78(plane,index,enabled)`. Return that service result. On result 0 set pending for unsigned plane <=1, dirty otherwise; on nonzero result preserve both (`117cf14-117cf60`). Inputs must name an admitted plane/index; there is no validation. A visibility-only change can therefore dirty a batch even without a new nonzero draw event.

## Command-cache replay

### `FUN_0117bedc()` at `0x117bedc`: bytes

Source aid `tu_0117b3fc.c:443-510`, NONMATCHING. No arguments or defined result. Its inputs are population byte `0x1665388`, mode words `D+8/+0xc`, command-cache comparison/mapping state and saved cache at `C = 0x1665660`. Its writes are delegated mapping/cache state and draw state produced by replay leaves; the zero-population branch directly clears `C[0]`. Preconditions: bounded cache count and valid 0x48-byte entries, serialized replay, retained helper contracts. No entry-count capacity check is present here.

Branch contract, confirmed at `117bee4-117bf94`:

- Population byte zero: call `ov48b_reset_draw_mappings(0)`, `0x117bc4c`, clear saved cache count byte, return.
- `current == 1 && previous == 0`: reset mappings with 0, call `0x117bc4c`, then `0x117bd5c`, and replay.
- Otherwise call comparison `0x117b8a4`. Nonzero calls reset mappings with 1, `0x117bdd4`, `0x117bb00`, `0x117bc4c`, `0x117bd5c`, then replay. Zero returns without replay unless current mode is 1.

Replay runs two passes, grouping entries by word `C+8+0x48*i` equal to pass 0 then pass 1, preserving index order within each pass (`117bf98-117bfac`, `117c08c-117c0c0`). For a matching entry, nonzero byte `C+0x48+0x48*i` first calls `0x1178338(1)`. Dispatch on word `C+4+0x48*i`: types 1/5 call `0x117b448`, 2/4 call `0x117b5ac`, 3 calls `0x117b70c`, 6 calls `0x117b65c`; other types skip the dispatch. Each receives payload pointer `C+4+0x48*i` and argument 1 (`117bfb0-117c058`). If word `C+0x34+0x48*i` is 1, call `0x117b4ac(payload,1)`, including for an unknown main type. Always call `0x1178338(0)` afterward (`117c05c-117c088`). Count is re-read while traversing, not snapshotted once.

The replay leaves' geometry, command-matching algorithm, cache ownership and 0x117bc4c/0x117bd5c side effects are not established by this report. They remain retained interfaces, not assumed no-ops. Exact mappings reset and exact slot-change comparison are separate contracts above.

## Replacement obligations

- S1 must preserve the event preamble, event-relative reuse set, ordered leaf effects and conditional flush branch while stock still consumes the pool and draw arrays. Emitting the same apparent pixels while omitting required callback/state transitions is not equivalent.
- S4 must keep the four tail words and both requested/applied arrays consistent at the same service boundaries. Dirty/pending record successful setter calls, not merely producer intent. Error paths consume or retain state exactly as specified above; there is no blanket rollback or retry.
- S7 must not call the batch callback merely because CPU drawing is finished if retained stock expects the VGLIB completion edge. Conversely, the clean-batch branch is synchronous and must not gain an invented frame wait. Buffer retirement/scanout acknowledgment is outside these functions and needs the separate frame-publication contract.
- An owned internal representation may differ, but the adapter must preserve raw stock-visible state while a retained reader remains. Bank 0 exclusion, global opacity, four-plane global gate versus five-plane apply, and partial failure effects are observable compatibility constraints, not opportunities to normalize behavior silently.

## Host-oracle feasibility and proposed cases

These are proposed, pre-run contract cases, not qualification results. Use stock slices from the pinned plain image and the candidate against identical initial memory, explicit service responses and independently derived expectations. Proprietary bytes belong under `private-data/analysis/x-m1-1.50/native-case/flush-and-pool/`. A future Haskell generator should bind whole-image identity and exact function slices before emitting cases. Compare return values only where defined, all externally read state, list order, and ordered boundary calls. Do not assert private implementation call structure in an owned-only behavioral test; shim-service calls are observable effects while those shims remain.

The current [native-runner contract](../../emulation/README.md#case-contract-and-execution-authority) admits ordinary A32 integer data processing, halfword/byte/word loads/stores, signed loads, multiply-address arithmetic, calls, aligned declared BLX targets and tracked returns. These cover the local integer pool/completion/replay bodies. It does not map display/vector peripherals. Register-only stubs cannot mutate pointed-to memory, model arbitrary delay progress or capture arbitrary stack packets: actual memory-copy/fill bodies or declared small synthetic service bodies are needed for those effects, or use a host oracle with explicit service transcripts. The runner bounds each case to 4 MiB mapped bytes, 32 stubs and 128 expected events, so do not collect every flush subpath into one huge case.

`FUN_0117c460` starts with `vpush {d8}` at `117c464` and uses VFP conversions, divide, arithmetic and FPSCR comparison (`117c51c-117c738`). The current user profile refuses coprocessor words before execution (`emulation/fujimod-xm1/src/native_case.rs:2199-2212`, `excluded`); its kernel CPACR/FPEXC register allowances do not admit general VFP arithmetic. These forms are consistent with the inferred R4F/VFPv3-D16 target, but native-runner admission must expand with explicit fixtures before full apply cases can run. A false global-change gate still executes the VFP prologue, so it does not avoid this boundary. No assembly substitute or altered stock slice should be used to hide it.

| Family | Proposed inputs and independently expected observations | Host boundary |
| --- | --- | --- |
| Reset isolation | Seed nonzero pool handles/payloads, all list heads/tails, event count, text count, both event arrays and tail flags. Pool reset changes only states in banks 1..18, those lists and the two counts; bank 0, handles and payloads remain. Separate request reset clears only its byte/counters/requested slots and palette dirty. | Integer native cases; execute real memory-zero service for request reset. Full pool is about 0x7de00 bytes, within mapping bounds. |
| Find/mark | First/last-slot hits; duplicate-key first hit; each key component mismatch; state other than 2; 0xfd family refusal; 0xfe pointer match plus equal/unequal inline string; x/y values narrowing to the same signed halfword. | Integer native with real string comparison or exact register-bound comparison response. Observe returned pointer/index and subsequent reuse behavior. |
| Allocation preference | Earlier state-2 nonzero-handle object and later state-2 zero-handle object: select the latter without delete. No zero handles: evict first state-2 object and zero it. No state-2 objects: null plus limit notification. Mark then transitions selected state to 0. | Native with delete/notification boundary stubs and actual memory fill; error text-copy body needed for the exhausted case. |
| Append | Empty list, two and three nodes, different banks, then read traversal through head and next. Require insertion order and terminated final next. | Integer native, no device access. Invalid duplicate/cyclic inputs remain outside the append precondition. |
| Clear | Null; kinds 3,4,5,6,7; handle zero/nonzero; busy 0 and 2 do not delay. Busy 1 with a defined external clear after one delay calls delay(3) once before deletion. Deletion failure still zeroes object and returns 1. | Integer native for nonwaiting paths; waiting path needs an executing callback/service body that clears busy, or host oracle. A permanently busy control must exhaust a bounded budget, not claim termination. |
| Commit sweep/list | Retire an untouched object before any create/update. Mix states 0,1,2,3 in bank lists, use banks 1 and 18 and sentinel bank 0. Create failure zeroes object+4..+0xd3 but preserves next so a later node is reached. Update failure does not abort. A declared long chain stops after 128 nodes. | Integer native with create/update as explicit retained boundaries and real fill. Partition large traces to stay below 128 expected stub events; a host oracle can inspect the complete sweep. |
| Clean/dirty completion | Cross product dirty zero/nonzero and callback null/non-null; busy 0/2 versus a one-delay busy-1 release; schedule success/failure. Require synchronous clean callback or busy/callback publication before scheduling, and dirty clear even on schedule error. | Integer native. Execute a synthetic schedule observer or host oracle to inspect state at call time; register-only stubs alone observe only arguments. |
| Completion ordering | Enabled gate callback present/absent; stored callback present/absent; gate changes stored callback; stored callback writes a replacement slot. Require busy already zero at callbacks, fresh slot load after gate, final slot clear after stored callback. Pending zero/nonzero with success/error return, with pending still set during service. | Integer native with declared callback bodies or a host oracle. No frame interrupt is required to exercise the dispatcher directly. |
| Enqueue | Counts 0,29,30; plane 0/1/2/3/4 counter selection; normal and transformed coordinates; two events with distinct opacity values. Require no writes on count 30, correct requested-slot addresses, flags 0x12, three-byte color copy, final global opacity from last admitted event, applied slots unchanged. | Integer native with actual memcpy; transform service can be a declared packet-effect boundary. A stubbed transform does not qualify the transform mathematics. |
| Change predicates | Change each exact compared slot field separately; change each omitted field separately; plane-4-only mutation for global gate, then add a plane-0 change. | Integer native for comparators. Establish gate's offset-level expectations from bytes, not a shared candidate equality helper. |
| Dirty setters | Plane 0/1/2/3/4 crossed with underlying result 0, -10 and another nonzero. Require original arguments for ordinary planes, plane 0 and remapped local packet for plane 4, only the appropriate flag set on success, other flag preserved, return propagated. | Integer native; plane-4 local packet inspection needs executing observer or host oracle, not a register-only stub claiming to read its contents. |
| Apply | No global change; plane-4-only change; identifier deletion; nonzero accepted packet; bounds at -1/0/639/640/1919/1920 and 479/480/1079/1080 under both selectors; endpoints 0x7fff/0x8000; mixed global opacity; -10 injected at each setter versus another error; invalid slot followed by valid slot. Require exact stop/copy residue and status, and plane-major order. | VFP admission prerequisite or a host oracle capable of executing the actual stock bytes with an explicitly admitted CPU. Lower geometry and setters can remain declared service boundaries. |
| Replay | Population zero; mode 1 previous 0; changed/unchanged cache in other modes; pass values interleaved; types 1..6 plus unknown type; temporary flag and extra-operation flag combinations. Require pass-0 then pass-1 order, type dispatch, argument 1, temporary reset, and count clear only on zero-population branch. | Integer native, including declared jump-table targets. Run helpers as real stock bytes where their effects are asserted; otherwise keep them transcript boundaries. |
| Flush composition | Callback null/non-null, clean/dirty and pending zero/nonzero; deliberately failing plane/apply/schedule delegates. Require the exact flush order, ignored error results, byte 0xff around replay then 0 before apply, tail flag clear and return 0. | Integer orchestration case with apply and other helpers as declared boundaries. A host oracle can combine their contracts; it does not establish transitive hardware behavior. |
| UILIB branch | Suppressed draw; ordinary table handler; deferred pending zero/nonzero; alternate detail/suppression routes. Require reset-before-handler and direct callback versus flush distinction, notification ordering and return 1. | Integer local handler, but many retained dependencies. Partition branch cases or use a host oracle to remain within function/stub limits. |

A native case that exercises only orchestration and substitutes the draw/apply/VGLIB services establishes only that orchestration contract. The full callback chain from vector job through display interrupt requires the frame/display contract and device model, not this peripheral-free oracle. Host reruns are allowed; no guest pair or per-increment review is required for these future qualification cases.

## What is not established

- No proposed case has run, and there is no candidate renderer qualification, throughput result or pixel-equality claim.
- The full geometry-transform law, command-cache matching and eviction policy beyond the scanned allocation-state rule, object-handle creation/update ownership, and plane-descriptor reconciliation remain retained implementations. Their drafts are not elevated to facts here.
- The global gate's four-plane extent and slot comparator's omissions are byte/source facts; their intended UI rationale and whether callers always mask their observable differences are unknown.
- Hardware interrupt delivery, timing, vector-engine rasterization, scanout latch semantics, memory visibility and safe buffer reuse are not established by software flags or callback names.
- The handler's no-flush path is specified, but the later consumer that eventually submits such accumulated work is not traced here.
- Exactness is at the pinned committed tree under the decomp evidence rules, not an x70view build performed by this report. No result depends on the mutable x70view working tree.

## Reproduction

The source-inspection command used was `devenv shell -- bazel --output_base=$HOME/.cache/bazel/latent-image-RenderFlushPool run --jobs=2 //build-support:xm1-decomp -- show 0x1178378 --rev a72c95a3572562fc05f359b4f35915f3a0243b19`; it built the helper and returned the exact flush definition. Further `lookup`/`show` calls used the built helper with `XM1_DECOMP_INDEX` pointing to its built indexer and the same pin. Committed source/header reads used `git show COMMIT:PATH` in x70view; listing reads used address-anchored searches and bounded line ranges. These were research commands, not tests. Directly invoking the built helper without `XM1_DECOMP_INDEX` first failed to locate its indexer; setting that documented environment variable resolved it.
