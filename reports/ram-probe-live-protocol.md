# RAM probe candidate live protocol

Status: implemented and run twice on 2026-10-07. The first sixteen-window session established the installed-image correspondence below. Four RESET-ORIGIN-01 windows were then added, independently reviewed pass-with-notes and run in a twenty-window session. Both runs completed, restored the gate byte and closed the transport.

## Purpose

Installed-image correspondence. On installed 1.50 the probe addresses are the running firmware's own load addresses, so the same read that would have been a hypothesis test on 1.00 is a direct check: the bytes at the segment table, both decoded image loads and the plain image are compared against the private decoded DAT images, which binds [E-LZ-GRAMMAR](../docs/evidence.jsonl) on hardware (the comparison [firmware-extraction restart point 4](firmware-extraction.md#decoder-research-and-restart-points) specifies) and closes the "installed correspondence" limitation standing on G2, AF-STATIC-01, SERVICE-STATIC-01, UPDATE-01 and SAFE-JIG-EXIT-01.

## Static basis

The gate: every routine of D0 opcode `0x200001` returns without acting while `0x0069f8fc(0xf7)` is zero. `0x00d803cc` dispatches; `0x00647b1c` (function 2) returns 2 at its gate test, which reaches error pair `(1,0xffff)` ([E-DEBUG-ERROR-MAP](../docs/evidence.jsonl), [E-END-SETTING-GATEFREE-NEGATIVE-150](../docs/evidence.jsonl)). The byte reads `0x00` on this body under both 1.00 and 1.50 ([E-ADJ-GATE-BYTE-ZERO](../docs/evidence.jsonl), [E-ADJ-GATE-BYTE-150](../docs/evidence.jsonl)).

The enable write: config opcode `0x1a0002` function 3 reaches `0x00dc7564`, which writes the adjustment cache through `0x0069fc94` after its `0x420000` range check and before the mailbox and index tests, posts queue-7 `0x1707` only for start index `0x80`, and returns success for any other index. The whole-area storage writer is reachable only through that post, the update configuration completion or the on-camera editor exit, so a session that never writes index `0x80` leaves the byte in RAM ([E-ENABLE-COMMITFREE-150](../docs/evidence.jsonl), [E-ADJ-COMMIT-MAP](../docs/evidence.jsonl)). The public fffw tool writes and restores the same byte around every debug command ([E-FFFW-NO-END-SETTING](../docs/evidence.jsonl)).

Function 2: `0x00d803cc` calls `0x00647b1c(length, source, destination)`; the destination is reply `+0x24` (literal `0x00d8061c` = `0x014698c0`), the address and length echoes are at `+0x1c`/`+0x20` (`0x014698b8`/`0x014698bc`), and the advertised available reply length is `0x11c` (284) at `0x00d80608`/`0x00d8060c` on every exit. 284 is less than `0x24 + 0x100`, so a reply exposes only 248 data bytes; each 256-byte window is therefore read as two 128-byte halves, whose replies are exactly `0x24 + 0x80` = 164 bytes (the service-command map `private-data/analysis/x-m1-1.50/service-commands/findings.md` is private; the private end-setting error map `private-data/analysis/x-m1-1.50/end-setting/error-mapping.md` and an `llvm-objdump` read of `0x00d80618`/`0x00d8061c`/`0x00d80620` confirmed the constants on 2026-10-07; public service context is in [the service feasibility report](service-shoot-feasibility.md)).

The readback and the probes' source: the config opcode's own window 0 read (opcode `0x1a0002` function 2, 256 bytes, reply `0x124` with data at `+0x24`) is live-proven on this body and firmware ([E-SERVICE-READ-MAP](../docs/evidence.jsonl), the `gate-byte` session of [E-ADJ-GATE-BYTE-150](../docs/evidence.jsonl)).

