# arm-auto-act session: witness cells, outcomes and limits (1.50)

Status: on hold. The reviewed `arm-auto-act --wait-for-device` operation ran twice on 2026-10-09 (`20261009T143802Z-arm-auto-act`, `20261009T160839Z-arm-auto-act`); both wrote the arm byte and observed no sender, the second with its readback confirming `0xa2 = 2`. The delay unit is about 1 ms, so the one-shot gate test falls about 1 s after task start, and the observed C0-to-arm path alone takes longer than that (E-AUTOACT-160839-150, E-AUTOACT-UNIT-150). Do not repeat the sequence: the next step is a read-only boot-tick measurement of the window's position, specified in [the arming report](auto-act-arming.md#134-next-step-and-alternatives). The reviewed arming bytes are unchanged. Nothing here authorizes a run; the owner's go under [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs) covers one, and [the arming report](auto-act-arming.md) owns the design, the operation sequence and the state-changing byte set.

The camera contains the AUTO_ACT script facility in full and is disabled as shipped because the only enable gate is adjustment byte `0xa2 == 2` at the `mujin` task prologue, which reads `0x00` in all three captures of this body ([the script map](auto-act-script.md)). A host config write of `0x02` to index `0xa2` lands in the RAM mirror only, so the boot load refills it from storage and the next power cycle is the disarm. With the byte set, the task tests it about 1 s after task start and, on equality, loads `<active volume>:\DCAA\auto_act.scr` about 4 s later on its own queue, with no host involvement.

## 1. Card preparation

The loader builds the path from the template `A:\DCAA\auto_act.scr` at `0x00ed3ac4`, replacing the volume letter with the active one (byte-read); the volume on this body is the SD card (E-SRECORD-ROUTE-150). The loader does not create the `DCAA` directory, so it must exist, and the file must be 13 bytes with exactly these bytes:

```text
WT_LOG\r\nEND\r\n
```

SHA-256 `bdd37b75ad3e2664ea8fc42fc9611d4fd761abde4fe487b0d357348beddd11e8` (static-inference from the byte-read grammar). `WT_LOG` writes a log file the camera names itself into the active volume's `DCIM` tree, about 4 KiB; `END` stops the statement chain. The script presses no key and calls no camera routine. [The script map](auto-act-script.md#12-the-first-script-and-the-ranked-follow-ups-2026-10-09) owns why this is the smallest file that separates a parsed script from an unparsed one.

```sh
mkdir -p "<card>/DCAA"
printf 'WT_LOG\r\nEND\r\n' > "<card>/DCAA/auto_act.scr"
wc -c "<card>/DCAA/auto_act.scr"
sha256sum "<card>/DCAA/auto_act.scr"
od -c "<card>/DCAA/auto_act.scr"
```

All three checks must pass before the card goes in the camera. A card without the file or without the directory is also a valid session: see the outcome table.

## 2. The witness cells

The command reads the cells of the two 256-byte windows `0x1667000` and `0x1677700` through the reviewed `0x200001` function 2 requests while the `0xf7` gate is open. The addresses are byte-read from the pinned 1.50 images; the values come from the run.

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

## 3. Outcomes

| Outcome | guard | length | selector | status | index | means |
| --- | --- | --- | --- | --- | --- | --- |
| `ran-to-end` | 1 | 13 (nonzero) | 1 | 0 | `-1` | the file was read and parsed, the statement table ran, and `END` stopped the chain: the strongest card-side result this script can give |
| `file-op-failed` | 1 | 13 (nonzero) | 1 | nonzero | `-1` | the load and the statement table ran to `END`, and a file operation after the load failed: with this script its own `WT_LOG` write, so the `.msg` file is absent or short |
| `load-running` | 1 | 13 or 0 | 2 | 0 | not `-1` | the loader ran and the statement table is executing, still or stalled; a script with no `END`, a long `wait`, or a record whose code has no dispatch entry |
| `load-failed` | 1 | 0 or 13 | 1 | nonzero | not `-1` | the sender posted and the loader cleared the script area, but a file operation failed before `END`: the file did not open, or its read or close failed, so the card, the `DCAA` directory or the name is the thing to check |
| `no-send` | 0 | 0 | 0 | 0 | 0 | the gate test did not see 2 at its one instant this boot; compare the first tick and timing bound before deciding whether another trigger is required; no same-boot retry |
| `no-load` | 1 | 0 | 0 or 1 | 0 | 0 | the sender posted and the loader produced no state within the bound; the cells are the record of what the camera had done at the bound's end |

