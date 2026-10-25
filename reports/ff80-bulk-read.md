# FF80-BULK-READ-01: review request and camera-run proposal for `read-bulk`

[FF80-BULK-READ-01](https://github.com/skeet70/latent-image/issues/99) adds `read-bulk ADDRESS LENGTH` to `//transport:camera`: one DRAM window read through the stock ff80 bulk path of [H2 report section 2.5](h2-read-scope-proposal.md#25-stock-bulk-read-opcode-0x450006-tag-0x80-and-transport-command-e1). The operation, its bytes and its stops are in [the transport README](../transport/README.md). This report asks for its independent review and proposes its first camera runs under [the camera-run procedure](../spec/07-governance-and-handoff.md#31-procedure). Nothing here has been sent to the body.

## 1. Class and gate

The operation sends no config write, RAM write or gate write. Its effect on the camera is service-session state that stock handlers own: the tag-`0x80` query stores the source pair at `0x01469878`/`0x0146987c`, E1 stores the offset, length and flag at `0x01469880`, `0x01469868` and `0x01469c9c`, and each DMA completion posts transport event `0x340e` and an interrupt-IN status (E-BULK-READ-CONSUMER-150). By camera effect that is H1, a read or query. Two things set it apart from the reads the standing grant covers: its bytes are new mechanism, and the bulk-IN DMA it starts has been exercised on the body only for transfers of 292 bytes and less (D1 replies, E-ADJ-READ-150, E-DUMP-RAM-150), never for 64 KiB or 1 MiB. So the first live run waits for one independent review, as for new H2 mechanism bytes, and for the owner's go; the [read grant](../spec/07-governance-and-handoff.md#34-standing-confirmations) names `read-ram` and `read-config` only and does not cover it.

## 2. Review request

Reviewer: `openai-codex/gpt-6.1-sol:high` or `anthropic/claude-opus-5-5:high`, not the author (BulkRead). Scope: the commit on `agent/BulkRead` that adds `read-bulk`, with these files:

- `transport/FujiAF/ReadBulk.hs`: the frames (32-byte query, 36-byte function 2 read of `0x01469878`, E1 control data), the window rules, the reply decoders, the order of the session and the stop and cleanup paths.
- `transport/FujiAF/Transport.hs` and `transport/native_usb.c`: `supportedTransfer` and the shim now admit a bulk IN of up to `0x100000` bytes; control writes and bulk OUT keep the 292-byte limit. This applies to every operation, so the reviewer should confirm no other operation can now emit a bulk IN above 292 bytes.
- `transport/FujiAF/Native.hs`: the bulk-IN buffer is allocated rather than built from a list.
- `transport/Main.hs`: the CLI entry and the window check before the output directory exists.
- `transport/ReadBulkTest.hs`, `transport/ProviderParityTest.hs`, `transport/cli-rejections.sh` and the bulk camera in `transport/FujiAF/Rehearsal.hs`.

Questions for the reviewer:

1. Bytes: does every frame the operation can emit match the static map (E-BULK-READ-PATH-150, E-BULK-READ-CONSUMER-150)? In particular E1 byte 1 must be zero (bits 7:5 of that byte select a reject at `0x00d7f234`), and the offset, length and session are big-endian at `+2`, `+6` and `+10`.
2. Window: is `[0x00000000,0x20000000)` and `[0x40000000,0x60000000)`, 4-byte aligned address and a 64-byte multiple length, the right bound for a first release? The DMA reaches `((0x4b2ece00 + offset) mod 2^32) & 0x3ffffffc` and rounds its length up to 64, so this shape fetches nothing outside the window and never reaches ROM or `0xfff` MMIO. Populated DRAM size is unknown.
3. Pin: is the function 2 read of the published pair after the query sufficient to rule out computing offsets against a stale or different pointer, given that `(1,0xffff)` leaves an earlier pair in place?
4. Journal: the E1 frames are written to `outgoing.v1` and fsynced in one batch before the first E1, as `dump-ram` does; chunk replies are hashed, not journaled as hex. Is that acceptable for 1 MiB replies?
5. Static claims: re-derive from bytes at least the DMA masks `0x00d7c858`-`0x00d7c92c`, the cache clean `0x00d7bfc4`-`0x00d7c0f4`, the sum `0x00d7f01c`-`0x00d7f044`, the installer `0x00d82478` and its mode-3 caller, and the MPU setup in `boot-40048` `0x19e4`-`0x1a9c`. That MPU decode reprograms region 0, which the wording of E-XM1-MPU-REGIONS-150 does not.

Host checks the reviewer can run: `//transport:read-bulk-tests`, `//transport:provider-parity`, `//transport:cli-rejections`, `bazel run //transport:camera -- rehearse read-bulk 0x00c00000 0x10000`, and `//emulation:xm1-bulk-read-e2e` with the arguments of E-BULK-READ-E2E-150.

### 2.1 Review outcome

BulkReadReview (anthropic/claude-opus-5-5, not the author) reviewed `992102f` and approved with two must-fix findings, F1 and F2, and five recommendations, F3 to F7; it re-derived every static claim above from our decodes. It then confirmed every fix at `8cc0b96`, killing mutants M3, M5, M7, M9 to M13 with the tests and rerunning `//transport/...` uncached (28 of 28), and gave the final verdict **APPROVE** (E-BULK-READ-REVIEW-150; `private-data/analysis/x-m1-1.50/bulk-read/review.md`, SHA-256 `fdd1f1de4d60b54509d557cb2160fa0090d787e50db130f53630d63cdc44f504`). Its scan of the replies of all five 2026-10-09 read-media journals confirms the highest byte read from the body is physical `0x0b38cdff`. The operation now waits only for the owner's go for run 1 (section 4). The fixes:

- F1: a bulk IN above 292 bytes journals its `native-outcome` line as status, count and the SHA-256 of the partial bytes, so a short or failed chunk's line fits the journal cap; `//transport:read-bulk-tests` asserts the line after a short chunk (count `0xfffc0`) and after a provider failure (status -7, count 30000).
- F2: the rate gate and the stop recovery in section 4.
- F3: windows whose physical end exceeds `0x0b3f6e00` are refused (E-BULK-READ-BOUND-150).
- F4: `txWithin` refuses a bulk IN above 292 bytes before the provider; only the read-bulk chunk path, which calls `dispatchTransfer` directly, admits up to `0x100000`.
- F5: tests for a chunk past the 5000 ms budget, a `(1,0xffff)` or `(9,1)` query over an earlier published pair and a nonzero reply word at `+0x0c`, plus `fuji_usb_transfer_size_admitted`, the shim's per-kind size gate, called from `//transport:native-usb-tests`.
- F6 and F7: the zero-length-packet outcome, the check plan and run 2's acceptance in section 4.

## 3. Emulator evidence

`//emulation:xm1-bulk-read-e2e` ran the real `runReadBulk` and `runReadRam` against the emulated X-M1 1.50 through the stock guest USB task, controller and DMA path in the ordinary-boot ff80 attach with stored `0xf7 = 1` (E-BULK-READ-E2E-150). The guest programmed one 1 MiB DMA per E1 with source `0x00bf1a00` for address `0x00bf1a00` and with the same physical source for the alias `0x40bf1a00`. The 4 MiB capture, the 128-window D1 capture and the 1 MiB alias capture equal the emulated memory read afterwards; the 4 MiB capture differs from the load-time decode of image 5 in 9 bytes of writable words the running image has changed. In this emulator the bulk path moved 88.7 times as many bytes per guest instruction and 86 times as many per bridge second as the D1 reader. The bridge paces each 64-byte IN packet at 20,000 instructions, so neither figure predicts the body's rate.

## 4. Proposed camera runs

Preconditions: the review in section 2 approves the commit with no open finding; the owner gives a go naming `read-bulk` and the two windows below; the body is attached in an ff80 session with `0xf7` nonzero (an ordinary boot with stored `0xf7 = 0x01`, E-STORED-F7-REENABLE-RUN-150, or a jig boot) and idle at the shooting screen with no capture, live-view change, playback or card activity; `rehearse read-bulk` for each window prints the frames below with the synthetic session `0x1234`.

Run 1, one 64 KiB chunk of image-5 code:

```sh
devenv shell -- bazel run //transport:camera -- rehearse read-bulk 0x00c00000 0x10000
devenv shell -- bazel run //transport:camera -- read-bulk 0x00c00000 0x10000
devenv shell -- bazel run //transport:camera -- read-ram "$PWD/private-data/read-plans/read-bulk-check.json"
```

The E1 frame is `e1 00 b5 91 32 00 00 01 00 00 S_hi S_lo`. `private-data/read-plans/read-bulk-check.json` (SHA-256 `a2998a10e0e6adbefd0aa32c5aa2e6d53f2174e07e171d9e4c3762bb050af9a4`, rehearsed: 3 read requests, no write) lists three 248-byte windows at `0x00c00000`, `0x00c08000` and `0x00c0ff08`; `read-ram` is covered by the standing grant and its bytes are compared offline with the same offsets of the `read-bulk` capture.

Run 1 passes when the session completes, the three `read-ram` windows equal the capture at their offsets and the result line's `bytes-per-s` is at least 840,000: 1 MiB in at most 1250 ms, a quarter of the per-transfer budget, measured with E1 and the capture write included, so it under-reads the DMA rate. Below that, or on any other run-1 outcome, run 2 is not sent and the result is reported.

Run 2, after run 1 passes, 4 MiB of image 5 as in the emulator run:

```sh
devenv shell -- bazel run //transport:camera -- read-bulk 0x00bf1a00 0x400000
```

Its E1 frames are `e1 00 b5 90 4c 00 00 10 00 00 S_hi S_lo`, `e1 00 b5 a0 4c 00 ...`, `e1 00 b5 b0 4c 00 ...` and `e1 00 b5 c0 4c 00 ...`, each with length `0x00100000`. Run 2 is accepted when the session completes, its bytes at `[0x00c00000,0x00c10000)` equal run 1's capture and the `read-ram` windows, and every difference from the installed image-5 decode lies in a word the running image writes; the emulator shows 9 such bytes in this window, at `0x00d0cd9c`, `0x00d3d53c`, `0x00d7ee66`-`0x00d7ee76` and `0x00dca960` (E-BULK-READ-E2E-150). Its `bytes-per-s` is the first 1 MiB-chunk rate on this body.

A zero-length packet after a 64 KiB or 1 MiB transfer is a possible outcome: every bulk IN the body has answered so far is shorter than one high-speed packet, and whether the controller ends a 512-multiple transfer with one is unobserved. It would show as a zero-count C4 reply or a short next chunk; either stops the run, and it is reported as that observation, not as a camera fault.

Stops: any refusal, uncertain cleanup, disconnect, short chunk or reply other than the documented ones ends the runs, and camera runs pause until the owner has been told. After any stop inside the chunk loop (a transfer timeout, a short chunk, a disconnect or an uncertain cleanup) the endpoint may still hold undelivered chunk data, so no camera command of any kind, `read-ram` under the standing grant included, is sent until the owner has unplugged USB, power-cycled the body and confirmed ordinary operation. Neither run writes, so ending it needs no supply cut; the owner confirms ordinary operation afterwards (shooting screen, one shot and playback).

Not proposed: a whole-DRAM capture, and any window in the staging pool while the camera may be writing it. The tool refuses windows whose physical end exceeds `0x0b3f6e00`, the end of the stock staging window; the highest bytes read from the body so far end at physical `0x0b38cdff` (E-BULK-READ-BOUND-150).

## 5. Run outcomes

Run 1 passes (E-BULK-READ-RUN1-150). On 2026-10-24 the owner replied "Alright, go for that." to accept the proposed go "go read-bulk 0x00c00000 0x10000, run 1, cardless exit check", for an ordinary boot with no card, USB attached at the shooting screen, and the body enumerated as `04cb:ff80`. The rehearsal prints the E1 frame `e1 00 b5 91 32 00 00 01 00 00 12 34`. Session `20261025T042651Z-read-bulk` returns 1 chunk of 65,536 bytes, SHA-256 `80b00c2c6825e6396ed5b0f83306ffee467a0139a26f280ab1495d2488a8af6b`, `bulk-ms=31`, `bytes-per-s=2110022`, which is 2.5 times the 840,000 threshold. No short chunk or zero-length packet occurs. The `read-ram` check `20261025T042655Z-read-ram` returns its 3 windows, and each equals the capture at `0x00c00000`, `0x00c08000` and `0x00c0ff08`. The capture equals the installed image-5 decode (`id5-00bf1a00.bin`, base `0x00bf1a00`) over the whole 64 KiB with no differing byte.

The exit check is cardless because the card slot is broken, an owner-approved bounded exception that replaces the one shot and playback of section 4 with a shooting-screen check. The owner confirms it: "Yes, it's at a normal shooting screen and still works." That closes the exception for run 1.

Run 2 passes every acceptance condition of section 4 (E-BULK-READ-RUN2-150). On 2026-10-25 the owner gave the go "go read-bulk 0x00bf1a00 0x400000, run 2" with the body attached and enumerated as `04cb:ff80` at 480 Mb/s. The rehearsal prints the four E1 frames of section 4 with session `0x1234`, and the live session `0x1313` sends the same frames ending `13 13`. Session `20261025T043843Z-read-bulk` returns 4 chunks of 1,048,576 bytes, SHA-256 `7cc923eacaab45d0f97400c01e62dc96ec51aef9781d7115c6126ca2e102dc42`, `bulk-ms=398`, `bytes-per-s=10516606`. The chunks take 98.2, 100.1, 95.9 and 100.8 ms, 10.4 to 10.9 million bytes per second each, E1 and journal writes included. No short chunk or zero-length packet occurs, so a 1 MiB transfer on this body ends without one.

The capture at `[0x00c00000,0x00c10000)` equals run 1's capture and its three `read-ram` windows. Against the image-5 decode it differs in 3 bytes, `0x00d0cd9c`, `0x00d3d53c` and `0x00dca960`, each `0x00` on the body as in the emulator. The emulator's other 6 differing bytes, `0x00d7ee66`-`0x00d7ee76`, equal the decode on the body. They lie in the endpoint descriptors of bulk OUT `0x01`, bulk IN `0x82` and interrupt IN `0x83`, which hold the high-speed wMaxPacketSize `0x0200` and bInterval 4 on the body and full-speed `0x0040` and bInterval 1 in the emulator, whose bridge moves 64-byte packets. That the image rewrites these words by bus speed is inferred from the bytes; no write to them is traced.

Run 2's exit check is cardless under the same exception. Asked "is the body at a normal shooting screen and still working?", the owner answers "yes". That closes the exception for run 2.

## 6. Proposed run 3: low-DRAM snapshot at the idle shooting screen

Run 3 captures DRAM below the staging pool in one session with the body idle at the shooting screen. The snapshot is for offline comparison of live RAM with the emulators' state at the same screen (#51, #53) and for runtime-state questions (#28) that would otherwise each need their own reads. It is not atomic: the CPUs keep running during the transfer, and chunks covering the live-view buffers hold whatever frame was there when each chunk was read.

Class and review: H1 under [spec 07 section 3.2](../spec/07-governance-and-handoff.md#32-operation-classes). Run 3 sends the same closed operation, frame layouts, window rules and stops that BulkReadReview approved (E-BULK-READ-REVIEW-150); only the address and length inputs change, and the reviewed window check admits them. Section 3.2 asks for an independent review of new mechanism bytes, and a new window adds none, so run 3 needs no new review. The [read grant](../spec/07-governance-and-handoff.md#34-standing-confirmations) covers `read-ram` and `read-config` only, so run 3 needs the owner's go.

### 6.1 DRAM coverage below `0x0b2ece00`

The MPU's region 0 `[0,0x40000000)` is a window, not a DRAM size (E-XM1-MPU-BOOT-RECORD-150), and the emulator's 512 MiB is borrowed backing; its board file says "physical DRAM population unmeasured". What shows a physical range is DRAM is a read from the body or the firmware's own use of it on the body:

| Physical range | Evidence |
| --- | --- |
| `[0x00000000,0x00000100)` | Hardware-read: the live vector table of the boot record the ROM stages at 0, byte-identical in two captures (E-BOOTROM-EXTENT-150, E-BOOTROM-DISCRIMINATOR-150); the second record is staged at `0x10000` (E-BOOTROM-MODE4-150). |
| Loader A at `0x00040000`, the log store `[0x000a0800,0x000c07f4)`, loader B at `0x000d0000` | Hardware-read (E-RAM-PROBE-150, E-DUMP-RAM-150, the read-log sessions of [firmware-extraction.md](firmware-extraction.md#the-whole-store)). |
| `[0x0061c000,0x01460000)` | Hardware-read: the segment table and the image 4, image 5 and plain-image extents (E-DUMP-RAM-150); the RAM-push allocations near `0x00a8xxxx` were written and read on the body. Image 4's bss fill `[0x0089ec00,0x00bf1a00)` lies between them. |
| `[0x01460000,0x03360000)` | Hardware-read at `0x015c22a0`, `0x01667000`, `0x01677700` and `0x01acd800`. Firmware use: the face workspace at `0x021a1800`-`0x021afc00` (E-FACE-ADDRESS-WORDS-150), the load-path graphics fills `[0x0228c800,0x027b3200)` (emulated, [classic-chrome-patch.md](classic-chrome-patch.md)) and the updater's main-range staging from `0x02e4025c` or `0x02fb7600` (E-UPDATE-WRITE-ORDER-150). |
| `[0x03360000,0x05aa0000)` | Firmware use on every ff80 attach in an ordinary boot: the mode-3 start fills `0x43360000`-`0x45a9ffff` with `0xffffffff`, and the range holds the displayed live-view buffer and the display module's windows `[0x03700000,0x04a00000)` (E-STORED-F7-ORDINARY-READ-150, static; E-STORED-F7-ATTACH-EMU-150, emulated). The attaches of runs 1 and 2 took that path, and the body was at a working shooting screen after each. |
| `[0x05aa0000,0x0b2ece00)`, 88.3 MiB | No read from the body and no firmware use traced on the body. Its only lead is the DCAA object table at `0x06b5612c`, a static result (E-AUTOACT-150). |

Verdict: `[0x05aa0000,0x0b2ece00)` is the sub-range the evidence does not show is DRAM. Both of its edges are DRAM, so it is DRAM if the controller decodes populated memory contiguously up to `0x0b3f6e00`; that is an inference about the memory controller, not X-M1 evidence, and a DMA from an unpopulated address has unknown bus behaviour. Run 3 therefore reads `[0x00000000,0x05aa0000)` and leaves that span out. The unread stretches inside `[0,0x05aa0000)` lie between ranges listed above. The span becomes readable when a firmware buffer in it is traced statically and bound to ordinary operation, or when the owner accepts the contiguity inference for it.

The tool admits both the proposed window and the full one: `rehearse read-bulk 0x00000000 0x0b2ece00` prints 179 E1 frames, the first `e1 00 b4 d1 32 00 00 10 00 00 12 34` and the last `e1 00 bf f1 32 00 00 0e ce 00 12 34`, 187,616,768 bytes.

### 6.2 Run

Camera state: an ordinary boot with no card and stored `0xf7 = 0x01`, USB attached at the shooting screen, the body enumerated as `04cb:ff80`, idle with no menu, capture, live-view change or playback.

```sh
devenv shell -- bazel run //transport:camera -- rehearse read-bulk 0x00000000 0x05aa0000
devenv shell -- bazel run //transport:camera -- read-bulk 0x00000000 0x05aa0000
```

The rehearsal prints 91 E1 frames with the synthetic session `0x1234`: the first `e1 00 b4 d1 32 00 00 10 00 00 12 34` and the last `e1 00 ba 71 32 00 00 0a 00 00 12 34`, 90 chunks of `0x100000` and one of `0xa0000`, 95,027,200 bytes. Every offset wraps through `0x4b2ece00 + offset` as run 1's did. At run 2's 10.5 MB/s the transfer takes about 9 s. The capture is `ram-00000000-05aa0000.bin` in the new session directory. Host disk: `df -h /var/home` reports 61 GB available on 2026-10-25; the full window would need 179 MiB.

Stops: those of section 4, unchanged. Any refusal, uncertain cleanup, disconnect, short chunk or reply other than the documented ones ends the run, and camera runs pause until the owner has been told. After a stop inside the chunk loop no camera command of any kind is sent until the owner has unplugged USB, power-cycled the body and confirmed ordinary operation.

Acceptance: the session completes with 91 chunks of the sizes above, and the capture's bytes at `[0x00c00000,0x00c10000)` equal run 1's capture (SHA-256 `80b00c2c…af6b`). The result line's `bytes-per-s` is reported.

Exit check: cardless, as for runs 1 and 2. The model asks the owner whether the body is at a normal shooting screen and still working, and the owner's confirmation closes the exception for run 3.

### 6.3 Comparison after the run

With `S` naming the capture, this lists every byte that differs from the image-4 and image-5 decodes over their load ranges as address, body byte and decode byte:

```sh
S=private-data/sessions/SESSION-read-bulk/ram-00000000-05aa0000.bin
D=private-data/analysis/x-m1-1.50/lz
for img in id4-0061c400:0x0061c400:0x282800 id5-00bf1a00:0x00bf1a00:0x4f1e60; do
  IFS=: read -r name load size <<<"$img"
  cmp -l --ignore-initial=$((load)):0 --bytes=$((size)) "$S" "$D/$name.bin" |
    gawk -v base=$((load)) '{printf "0x%08x %02x %02x\n", base+$1-1, strtonum("0"$2), strtonum("0"$3)}' > "${S%.bin}-vs-$name.txt"
done
```

On run 2's capture, with its base subtracted from `--ignore-initial`, the same command gives the three image-5 bytes of section 5. The maintenance-boot baseline is E-DUMP-RAM-150: 542 differing bytes in image 4 and 233 in image 5.
