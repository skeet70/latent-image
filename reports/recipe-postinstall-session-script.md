# Recipe post-install byte readback session at the bench: do the installed bytes read back as the reviewed container's? (X-M1 1.50)

**Not queued for execution.** The historical container and install procedure below are withdrawn from installation readiness (E-RECIPE-PREBOOT-COMPOSITION-LIMIT-150). Rebind every expected artifact, extent and changed-address comparison to a complete, independently reviewed replacement before preparing another camera session.

Status: prepared 2026-10-11 for the owner's run after [the recipe-candidate install session](recipe-install-session-script.md); nothing here authorizes a run. The owner's go under [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs) covers it, naming the reviewed operations and their classes. Every operation in this script is an existing reviewed read-only operation (`dump-ram`, `read-config`, `read-media`); no new byte is sent and no persistent byte is written, only the RAM gate and staging bytes named in section 10. The session ends by cutting the dummy-battery supply. It owes the readback that [the install session script](recipe-install-session-script.md) section 8 leaves open.

## 1. The question, and what each answer means

**Do the bytes the accepted container carries read back from the running body at every address the install changed, and what did the install leave in the adjustment and ordinary-settings areas?**

- Positive: the image-4 decode `[0x0061c1fc, 0x0081c000)` reads byte-identical to `id4-final.bin` over the read extent, the image-5 words and the 85 plain-window writes hold the plan's own bytes, the segment-table entry-4 words carry the new layout, and the adjustments restore the body's bytes through the update. The installed bytes then live in the body's memory, which is the direct proof that the install placed what the container carried, closing the install session's residual that the installed bytes are only inferred.
- A stock or zeroed value at the decode prefix, the film conversion rows or the bank words is the predicted wrong-image or placement failure: it contradicts the nine-entry menu the owner will have photographed, so it is a stop for host analysis, not a reflash.
- A mismatch outside the intended change set and the named runtime-cell classes (sections 8.5, 8.6) is a writer or an image difference the container does not carry; name it before concluding anything.
- The adjustment half answers two independent questions: whether the update refreshed and committed the mirror without losing the body's restore-covered bytes, and whether its stamp writers ran. The bank half answers what the owner's post-install acts recorded in the ordinary-settings ledger. Either outcome of either question is a result.

The reviewed deliverable is `private-data/analysis/x-m1-1.50/recipes/runtime/combined-candidate/install/FWUP0002.DAT` (38,405,148 bytes, SHA-256 `b1aabeb61f8ae19c83e4f76efcf0710687e8008daa998c625b7c0bf0f5ca0757`, `rebuild-update verify` 42/42; record `install/install-artifact.json`, chain `combined-candidate.json` and `chain.log`). The expected files are the candidate's own `id4-final.bin` (`84d18cbd...`), `id5-final.bin` (`c9a22267...`), the deliverable's `extract-plain` window and the plan's 127 writes, all under `recipes/runtime/`.

## 2. Read this first

- **One maintenance boot.** The entry, the seven reads and the ending all happen in it. Nothing here is retried in the same boot: a refusal, an unexpected reply, a disconnect or uncertain cleanup ends the session with the section 6 supply cut and a report.
- **Cost.** One boot and seven host operations: five 1,048,576-byte RAM segments at 4,229 requests each (21,145 requests; 2.5 s to 6.3 s per segment on this body, E-DUMP-RAM-150), the 1,309-window adjustment read (5,240 transfers, 335,104 payload bytes) and the two-bank raw media read (1,058 capture slices, 4,368 transfers). The 2026-10-09 four-act readback ran at the bench in about nine minutes wall time; this script adds three segments.
- **What the host writes.** No persistent byte. The RAM-only bytes are adjustment-cache index `0xf7` in each `dump-ram`, and indices `0x80`-`0x8a` plus `0xf7` in `read-media`; `read-media` restores `0x81`-`0x8a` and `0xf7` to their session-start values on every exit path and leaves `0x80` at the `0x33` disarm. Section 10 names them all.
- **No photographs, no shot, no playback, no live view in the maintenance boot.** `read-media` stages through the frame-buffer pool, so nothing may be in flight (byte-read transport contract). The section 6 restoration boot takes the one governance shot and plays it back after the reads.
- **The screen stays blank and the green LED lit** in maintenance mode. That is the documented jig state.
- **A normal photography card stays in and the lens stays mounted.** No firmware card is near the body, AUTO_ACT is not armed and End Setting is not sent.
- **Order matters once:** `read-media` runs last because it shuts the storage driver down; nothing follows it in this boot.

