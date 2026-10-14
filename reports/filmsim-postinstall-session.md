# Post-install byte readback session at the bench: do the installed bytes read back as the reviewed container's? (1.50)

Status: prepared 2026-10-09 and run at the bench the same day from 23:35Z; the task board's FILMSIM-READBACK-01 row carries the run and its result. Nothing here authorizes a run; the owner's go under [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs) covers it. Every operation in this script is an existing reviewed operation (`dump-ram`, `read-config`, `read-media`); no new byte is sent and no persistent byte is written, only the RAM gate and staging bytes named in section 12. The session ends by cutting the dummy-battery supply. It owes the readback that [the install session script](filmsim-install-session-script.md) section 8 and E-FILMSIM-INSTALL-150 leave open.

## 1. The question, and what each answer means

**Do the bytes the accepted RC0 container carries read back from the running body at every address the install changed, and what did the install leave in the adjustment and ordinary-settings areas?**

- Positive: the 192-byte decode prefix at `[0x0061c340, 0x0061c400)` reads byte-identical to `id4-patched.bin` `[0, 192)`, the six pointer words and fourteen matrix cells read at the schema-3 manifest's values, and both segments read byte-identical to `id4-patched.bin` over their decode extents. The patch's own bytes then live in the body's memory, which is the direct proof that the install placed what the container carried and closes E-FILMSIM-INSTALL-150's "the installed bytes are inferred" limitation as read back.
- Zero or stock array bytes at the prefix is the placement residual's predicted functional failure ([patch report](filmsim-patch-01.md) section 5.3). It contradicts the seven-entry menu the owner already photographed, so it is a stop for host analysis, not a reflash.
- A mismatch outside the manifest's 114 changed bytes is a writer or an image difference the container does not carry; name it before concluding anything.
- The adjustment half answers two independent questions: whether the updater recorded this install at its update stamp, and whether the restore lists preserved this body's bytes. Either outcome of either question is a result.

The before state is on record and verified clean, so this session takes no before capture: the 2026-10-08 `dump-ram` segments `20261008T045459Z-dump-ram-id4-image-0` and `20261008T045506Z-dump-ram-id4-image-1` (E-DUMP-RAM-150), the adjustment mirror `20261009T163324Z-read-config` (E-PREINSTALL-CONFIG-150, byte-identical to the 2026-10-08 mirror) and the settings banks `20261009T163721Z-read-media` (E-PREINSTALL-MEDIA-150).

## 2. Read this first

- **One maintenance boot.** The entry, the four reads and the ending all happen in it. Nothing here is retried in the same boot: a refusal, an unexpected reply, a disconnect or uncertain cleanup ends the session with the section 6 supply cut and a report.
- **Cost.** One boot and four host operations: two 1,048,576-byte RAM segments (4,229 requests each), the 1,309-window adjustment read (5,240 transfers) and the two-bank raw media read (529 capture slices per bank, 1,058 in all, plus its polls). The segment transfers are timed on this body: the 2026-10-08 before sessions ran the same two segments 7 s apart (E-DUMP-RAM-150), and the pre-install `read-config`/`read-media` pair ran here on 2026-10-09.
- **What the host writes.** No persistent byte. The RAM-only bytes are adjustment-cache index `0xf7` in each `dump-ram`, and indices `0x80`-`0x8a` plus `0xf7` in `read-media`; `read-media` restores `0x81`-`0x8a` and `0xf7` to their session-start values on every exit path and leaves `0x80` at the `0x33` disarm. Section 12 names them all.
- **No photographs, no shot, no playback, no live view in the maintenance boot.** `read-media` stages through the frame-buffer pool, so nothing may be in flight (byte-read transport contract). The section 6 restoration boot takes the one governance shot and plays it back after the reads.
- **The screen stays blank and the green LED lit** in maintenance mode. That is the documented jig state.
- **A normal photography card stays in and the lens stays mounted.** No firmware card is near the body, AUTO_ACT is not armed and End Setting is not sent.

