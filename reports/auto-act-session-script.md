# AUTO_ACT arming session: wait before power-on, arm, measure, watch, cut supply (1.50)

Status: revised 2026-10-09 after the owner-run session `20261009T143802Z-arm-auto-act` returned `no-send` with all witness cells zero and no `.MSG` file, and **on hold** after `20261009T160839Z-arm-auto-act` armed and again observed no sender: the delay unit is about 1 ms, so the one-shot test falls about 1 s after task start (E-AUTOACT-160839-150, E-AUTOACT-UNIT-150), and the acts below apply only once the window's position has been measured (arming report section 13.4). The verified card file was already in place. The reviewed arming bytes remain unchanged; this revision adds device waiting and read-only boot-tick measurements, and moves standalone capture-file writes after the arm while retaining every precondition before it. Only the owner runs the live command under [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs).

## 1. What this session is for

The camera contains the AUTO_ACT script facility in full: the loader `0x00ed1de0`, the 13-keyword parser and the self-paced executor all live in the official 1.50 images ([the script map](auto-act-script.md)). It is disabled as shipped because the only enable gate is adjustment byte `0xa2 == 2` at the task prologue, and that byte reads `0x00` in all three captures of this body. A host config write of `0x02` to index `0xa2` lands in the RAM mirror of the adjustment area and posts nothing, so the arming is temporary: the boot load refills the mirror from storage and the next power cycle clears it.

With the byte set, the camera's `mujin` task reads index `0xa2` once per boot, waits, and then loads `<card>:\DCAA\auto_act.scr` on its own queue with no host involvement. **The camera reads the card once per boot while armed**: the test is a single prologue read about 1 s after the task starts, and the loader follows about 4 s later ([the timing section](auto-act-arming.md#13-the-delay-unit-the-host-command-latency-and-the-window-verdict-2026-10-09)). The previous acts made the owner enter maintenance mode, wait for enumeration and only then launch the command. His `no-send` run missed the test. This time the command starts with the camera off and polls for enumeration, then opens, checks and arms without another human act.

This session is the smallest experiment that shows the camera read, parsed and ran a script this project wrote. It changes no persistent state: the arm byte is RAM only, the script's own file is the only card-side artifact, and the session ends by power cycle because the power cycle is the clean restoration. **The witness cells are read inside the session, before the power cycle, because the power cycle clears them.**

## 2. Card preparation, on the host, before the camera is touched

The loader builds `<active volume>:\DCAA\auto_act.scr` from the template `A:\DCAA\auto_act.scr` at `0x00ed3ac4`, replacing the volume letter with the active one (byte-read). The volume on this body is the SD card (E-SRECORD-ROUTE-150). The loader does not create the `DCAA` directory, so it must exist on the card, and the file must be 13 bytes with exactly these bytes:

```text
WT_LOG\r\nEND\r\n
```

SHA-256 `bdd37b75ad3e2664ea8fc42fc9611d4fd761abde4fe487b0d357348beddd11e8` (static-inference from the byte-read grammar; the same string is the first script of the payload design).

| Step | Do this | Expect |
| --- | --- | --- |
| 2.1 | Put the card in the host's reader. Note what is already in its `DCIM` tree (a new `.MSG` file appears there afterwards). | the card mounts; write protection off |
| 2.2 | Make the directory `DCAA` at the card's root if it is not there. | `<card>/DCAA/` exists |
| 2.3 | Write the 13 bytes above to `<card>/DCAA/auto_act.scr`. FAT is case-insensitive; keep the name lowercase and the file under 1 MiB (it is 13 bytes). | the file exists |
| 2.4 | Verify the bytes before the card goes in the camera: `wc -c` is 13, `sha256sum` is the digest above, `od -c` shows `W T _ L O G \r \n E N D \r \n`. | all three checks pass |
| 2.5 | Eject the card and put it in the camera. | the card door shuts |

```sh
mkdir -p "<card>/DCAA"
printf 'WT_LOG\r\nEND\r\n' > "<card>/DCAA/auto_act.scr"
wc -c "<card>/DCAA/auto_act.scr"
sha256sum "<card>/DCAA/auto_act.scr"
od -c "<card>/DCAA/auto_act.scr"
```

