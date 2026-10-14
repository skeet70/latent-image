# First AF observation session at the bench: does a maintenance boot carry the autofocus layer? (1.50)

Status: prepared, not run. Nothing here authorizes a run; the owner's go under [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs) covers it. Every operation in this script is an existing reviewed read operation (`dump-ram`, `read-log`); no new byte is sent and nothing is written except the adjustment index `0xf7` gate that every read operation already writes and restores. The session ends by cutting the dummy-battery supply. The instrument reasoning is in [the instrument survey](af-observe-instruments-150.md).

## 1. The question, and what each answer means

**Do the AF task's own state cells and message trace read back in one maintenance boot, and does the physical shutter press move either?**

- Positive: the AF task's state vector, orchestration state and search fields hold defined, repeatable values; the request context is bound; the store's trace shows the AF and lens send set for the boot. The jig session is then a working carrier for AF state, and the next step is a reviewed operation that drives one recorder job inside a readable boot, with these values as the predeclared idle control.
- Negative: the cells read zero, the loader's fill, or the `-0x8000` query-invalid sentinel in the lens position. The jig session cannot carry AF state, and the AF lane keeps to the external kit and G4/G5.
- A press-caused change in any AF cell the after-reads re-read, or a `0x3f2e`, `0x3b0d` or `0x13a5` record after an act, is a finding on its own: it would show the press reaching the AF layer in a mode whose key dispatch says it does not.

The session is one boot. The log store at `0x400a0800` is cleared by loader A in every boot and cannot be read again after this boot ends, so the reads happen before any power cycle.

## 2. Read this first