A settled run additionally shows `in-flight` 0. `settled=no` means the watch reached its 90-poll bound before the predicate held; the cells are then the camera's state at the bound, and a `selector` of 2 with records present is a script still running. The status is read as the script's own only when the statement index is at `-1`, and a failed load never reaches `END`: the loader clears the index and only `END` writes `-1`.

The card-side artifact is the independent witness: a new ~4 KiB `.MSG` file in the active volume's `DCIM` tree proves the camera read, parsed and executed statement 0 of a script this project wrote, whatever the RAM cells say. A `.MSG` file with `load-failed` cannot happen; the pairing of the two is the point of the run. `file-op-failed` is the one outcome where the script ran and its log write still came back failed, so the file is absent or short while the statement cells show a completed run.

## 4. First-contact timing

The first tick reads the little-endian u32 at `0x400c0800` immediately after the gate enable readback and before the baseline witnesses and arm; the end tick reads the same cell after the final witnesses and before gate restore. Both use the existing 128-byte `0x200001` function 2 shape, and the conversion is `seconds = ticks / 99.88`, the measured rate of E-AF-TRACE-MECH-150.

The first uptime includes the camera's boot before image 5 creates `mujin`, plus the host's open, session-start config read and gate sequence. It is an upper bound on elapsed time since task start at the tick sampling instant, not the time remaining before the test and not a measurement at enumeration or at the first USB byte. Task-start tick and the test's actual tick remain unmeasured. With the corrected unit the test falls about 1 s after task start, but task creation follows power-on and the task-start tick is unmeasured, so the uptime by itself neither proves nor excludes that the test has passed. The read set in [the arming report section 13.4](auto-act-arming.md#134-next-step-and-alternatives) measures the window's position directly.

Both reviewed runs measured about 1.18 s from prepared to the arm frame's outgoing record, including all existing preconditions, which is longer than the whole corrected task delay, so the earlier 8.82 s margin claim is withdrawn (E-AUTOACT-UNIT-150). If the first-contact bound combined with a task-start or gate-test bound shows the test had already passed before the host could arm, this route cannot work by promptness; stop it and choose a different trigger rather than seeking a faster command. A `no-send` without that task-relative bound proves no sender was observed, not a numeric lateness.

## 5. What a run proves, and what it does not

It would prove, on this body, that a host config write of `0x02` to adjustment index `0xa2` is visible in the same session's config readback; that the camera's own task acted on it in the same boot, witnessed by the guard and the loader's words; and, if the card carried the file, that the camera read, parsed and ran a script this project wrote, witnessed by the `.msg` file and by the statement records. Labels: the write's path and the mirror address are byte-read; the load behaviour and the cells' meanings are static-inference over those bytes; the arm byte's stored value and the host latency are hardware-observed from this body's own captures.

It would not prove that the arming survives a power cycle (it does not by design), nor give a precise task-relative margin, nor bind the `.msg` name the allocator chooses or the byte identity of its contents beyond its size. It makes no persistent change and cannot leave the camera armed after the session ends.

## 6. Stops and ending

Every stop ends the same way: cut the dummy-battery supply. The session writes no storage, so the boot load is the cleanup, and the next boot returns `0xf7` and `0xa2` to their stored `0x00`. Do not touch the POWER switch, which would write a defaults record and reset date, time, language and the C slot (E-JIG-CYCLE-D-150); unplug USB only after power is gone.

Never after a stop: a retry, a second camera command in the same boot, or a powered cable pull with a request outstanding. A second run of the command in the same boot refuses at its precondition, because the byte reads `0x02` rather than the boot value `0x00`; that refusal is the designed stop, not a fault.

## 7. Offline rehearsal

`devenv shell -- bazel run //transport:camera -- rehearse arm-auto-act` prints the transfer plan with synthetic replies and runs the operation without constructing the native USB provider. Its first tick is 499 (4.996 s), its end tick 9487 (94.984 s), and the guard settles at the second poll: `ran-to-end;guard=1;length=13;selector=1;status=0;open=0;completion-count=4095;index=-1;count=3;settled=yes;polls=2;tick-first=499;uptime-first-s=4.996;tick-end=9487;uptime-end-s=94.984`. These ticks are fixture values, not measurements of this revision on the camera.

Evidence labels: loader path, clear size, cell addresses, task waits and write callback are **byte-read** from pinned 1.50 images; load behaviour, the task-relative send instant and card-side file size are **static-inference**; the two reviewed sessions' zero witnesses and host latency are **hardware-observed**, and the absent `.MSG` is **owner-reported**. Early enumeration arming, its first and end ticks, armed-boot card reading and the `.msg` name remain **unproved** on this body (E-AUTOACT-UNIT-150, [the arming report section 13.3](auto-act-arming.md#133-the-reachable-host-timing)).