## 3. Before anything

1. Power the camera from the dummy-battery supply and leave it there. The ending is a supply cut, so the supply must be the thing that is cut.
2. Have the note sheet of section 7 in front of you with a clock you can read to the second.
3. Know the entry: hold the shutter fully down and Up together, switch on with USB already attached, release both when the screen is blank and the green LED is lit (hardware-observed entry, E-JIG-ENTRY-KEY-BIND-150).
4. Have the deliverable, the candidate files and the before captures from [the install session](recipe-install-session-script.md) section 3 in the host analysis directory, and the comparison commands of section 8 ready.
5. Each operation prints its own session folder path; every note row takes the printed path.

## 4. Act 0: the entry

Enter maintenance as section 3 step 3 describes. Write the entry wall time into note row 0. A wrong screen, an ordinary boot or an upgrade prompt stops the session before any command. The menu photographs and ordinary-operation checks of [the install session](recipe-install-session-script.md) section 6 are done before this boot; keep the normal photography card in it.

## 5. Acts 1 to 7: the reads

Run one command at a time and write each printed folder path into its note row before starting the next.

| Act | Owner action | Expected observation and record |
| --- | --- | --- |
| 1 | Run `dump-ram id4-image-0` below once. | Result line `dump-ram id4-image-0: 4229 requests`; `id4-image-0.bin` of 1,048,576 bytes and the two gate readbacks. Its first 508 bytes are the segment-table head (the entry-4 layout words at file offsets `0xd4`/`0xd8`); the image-4 decode starts at file offset 508. |
| 2 | After act 1 succeeds, run `dump-ram id4-image-1`. | `4229 requests`; `id4-image-1.bin` of 1,048,576 bytes. It carries the film conversion rows, the availability cells, the node pointer words and the bank's registry words. |
| 3 | After act 2 succeeds, run `dump-ram id5-image-0`. | `4229 requests`; `id5-image-0.bin` of 1,048,576 bytes. It carries six of the nine image-5 words. |
| 4 | After act 3 succeeds, run `dump-ram id5-image-1`. | `4229 requests`; `id5-image-1.bin` of 1,048,576 bytes. It carries the `case 0xfe` site instruction and the remaining three image-5 words. |
| 5 | After act 4 succeeds, run `dump-ram plain-0`. | `4229 requests`; `plain-0.bin` of 1,048,576 bytes. It covers every one of the 85 plain-window writes. |
| 6 | After act 5 succeeds, run the `read-config` command with the existing full plan. | 1,309 windows of 256 bytes under `windows/`, 335,104 payload bytes, plan `read-adjustment-42000-v1`; the oracle `X-M1` and signature `55 aa` sit in the named windows. |
| 7 | After act 6 succeeds, run `read-media` last. | `bank-0.bin` and `bank-1.bin` of 131,072 bytes each, `cache-snapshot.bin` and the two gate readbacks; the result line names the captured byte count, 262,144 in all. |

```sh
devenv shell -- bazel run //transport:camera -- dump-ram id4-image-0
devenv shell -- bazel run //transport:camera -- dump-ram id4-image-1
devenv shell -- bazel run //transport:camera -- dump-ram id5-image-0
devenv shell -- bazel run //transport:camera -- dump-ram id5-image-1
devenv shell -- bazel run //transport:camera -- dump-ram plain-0
devenv shell -- bazel run //transport:camera -- read-config "$PWD/private-data/read-plans/adjustment-42000.json"
devenv shell -- bazel run //transport:camera -- read-media
```

