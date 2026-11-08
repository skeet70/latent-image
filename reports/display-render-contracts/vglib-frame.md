# VGLIB frame publication contract

## Scope and evidence

This is the decomp-first contract for the future owned frame publisher, [target design 2.3](../display-render-target-design.md#23-owned-frame-publisher), replacement step S7. It specifies task 73's `0x5902` frame step and its submission, buffer, display-shadow and completion boundaries. It does not implement or qualify an owned publisher. No native cases, emulator runs or camera operations were performed for this report.

All decomp references below are to x70view commit `a72c95a3572562fc05f359b4f35915f3a0243b19`. Exactness follows [the evidence rules](../xm1-decomp.md#evidence-rules). `exact` means source-inspected byte-verified reconstruction under that report's building-tree assumption, not a new build of x70view here. Names and C types remain inferred. Every behavioral claim about NONMATCHING C below is grounded in the cited listing instructions, not its reconstructed pointer arithmetic.

`I4:ADDRESS-ADDRESS` cites inclusive instruction addresses in `private-data/analysis/x-m1-1.50/lz/id4.dis`, from image `lz/id4-0061c400.bin`, base `0x0061c400`, SHA-256 `f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16`. Table words in that listing are data even when the disassembler prints instruction mnemonics.

| Entry | Extent, exclusive end | Decomp definition | Status |
| --- | --- | --- | --- |
| `vglib_task` | `0x6d7540..0x6d76ec` | `app/graphics/openvg/vglib/vglib_task_api_more.c:43` | NONMATCHING |
| `render_next_buffer` | `0x6d700c..0x6d7040` | `app/graphics/backend/render_transition_more.c:99` | NONMATCHING |
| `FUN_006d7920` | `0x6d7920..0x6d7e00` | `app/graphics/openvg/vglib/tu_006d7540.c:236` | NONMATCHING |
| `FUN_006d724c` | `0x6d724c..0x6d7324` | `app/graphics/backend/tu_006d6ea8.c:24` | NONMATCHING |
| `render_visit_object` | `0x6d7324..0x6d7438` | `app/graphics/backend/render_transition_more.c:150` | NONMATCHING |
| `vglib_select_surface` | `0x6d788c..0x6d7920` | `app/graphics/openvg/vglib/vglib_native_context.c:26` | exact |
| `FUN_006d76ec` | `0x6d76ec..0x6d7864` | `app/graphics/openvg/vglib/tu_006d7540.c:194` | NONMATCHING |
| `FUN_006cfc68` | `0x6cfc68..0x6cfcac` | `app/drivers/display/tu_006cfbe8.c:45` | exact |
| `vglib_post_display_event` | `0x6d7f30..0x6d7f48` | `app/graphics/openvg/vglib/vglib_task_api.c:30` | exact |
| `FUN_006d80f0` | `0x6d80f0..0x6d8130` | `app/graphics/openvg/vglib/tu_006d7540.c:411` | NONMATCHING |
| `vglib_schedule_animation` | `0x6dcb60..0x6dcc08` | `app/graphics/openvg/vglib/vglib_api_more.c:172` | NONMATCHING |
| open `FUN_006dc8d4` | `0x6dc8d4..0x6dca18` | `app/graphics/openvg/vglib/tu_006d7540.c:1620` | NONMATCHING |
| close `FUN_006dca18` | `0x6dca18..0x6dcae4` | `app/graphics/openvg/vglib/tu_006d7540.c:1665` | NONMATCHING |

## Shared state and terminology

Let `R = 0x8e2098`, `A = 0x8fa5dc`, and `G = 0x828d9c`. The decomp's `render_state.h` supplies field names; the offsets and access widths below come from the listed consumers. `front` means the software byte, not proof that hardware is currently scanning that buffer.

| Location | Meaning at this boundary | Byte evidence |
| --- | --- | --- |
| `R+0`, `+4`, `+8`, `+0xc..+0x3b` | Command halfword, message capacity, payload pointer, 48-byte received payload | `I4:6d7550-6d755c`, `6d7588`, `6d7684-6d7690` |
| `R+0x3c,+0x40,+0x44,+0x48,+0x4c` | Transition active, state, count, table pointer, status | `I4:6d757c-6d7584`, `6d7694-6d76d8`, `6d6eb8-6d6f34` |
| `R+0x7c,+0x80` | Requested and current mode | `I4:6d7674-6d7678`, `6d7950-6d795c`, `6d79c8-6d79cc` |
| `R+0x84,+0x88,+0x8c` | EGL display, context, configuration | Exact `vglib_select_surface`; `I4:6d7184-6d7194`, `6d78c8-6d78e0` |
| `R+0x90+8*s+4*b` | Surface handle, stage `s` in `0..5`, buffer `b` in `0..1` | `I4:6d7ab0-6d7ab8`, `6d7af0-6d7b04` |
| `R+0xc0+8*s+4*b` | Image-object pointer for that stage/buffer; `object+0x1c` is handed to display configuration | `I4:6d7b00-6d7b38`, `6d7b80-6d7b8c`, `6d7bdc-6d7c40` |
| `R+0xf0,+0xf4` | Special surface/object used by mode 10's second rendering pass | `I4:6d7cb0-6d7ce8` |
| `R+0x100..+0x114` | Six status-completion slots, cleared before calls with status in `r0` | `I4:6d6fc4-6d7004` |
| `R+0x118..+0x12c` = `0x8e21b0..0x8e21c4` | Six frame-completion slots, clear-before-call, no explicit argument | `I4:6d7da4-6d7dc4` |
| `R+0x130` = `0x8e21c8` | Start of the six-word secondary-display callback vector; first word set to `0x6d80f0` | `I4:6d7d80-6d7d90`, `6c8fe0-6c9000` |
| `R+0x148` | Viewport copied from API before a frame; passed to OpenVG as a four-element vector | `I4:6d766c-6d7670`, `6d7274-6d7280` |
| `R+0x158,+0x15c` | Visited count and accumulated render result | `I4:6d7298-6d72a0`, `6d72e0-6d7318`, `6d740c-6d7414` |
| `R+0x160` = `0x8e21f8` | Front-buffer byte | `I4:6d7014-6d7030`, `6d7d30-6d7d58` |
| `R+0x161` | Feature byte snapshotted by command handling | `I4:6d767c-6d7680` |
| `A+0,+4,+0xc..+0x1b,+0x1c,+0x20,+0x204,+0x205` | Open count, resource handle, 16-byte options, synchronous result, mode, synchronous completion byte, feature byte | `I4:6dc8e4-6dca04`, `6dca24-6dcac0`, `6dcbb8-6dcbd4` |

The frame-callback range is six entries ending at `0x8e21c4`; `0x8e21c8` is not a seventh frame callback. It belongs to the later display/scheduler bridge. The vector queued from `R+0x130` is six words, not merely the first function pointer (`I4:6c8fe8-6c9000`). Contents of its other five words are not established by this report.

`render_next_buffer()` reads `front` without changing it. It computes the integer `front+1`, returns that as a byte if it is at most 1, otherwise returns 0. Thus valid front 0 gives back 1, and front 1 gives back 0; any input byte 2..255 gives 0 (`I4:6d701c-6d7030`). The frame-step increment is different for invalid inputs: it truncates to a byte before comparing with 1 (`I4:6d7d38-6d7d58`). Do not replace both operations with an unchecked XOR law outside the valid two-buffer invariant.

`render_stage_index(mode)` maps 2 to 1, 3 to 2, 5 to 3, 6 to 4, 7 to 5, and every other value to 0. This includes 8 and 10 when reached through that helper (`I4:6d7048-6d7098`). Geometry records are 16 bytes each at `G+16*s`, beginning with unsigned width/height halfwords (`I4:6d7ad8-6d7ae4`).

## Command loop and transition tables

`vglib_task` has no normal return. After clearing transition state through `0x6d6e70`, it sets capacity 48 and payload pointer `R+0xc`, then repeatedly receives on task/channel 73 through `0x725318` with timeout `-1`. The receive return is not tested. If transition-active is nonzero, it skips new command setup. Unknown command halfwords also skip setup. Both routes still inspect existing status and complete status waiters if nonzero (`I4:6d7548-6d75a8`, `6d76d4-6d76e4`).

| Command | Setup | Table data and actions |
| --- | --- | --- |
| `0x5900` | Snapshot mode into requested/current, zero six status slots, install payload word at `R+0x10` in slot 0 | At `0x7cfc70`: `(0x5900,0,0x6d7e00)`, `(0x5900,1,0x6d6fb4)`, sentinel `(-1,-1,0)` at `0x7cfc88` |
| `0x5901` | Zero six status slots, install payload word at `R+0xc` in slot 0 | At `0x7cfc4c`: `(0x5901,0,0x6d7438)`, `(0x5901,1,0x6d6fb4)`, sentinel at `0x7cfc64` |
| `0x5902` | Snapshot viewport through `0x6dd9c4`, mode through `0x6dd9a4`, feature through `0x6dd9e0`; copy only the first 24 payload bytes into frame slots | At `0x7cfc34`: `(0x5902,0,0x6d7920)`, sentinel at `0x7cfc40` |

Setup evidence: `I4:6d75a8-6d76d0`. Table evidence: data words `I4:7cfc34-7cfc90`. Rows have three words, command/state/action. The dispatcher's next-state word is the following row's state, not an independent fourth word (`I4:6d6ed8-6d6f34`).

Each selected table is counted through `0x6d6f60`. A missing terminator within its bounded scan yields `-1`; the loop stores `-40` as status and takes the status-completion path. Otherwise it installs state 0, active 1, status 0 and calls `0x6d6ea8`. The dispatcher matches command and state, invokes a non-null action, clears transition state and propagates a nonzero action return, or advances to the following row's state; next state `-1` clears the transition and returns 0 (`I4:6d6f60-6d6fa8`, `6d76a0-6d76d0`, `6d6ed8-6d6f3c`). A successful `0x5902` therefore needs no extra terminal action: its next row is the sentinel.

The error/status completion helper clears each of six `R+0x100` slots before calling it with current `R+0x4c`, then sets status to zero (`I4:6d6fc4-6d7004`). This is separate from the frame callbacks. A failed render does not, on the inspected path, drain `R+0x118` frame callbacks as error callbacks.

## `FUN_006d7920`: render and publish one frame

### Inputs and mode transition

There are no explicit arguments. Inputs are the shared requested/current modes, viewport, buffer byte, tables and frame callbacks. It calls lock `0x6d9478`, initializes a local event-result word to 0, and requests option `(0x30d,0x16,9)` through `0x6499d0`. The lock and option returns are not tested (`I4:6d792c-6d7944`). Every explicit return passes through unlock `0x6d9494` (`I4:6d7df0-6d7dfc`).

When requested differs from current, mode 8 first ensures stages 0 and 3, mode 10 stages 0 and 5, by `render_manage_surface(stage,1)` at `0x6d70a8`, in that order. Either nonzero result returns `-255`. Current mode is updated only after both succeed (`I4:6d79d4-6d7a68`). There is no rollback of the first allocation on failure of the second.

For other requested modes, derive the old stage from current (8 gives 3, 10 gives 5, otherwise `render_stage_index(current)`), and new stage from requested. If stages differ, ensure the new stage and return `-255` on failure; otherwise set the old-stage variable to `-1`. Then copy requested to current (`I4:6d7970-6d79d0`). The stock bytes pass the actual current mode to the old-stage helper; the NONMATCHING C's `render_stage_index(0)` is not the contract.

If modes already match, derive the stage and set old stage `-1`. All successful setup paths call `render_manage_surface(old_stage,0)` and ignore its return, including `old_stage == -1` (`I4:6d7a6c-6d7a84`). The helper explicitly returns `-30` for stage `-1` (`I4:6d70fc-6d7104`), so that result is not a frame failure. Surface creation ensures both buffers, skips already nonzero surfaces, and returns `-255` for image allocation or surface creation failure; partially created state is not unwound here (`I4:6d7110-6d71c4`, `6d7220`).

### Render paths

Let `b = render_next_buffer()`, `S(s,b) = *(R+0x90+8*s+4*b)`, and `O(s,b) = *(R+0xc0+8*s+4*b)`. The code recomputes `b` repeatedly, not once; none of these inspected calls advances front before the final hand-off sequence. Each failed `vglib_select_surface` returns `-255` from the frame step without increment, callback queueing or frame-callback delivery.

| Current mode | Ordered drawing and hand-off | Evidence |
| --- | --- | --- |
| Other than 8/10 | Select `S(s,b)`; render `(-1,width(s),height(s),S(s,b),O(s,b))`; hand off `(G+16*s, O(s,b)->+0x1c, 0, low_byte(render_result))` | `I4:6d7aa4-6d7b38` |
| 8 | Select/render stage 0 with filter 0 and geometry `G`; select/render stage 3 with filter 3 and geometry `G+0x30`; hand off stage 0 with third argument 1, then stage 3 with third argument 0 | `I4:6d7b40-6d7c40` |
| 10 | Select/render stage 0 with filter 0 and geometry `G`; select special surface `R+0xf0`, render with filter 3, geometry from `G`, object `R+0xf4`; hand off stage 0 with third argument 1; then hand off geometry `G+0x50`, address `O(5,b)->+0x1c`, third argument 0 | `I4:6d7c48-6d7d2c` |

Mode 10 is deliberately asymmetric: its second render target is fixed special surface/object, whereas its second display address comes from the stage-5 two-buffer object table. The second render uses stage-0 dimensions, not the dimensions passed to its hand-off. These are byte-derived behaviors, not corrected interpretations of the draft C.

`vglib_select_surface(surface)` makes that surface both draw/read target with the shared display/context and template attributes, returns `-255` if the underlying call returns zero, otherwise ensures integer OpenVG parameter `0x1103` equals `0x1202` and returns zero. This is exact C; calls and result handling are also at `I4:6d78c8-6d7918`.

`FUN_006d724c(filter,width,height,surface,object)` passes the viewport as OpenVG integer vector `(0x1121,4,R+0x148)`, clears `(0,0,width,height)` through `0x85a1b0`, zeros visited/result, traverses via `0x6d92d8(render_visit_object,&filter,0xff)`, calls `0x8586d8`, then `0x88d620(display,surface,object)`. A zero return from the latter records the EGL error through `0x6d7230`, rather than directly returning `-255`. Its output is accumulated `R+0x15c`; if that is still zero and the float at `R+0x154` is positive, it sets the result to 1 (`I4:6d7258-6d7318`). The frame step narrows this result to a byte for hand-off and does not test it as an error.

The traversal visitor returns 1 for a null object and 0 for an inactive object. For an active object it reads dispatch-table `+0x84`, and calls draw method `+0xa0` when the filter is neither 0 nor 3, or filter 0 matches result 0, or filter 3 matches result 1. It ORs draw-method results into `R+0x15c`, increments visited count, and delays 0 ticks when bit 1 of the updated count is clear (`I4:6d732c-6d7434`). The draw-method implementations and resulting vector-engine pixels are outside this publisher contract.

### Publication, rotation and completion order

On every successful render path, hand-offs precede front rotation. The byte is incremented, truncated to eight bits, then set to zero if greater than 1 (`I4:6d7d30-6d7d58`). For valid starting bytes this makes front equal the buffer just rendered. Surface selection/creation failures branch to the common `-255` exit before this sequence (`I4:6d79e0-6d7a4c`, `6d7ac0-6d7ac8`, `6d7b58-6d7b60`, `6d7bb8-6d7bbc`, `6d7c60-6d7c68`, `6d7cb8-6d7cbc`, `6d7ddc-6d7dfc`). A later failure cannot be inferred to roll it back.

After rotation, the step restores option `(0x30d,0x16,*(0x8e2094))`, writes `0x6d80f0` to `R+0x130`, queues that vector through `0x6cfc68`, then calls `dispatch_wait_current_all(0xfefe,&local,100)` at `0x7243a8`. It ignores all those returns, including timeout/failure from the wait (`I4:6d7d5c-6d7da0`). It then visits the six frame callbacks in increasing address order, clearing a nonzero slot before `blx`, sets status `R+0x4c` to zero, unlocks, and returns 0 (`I4:6d7da4-6d7dfc`). The callback calls have no deliberately installed argument; treating `r0` as a completion status is unsupported.

Consequently stock success is not evidence that a presentation event arrived within 100 ticks. The inspected path runs its frame callbacks even when the wait fails. Early `-255` exits do not restore option `0x16` on this path and do not reverse mode/resource changes already made. These partial effects belong in host expectations rather than an invented transactional model.

## `FUN_006d76ec`: display hand-off

Inputs are geometry pointer in `r0` (unsigned width/height halfwords), source address in `r1`, a third argument in `r2`, and render-result byte widened in `r3`. The function immediately replaces `r2` with the source address and never uses its original value. In particular the caller's 1/0 mode does not select the engine here (`I4:6d76f4-6d7704`). Output is shared shadow configuration and downstream calls, not a checked success result.

It writes frame origin `(0,0)` at `R+0x54`, source origin `(0,0)` at `R+0x5c`, source width/height at `+0x60/+0x62`, stride width at `+0x64`, byte `0x80` at `+0x66`, and words `(1,11,0)` at `+0x68/+0x6c/+0x70`. The frame extent is normally width/height with flags `+0x74/+0x78 = 0`. For 360x240 it uses frame 720x480 and both flags 1; for 960x544 it uses frame 1920x1080 and flags 1, with the height computed modulo 16 bits as `2*(544+0x7ffc)` (`I4:6d7708-6d7810`).

Ordered calls are `0x6d03b8(1,0,address)`, `0x6cfed0(2,R+0x54)`, `0x6d040c(1,0,R+0x5c)`, `0x6d0498(1,0,render_result)`, `0x6d04ec(1,R+0x68)` (`I4:6d7814-6d785c`). The first wrapper selects engine 1 iff `*(0x955af8+0x28)==0` and `*(0x955af8+0x10)==1`, otherwise engine 0, then calls `display_set_item_configuration(engine,1,0,address)` (`I4:6d03c0-6d0404`).

`display_set_item_configuration` rejects unsigned item >3 with `-30`; otherwise it replaces only the low 30 bits of shadow word `0x96b9bc+420*engine+76*group+16*item+0x64`, preserving its upper two bits and returns 0. Group 1/item 0 is shadow offset `+0xb0` (`I4:6c9cfc-6c9d34`). The hand-off ignores this result. This is a RAM-shadow update, not a synchronous scanout-register write or acknowledgment of scanning. The other selected setters' device commit semantics belong to the scanout contract.

## Display completion bridge

Exact `FUN_006cfc68(callback_vector)` chooses the same engine predicate as above, then forwards to `display_queue_secondary_callbacks(engine,callback_vector)` at `0x6c8fac` (`I4:6cfc70-6cfca4`). The latter selects per-engine state at `0x9553cc+88*engine`, returns `-10` if its enabled byte `+0x54` is zero, otherwise enters a critical section, visits six destination slots at `+0x3c`, and copies each source word only where its corresponding destination is zero. If state `+0x10` is zero it sets `+0xc` to 1. It exits the critical section and returns 0 (`I4:6c8fb8-6c9024`). Occupied slots are retained, not appended or overwritten; the void wrapper/frame step do not act on refusal.

`FUN_006d80f0()` constructs a four-word request `(vglib_post_display_event,0,2,0)` and passes its address plus a local token-output address to scheduler registration `0x6c7318`; it does not consume the returned token or check the result (`I4:6d80fc-6d8128`). This is the one-shot request shape: last word 0, not persistent flag 1. Registration copies the callback/request into scheduler state rather than retaining the stack pointer (`I4:6c73d0-6c7424`); the persistent count changes only for last word 1 (`I4:6c7428-6c7440`). Registration can fail, and this bridge has no error-reporting path to the waiting task. Actual interrupt timing and scheduler delivery require the separate scheduler/scanout contract.

Exact `vglib_post_display_event()` calls notification `0x724558` with `(73,0xfefe)` (`I4:6d7f38-6d7f44`). It does not carry a frame id or buffer id. The byte-derived chain is hand-off, software front advance, secondary callback queue, wait; later display completion runs `0x6d80f0`, scheduler delivery runs `0x6d7f30`, task notification releases the wait. No inspected byte establishes that this notification uniquely identifies the just-rendered frame.

## Scheduling and lifecycle entry contracts

### `vglib_schedule_animation(index, completion)` at `0x6dcb60`

If open count `A+0` is zero, it invokes a non-null completion immediately with the original index still in `r0`, then returns `-11`; no lock or message is issued (`I4:6dcb74-6dcb9c`). When open, unsigned index >6 returns `-30` without invoking completion (`I4:6dcba0-6dcba8`).

Otherwise it locks, copies mode `A+0x20`, the 16-byte options at `A+0xc`, and feature byte `A+0x205` into a 48-byte local packet, zeroes its first 24 bytes, writes completion at `packet+4*index`, unlocks, and calls `vglib_send_parameters(packet)` at `0x6d80c8` (`I4:6dcbac-6dcbfc`). That wrapper sends command `0x5902`, size 48, via message helper `0x6d7f48`, normalizing a nonzero send result to `-255` and zero to 0 (`I4:6d80d0-6d80ec`). There is no retry or synchronous presentation wait in this entry.

Index 6 is admitted by the bytes but writes packet offset 24, overwriting the first option word, not a seventh frame callback. Command handling copies only packet offsets 0..23 into frame callbacks. This report preserves that boundary behavior and does not infer an intended seventh slot. Index 0..5 populate the six actual callbacks (`I4:6dcbc0-6dcbf0`, `6d7684-6d7690`).

### Open `FUN_006dc8d4(input, completion)`

With count zero it requests resource `0x1b` with output at `A+4`; a nonzero resource return is returned unchanged. It next sets option `(0x30d,0x18,*(0x8fa828))`, again returning a nonzero result unchanged. It copies four bytes from input to `A+0x20`, copies 16 default-option bytes from `0x7cfd3c` to `A+0xc`, and calls `0x6d919c`; a nonzero result becomes `-255` without incrementing count (`I4:6dc8e4-6dc960`, `6dca0c-6dca10`). Additional argument registers at the resource boundary are not assigned a stronger public meaning here.

For a non-null completion it sends initialize `(input,completion)` through `0x6d8048`. For null completion it clears `A+0x204` and `A+0x1c`, sends initialize with internal callback `0x6dbf6c`, and repeatedly delays 5 ticks until the byte becomes nonzero; the send result is ignored on this synchronous branch. It then returns `A+0x1c`. Both branches increment open count even if the asynchronous send or eventual completion result is nonzero (`I4:6dc964-6dc9d8`, `6dc9f4-6dca08`). Internal callback `0x6dbf6c(status)` writes completion byte 1 and status `A+0x1c` (`I4:6dbf74-6dbf84`). No timeout or failure rollback is present in the inspected open path.

With an already nonzero count, it calls a supplied completion synchronously with 0, ignores its return, increments count and returns 0, or just increments and returns 0 for null completion (`I4:6dc9dc-6dca08`).

### Close `FUN_006dca18(completion)`

It decrements open count before any validation. A resulting nonzero count returns 0 without callback or teardown. Closing at zero therefore underflows the word and takes that same path; this is not an established safe API use (`I4:6dca24-6dca40`). On transition to zero it traverses release callback `0x6dc414` with filter 0 and mask `0xff`, clears active groups through `0x6d91fc`, clears synchronous byte/result, sends `0x5901` completion with internal callback `0x6dbf6c`, and delays 5 ticks until completion (`I4:6dca44-6dcaa4`).

It releases resource `A+4` via `0x649ddc`, zeroes that handle, reads the completion result, optionally calls caller completion with it, and returns the saved result. Traversal, clear, send and resource-release results are not checked; the wait has no timeout (`I4:6dcaa8-6dcae0`). The public close completion is synchronous even though the task command is queued.

## Proposed host contract cases and runner feasibility

These are pre-run expectations, not passed cases. Implement paired stock/candidate cases in `//emulation:native-runner` with proprietary slices under `private-data/analysis/x-m1-1.50/native-case/vglib-frame/`. Follow [native-case batches](../../emulation/README.md#native-case-batches) and the [schema](../../schemas/native-case.schema.json). A Haskell generator should record image extents and hashes, seed shared RAM and sentinels, and independently encode expected external calls, output registers and complete final writable memory before execution. Candidate-specific internal function organization must not be the oracle.

| Case family | Inputs and independently expected observations | Feasible isolation |
| --- | --- | --- |
| Back buffer | Front 0/1 gives 1/0 and no writes; invalid bytes 2/255 both give 0 | Execute stock `0x6d700c` directly, no OS stubs |
| Ordinary frame | Both front values, unchanged mode, distinct per-stage/per-buffer handles/addresses; verify selected back surface, geometry, publication address, new front, callback order and result 0 | Execute frame bytes; stub lock/unlock, option, surface selection, render entry, hand-off, queue and wait |
| Stage mapping and transitions | Modes 2/3/5/6/7/default; transitions from 8/10 and between modes sharing stage 0; ensure-before-current-write, old release after update; `-1` release refusal ignored | Execute mapping helper as stock or include it; surface-manager stubs return scripted results |
| Ensure failures | Fail first/second ensure in modes 8/10 and ordinary transition; result `-255`, front unchanged, no hand-off/queue/wait/frame callback; retain earlier allocation effects | Script nonzero ensure returns; compare caller-visible effects, including absent option restoration |
| Select failures | Fail ordinary select, first dual-mode select, or second dual-mode select; no rotation/hand-off/queue after failure; first render already occurred on second-select failures | Stub rendering with distinct results and record event sequence |
| Modes 8 and 10 | Distinct stage-0/stage-3/stage-5/special handles and geometries, distinct render-result words; assert the two path-specific render targets and hand-off addresses, low-byte narrowing | Two passes remain within bounded stub events; no pixel engine required |
| Wait success/failure | Same successful render with wait returning success, timeout or another failure; all return 0 and deliver the same six frame callbacks after the wait | Stub `dispatch_wait_current_all`; no scheduler emulation claimed |
| Slot delivery | Empty, sparse and six occupied slots; callbacks observe own slot zero, later slots still present; increasing address order; error path leaves frame slots undelivered | Synthetic callback functions may record only externally observable state/effects |
| Shadow hand-off | Generic, 360x240 and 960x544 geometry; ignored third argument 0/1; render-result byte; verify ordered setter boundary calls and exact scratch-state bytes | Execute `0x6d76ec` with selected setters stubbed; separately execute source wrapper and setter against RAM shadow |
| Shadow masking | Both engine selections, nonzero upper shadow bits, addresses with upper bits set; preserve top 2 bits and replace low 30; item 4 gives `-30` without store | Execute `0x6d03b8`/`0x6c9cf4` with mapped state; no MMIO needed |
| Secondary queue | Engine predicate combinations, enabled/disabled, occupied destination slots; only empty slots copied, disabled return `-10`; wrapper has no error propagation contract | Execute queue bytes with critical-section stubs and six readable source words |
| Bridge requests | `0x6d80f0` submits `(0x6d7f30,0,2,0)` and ignores registration failure; `0x6d7f30` posts `(73,0xfefe)` | Stub registration with pointer capture; execute notification wrapper with OS stub |
| Schedule | Closed with null/non-null callback returns `-11`; open index 7/unsigned-max returns `-30`; indices 0..5 set one slot; 6 changes option word; send refusal becomes `-255` | Execute schedule/send wrappers with memcpy/fill and OS/message stubs, capturing full 48-byte packet |
| Open | Existing count with null/non-null completion; first-open resource/option/group-init failure; async send failure still increments; sync internal completion returns seeded status | Execute bytes; delay stub writes completion byte/result on chosen iteration; resource and queue calls are stubs |
| Close | Count 2 returns 0/no callback; count 1 runs teardown and returns callback status; count 0 underflows/no teardown; handle reset despite release failure | Same bounded delay injection; lifecycle service calls stubbed |
| Task/transition | `0x5902` snapshots and copies six slots, dispatches exactly frame action; initialize/close tables preserve action order; active/unknown commands skip setup; table overflow yields `-40` | Prefer finite transition-helper cases plus one bounded receive-loop prefix, not an expectation that the infinite task returns |

The runner uses A32 tracked functions/stubs and writable memory regions. The inspected top-level paths use supported scalar/branch/table instructions; `FUN_006d724c` also has VFP loads/comparison (`I4:6d72f4-6d730c`). Executing that entry rather than stubbing it requires the documented VFP profile and mapped object traversal closure. Feasibility is static, not an admission result from an executed case. Callback `blx` targets must be declared A32 functions or stubs. A finite loop-prefix task case must use an explicitly expected instruction-budget stop or a harness stop boundary; it must not pretend task 73 returns. Stubs that inject completion must be declared as environmental effects, not evidence that real callbacks arrived.

Calls such as `dispatch_wait_current_all`, dispatch receive/send, delays, critical sections and resource services would be stubs. Full-frame cases can stub the draw-list/OpenVG entry while smaller cases exercise its viewport/visitor/accumulation behavior independently. Neither arrangement establishes rasterized pixels. Lifecycle tests must inject completion state rather than allowing an unbounded delay loop. Native batches isolate case state; callback, surface and token addresses must not leak between cases.

## Owned publisher boundary and unestablished properties

The owned design should use typed surface, buffer and frame identities and represent the two-buffer lifecycle in one record. The stock contract supplies ordering and completion-adapter obligations, not the design's stronger `Free -> Drawing -> Queued -> Scanning -> Free` law. Stock rotates the software byte before waiting, ignores wait failure, has untagged notifications, and has no inspected acknowledgment that a newer buffer is scanning. An owned `FramePresented(FrameId)` adapter must be tied to the scanout driver's actual acknowledgment, not merely a successful return from `0x6d7920`.

The following remain unestablished: real scanout timing, vector-engine rasterization, cache/DMA coherency, whether all producers serialize on this lock, how the five remaining secondary callback-vector words are initialized, real scheduler one-shot delivery under load, notification coalescing and stale-event behavior, valid external use of schedule index 6, and camera throughput. The two special modes' full routing semantics are not inferred from their numeric values. No host equivalence, device timing law or safe buffer-reuse proof is claimed. The report does establish byte-derived function boundaries and proposed host observations without adding code or changing the target's scope.
