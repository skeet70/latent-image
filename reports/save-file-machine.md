# Save file machine (X-M1 1.50)

Steps 4, 6, 7 and 9 of the [capture save path](capture-save-path.md): the overlay-46 state machine that takes a descriptor from the save facade's `0x2b01` message to a closed file and calls the facade's completion. Everything here is read from the A32 bytes of the plain image (`plain-01100000.bin` sha256 `ee211bda…d2889d`), image 4 and image 5; the x70view decomp (`55436fc85c`) served only as a reading aid. The owned unit is [`replacement/save_file_machine`](../replacement/save_file_machine/).

## Record and registration

The machine is one message-registry record at `0x014ddbfc`; the machine's other RAM is the saved path `0x014ddba8` (48 bytes), the pending cancel completion `0x014ddbd8` and the output mode `0x014ddbe4`. No code outside the extent names `[0x014ddba8,0x014ddc28)`.

| Offset | Field |
| --- | --- |
| `+0x0` | registry worker (1 for a save, 2 for a cancel that registers) |
| `+0x4`, `+0x6` | slot index the registry assigns; halfword 0 |
| `+0x8` | 12 |
| `+0xc` | state 0..4 |
| `+0x10`, `+0x14` | event table `0x011d5d44`, state table `0x011d5dac` |
| `+0x18` | the message being dispatched |
| `+0x1c` | owner, the record itself |
| `+0x20` | completion |
| `+0x24` | result passed to the completion |
| `+0x28` | file descriptor |

`0x6e27ac(record, worker, 0, 12, [sp] 0, events, states, record)` clears the first 32 bytes, fills `+0x0..+0x1c` and takes a free slot of the worker's registry (`0x96e04c + 12*worker`, slots `0x95761c + 128*worker`), storing its index at `+0x4`. `0x6e2808(record)` frees it. `0x6e2b80(record, code, word2, word3, [sp] word4)` sends the 16-byte message `{code, slot, queue, word2, word3, word4}` to the worker's queue (`0x7d088c[worker]`: 0x1d for worker 1, 0x1b for worker 2) through `0x72503c`. The worker's dispatch `0x6e2aa0(registry, message)` finds the record by the slot byte, stores the message at `+0x18`, maps the code through the event table (halfword code, word event; 91 when absent) and calls the handler the current state's list names for that event with the record. An event without a handler in the current state is dropped.

## Entries

| Entry | Arguments | Effect |
| --- | --- | --- |
| `0x011d5600` | `(worker, descriptor, completion)` | register; descriptor `+0x14c` = 0; result 0; completion and descriptor stored; post `0x2b01` |
| `0x011d5688` | same | as `0x011d5600` with descriptor `+0x14c` = 1 (append to an existing file) |
| `0x011d5714` | `(pending)` | cancel: when the state is 0, register with worker 2 and clear the result; store `pending` at `0x014ddbd8`; post `0x2b18` |
| `0x011d57ac` | `(descriptor, data, size)` | append a buffer to the scatter list; 0, or -1 when full |
| `0x011d5850` | `(mode)` | output mode 0..3: call `0x70f790` with 0, 0, 1, 2, store the mode, return 1; above 3 return 0 |
| `0x011d591c` | `(buffer, descriptor)` | copy 48 bytes of the descriptor's name out |
| `0x011d5934` | `(text, descriptor)` | cut `text` at its first `.` within 48 bytes; when the cut is at index 43 or below, append `.RAF` (kind 3), `.JPG` (kind 0) or `.MPO` (other kinds), copy 48 bytes into the name and set kind 7; no `.` or a later one changes nothing else |