These are the existing reviewed fixed segments and the existing plan, not a new reader. The `probe-ram` fixed windows touch only small fixed spots of the read extents: its four 256-byte table windows cover parts of the 516-byte prefix, and none of its windows covers a film row, availability cell, bank word or plain-window write. The five `dump-ram` segments are the read set because they are the fixed ranges that cover every write extent whole; the image-4 tail `[0x81c000, 0x0089ec00)`, the rest of image 5 beyond `0xdf1a00`, image 2 and the firmware storage stay unread (section 11).

## 6. Ending

**End the session by cutting the dummy-battery supply**, then unplug USB and set the POWER switch to OFF while unpowered. Do not end it with the POWER switch: a switch-ended jig cycle resets date, time, language and the C slot (E-JIG-CYCLE-D-150, hardware-observed), and the cut ending preserved settings in every prior observation (E-JIG-POWERCUT-PRESERVES-150). Then boot normally once to restore ordinary operation after `read-media`'s storage-driver shutdown and confirm the watched settings and the nine-entry film-simulation list again, then take one shot and play it back ([spec 3.1 step 4](../spec/07-governance-and-handoff.md#31-procedure)). That boot is the restoration check and its result goes in note row E; the byte evidence is complete when act 7's folder and the comparisons of section 8 are in the handoff of [the install session](recipe-install-session-script.md) section 8.

## 7. The note sheet

| Row | Step | What you did | Wall time | Camera showed | Anything unexpected |
| --- | --- | --- | --- | --- | --- |
| 0 | entry | maintenance entry, shutter + Up held at power-on with USB attached | | blank screen, green LED | |
| A1 | read | `dump-ram id4-image-0` folder | | | |
| A2 | read | `dump-ram id4-image-1` folder | | | |
| A3 | read | `dump-ram id5-image-0` folder | | | |
| A4 | read | `dump-ram id5-image-1` folder | | | |
| A5 | read | `dump-ram plain-0` folder | | | |
| A6 | read | `read-config` folder | | | |
| A7 | read | `read-media` folder | | | |
| E | ending | supply cut, USB unplugged, POWER off while unpowered; restoration boot: settings and list checked, one shot and playback | | | |

A refused or stopped operation is a result: write what it printed into the row and follow section 6.

## 8. The comparison method and the expected bytes

All comparisons are host-only. Set the capture paths once:

```sh
D0="<A1 folder>/id4-image-0.bin"; D1="<A2 folder>/id4-image-1.bin"
E0="<A3 folder>/id5-image-0.bin"; E1="<A4 folder>/id5-image-1.bin"
P0="<A5 folder>/plain-0.bin"
C=private-data/analysis/x-m1-1.50/recipes/runtime/combined-candidate
D3M=private-data/analysis/x-m1-1.50/recipes/runtime/d3-compiled/patch-set-image4.json
D5M=private-data/analysis/x-m1-1.50/recipes/runtime/d3-compiled/patch-set-image5.json
PLAN=private-data/analysis/x-m1-1.50/recipes/runtime/builder-out/bank-plan.json
```

### 8.1 Capture integrity first

Before any content comparison, each act must verify clean the way the 2026-10-08 dumps and the 2026-10-09 readback did: the prepared request count equals the fixed table (4,229 for a 1 MiB `dump-ram` segment), every request status is `(0,0)`, every request summary SHA-256 equals the hash of its slice of `<segment>.bin`, the segment-complete byte count and hash equal the capture file, and the two gate readbacks carry index `0xf7` = 1 then 0 with no refusal or uncertain outcome. Act 6's journal is the plan's own 5,240-transfer shape with a unique terminal success; act 7's is the documented 1,058-slice read with its restores. A segment that fails its own journal checks is not compared.

### 8.2 The extents