The script is two statements: `WT_LOG` writes a log file the camera names itself, into the active volume's `DCIM` tree, about 4 KiB from the firmware's log buffer; `END` stops the statement chain. It presses no key and calls no camera routine (byte-read statement table `0xed3a00`; `WT_LOG` handler `0x00ed28b4`, writer `0x00ed1850`). A card without the file, or without the directory, is also a valid session: see the interpretation table.

## 3. Before anything

1. Power the camera from the dummy-battery supply. Do not move the supply switch during the session.
2. SD card in the camera with the prepared file. Lens optional; it stays mounted for the whole session.
3. USB cable attached to the host; the host is on at the repository.
4. Camera off.

Start the command before power-on. While no camera is present it lists devices only, sends no service frames and waits for at most 120 s. The waiting message is the cue to enter maintenance mode.

## 4. The session

Every act is one physical act plus one line in the note sheet of section 5. Do the acts in order. The camera shows nothing for any of this: the script writes a file to the card, not to the screen.

| Act | Do this | What the command should show |
| --- | --- | --- |
| 4.0 | Open the note sheet. Write the date, the body, the card's pre-session `DCIM` listing and "file in place". | nothing yet |
| 4.1 | **Start before power-on.** With the camera off and USB already attached, run `devenv shell -- bazel run //transport:camera -- arm-auto-act --wait-for-device`. Write the printed output folder path into the note sheet. Do not start any other camera command. | `output: …`, then `arm-auto-act: waiting for 04cb:ff80, up to 120 s; power the camera into maintenance mode now` |
| 4.2 | Maintenance entry, only after the waiting message: hold the shutter fully down and the Up key together, switch the power on, keep both held until the screen is blank and the green LED is lit, then release both. Leave USB connected. | the front LED turns green; the command proceeds automatically when `04cb:ff80` appears |
| 4.3 | Wait. The command requires both session-start adjustment bytes to be zero, enables and confirms the reviewed `0xf7` gate, reads the first boot tick, requires clear witness cells, writes `0xa2 = 0x02` and confirms it. It then watches for up to 90 polls and reads final witnesses and the end tick before gate restore and C4. | `arm-auto-act: <outcome>;guard=…;length=…;selector=…;status=…;open=…;completion-count=…;index=…;count=…;settled=…;polls=…;tick-first=…;uptime-first-s=…;tick-end=…;uptime-end-s=…` |
| 4.4 | Write the whole result line, including both ticks and uptime fields, into the note sheet and compare it with section 6. A failed run with a first-contact timing bound is useful data. | one of `ran-to-end`, `file-op-failed`, `load-failed`, `load-running`, `no-send`, `no-load` |
| 4.5 | **The ending.** Cut the dummy-battery supply. Do not touch the POWER switch: a POWER-switch ending of a maintenance session writes a defaults record and resets date, time, language and the C slot (E-JIG-CYCLE-D-150). Unplug the USB cable only after power is gone. | power off |
| 4.6 | Read the card on the host and compare the `DCIM` listing with act 4.0. A new `.MSG` file of about 4 KiB is the outside witness that the camera read, parsed and executed statement 0. Delete it host-side if it is not wanted. | the listing differs by the one new file, or it does not |

Never during the session: a second camera command, a powered cable pull, the POWER switch, removing the card, or opening the card door.

The power cycle of act 4.5 is the whole disarm. The command sends no write of `0x00` to index `0xa2`: both senders re-read the byte when they run, so a disarm that landed before them would silently suppress the run, and the boot load makes the power cycle clean.

### Aborts and stops

Every stop ends the way act 4.5 does, with the supply cut: the session writes no storage, so the boot load is the cleanup and the next boot returns `0xf7` and `0xa2` to their stored `0x00`.

| Symptom | Action |
| --- | --- |
| a refusal or precondition line, or any stop before the arm write | cut the supply; only the session RAM changed |
| wait expires with no device | no camera service requests were sent; if the body is powered, cut the supply; do not launch a second command in that boot |
| a stop after the arm write (a readback, decode or reply check) | cut the supply; the arm and the gate are RAM mirror bytes |
| `cleanup uncertain` in the stop message | cut the supply. An exhausted aggregate budget can end the session before the gate restore, and the boot load is what returns `0xf7` and `0xa2` to their stored values |
| USB drop, host crash or reset mid-session | power the host back, cut the supply, report |
| any unexpected camera behaviour | stop camera commands, cut the supply, report to the owner |