Callers (bytes, direct A32 `bl`): `0x011d5600` from image 5 `0xe51ae8` and plain `0x12353b8`, `0x1247af8`, `0x124afc8`; `0x011d5688` from `0xe51a94`; `0x011d5714` from `0xe50e74`; `0x011d57ac` from `0xe51cf0`; `0x011d5850` from `0xe51a0c`, `0xe82518`, `0xe82d10` and plain `0x12353a4`, `0x1247ae4`, `0x124afb4`, `0x12551b0`; `0x011d591c` from `0xe51b48`; `0x011d5934` from `0xe51a50`. The census over image 4, image 5, the plain image, both loaders and both boot images (branches and MOVW/MOVT pairs from the disassemblies, words at every halfword-aligned offset) finds nothing else naming the extent `[0x011d5090,0x011d5df8)` from outside except two image-5 data words inside a run of high-entropy bytes: `0x011d5471` at `0x1090cbc`, odd, and `0x1a04f515` at `0x1098488`, which decodes as a `bne` to `0x011d58e4` between undefined encodings.

The callbacks the machine hands to the file service are the other addresses that leave it: open `0x011d5198` posts `0x2b15`, seek `0x011d5164` posts `0x2b17`, write `0x011d5130` posts `0x2c38`, each with `(status, value)`; close `0x011d5100` posts `0x2b16` and delete `0x011d50d0` posts `0x2c3c`, each with `(status, 0)`.

## Descriptor and scatter list

The descriptor sits at `frame + 0x27d0 + 0x150*bank` (facade). `+0x0` handle, `+0x4` kind, sixteen entries `{data, size}` at `+0x8 + 8*i`, `+0x88` write index, `+0x8c` name (48 bytes), `+0x148` prefix source, `+0x14c` append flag.

The write index starts at 0 when the file opens. Entry `i` is written as `0x71c75c(write callback, handle, size, data)`; each successful write result advances the index, and the first entry whose data word is 0 ends the list and closes the file. Entry 0 is written even when its data word is 0. `0x011d57ac` puts its buffer in the first entry whose data and size are both 0, provided that entry is below 15, zeroes the next entry and clears the write index.

## Messages, events and states

| Message | Event | Source |
| --- | --- | --- |
| `0x2b01` | 6 start | `0x011d5600`, `0x011d5688` |
| `0x2b18` | 7 cancel | `0x011d5714` |
| `0x2b15` | 15 opened | open callback |
| `0x2b17` | 16 positioned | seek callback |
| `0x2c38` | 19 written | write callback |
| `0x2b16` | 18 closed | close callback |
| `0x2c3c` | 21 deleted | delete callback |

| State | Event | Handler | Action |
| --- | --- | --- | --- |
| 0 idle | 6 | `0x011d5aac` | name and open; state 1 |
| 0 | 7 | `0x011d51f0` | saved path set: delete it; otherwise run and clear the pending completion |
| 0 | 21 | `0x011d52f4` | clear the saved path, unregister, run and clear the pending completion |
| 1 open | 15 | `0x011d5540` | status 0: append flag 1 seeks `0x71c8d8(seek callback, handle, 0, 0, [sp] 0)`, otherwise writes the current entry; error below |
| 1 | 16 | `0x011d54b8` | status 0: write the current entry; error below |
| 1 | 19 | `0x011d5400` | status 0: advance and write the next entry, or close at the end of the list (state 2); error below |
| 1 | 7 | `0x011d533c` | release and close `0x71c938(close callback, handle)`; state 3 |
| 2 closing | 18 | `0x011d536c` | status 0: copy the name to the saved path; state 0, unregister, completion(result); error below |
| 2 | 7 | `0x011d5090` | state 4 |
| 3 cancelled open | 15, 19 | `0x011d50ac` | completion(0) when set |
| 3 | 18 | `0x011d5250` | state 0, unregister, run and clear the pending completion |
| 4 cancelled close | 18 | `0x011d51cc` | delete the descriptor's name |
| 4 | 21 | `0x011d5290` | clear the saved path; state 0; unregister; completion(0) when set; run and clear the pending completion |

"Run and clear" calls the word at `0x014ddbd8` with no argument when it is nonzero and then zeroes it.