| Extent | RAM range | What it carries | Read act | Expected bytes against the recorded hashes |
| --- | --- | --- | --- | --- |
| Image-4 decode | `[0x0061c1fc, 0x0081c000)` | the 516-byte prefix (nine-row arrays, row tables, property table, commit hook), the two compiled-recipe rows, the 14 availability cells, the 8 pointer words, the 34 bank words | 1 (decode part, from file offset 508) and 2 | `id4-final.bin` (`84d18cbd...`, 2,632,196 bytes) over the same extent, section 8.3 |
| Segment-table entry-4 words | `0x0061c0d4` and `0x0061c0d8` | load `0x0061c1fc`, size `0x00282a2c` | 1 (file offsets 0xd4/0xd8) | the container's own words, section 8.4 |
| Image-5 words | `0x00bfa3f0`-`0x00d526fc` | the `case 0xfe` site instruction at `0x00d50a48`, three page-entry cache shim calls, the modal action entry and three cell words | 3 and 4 | `id5-final.bin` (`c9a22267...`) at the words and over the read extent, section 8.5 |
| Plain-window writes | `[0x011122c0, 0x011ad794)` | 42 label-class words (label gates, id-minus-one gates, record-base `movw`/`movt`), the menu values/labels/parent/name-record regions, the edit rows and the seven stock-mode film edit label quadruples | 5 | the plan's own `new` bytes and the deliverable's extracted window, section 8.6 and 8.7 |
| Adjustment mirror | `[0, 0x42000)` and `[0x400000, 0x40fd00)` | update stamp, restore-covered body bytes, develop tables, arm/gate bytes | 6 | the before capture of [the install session](recipe-install-session-script.md) act B5, section 8.8 |
| Area-2 banks | raw blocks `0x3ac`, `0x3b2` | the ordinary-settings ledger and its record history | 7 | the before capture of act B6, section 8.9 |

### 8.3 Image-4: the decode extent `[0x0061c1fc, 0x0081c000)`

The decode base is `0x0061c1fc` (byte-read segment table). `cmp` with no output and exit 0 is the expected result for both lines; the record's `id4-final.bin` is the exact decode the chain's `fflzd` round trips put in the container.

```sh
cmp -i 508:0 -n 1048068 "$D0" "$C/id4-final.bin"     # [0x61c1fc, 0x71c000)
cmp -i 0:1048068 -n 1048576 "$D1" "$C/id4-final.bin" # [0x71c000, 0x81c000)
dd if="$D0" bs=1 skip=508 count=516 status=none | cmp -n 516 - "$C/id4-final.bin"
```

The expected file offsets: RAM `0x61c1fc` is `id4-final.bin` offset 0, RAM `0x71c000` is offset `0xffe04` (1,048,068), RAM `0x81c000` is offset `0x1ffe04` (2,096,644). The two compared slices' SHA-256 values, if the segments are compared as files rather than in place: `4aef770d03ec007695a7634007740ebac74d394efb7bb1bb68bc95b5ea6bae3d` for the D0 decode slice and `edf08381e46a6273d83594ca3ce41e277a2036c7b03d9c5ab735e2e97e867d3c` for the whole D1 file. The 2026-10-08 census put every image-4 runtime-written cell at or above `0x00820f20`, so equality is expected over this extent; the 2026-10-09 readback observed the same shape on the RC0 image. A differing byte inside the extent is a writer or a wrong image: report it, do not screen it without a mapped mechanism.

### 8.4 The segment-table entry-4 words

The head carries the container's 1 KiB table region; the entry-4 words are its only changed bytes and the boot's own table copy shows them at the same file offsets.

```sh
od -An -v -tx4 -j 0xd4 -N 8 "$D0"     # expect 0061c1fc 00282a2c
```

The head's remaining bytes are the container's media table region. Compare them host-side against the deliverable's own region (media `[0x140000, 0x1401fc)`, complemented) and expect differences only at five runtime cells, file offsets `0x188`, `0x189`, `0x1c0`, `0x1d8` and `0x1f0` (observed runtime-written and stable across the 2026-10-09 before/after captures, `0x10`/`0x43` and `0x01` there); the entry-4 words at `0xd4`/`0xd8` must match exactly. Anything else in the head goes to host analysis.

### 8.5 Image-5: the words and the read extent

The words: the D3 site word at `0x00d50a48` (new `0xebe32e57`), and the eight plan words `0x00bfa3f0`, `0x00bfa7ec`, `0x00bfacf0`, `0x00cd20a4`, `0x00cd211c`, `0x00cd243c`, `0x00d04084`, `0x00d526fc`. The extent comparison:

```sh
cmp -i 0:0 -n 1048576 "$E0" "$C/id5-final.bin"         # whole E0 equals id5-final[0:0x100000]
cmp -i 0:1048576 -n 1048576 "$E1" "$C/id5-final.bin"   # E1 equals id5-final[0x100000:0x200000] modulo the two cells below
```

Expected result: `id5-image-0.bin` equals `id5-final.bin` `[0, 0x100000)` exactly, whole-file SHA-256 `9e1551a05eb28c6173a203681d5ee203cb1dff05ab9d6223ee8866291d8a94ed`. **Byte-read:** the 2026-10-08 census found no runtime-written byte in `[0xbf1a00, 0xcf1a00)` and exactly two in `[0xcf1a00, 0xdf1a00)` at RAM `0x00d0cd9c` (artifact `ff` to RAM `00`) and `0x00d3d53c` (artifact `01` to RAM `00`), writers unbound. For E1 expect either no `cmp` differences or, at most, comparison-relative byte numbers 111,517 and 310,077; run the listed `cmp -l` line and check that no other number appears:

```sh
cmp -l -i 0:1048576 -n 1048576 "$E1" "$C/id5-final.bin"   # allowed lines: 111517 and 310077 only
```

If both cells hold their artifact values the E1 file hashes to `b0e6db80f3b4f67effbb0cdfd9d6fd092cb7635b68f900014a4952530b3b1447`; if they read as the 2026-10-08 runtime values (`00` at both) it hashes to `aefbbc85c45e56875811aa37083075ee4626187d4863860b4b74ed97eb7e2d52`. Record the captured bytes there; any other difference is a finding.

### 8.6 Plain-window: the read extent and the per-write checks

`plain-0` maps `[0x01100000, 0x01200000)`; the deliverable's window is the loader's media window from flash `0x005a0000` (3,538,944 bytes), extracted host-side:

```sh
bazel-bin/host/firmware/rebuild-update extract-plain "$C/install/FWUP0002.DAT" /tmp/recipe-plain-window.bin
cmp -i 0:0 -n 1048576 "$P0" /tmp/recipe-plain-window.bin
cmp -l -i 0:0 -n 1048576 "$P0" /tmp/recipe-plain-window.bin   # allowed lines: 39137 and 39143 only
```

Expected result: `plain-0.bin` equals the extracted window's first 1 MiB (`SHA-256` of the whole window `af7af689a616bdfea6b14375197d8fc971c35205deeb847337227a4512f0f1e7`) except, at most, two cells. **Byte-read:** the 2026-10-08 census found exactly two runtime-written bytes in the plain image, `0x011098e0` and `0x011098e6` (artifact `00` to RAM `02`, state bytes beside the glyph-id table, both below the first write region); they are comparison-relative byte numbers 39,137 and 39,143 of the `plain-0` file, and no other line may appear in the `cmp -l` output. If both cells hold their artifact values the file hashes to `8e813f00f81de5fe943f33d80e962447d2584bcd1d304f307bf656af476cefd1`; if they read as the 2026-10-08 runtime value (`02` at both) it hashes to `6d159069b2ab0643060f87f2bfa5f03b3b6e0ac46144a4bdbab8561eb973b6fe`. Record the captured bytes there; any other difference is a finding.

### 8.7 The 127 plan writes and the D3 words and blobs, per word

The extent comparisons above already bind these bytes; these loops are the per-write evidence that names each one. They read each word at `RAM - segment start` from the right capture file. The `slice_for` helper maps an address to its capture file and offset:

```sh
slice_for() {
  a=$1
  if   (( a >= 0x61c000 && a < 0x71c000 )); then f=$D0; off=$((a-0x61c000))
  elif (( a >= 0x71c000 && a < 0x81c000 )); then f=$D1; off=$((a-0x71c000))
  elif (( a >= 0xbf1a00 && a < 0xcf1a00 )); then f=$E0; off=$((a-0xbf1a00))
  elif (( a >= 0xcf1a00 && a < 0xdf1a00 )); then f=$E1; off=$((a-0xcf1a00))
  elif (( a >= 0x1100000 && a < 0x1200000 )); then f=$P0; off=$((a-0x1100000))
  else f=/dev/null; off=0; fi
}

# the 28 D3 image-4 body words (six film-row words, 14 availability cells, eight pointer words)
jq -r '.words[] | [.order, .address, .new] | @tsv' "$D3M" | while IFS=$'\t' read -r ord addr new; do
  a=$((addr)); slice_for "$a"
  got=$(od -An -v -tx4 -j "$off" -N 4 "$f" | tr -d ' \n'); want=$(printf '%08x' "$((new))")
  if [ "$got" = "$want" ]; then printf 'word %s %s ok\n' "$ord" "$addr"; else printf 'word %s %s DIFFERS want %s readback %s\n' "$ord" "$addr" "$want" "$got"; fi
done

# the six D3 prefix blobs (values array, shooting labels, Q labels, row tables, property table, hook)
jq -r '.blobs[] | [.id, .address, .length, .new_hex] | @tsv' "$D3M" | while IFS=$'\t' read -r id addr len new; do
  a=$((addr)); slice_for "$a"
  got=$(od -An -v -tx1 -j "$off" -N "$len" "$f" | tr -d ' \n')
  if [ "$got" = "$new" ]; then printf 'blob %s ok\n' "$id"; else printf 'blob %s DIFFERS\n' "$id"; fi
done

# the one D3 image-5 word (the case-0xfe site instruction)
jq -r '.words[] | [.order, .address, .new] | @tsv' "$D5M" | while IFS=$'\t' read -r ord addr new; do
  a=$((addr)); slice_for "$a"
  got=$(od -An -v -tx4 -j "$off" -N 4 "$f" | tr -d ' \n'); want=$(printf '%08x' "$((new))")
  if [ "$got" = "$want" ]; then printf 'word %s %s ok\n' "$ord" "$addr"; else printf 'word %s %s DIFFERS want %s readback %s\n' "$ord" "$addr" "$want" "$got"; fi
done

# all 127 plan writes (the 85 plain-window carrier writes and the 42 image words)
jq -r '.writes[] | [.region, .address, .new] | @tsv' "$PLAN" | while IFS=$'\t' read -r region addr new; do
  a=$((addr)); n=$((${#new}/2)); slice_for "$a"
  got=$(od -An -v -tx1 -j "$off" -N "$n" "$f" | tr -d ' \n')
  if [ "$got" = "$new" ]; then printf 'write %s %s ok\n' "$region" "$addr"; else printf 'write %s %s DIFFERS\n' "$region" "$addr"; fi
done
```

Expected result: every line ends `ok`, 28 word lines, six blob lines, one image-5 word line and 127 write lines; no line contains `DIFFERS`. `od -v` is required: without it `od` collapses repeated lines to `*` and long repeated runs print wrong. The compiled-recipe rows are the two film conversion rows at `0x0077ac48` and `0x0077ac60` (`0x0000000d 0x7ffffffe 0x00000000` and `0x0000000e 0x7ffffffe 0x00000003`), read directly with `od -An -v -tx4 -j $((0x77ac48-0x71c000)) -N 12 "$D1"`; the label words are the 42 `plain_sites` writes and the edit-region writes, each checked by the loop.

### 8.8 The adjustment comparison plan

Act 6's `windows/` compares byte by byte against the before capture of [the install session](recipe-install-session-script.md) act B5 (the 2026-10-09 capture is the last full one before this install and is the cross-check for field identity), all 1,309 windows over `[0, 0x42000)` and `[0x400000, 0x40fd00)`. The named fields first:

| Field | Window and byte | Before (expected) | Expected after | Reading |
| --- | --- | --- | --- | --- |
| Product oracle | `000100.bin` `+0x5c` | `58 2d 4d 31` | unchanged | the body is using its own calibration |
| Update stamp | `000100.bin` `+0xe8`/`+0xe9` | `01 78` | `01 78` unchanged (static-inference) | the refresh stamps the executing build's constants; this install executes a 1.50-derived build whose constants are already the stored ones, so no move is expected. Any other value stops for host analysis |
| Signature | `000200.bin` `+0x02` | `55 aa` | unchanged | as the oracle |
| Boot-build stamp | `000200.bin` `+0x04`/`+0x05` | `01 78` | unchanged | rewritten every boot from the installed build; not proof of the new image |
| Pre-erase controls | `000200.bin` `+0x75`/`+0x76` | `01 0a` | unchanged | comparison expectation only |
| Arm and gate bytes | `000000.bin` `+0xa2`/`+0xf7` | `00 00` | unchanged | AUTO_ACT stays disarmed |