Never after a stop: a retry, a second command in the same boot, a POWER-switch ending, or a powered cable pull with a request outstanding.

## 5. The note sheet

One row per act. "Result line or listing" is the command's printed line, or the card listing.

| Row | Act | Time | Result line or listing |
| --- | --- | --- | --- |
| 1 | 4.0 |  |  |
| 2 | 4.1 |  |  |
| 3 | 4.2 |  |  |
| 4 | 4.3 |  |  |
| 5 | 4.4 |  |  |
| 6 | 4.5 |  |  |
| 7 | 4.6 |  |  |

On a completed run the printed output folder contains `precondition.bin`, `arm-readback.bin`, both gate readbacks, and the two `witness/before-*.bin` and two `witness/after-*.bin` windows. The before windows were read and checked before the arm; their standalone file writes now happen after it. `journal.v1` carries the raw replies, preconditions, every watch poll and both `camera-tick` records, each with the raw tick, seconds and rate `99.88` Hz. A refusal before the deferred file writes can leave only the journal's raw replies.

## 6. The witness cells, and how to read them

The command prints, and the journal carries, the cells of the two 256-byte windows `0x1667000` and `0x1677700`, read through the reviewed `0x200001` function 2 requests while the `0xf7` gate is open. The addresses are byte-read from the pinned 1.50 images; the values come from the run.

| Cell | Address | Meaning |
| --- | --- | --- |
| guard | `0x1667048` | 0 before the task's send; the sender sets 1 after posting both messages |
| sentinel | `0x1667050` | the loader's `-1`; a `wait` statement overwrites it with the resolved delay |
| statement index | `0x1667054` | 0 after a load, one per executed statement, `-1` at `END` |
| statement count | `0x1667058` | one more than the file's statement lines; 3 for the 13-byte script |
| record 0 | `0x166705c`, `0x1667060` | the first statement's code (`0x18` is `WT_LOG`) and its keyword text |
| input length | `0x16777ac` | the load's size: the file size, written only when the completion count equals the requested size |
| in flight | `0x16777b0` | 1 while a file operation is pending |
| status | `0x16777b4` | the last file operation's status; 0 is success |
| open result | `0x16777b8` | the open's result word |
| completion count | `0x16777bc` | the last completed file operation's byte count: the loader's read, then the script's own file operations, so a settled `ran-to-end` shows the `WT_LOG` write's count (about 4 KiB, the `0xfff` the handler passes its writer) |
| selector | `0x16777c0` | 0 before the arm message, 1 after it and at `END`, 2 while the statement table runs |

The outcomes the command names, and what the cells must hold for each:

| Outcome | guard | length | selector | status | index | means |
| --- | --- | --- | --- | --- | --- | --- |
| `ran-to-end` | 1 | 13 (nonzero) | 1 | 0 | `-1` | the file was read and parsed, the statement table ran, and `END` stopped the chain: the strongest card-side result this script can give |
| `file-op-failed` | 1 | 13 (nonzero) | 1 | nonzero | `-1` | the load and the statement table ran to `END`, and a file operation after the load failed: with this script its own `WT_LOG` write, so the `.msg` file is absent or short |
| `load-running` | 1 | 13 or 0 | 2 | 0 | not `-1` | the loader ran and the statement table is executing, still or stalled; a script with no `END`, a long `wait`, or a record whose code has no dispatch entry |
| `load-failed` | 1 | 0 or 13 | 1 | nonzero | not `-1` | the sender posted and the loader cleared the script area, but a file operation failed before `END`: the file did not open, or its read or close failed, so the card, the `DCAA` directory or the name is the thing to check |
| `no-send` | 0 | 0 | 0 | 0 | 0 | the gate test did not see 2 at its one instant this boot; compare the first tick and timing bound before deciding whether another trigger is required; no same-boot retry |
| `no-load` | 1 | 0 | 0 or 1 | 0 | 0 | the sender posted and the loader produced no state within the bound; the cells are the record of what the camera had done at the bound's end |