## 3. Before anything

1. Power the camera from the dummy-battery supply and leave it there. The ending is a supply cut, so the supply must be the thing that is cut.
2. Have the note sheet of section 7 in front of you with a clock you can read to the second.
3. Know the entry: hold the shutter fully down and Up together, switch on with USB already attached, release both when the screen is blank and the green LED is lit (hardware-observed entry, E-JIG-ENTRY-KEY-BIND-150).
4. Each operation prints its own session folder path; every note row takes the printed path.

## 4. Act 0: the entry

Enter maintenance as section 3 step 3 describes. Write the entry wall time into note row 0. A wrong screen, an ordinary boot or an upgrade prompt stops the session before any command.

## 5. Acts 1 to 4: the reads

Run one command at a time and write each printed folder path into its note row before starting the next.

| Act | Owner action | Expected observation and record |
| --- | --- | --- |
| 1 | Run `dump-ram id4-image-0` below once. | Result line `dump-ram id4-image-0: 4229 requests`; `id4-image-0.bin` of 1,048,576 bytes and the two gate readbacks. The decode prefix lies at file offset `0x340` (832). |
| 2 | After act 1 succeeds, run `dump-ram id4-image-1`. | `4229 requests`; `id4-image-1.bin` of 1,048,576 bytes and the two gate readbacks. The six pointer words and fourteen matrix cells lie inside it. |
| 3 | After act 2 succeeds, run the `read-config` command with the existing full plan. | 1,309 windows of 256 bytes under `windows/`, 335,104 payload bytes, plan `read-adjustment-42000-v1`; the oracle `X-M1` and signature `55 aa` sit in the named windows. |
| 4 | After act 3 succeeds, run `read-media` last. | `bank-0.bin` and `bank-1.bin` of 131,072 bytes each, `cache-snapshot.bin` and the two gate readbacks; the result line names the captured byte count, 262,144 in all. |

```sh
devenv shell -- bazel run //transport:camera -- dump-ram id4-image-0
devenv shell -- bazel run //transport:camera -- dump-ram id4-image-1
devenv shell -- bazel run //transport:camera -- read-config "$PWD/private-data/read-plans/adjustment-42000.json"
devenv shell -- bazel run //transport:camera -- read-media
```

`read-media` runs last because it leaves the storage driver shut down (byte-read transport contract); nothing may follow it in this boot. These are the existing reviewed fixed segments and the existing plan, not a new reader.

## 6. Ending

**End the session by cutting the dummy-battery supply**, then unplug USB and set the POWER switch to OFF while unpowered. Do not end it with the POWER switch: a switch-ended jig cycle resets date, time, language and the C slot (E-JIG-CYCLE-D-150, hardware-observed), and the cut ending preserved settings in every prior observation (E-JIG-POWERCUT-PRESERVES-150). Then boot normally once to restore ordinary operation after `read-media`'s storage-driver shutdown and confirm the watched settings and the seven-entry film-simulation list again, then take one shot and play it back ([spec 3.1 step 4](../spec/07-governance-and-handoff.md#31-procedure)). That boot is the restoration check and its result goes in note row E; the byte evidence is complete when act 4's folder is on disk.

## 7. The note sheet

| Row | Step | What you did | Wall time | Camera showed | Anything unexpected |
| --- | --- | --- | --- | --- | --- |
| 0 | entry | maintenance entry, shutter + Up held at power-on with USB attached | | blank screen, green LED | |
| A0 | read | `dump-ram id4-image-0` folder | | | |
| A1 | read | `dump-ram id4-image-1` folder | | | |
| A2 | read | `read-config` folder | | | |
| A3 | read | `read-media` folder | | | |
| E | ending | supply cut, USB unplugged, POWER off while unpowered; restoration boot: settings and list checked, one shot and playback | | | |

A refused or stopped operation is a result: write what it printed into the row and follow section 6.

## 8. The window question and the readback comparisons