The addresses: the boot reads flash `0x140000` to RAM `0x0061c000` and copies the loader records at flash `0x80000`/`0xa0000` to `0x00040000`/`0x000d0000`; image 4 decompresses to `0x0061c400` and image 5 is loaded at `0x00bf1a00`; the plain image is linked for `0x01100000` with the sub-list at payload `0x1401f0` giving the second and third extents at `0x011c0000` and `0x011e0000` ([E-BOOT-LOADPATH](../docs/evidence.jsonl), [the payload survey](firmware-extraction.md#segmentation)).

## Bytes

| Stage | Control data | Following transfer |
| --- | --- | --- |
| Open C0 | `c0 00 00 00 00 00 00 00 00 08 00 00` | bulk IN, exactly 8 bytes |
| Enable | `d0 00 00 00 00 00 00 00 00 25 S_hi S_lo` | bulk OUT, 37 bytes: `02 00 1a 00`, twelve zero bytes, `03 00 00 00`, `01 00 00 00`, `00 00 00 00`, `f7 00 00 00`, `01 00 00 00`, `01` |
| Readback | `d0 00 00 00 00 00 00 00 00 24 S_hi S_lo` | bulk OUT, 36 bytes: `02 00 1a 00`, twelve zero bytes, `02 00 00 00`, `01 00 00 00`, `00 00 00 00`, `00 00 00 00`, `00 01 00 00` |
| (readback) | `d1 00 00 00 00 00 00 00 01 24 S_hi S_lo` | bulk IN, exactly 292 bytes; data at `+0x24`, byte `+0xf7` of the data is the gate byte |
| Probe | `d0 00 00 00 00 00 00 00 00 24 S_hi S_lo` | bulk OUT, 36 bytes: `01 00 20 00`, twelve zero bytes, `02 00 00 00`, `00 00 00 00`, `00 00 00 00`, address LE32, `80 00 00 00` |
| (probe) | `d1 00 00 00 00 00 00 00 00 a4 S_hi S_lo` | bulk IN, exactly 164 bytes; data at `+0x24`, 128 bytes |
| Restore | as Enable | as Enable with appended byte `00` |
| Close C4 | `c4 00 00 00 00 00 00 00 00 02 S_hi S_lo` | bulk IN, exactly 2 bytes |

## Probe list

Twenty 256-byte windows, in order, each two 128-byte requests, address then address+0x80. Windows 4, 14, 19 and 20 were added for RESET-ORIGIN-01 before the second run.

| # | Address | Purpose | Comparison artifact |
| ---: | --- | --- | --- |
| 1 | `0x41acd900` | cache product oracle at `+0x15c` | bound bytes `X-M1\0`, verified in both captures |
| 2 | `0x41acd800` | cache page 0, readback comparison | the session's own enable readback must equal it |
| 3 | `0x41acda00` | cache signature oracle at `+0x202` | bound bytes `55 aa` |
| 4 | `0x41acdb00` | cache page 3, indices `0x3b9` and `0x3ba` | none; the two bytes gate `0x006a5c08`, which can write retained register `0x58` without recomputing the header checksum |
| 5 | `0x00040000` | loader A load base | flash `0x80000` record body |
| 6 | `0x000d0000` | loader B load base | flash `0xa0000` record body |
| 7–10 | `0x0061c000`–`0x0061c300` | the segment-table region | flash `0x140000` |
| 11 | `0x0061c400` | image 4 decoded load base | `lz/id4-0061c400.bin` |
| 12 | `0x0062c400` | image 4 `+0x10000` | same image at `+0x10000` |
| 13 | `0x0081c400` | image 4 `+0x200000` | same image at `+0x200000` |
| 14 | `0x0089f800` | settings fallback flags and boot discriminator | none; `0x0089f834` is set by the reject arm at `0x006a77a8` and cleared by `0x006a8abc` in the same invocation, so a mid-session reading is 0 whether or not that boot defaulted. The persistent content is the discriminator at `0x0089f814`/`0x0089f818`/`0x0089f81c` written by `0x006a5aac` and the area-3 live copy at `0x0089f838` with its snapshot at `0x0089f874` |
| 15 | `0x00bf1a00` | image 5 decoded load base | `lz/id5-00bf1a00.bin` |
| 16 | `0x00c01a00` | image 5 `+0x10000` | same image at `+0x10000` |
| 17 | `0x01100000` | plain image load base | `lz/plain-01100000.bin` |
| 18 | `0x011c0000` | plain image second extent base | same image at `+0xc0000` |
| 19 | `0x409c2800` | retained header live bytes at `0x409c281c` | none; the sixteen bytes `0x006a6ddc` writes through to the retained device |
| 20 | `0x409c2900` | retained header snapshot at `0x409c29b8` | none; marker byte `0x409c29c6` must be `0xaa` and checksum byte `0x409c29c7` must make the sixteen-byte sum zero, which is what the boot gate `0x006a75a4` tests |

Windows 1–3 are the channel oracle and run first, so a wrong channel stops before any code window is trusted. The loaders and the segment table have no private decoded artifact; their capture is compared against the payload records host-side, where a mismatch is a mapping result rather than a stop.

Comparison offsets, with the copy each one assumes:

| Window | Artifact | Offset | Assumption |
| --- | --- | --- | --- |
| cache ×3 | none | | the two bound byte strings and the readback equality are the whole check |
| `0x0061c400` +0x0/0x10000/0x200000 | `lz/id4-0061c400.bin` | 0, `0x10000`, `0x200000` | the boot decompresses id 4 to `0x0061c400`; per-4 KiB block manifest `blocks-id4-0061c400.txt` |
| `0x00bf1a00` +0x0/0x10000 | `lz/id5-00bf1a00.bin` | 0, `0x10000` | id 5 is decoded to `0x00bf1a00`, loader unknown; manifest `blocks-id5-00bf1a00.txt` |
| `0x01100000`, `0x011c0000` | `lz/plain-01100000.bin` | 0, `0xc0000` | the payload range from `0x5a0048` is copied to `0x01100000` unchanged; the loader is unestablished |
| `0x0061c000`–`0x0061c3ff` | payload | `0x140048` + (address − `0x0061c000`) | the boot reads flash `0x140000` to `0x0061c000`; flash = payload − 0x48 |
| `0x00040000`, `0x000d0000` | payload | `0x80048`/`0xa0048`, plus 0 and `0x20` candidates | the loader records declare load and size; whether the body maps from the record or from `load+0x20` is untested |

The image-4, image-5 and segment-table windows all sit inside the ranges the private notes already hash end to end, so a clean comparison can also be stated as a range hash; the plain-image and loader windows cannot until the copy is established.

## Session checks and stops

The enable and the restore each require a 292-byte reply with the opcode, function 3, mailbox 1, page/offset/length echoes `(0, 0xf7, 1)` and error words `(0,0)`. The two readbacks require the config read contract of `read-config` and byte `+0xf7` of window 0 to be `1` and `0`. Every probe reply must be 164 bytes with the opcode echo, function 2, a zero reserved word, the address and length echoes and error words `(0,0)`; `(9,3)` and `(1,0xffff)` are named refusals and anything else stops. The assembled cache windows must satisfy the three oracles. Any stop leaves earlier windows captured, sends no retry, attempts one C4 and releases host resources once.

The session budget is `transferTimeoutMs` times 184 transfers (8 fixed, 16 for the gate writes and readbacks, 160 for the twenty windows), about 15.3 minutes at the clamped 5000 ms per-transfer ceiling; the journal cap is 256 KiB. The sixteen-window list budgeted 152 and the session used 148.

## Run

1. Note date, time, language and one value in the C slot: the jig exit resets them as usual ([E-JIG-OBSERVED](../docs/evidence.jsonl)). Fully charged battery; no firmware file on the card.
2. Power off, then enter jig mode as usual with USB connected.
3. Run `devenv shell -- bazel run //transport:camera -- probe-ram`. The command selects the single `04cb:ff80` device, journals every outgoing frame, writes its session under `private-data/sessions/` and prints `probe-ram: 20 windows`.
4. Exit jig mode, power-cycle, take a shot and play it back, and report the result in chat.

## Result

The session ran at 2026-10-07T18:50:12Z and printed `probe-ram: 16 windows`. All 18 files hash to the SHA-256 values in the journal, the two gate readbacks hold `0x01` then `0x00` at index `0xf7`, no window was refused and the close completed. The owner reports normal operation after: jig exit, the usual setup pass, and a shot with good playback.

Comparison against the private artifacts, all windows byte-identical unless noted:

| Window | Artifact and offset | Result |
| --- | --- | --- |
| `0x0061c400` | `lz/id4-0061c400.bin` +0 | match |
| `0x0062c400` | `lz/id4-0061c400.bin` +0x10000 | match |
| `0x0081c400` | `lz/id4-0061c400.bin` +0x200000 | match |
| `0x00bf1a00` | `lz/id5-00bf1a00.bin` +0 | match |
| `0x00c01a00` | `lz/id5-00bf1a00.bin` +0x10000 | match |
| `0x01100000` | `lz/plain-01100000.bin` +0 | match; the artifact is the uncompressed payload slice from `0x5a0048` |
| `0x011c0000` | `lz/plain-01100000.bin` +0xc0000 | match; payload `0x660048` |
| `0x00040000` | payload `0x80048` (record start) | match; the record+0x20 placement differs |
| `0x000d0000` | payload `0xa0048` (record start) | match; the record+0x20 placement differs |
| `0x0061c000` | payload `0x140048` | match |
| `0x0061c200`, `0x0061c300` | payload `0x140248`, `0x140348` | match; all 512 bytes are zero on both sides, so these two windows carry no copy evidence |
| `0x0061c100` | payload `0x140148` | differs in five bytes, below |
| cache `41acd800` | the session's config readback | identical, and it differs from the 2026-10-07T06:08:12Z 1.50 capture only at `+0xf7` (`0x00` to `0x01`) |
| cache `41acd900`, `41acda00` | bound oracles and the 06:08:12Z capture | pass (`X-M1` at `+0x15c`, `55 aa` at `+0x202`); both windows are identical to that capture's config windows 1 and 2 |

The five differing bytes are RAM `0x0061c188` = `0x00004310`, the word after `0x00001234` at `0x0061c184`, and the word at `0x0061c1c0`, `0x0061c1d8` and `0x0061c1f0` = `0x00000001`, all zero in the payload. The word at `0x0061c1a4` is `0x4061c1b0`, the `0x40000000` alias of `0x0061c1b0`, followed by `0x01100000` and `0x00360000`, the plain image's linked base and the artifact's length. From `0x0061c1b0` run three consecutive `0x18`-byte entries whose `+0x4`/`+0x8` words are the plain-image extents `0x01100000`/`0xac5a0`, `0x011c0000`/`0x1cca0` and `0x011e0000`/`0x26a5e0`, and `0xffffffff` at `0x0061c1f8` follows the third. Each of the three changed words is `+0x10` of one of those entries, so the running firmware has set one word per plain-image extent. Which code writes these words and what they mean are unestablished; a per-extent load or state flag is a candidate and a lead on the plain image's loader.

What this establishes: the seven image windows hold the artifact bytes. The five image 4 and image 5 windows bind the inferred LZ grammar on hardware at the comparison [restart point 4](firmware-extraction.md#decoder-research-and-restart-points): 105 to 229 of each window's 256 bytes come from match tokens, each window occurs once in its artifact, and none occurs in the raw payload. The two plain-image windows are uncompressed payload bytes and bear on placement only. Together they turn the "installed correspondence unknown" limitation into evidence at those addresses. The loader records map into RAM from their own start, header included, not from `load+0x20`. The debug RAM read channel returns the true cache bytes: its window 0 equals the config readback and differs from the earlier verified capture only at the byte the session itself set.

What this leaves open: the plain image's loader (its bytes match the payload slice and the table names its base and length, but no code reading that table entry is identified), the writers and meaning of the five table bytes, and every byte of the three images outside the seven windows (those bytes were captured by the 2026-10-08 `dump-ram` sessions, E-DUMP-RAM-150 in [the evidence ledger](../docs/evidence.jsonl)).

### Second session

The reviewed twenty-window session ran at 2026-10-07T19:30:28Z and printed `probe-ram: 20 windows`. All 22 files match the SHA-256 values in the journal, every request completed without refusal, the gate readbacks hold `0x01` then `0x00` at index `0xf7`, C4 returned and all host resources were released. The eighteen files shared with the first session are byte-identical between sessions.

The four added windows establish these mid-session facts:

- Adjustment-cache indices `0x3b9` and `0x3ba` are both `0x00`, so the gated `0x006a5c08` header-byte writer was disabled at probe time. The physical POWER/OFF path does not call that writer; an earlier or indirect invocation remains unproved.
- The live retained header at `0x409c281c` and its snapshot at `0x409c29b8` are identical: `04 0c c3 03 d0 07 01 ff 00 80 00 07 00 00 aa 22`. The marker is `0xaa` and the sixteen-byte sum is zero, so the exact header predicate tested by `0x006a75a4` passes while the maintenance session is running.
- The area-3 live copy at `0x0089f838` and snapshot at `0x0089f874` are byte-identical across all `0x28` bytes. This is a mid-session consistency check, not proof that the area-3 boot read succeeded, because its fallback repairs both copies.
- The discriminator cells at `0x0089f814`, `0x0089f818` and `0x0089f81c` read `0x10`, `0x02` and `0x01`. For input `0x10`, the table row at `0x007cb7ac` selects output 2. The earlier description of outputs 1/2/3 as an alternate boot program that cannot coexist with the service application is wrong: this live service session has output 2 and answered all image-5 requests. Output 2 selects the nonzero return from `0x0069cfa4`; it does not by itself identify the area-2 invalidation.

The owner then completed an orderly ordinary-mode power-off after the expected setup pass. The run did not observe the later ordinary boot that tests area 2. The owner subsequently reported normal-functionality boots, but date, language and C-slot settings have not remained intact after any observed jig power-off, including this cycle (E-RESET-PROBE-POST-BOOT-150).

## What a result means

A match at a window supports installed correspondence at those addresses only; the four table windows cover the region `0x0061c000`-`0x0061c3ff`, and since the boot's copy length to `0x0061c000` is not established, a match there is evidence for the copy rather than proof of its extent. A mismatch is decisive and names the channel, the mapping or the installed build as the open question. This is not a full-image dump: at 256 bytes per request the decoded images are about 43,700 requests, which the host journal design excludes, so no result can prove the whole installed image byte-identical.

## Evidence status and limits

An independent byte review on 2026-10-07 re-derived the handler, the function-2 routine, the config path and every frame from the private 1.50 images and returned pass-with-notes. Its three documentation findings (the readback control bytes, the window-16 comparison offset, the segment-table region wording) and one test note are applied; it confirmed the request and reply offsets, the 248-byte observability, the status tables, the commit-free claim and the address list, and it found no defect in the camera bytes.

The operation is H2: the only direct state change is one calibration-cache byte written to 1 and back to 0 in RAM, with no commit command posted; `probe-ram` never writes index `0x80`. Both runs restored the installed value `0x00` verified by [E-ADJ-GATE-BYTE-150](../docs/evidence.jsonl) rather than re-reading the pre-write byte, so on a body whose index `0xf7` were nonzero a session would close the gate until the next power cycle, and no commit is posted either way. Every probe address rests on the static boot load path, not on an observed mapping, and the operation cannot detect a wrong mapping beyond the cache oracles. The residual exposure is the exit/commit ordering that [SERVICE-STATIC-01](https://github.com/skeet70/latent-image/issues/4) leaves open, and that a power cycle reloads the area from storage is mapped, not observed for this exact sequence. Function 2 takes an arbitrary address with no range check and runs with the scheduler lock held, so the fixed list is an allowlist and the operation cannot name another address. The exit resets date, time, language and the C slot, and the calibration-area capture of 1.00 remains the only body-specific backup ([E-S03-DURABLE-PRESERVATION](../docs/evidence.jsonl)).