## File requests

| Request | Service | Arguments |
| --- | --- | --- |
| open (`0x2c17`) | `0x71c648` | open callback, name, mode 1 (create) or 2 (append flag 1); the return is stored as the handle |
| seek (`0x2c1c`) | `0x71c8d8` | seek callback, handle, 0, 0, [sp] 0 |
| write (`0x2c19`) | `0x71c75c` | write callback, handle, size, data |
| close (`0x2c1a`) | `0x71c7bc` | close callback, handle, 0, 0 |
| release and close (`0x2c1d`) | `0x71c938` | close callback, handle |
| delete (`0x2c03`) | `0x70f430` | delete callback, saved path or descriptor name |

Naming (`0x011d5aac`, append flag 0 and kind not 7): `0x011d59f4` calls `0x70fbfc(name, 0, prefix)` with `prefix` from properties `0x14f..0x152` when `+0x148` is 1, from `0x153..0x156` when the output mode is 1, and 0 otherwise; then `0x721bac` appends `.JPG` (kind 0), `.MPO` (1), `.RAF` (3), `.RAW` (6) or `.WAV` (any other kind but 7). Kind 7 and the append flag open the name as it stands.

## Errors, cancel and completion

| Failure | Result | Report `0x726c74(1, n)` | Then |
| --- | --- | --- | --- |
| open status nonzero | -4 for status 3, else -1 | `0x70` | state 0, unregister, completion(result) when set |
| seek status nonzero | -4 for status 3, else -2 | `0x71` | state 0, unregister, completion(result) when set |
| write status nonzero | -4 for status 3, else -2 | `0x71` | release and close; state 2, so the close result completes with this result |
| close status nonzero | -3 | `0x72` | state 0, unregister, completion(-3) when set |

Cancel (`0x011d5714`, event 7) closes an open file without completing (state 3, then idle on its close result), turns a pending close into close-then-delete (state 4), and in idle deletes the last saved file. The facade's completion `0x00e4e5dc(result)` stores 0, 1 (result -4) or 2 at `0x0154d970` and, when the task word `0x0155ccb8` is set, sets flag `0x10` on it through `0x724558`.

## Owned extent

`[0x011d5090,0x011d5df8)`: 29 functions, the event and state tables and the five extension strings. The owned link keeps the two tables at `0x011d5d44` and `0x011d5dac`, whose addresses the record holds, and branch veneers at the seven called entries and the five callbacks; the handlers and state lists move. Retained calls: registry `0x6e27ac`, `0x6e2808`, `0x6e2b80`; file service `0x71c648`, `0x71c75c`, `0x71c7bc`, `0x71c8d8`, `0x71c938`, `0x70f430`; report `0x726c74`; name `0x70fbfc`, prefix mode `0x70f790`, property `0x69f8fc`; `strcpy` `0x721b04`, `strcat` `0x721bac`, `memcpy` `0x722054`.

The [Exif writer](exif-writer-interface.md) shares the overlay: its extents end at `0x011d0404` and start again at `0x011db098`, so both owned images patch one overlay-46 carrier without overlap.

## Staged image

[`plain-overlay-stage`](../replacement/README.md#plain-image-overlay-staging) stages both units from their `plain-overlay-unit.json` into the plain image and the DAT. Overlay 46 is resident from attach on the cardless menu, card playback and PROTECT routes, and holds the staged bytes there. No route enters or fetches the machine, so the staged pairs show residency and non-interference for it, not behaviour (E-OVERLAY46-STAGED-PAIR-150).

## Blocks to a complete owned version

- No emulated capture reaches save ([#139](https://github.com/skeet70/latent-image/issues/139)); the unit is qualified natively through the stock registry dispatch, with every stock branch direction reached (E-SAVE-FILE-MACHINE-NATIVE-150, [suite](../replacement/README.md#save-file-machine)).