**Which window reaches the prefix.** The existing `dump-ram id4-image-0` segment does: its fixed range `[0x0061c000, 0x0071c000)` contains the whole decode prefix `[0x0061c340, 0x0061c400)`, at file offset `0x340` = 832 of `id4-image-0.bin` (byte-read transport segment table). `probe-ram`'s fixed window `0x0061c300`, one of its four 256-byte segment-table windows, also covers the whole prefix at in-window offset `0x40`, but no `probe-ram` window reaches the six pointer words or the fourteen matrix cells; `dump-ram id4-image-1` covers those at file offset address minus `0x0071c000`. No new window and no new byte is needed.

**8.1 The prefix.** Compare the captured 192 bytes against the accepted extended decode's first 192 bytes: the relocated values array `[0x0061c340, 0x0061c380)` and labels array `[0x0061c380, 0x0061c400)`.

```sh
D0="<A0 folder>/id4-image-0.bin"
D1="<A1 folder>/id4-image-1.bin"
PATCH="private-data/analysis/x-m1-1.50/update/prefix/id4-patched.bin"
MANIFEST="private-data/analysis/x-m1-1.50/filmsim/patch-01/patch-set.json"
dd if="$D0" bs=1 skip=832 count=192 status=none | cmp -n 192 - "$PATCH"
```

**8.2 The twenty body words.** The manifest's `new` field is the intended word value; `od -tx4` prints the captured little-endian word in the same form.

```sh
jq -r '.words[] | [.order, .address, .current, .new] | @tsv' "$MANIFEST" |
while IFS=$'	' read -r ord addr cur new; do
  got=$(od -An -tx4 -j $((addr - 0x0071c000)) -N 4 "$D1" | tr -d ' \n')
  printf 'word %s %s before %s new %s readback %s\n' "$ord" "$addr" "$cur" "$new" "$got"
done
```

The six pointer words are `0x0077d1a4`/`0x0077d1a8`, `0x007823c4`/`0x007823c8` and `0x00785d94`/`0x00785d98`, three nodes' values and labels pointers moving from `0x0078319c`/`0x0078d4f8` to `0x0061c340`/`0x0061c380`; the fourteen matrix cells are the manifest words from `0x00795db4` to `0x00795e60`. The 48 array words are the prefix bytes of 8.1. The intended bytes come from `patch-set.json` (schema 3, status `accepted-static-prefix`, 68 words) and `id4-patched.bin`, 2,631,872 bytes, SHA-256 `a4d169bd961ec7df34e3b9b7108b624b79680c3cc50297e434e0b3607fb65551`.

**8.3 Whole-extent binding.** One comparison per segment binds the running image to the container's decode over everything the two segments share with it (byte-read: before the install image 4 read equal to its decode over its whole extent, E-DUMP-RAM-150).

```sh
cmp -i 832:0 -n 1047744 "$D0" "$PATCH"
cmp -i 0:1047744 -n 1048576 "$D1" "$PATCH"
```

No output and exit 0 is the expected result over `[0x0061c340, 0x0081c000)`.

**8.4 Against the 2026-10-08 dumps.** The install's RAM delta is the manifest's 114 changed bytes (82 in the prefix, 32 in the twenty body words; byte-read applier census) plus three changed bytes across the segment-4 table words at RAM `0x0061c0d4`/`0x0061c0d8`, which the container changes at media `0x1400d4`/`0x1400d8` from load/size `0x0061c400`/`0x00282828` to `0x0061c340`/`0x002828e8` (byte-read intended fields, [patch report](filmsim-patch-01.md) section 5.1) and the boot's table-block copy shows at the same file offsets (static-inference from that report's section 5.3). Every other byte over `[0x0061c000, 0x0081c000)` should read as in `20261008T045459Z-dump-ram-id4-image-0` and `20261008T045506Z-dump-ram-id4-image-1`; the table head is the flash copy plus five runtime bytes (E-RAM-PROBE-150) and a mover there is reported, not predicted.

## 9. The adjustment comparison plan