Then the whole mirror, as a mover census against the before capture. **Byte-read:** the update's configuration completion `0xdb91e0` snapshots cache `[0, 0x42000)`, refreshes it from the template in raw blocks 7-9, stamps `+0x1e8`/`+0x1e9` and `+0x204`/`+0x205` with the executing build's constants, restores the container's 98 auxiliary-1 and 3 auxiliary-2 (offset, length) pairs covering 1,487 bytes from the snapshot, and commits. This container's two restore lists are byte-identical to stock and its template blocks `[0x000e0000, 0x00140000)` are byte-identical to the stock container's (carry checks in the private install-artifact record `private-data/analysis/x-m1-1.50/recipes/runtime/combined-candidate/install/install-artifact.json`; [the public rebuild contract](update-container-rebuild.md) section 2.3 describes the restore mechanism, not that private artifact's checks), so the expected mover set is **empty**: the stamps write values already stored and the restore lists return every covered byte to its snapshot value. `+0x204`/`+0x205` are also the boot-load stamps and hold `01 78` in both captures; `+0x4069`-`+0x406e` and `+0xd8` are the other boot-load stamps of the census. The 224 body-specific restore-covered bytes must hold their before values: a restore-covered byte at its template value refutes the restore, and a template-valued byte holding a non-template value refutes the refresh. `+0x1e8`/`+0x1e9` are not restore-covered (pair 8 starts at `0x1ea`), so their unchanged `01 78` is the refresh's own record of the executing build and the boot load never rewrites them. The one non-restore, non-stamp mover a prior readback observed is the 79-byte block at `[0x408a00, 0x408a70)` (init-time builder inputs, trigger open); if it moves again it is reported and classified, and any mover it does not explain goes to host analysis before any success claim.

The 253 windows of `[0x400000, 0x40fd00)` sit outside the mapped snapshot, refresh and restore (E-ADJ-MOVERS-150 limitation); a clean hold is the expected result, the one named mover there is the 79-byte init-time class above, and any other mover is reported for host analysis.

Because the executing build's stamp constants equal the stored ones, the mirror alone cannot timestamp the refresh: an unchanged mirror after this install shows that nothing was lost, and the completion screen and the S5 commit stand as the update's own record rather than as mirror evidence.

### 8.9 The area-2 banks comparison plan