A settled run additionally shows `in-flight` 0. `settled=no` means the watch reached its 90-poll bound before the predicate held; the cells are then the camera's state at the bound, and a `selector` of 2 with records present is a script still running.

The status is read as the script's own only when the statement index is at `-1`, and a failed load never reaches `END`: the loader clears the index and only `END` writes `-1`.

### First-contact timing

The first tick reads the little-endian u32 at `0x400c0800` immediately after the gate enable readback and before the baseline witnesses and arm. The end tick reads the same cell after the final witnesses and before gate restore. Both use the existing 128-byte `0x200001` function 2 shape. The conversion is `seconds = ticks / 99.88`, the measured rate of E-AF-TRACE-MECH-150 (`[0xFFF91000]/1875`).

The first uptime includes the camera's boot before image 5 creates `mujin`, plus the host's open, session-start config read and gate sequence. It is an upper bound on elapsed time since task start at the tick sampling instant, not the time remaining before the test and not a measurement at enumeration or at the first USB byte. Task-start tick and the test's actual tick remain unmeasured. With the corrected unit the test falls about 1 s after task start, but task creation follows power-on and the task-start tick is unmeasured, so the uptime by itself neither proves nor excludes that the test has passed. The read set in the arming report's section 13.4 measures the window's position directly.

Both reviewed runs' host journals measured about 1.18 s from prepared to the arm frame's outgoing record, including all existing preconditions (1.180686930 s in the 16:08 run). That interval is longer than the whole corrected task delay, so the earlier 8.82 s margin claim is withdrawn (E-AUTOACT-UNIT-150): a run that reaches the test at all must arm within about a second of task start. The separate capture-file writes move after the arm to remove filesystem work from that path; the zero-byte preconditions, gate confirmation and clear-witness check cannot move after it without weakening the reviewed refusal guarantees. Raw replies and outgoing bytes remain durable before their checks and dispatches.

If the first-contact bound combined with a task-start or gate-test bound shows the test had already passed before the host could arm, this route cannot work by promptness. Stop this route and choose a different trigger rather than seeking a faster command. A `no-send` without that task-relative bound still proves no sender was observed, not a numeric lateness.

The card-side artifact is the independent witness: a new `~4 KiB` `.MSG` file in the active volume's `DCIM` tree proves the camera read, parsed and executed statement 0 of a script this project wrote, whatever the RAM cells say. A `.MSG` file with `load-failed` cannot happen; the pairing of the two is the point of the run. `file-op-failed` is the one outcome where the script ran and its log write still came back failed, so the file is absent or short while the statement cells show a completed run.

## 7. What the run would prove, and what it would not

It would prove, on this body, that a host config write of `0x02` to adjustment index `0xa2` is visible in the same session's config readback; that the camera's own task acted on it in the same boot, witnessed by the guard and the loader's words; and, if the card carried the file, that the camera read, parsed and ran a script this project wrote, witnessed by the `.msg` file and by the statement records. Labels: the write's path and the mirror address are byte-read; the load behaviour and the cells' meanings are static-inference over those bytes; the arm byte's stored value and the host latency are hardware-observed from this body's own captures.

It would not prove that the arming survives a power cycle (it does not by design); the first tick bounds camera uptime at first gated contact, but the task-start tick and gate-test instant remain unmeasured, so it does not give a precise task-relative margin. It would not bind the `.msg` name the allocator chooses, nor the byte identity of its contents beyond its size; it makes no persistent change and cannot leave the camera armed after act 4.5.

A missing `.msg` file is not a silent nothing when the guard reads 1: the guard witnesses that the sender posted both messages, and the async block separates the ways the file can come to nothing. A nonzero `status` with a zero `input length` and the statement index short of `-1` is a file that did not open or whose size or read failed, so the card, the `DCAA` directory or the byte-verified file is the thing to check. A nonzero `status` with the statement index at `-1` is the load and the script having run and the script's own file operation failing, with this script its log write. A status of 0 with a nonzero `input length` but no statement records is a read that happened and a parse that produced nothing, so the file's bytes are the thing to check. With the guard still 0, neither reading is available and the session says only that the camera's test instant did not see the arm byte.

