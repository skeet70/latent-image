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