Act 7's `bank-0.bin`/`bank-1.bin` compare against the before capture of act B6. The expected differences are records the owner's post-install ordinary acts appended (the recipe selections, parameter changes, shots and the power-cycle check of [the install session](recipe-install-session-script.md) section 6) and any bank rotation the log accounting explains; the [settings instrument](settings-log-map-session-script.md#1-capacity-read-this-first) owns record selection, capacity and bank-erasure accounting. **Byte-read:** the install adds no record there on the mapped evidence: S4's configuration work writes media `[0x520000, 0x940000)` while the banks live at raw blocks `0x3ac` and `0x3b2`, byte offsets `0x7580000` and `0x7640000` (hardware-observed map resolution, E-AREA2-BANKS-150), so a bank unchanged from before with no new records is the expected control outcome and every new record must be attributable to a named owner act.

## 9. What each act proves, and what it cannot

| Act | Proves | Cannot |
| --- | --- | --- |
| 0 entry | The reads happen in one maintenance boot on the body the note sheet names | Nothing about bytes; a wrong screen stops before any command |
| 1 `dump-ram id4-image-0` | The 516-byte prefix (arrays, row tables, property table, hook), the bank's image-4 words in the head of the decode, and the segment-table layout words are live in the running body at the container's values | The write's time or writer; a writer producing the same values; any later boot |
| 2 `dump-ram id4-image-1` | The two compiled-recipe rows, the 14 availability cells, the eight pointer words and the bank's registry words hold the container's values, so with act 1 the write set's image-4 half is read back | The same limits; the menu itself, which the photographs already bind; anything in the image-4 tail beyond `0x81c000` |
| 3 and 4 `dump-ram id5-image-*` | The `case 0xfe` site instruction, the modal entry, cell words and shim calls hold the container's values | The same limits; the image-5 tail beyond `0xdf1a00` |
| 5 `dump-ram plain-0` | Every one of the 85 plain-window writes (label words, menu, name records, edit rows) and their neighbours hold the container's bytes | The same limits; the window beyond 1 MiB; writes outside the read range |
| 6 `read-config` | The adjustment mirror's bytes after the install; whether the refresh and its stamp writers ran; whether the restore lists held the body's 224 bytes | That the new image landed, since the stamp is the executing updater's record; firmware storage; the decode prefix |
| 7 `read-media` | The two area-2 banks' bytes against the before ledger and what the owner's post-install acts recorded | Firmware storage; the prefix; anything outside the two banks |
| E ending | Settings-preserving close of the boot `read-media` ended (hardware-observed prior cuts); the restoration boot shows capture and playback working again after the storage-driver shutdown | Nothing itself; it is the control that keeps the verified state |

## 10. State-changing bytes, and what the review needs

**With respect to persistent state this session is read-only.** No operation sends a configuration commit (`0x80 = 0x22`), an End Setting, an update request, an AUTO_ACT arm or any internal-media program or erase request. The bytes it changes are adjustment-cache RAM at `0x41acd800` and session/driver state:

| Operation | Byte | Values written | Persistence |
| --- | --- | --- | --- |
| `dump-ram` acts 1-5 | cache index `0xf7`, RAM `0x41acd8f7` | `01`, readback, work, `00`, readback | none; a function-3 write at an index other than `0x80` posts no service command (byte-read transport contract) |
| `read-config` act 6 | none | none | H1 reads only; C0/C4 session and reply RAM |
| `read-media` act 7 | cache indices `0x80`, `0x81`, `0x82`, `0x83`, the restore-only `0x84`-`0x8a`, and `0xf7` | `0x35` trigger and `0x33` disarm at `0x80`, left at the `0x33` disarm; zero at `0x81`, the per-bank selector at `0x82`, the `0xff` sentinel at `0x83`; `0x81`-`0x8a` and `0xf7` restored to their session-start values on every exit path | none; `0x35`/`0x33` are the raw-read action's trigger and disarm, not commit values; the operation shuts down the storage driver |
| Section 6 ending | dummy-battery supply cut | volatile gates and session state | no POWER/OFF defaults record (hardware-observed prior cuts) |

A gate-using operation that stops before its restore leaves `0xf7` at `1` until the next power cycle (byte-read transport contract); the supply-cut ending supplies that power cycle. Because every byte above is an existing reviewed byte of an existing reviewed operation, the session sends no new byte and needs no new independent byte review. What it needs is the owner's go under [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs), naming these seven operations: H1 for `read-config`, H2 for the five `dump-ram` segments and `read-media` with the RAM-only writes above.

## 11. What this session cannot see

- The write itself. No write trace exists; the readback is the state at read time in this boot.
- A value-identical writer. Byte equality binds the values to the container's payload and cannot exclude a writer that produced the same values by another route.
- Other boots and ordinary-mode transitions. The nine-entry menu photographs bind the arrays at draw time; a later writer would show in a later readback or menu session.
- The image-4 tail `[0x81c000, 0x0089ec00)` and the image-5 tail beyond `0xdf1a00`: no plan write, availability cell or pointer word reaches them, they carry known runtime-written cells (E-DUMP-RAM-150), and no segment over them is in the read set.
- The image-2 extent and its id-`0x83d` name record (`WARM STANDARD`): no reviewed operation reads the id-2 load area at `0x00130000`, so the name is bound by the menu-row photograph, not by a byte read.
- Firmware storage, the C-slot's area-3 banks and the trailer are unread by this session. Recovery: nothing here is a repair, a restore or a demonstrated recovery.