A second run of the command in the same boot refuses at its precondition: the byte reads `0x02`, not the boot value `0x00`, so the arm would not be this session's write. That refusal is the designed stop, not a fault.

## 8. State-changing bytes for the independent review

The whole new byte set is three config writes and the session framing; every other request is a read.

| Purpose | Bytes | Effect |
| --- | --- | --- |
| gate enable | config `0x1a0002` function 3, mb 1, page 0, in-page offset `0xf7`, length 1, data `0x01`, D0 length `0x25` | RAM `0x41acd8f7 = 1`; the reviewed byte of `probe-ram` |
| arm | config `0x1a0002` function 3, mb 1, page 0, in-page offset `0xa2`, length 1, data `0x02`, D0 length `0x25` | RAM `0x41acd8a2 = 2`; no storage write, no queue post |
| gate restore | as the gate enable with data `0x00` | RAM `0x41acd8f7 = 0`; sent on every exit path after the gate was opened |
| confirm | config `0x1a0002` function 2, mb 1, page 0, length 256, D1 `0x124` | read of page 0; the byte at `0xa2` must read `0x02` |
| witness reads | `0x200001` function 2 at `0x1667000`, `0x1667080`, `0x1677700`, `0x1677780`, 128 bytes each, D1 `0xa4` | read of the two windows |
| tick reads | `0x200001` function 2 at `0x400c0800`, 128 bytes, D1 `0xa4`, before baseline witnesses and after final witnesses | read-only boot counter, u32 at reply data `+0` |
| session | C0 open, its 8-byte reply, C4 close and its 2-byte reply | session RAM only |

Never sent, in any value: a config write whose start index is `0x80` (value `0x22` commits the whole `0x420000` area to storage, `0x21` reloads it), any write at index `0x265`, any index in `0x80`..`0x8a` outside the reviewed `read-media` set, and any write of `0x00` to index `0xa2` (the disarm is the power cycle). The operation's index type holds only `0xa2` and `0xf7`, and its builder refuses every other start index with `0x80` named.

The camera's own writes during an armed boot, listed so the run is reviewed as a whole: the `0x10764`-byte clear at `0x166704c`; the async words `0x16777b0`..`0x16777bc`; the input length word `0x16777ac`; the guard `0x1667048`; the selector `0x16777c0`; the performance-level entry and restore through `0x0064982c(7,8,0)` and `(7,0,0)`; the file read into `0x43e6ff00` under the 1 MiB cap; the parse output over `0x166704c`; and, if the script is loaded, whatever its own statements do.

## 9. Offline rehearsal

`devenv shell -- bazel run //transport:camera -- rehearse arm-auto-act` prints the transfer plan with synthetic replies and runs the operation without constructing the native USB provider. Its first tick is 499 (4.996 s), its end tick 9487 (94.984 s), and the guard settles at the second poll. The result is `ran-to-end;guard=1;length=13;selector=1;status=0;open=0;completion-count=4095;index=-1;count=3;settled=yes;polls=2;tick-first=499;uptime-first-s=4.996;tick-end=9487;uptime-end-s=94.984`. These ticks are fixture values, not measurements of this revision on the camera.

Evidence labels: loader path, clear size, cell addresses, task waits and write callback are **byte-read** from pinned 1.50 images; load behaviour, the task-relative ~5 s send instant and card-side file size are **static-inference**; the two reviewed sessions' zero witnesses and host latency are **hardware-observed**, and the absent `.MSG` is **owner-reported**. Early enumeration arming, its first/end ticks, armed-boot card reading and the `.msg` name remain **unproved** on this body; the delay unit correction and the window's unmeasured position are E-AUTOACT-UNIT-150 and the arming report's section 13.3. Sources: [the arming report](auto-act-arming.md), [the script map](auto-act-script.md), E-AUTOACT-NOSEND-150, E-AUTOACT-160839-150, E-AUTOACT-UNIT-150, E-AF-TRACE-MECH-150, E-AUTOACT-150, E-AUTOACT-ARM-150 and E-AUTOACT-SCRIPT-150.