- **One boot, no interruption.** The jig entry is the boot; the reads and the acts all happen inside it. Do not power off, do not touch the POWER switch, do not press PLAY and do not unplug or replug USB between the reads. Any of those ends the boot and loses the trace this session exists to capture. A refusal, unexpected reply, disconnect or uncertain cleanup stops the session with the section 7 supply cut. The shutter firing, a lit screen, a sound or card-light activity at any act is unexpected camera behaviour ([spec 3.3](../spec/07-governance-and-handoff.md#33-standing-rules)) and stops the session the same way, before the after-reads.
- **Cost.** Seven host operations: four RAM segments before the acts at roughly 6.5 s, 6.5 s, 6.5 s and 5 s each, two segments after the acts at the same cost, and the whole-store read at roughly 151 s. About four minutes of transfers plus note-taking.
- **What the host writes.** Each RAM read enables adjustment index `0xf7` with a config write of 1, reads it back, does its work, writes 0 back and reads that back. That is the sequence every `probe-ram`, `dump-ram` and `read-log` session has run before, it is not persistent, and it is unchanged here.
- **No photographs in the maintenance boot.** Do not take a shot even if the camera seems to respond; a capture would add records the trace would then have to explain. The section 7 restoration boot takes the one governance shot after the trace is read.
- **The screen stays blank and the green LED lit.** That is the documented jig state; it is not a failure.
- **The lens stays mounted and the SD card stays in.** A lens swap is a deliberate later step, not part of this session.

## 3. Before anything

1. Power the camera from the dummy-battery supply and leave it there. The ending is a supply cut, so the supply must be the thing you cut.
2. SD card in the camera, lens mounted; neither moves during the session.
3. Have the note sheet of section 6 in front of you, with a clock you can read to the second.
4. Know the entry before you start: hold the shutter fully down and the Up key together, switch on with USB already attached, and release both when the screen is blank and the green LED is lit.

## 4. Act 0: the entry, then the before-reads

1. Enter jig mode as section 3 step 4 describes (shutter + Up held at power-on with USB attached). Write the entry wall time into note row 0.
2. On the host, run the four segment reads in this order, each in its own session, writing the printed folder path into its note row:

```sh
devenv shell -- bazel run //transport:camera -- dump-ram id4-bss-0
devenv shell -- bazel run //transport:camera -- dump-ram id5-bss-0
devenv shell -- bazel run //transport:camera -- dump-ram id5-bss-1
devenv shell -- bazel run //transport:camera -- dump-ram id5-bss-2
```

3. Write the four folder paths into note rows A0 to A3 in that order.

## 5. The acts

Each act is a few seconds of physical work; nothing about the camera is changed by it. Write the wall time of every press and release into the note row. The shutter button is the only act that could plausibly reach AF, and the two OK taps bracket the acts as tick anchors in the store. The shutter firing, a lit screen, a sound or the card light at an act is unexpected in this mode: note it and end the session with the section 7 ending before the after-reads.

| Act | What you do | What to write down |
| --- | --- | --- |
| 1 | Press OK once and release after about half a second | press time, release time |
| 2 | Press the shutter button half way, hold about 2 seconds, release | half-press time, release time |
| 3 | Press the shutter button fully down once and release | full-press time, release time, whether anything on the body changed (LED, sound, card light) |
| 4 | Press OK once and release after about half a second | press time, release time |

After act 4, run the after-reads.

## 6. The after-reads

```sh
devenv shell -- bazel run //transport:camera -- dump-ram id5-bss-2
devenv shell -- bazel run //transport:camera -- dump-ram id4-bss-0
devenv shell -- bazel run //transport:camera -- read-log
```

Write the three folder paths into note rows B0 to B2. The first two repeat the before-reads, so the analysis gets a same-boot before-and-after of the AF state cells and the AF-frame cells with the acts between. The third carries the boot's whole message trace, the acts included. Do not retry a stopped read in this boot.

## 7. Ending

**End the session by cutting the dummy-battery supply**, then set the POWER switch to OFF while unpowered and unplug USB. Do not end the boot with the POWER switch while powered: an orderly power-off in jig mode writes a defaults record and resets date, time, language and the C slot (E-JIG-CYCLE-D-150, hardware-observed), and it ends the boot whose trace has just been read.

Then reconnect the supply and boot normally once without USB. That ordinary boot is the restoration check ([spec 3.1 step 4](../spec/07-governance-and-handoff.md#31-procedure)): confirm date, time, language and the C slot, take one shot and play it back, and write what you see into note row E. Report the check's result; anything unexpected is described, not repaired at the bench.

## 8. The note sheet

| Row | Step | What you did | Wall time | Camera showed | Anything unexpected |
| --- | --- | --- | --- | --- | --- |
| 0 | entry | jig entry, shutter + Up held at power-on with USB attached | | blank screen, green LED | |
| A0 | read | `dump-ram id4-bss-0` folder | | | |
| A1 | read | `dump-ram id5-bss-0` folder | | | |
| A2 | read | `dump-ram id5-bss-1` folder | | | |
| A3 | read | `dump-ram id5-bss-2` folder | | | |
| 1 | act | OK tap | press / release | | |
| 2 | act | shutter half press, 2 s hold, release | press / release | | |
| 3 | act | shutter full press | press / release | | |
| 4 | act | OK tap | press / release | | |
| B0 | read | `dump-ram id5-bss-2` folder | | | |
| B1 | read | `dump-ram id4-bss-0` folder | | | |
| B2 | read | `read-log` folder | | | |
| E | ending | supply cut, POWER off while unpowered, USB unplugged; restoration boot: date/time, language and C slot checked, one shot and playback | | | |

Record every folder path exactly as printed. If any operation prints a stop or a refusal, write that in the row and end the session with the section 7 supply cut. Retain the captures already written and report the stop for host analysis; do not run another command in this boot. A body response at an act is a stop too: write it into the act's row and end the session the same way.

## 9. What the analysis reads out of the captures

Addresses are the static map's AF cells; the segment that carries each is in brackets. The analysis compares the before-and-after dumps and reads the store.

| Cell | Address | Segment | What a defined value means |
| --- | --- | --- | --- |
| AF state vector and event vector | `0x0169d4b4`, `0x0169d468` | id5-bss-2 | The AF orchestration task brought its state machine up |
| Orchestration state | `0x0169d4e2` | id5-bss-2 | Which terminal or idle state the AF layer sits in |
| Search state and step | `0x016c0fbc`, `0x016c0fc0`, `0x016c0fe2`, `0x016c0fe4`, `0x016c0fe5` | id5-bss-2 | The search state field and its direction, zero when no search has run |
| Search flags and targets | `0x016c1f10`, `0x016c1f14`, `0x016c1f74` | id5-bss-2 | A bound or estimated target left over from a search |
| Accepted statistic | `0x016c1160`-`0x016c1178`, `0x016c11fc` | id5-bss-2 | Whether any measurement has been accepted in this boot |
| In-flight, movement, status | `0x016c2200`, `0x016c2204`, `0x016c2208` | id5-bss-2 | Evaluation in flight, a pending movement request, the last callback status |
| Retry counters | `0x016c21e4`, `0x016c21e6` | id5-bss-2 | How many retries the layer has spent |
| Request context handle | `0x01529fb8`, `0x01529fbc` | id5-bss-0 | The AF-area-to-request handle; `-1` and the default pointer `0x00eb2888` are the unbound readings |
| Request slot record | `0x015b73b8 + 0x30 + (slot-1)*0x17d0` | id5-bss-1 | The context the request API copies, 0x90 bytes |
| Lens position and protocol flags | `0x00942b84`, `0x00942b88`, `0x0095b394`, `0x0095b398` | id4-bss-0 | A live lens position against the `-0x8000` query-invalid sentinel |
| AF-frame queue, latch and published state | `0x00950544`, `0x00951424`, `0x00951428`, `0x0095142c`, `0x0094f64c`-`0x0094f65c` | id4-bss-0 | The presentation side, expected zero |
| Indicator bits and action cells | `0x0094f888`-`0x0094f894`, `0x0094f650`, `0x0094f654` | id4-bss-0 | The UI latch and its action words, expected zero |
| Selected AF area | `0x0091682c`, `0x0091682e` | id4-bss-0 | The selected-area index and halfword, `1` after initialisation |
| Text-ring install flag and callback slot | `0x0089edd4`, `0x0089edd8` | id4-bss-0 | Whether the console text ring was installed at the connect edge |
| The boot's trace | whole store | `read-log` | Every traced send with sender, receiver, message id and tick |

The analysis then decodes the store with the census tool:

```sh
devenv shell -- runghc private-data/analysis/x-m1-1.50/af-observe/TraceCensus.hs <read-log-folder> 00 25 2b 32 62 63 64 65
```

and looks up the triples that matter: `0x3f2e` (search start), `0x3b0d` (request), `0x420c` and the `0x4200`-`0x4214` family (lens commands), `0x3b2f`/`0x3b30`/`0x3b32` (lens responses), `0x3f0d` (completion relay), `0x13a5` (result), and the acts' own records: OK `0x0001`/`0x0081`, S1 `0x0007`/`0x0087`, S2 `0x0008`/`0x0088` (OK and the shutter releases come from keytimer `0x25`, the S1/S2 press forms from thread `0x00`, which is why the command selects both).

## 10. What each read teaches, and what a null means

- The four before-reads bind the static map's AF cells to live values in this boot, the next sample after jig boot A's 2026-10-08 dumps of the same four segments (`20261008T045420Z-dump-ram-id4-bss-0`, `20261008T045442Z-dump-ram-id5-bss-0`, `20261008T045448Z-dump-ram-id5-bss-1`, `20261008T045454Z-dump-ram-id5-bss-2`). The analysis states each cell's value and whether it matches the map's expected idle reading.
- The two after-reads give a same-boot before-and-after of the `id5-bss-2` and `id4-bss-0` cells with the acts between; the `id5-bss-0` request-context and `id5-bss-1` slot-record cells have before values only. A changed re-read cell with no act addressed to AF is the finding of the session; an unchanged set is the idle baseline the next session's drive operation is read against.
- The store read gives the boot's trace and the acts' ticks. The expected content is the AF and lens boot handshake from the instrument survey's section 3.3: `0x4200`-`0x4214`, `0x1394`-`0x139d`, `0x3f13`-`0x3f27`, `0x720d`-`0x722b`, `0x1a0d`/`0x1a0e`. Absence of `0x3f2e`, `0x3b0d` and `0x13a5` is the expected "no search ran" result and is what makes a later drive session's positive signal readable.
- A null result on the AF state cells (all zero, or the loader's fill) says the AF layer did not stand up despite its task existing, and it closes the jig-session route for AF until a different agency for running AF in a readable boot appears.
- The lens position cell is the one cell with prior body evidence: `0x36`, `0x20` and `0` in three jig captures. A repeat of a small nonzero value supports a live lens layer; the `-0x8000` sentinel means the layer is inert in mode `0xb`, and no jig-session instrument can move the lens.

## 11. What this instrument cannot see

- Any AF behaviour. Maintenance mode dispatches no shutter action, starts no preview and runs no search, so this session can only show the AF layer's state and its message traffic in a boot that is not shooting.
- An ordinary session's AF timeline. The store is cleared at every boot and can be read only in a maintenance boot, so an ordinary boot's trace cannot be read after the fact.
- The console text ring's contents. No built operation sends `0x200001` functions 1 or 4, and the ring was not installed in the captured jig boots (`0x0089edd4` reads 0), so the firmware's own AF debug text stays unreachable.
- The same-thing-again question. One boot's values are one sample; repeatability needs a second session, which this script's note sheet is designed to support without change.
- The trace filter and buffer cells (`0x40a5b5e1`, `0x40a5b5e2`, `0x40a5b5e4`, `0x40a5b5ec`, the bit-30 aliases of `0x00a5b5e1`-`0x00a5b5ec`). They lie in `id4-bss-1` (`[0x0099ec00, 0x00a9ec00)`, file offset `0xbc9e1`), which no operation here reads; the only reading of them is the cross-boot 2026-10-08 capture `20261008T045426Z-dump-ram-id4-bss-1`.
- RAM outside the four segments this session reads, the C-slot record, and anything on the media.