Act 3's `windows/` compares byte by byte against `private-data/sessions/20261009T163324Z-read-config/windows/`, all 1,309 windows over `[0, 0x42000)` and `[0x400000, 0x40fd00)`. The named fields first, with before values from E-PREINSTALL-ADJ-VALUES-150:

| Field | Window and byte | Before | Expected after | Reading |
| --- | --- | --- | --- | --- |
| Product oracle | `000100.bin` `+0x5c` | `58 2d 4d 31` | unchanged | the body is using its own calibration |
| Update stamp | `000100.bin` `+0xe8`/`+0xe9` | `01 54` | `01 78` (static-inference) | moved: the updater recorded this install at its stamp. Unchanged: the updater does not re-stamp this install and that expectation is refuted. Any other value stops for host analysis |
| Signature | `000200.bin` `+0x02` | `55 aa` | unchanged | as the oracle |
| Boot-build stamp | `000200.bin` `+0x04`/`+0x05` | `01 78` | unchanged | rewritten every boot from the installed build; not proof of the new image |
| Pre-erase controls | `000200.bin` `+0x75`/`+0x76` | `01 0a` | unchanged | comparison expectation only |
| Arm and gate bytes | `000000.bin` `+0xa2`/`+0xf7` | `00 00` | unchanged | AUTO_ACT stays disarmed |

Then the whole mirror, as a mover census against the before capture. The expected mover set is `+0x1e8`/`+0x1e9` alone (static-inference over the mapped flow, E-ADJ-MOVERS-150): the update's configuration completion `0xdb91e0` snapshots cache `[0, 0x42000)`, refreshes it from the template in raw blocks 7-9, stamps `+0x1e8`/`+0x1e9` and `+0x204`/`+0x205` with the executing build's constants, restores the container's 98 auxiliary-1 and 3 auxiliary-2 (offset, length) pairs covering 1,487 bytes from the snapshot, and commits. The RC0 container's two restore lists are byte-identical to stock and its template in blocks 7-9 is unchanged (byte-read), so a full run rewrites template bytes to their own values and returns the restored bytes to their snapshot values. The executing updater is the installed 1.50 build, whose constants are the `01 78` the boot-build stamp already shows.

The restore-list question reads on the 224 body-specific restore-covered bytes, the 2026-10-08 census's non-template set apart from nine stamp bytes. They must read as in the before capture: a restore-covered byte at its template value refutes the restore, and a template-valued byte holding a non-template value refutes the refresh. With the stamp moved and the 224 held, the mapped function's stamp step ran and the lists applied as expected (the stamp step sits inside the same function, with no mapped partial path; static-inference). With the stamp unmoved, the comparison still reports what the update touched, but whether the mapped flow ran stays unbound.

The 253 windows of `[0x400000, 0x40fd00)` sit outside the mapped snapshot, refresh and restore (E-ADJ-MOVERS-150 limitation); movers there are reported and attributed or left open, and a clean hold is the expected result because the whole mirror was byte-identical across the install's before captures. What the mirror cannot prove: the stamp is the executing updater's own record, not the new image's; the image binding is section 8's RAM readback.

## 10. The area-2 banks comparison plan

Act 4's `bank-0.bin`/`bank-1.bin` compare against `private-data/sessions/20261009T163721Z-read-media/`. The expected differences are records the owner's post-install ordinary acts appended (the film-simulation selections and settings of the rendered-output session, E-FILMSIM-RENDER-150) and any bank rotation the log accounting explains; the [settings instrument](settings-log-map-session-script.md#1-capacity-read-this-first) owns record selection, capacity and bank-erasure accounting. The install adds no record there on the mapped evidence: S4's configuration work writes media `[0x520000, 0x940000)` (byte-read) while the banks live at raw blocks `0x3ac` and `0x3b2`, byte offsets `0x7580000` and `0x7640000` (hardware-observed map resolution, E-AREA2-BANKS-150), outside that span (static-inference). A record or byte difference with no recorded act is a finding. What it cannot prove: firmware storage or prefix identity, since the banks carry ordinary settings only.

## 11. What each act proves, and what it cannot

| Act | Proves | Cannot |
| --- | --- | --- |
| 0 entry | The reads happen in one maintenance boot on the body the note sheet names | Nothing about bytes; a wrong screen stops before any command |
| 1 `dump-ram id4-image-0` | The 192-byte prefix, the relocated arrays' own bytes and the segment-4 table words are live in the running body at the container's values | The write's time or writer; a writer producing the same values; any later boot |
| 2 `dump-ram id4-image-1` | The three nodes point at `0x0061c340`/`0x0061c380` and the fourteen availability cells hold the manifest values, so with act 1 the container's 68-word write set is read back | The same limits; and the menu itself, which the photographs already bind |
| 3 `read-config` | The adjustment mirror's bytes after the install; whether the updater recorded the install at `+0x1e8`/`+0x1e9`; whether the restore lists held the body's 224 bytes | That the new image landed, since the stamp is the executing updater's record; firmware storage; the prefix |
| 4 `read-media` | The two area-2 banks' bytes against the before ledger and what the owner's post-install acts recorded | Firmware storage; the prefix; anything outside the two banks |
| E ending | Settings-preserving close of the boot `read-media` ended (hardware-observed prior cuts); the restoration boot shows capture and playback working again after the storage-driver shutdown | Nothing itself; it is the control that keeps the verified state |

## 12. State-changing bytes, and what the review needs

**With respect to persistent state this session is read-only.** No operation sends a configuration commit (`0x80 = 0x22`), an End Setting, an update request, an AUTO_ACT arm or any internal-media program or erase request. The bytes it changes are adjustment-cache RAM at `0x41acd800` and session/driver state:

| Operation | Byte | Values written | Persistence |
| --- | --- | --- | --- |
| `dump-ram` acts 1 and 2 | cache index `0xf7`, RAM `0x41acd8f7` | `01`, readback, work, `00`, readback | none; a function-3 write at an index other than `0x80` posts no service command (byte-read transport contract, E-ENABLE-COMMITFREE-150) |
| `read-config` act 3 | none | none | H1 reads only; C0/C4 session and reply RAM |
| `read-media` act 4 | cache indices `0x80`, `0x81`, `0x82`, `0x83`, the restore-only `0x84`-`0x8a`, and `0xf7` | `0x35` trigger and `0x33` disarm at `0x80`, left at the `0x33` disarm; zero at `0x81`, the per-bank selector at `0x82`, the `0xff` sentinel at `0x83`; `0x81`-`0x8a` and `0xf7` restored to their session-start values on every exit path | none; `0x35`/`0x33` are the raw-read action's trigger and disarm, not commit values; the operation shuts down the storage driver |
| Section 6 ending | dummy-battery supply cut | volatile gates and session state | no POWER/OFF defaults record (hardware-observed prior cuts) |

A gate-using operation that stops before its restore leaves `0xf7` at `1` until the next power cycle (byte-read transport contract); the supply-cut ending supplies that power cycle. Because every byte above is an existing reviewed byte of an existing reviewed operation, the session sends no new byte and needs no new independent byte review. What it needs is the owner's go under [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs), naming these four operations: H1 for `read-config`, H2 for `dump-ram` and `read-media` with the RAM-only writes above.

## 13. What this session cannot see

- The write itself. No write trace exists; the readback is the state at read time in this boot.
- A value-identical writer. Byte equality binds the values to the container's payload and cannot exclude a writer that produced the same values by another route; that is E-FILMSIM-INSTALL-150's residual in its precise form.
- Other boots and ordinary-mode transitions. The seven-entry menu photographs bind the arrays at draw time; a later writer would show in a later readback or menu session.
- Recovery. Nothing here is a repair, a restore or a demonstrated recovery.
- Media beyond the two area-2 banks. Firmware storage, the C-slot's area-3 banks and the trailer are unread by this session.
