# Reading back the boot card's RAM signature

Answers one question: after a card entry attempt, can the payload's signature at RAM `0x00000100` be read? Host-only work, no card written and no camera or USB device touched. Labels: **byte-read** names ROM, record or payload bytes, **hardware-observed** names an existing reviewed capture of this body, **static-inference** names a deduction, **unproved** names what nothing has established.

## 1. The answer

**The RAM at `0x00000100` does not survive a reboot into ordinary mode: it is overwritten before any reader runs.** The debug read would reach the address, since the same primitive already read the 256 bytes below it, but by the time the read can run the bytes there belong to the next boot's own first-stage content.

The payload writes its signature into its own staged span at `0x40000000`, the alias of RAM `0x00000000`, so the signature shares the address space's first 512 bytes with whatever the next power-up stages there. On this body the next power-up selects mode 4 and its first-stage load writes 256 KiB over RAM `0x00000000`-`0x0003ffff` before the handoff jump. The reader exists only inside a maintenance session under the running application, which the same boot creates afterwards, so the overwrite always precedes it.

## 2. What writes RAM `0x00000100` between the payload and a reader

| Stage | What it writes over the region | Class |
| --- | --- | --- |
| The payload itself | `stmia r0, {r1-r4}` at `0x00000100`, 16 bytes, inside its own sector-rounded staged span at `0x40000000` | byte-read, [card report](diagnostic-sd-boot-card.md) section 4 |
| ROM mode 4, this body's route | Destination `0x40000000`, device select 1, count `[config+0x18] & 0x3ffff` = 2 page groups of 128 KiB, so RAM `[0x00000000, 0x00040000)` is written from media `[0x40000, 0x80000)`; media `0x40000` is the first boot record | byte-read, [window map](bootrom-window.md) section 9 steps 5 and 6; the config word at `payload.bin + 0x60` reads `0x00000002` |
| ROM mode 1, the card's own route, only if entry is found | `ffboot.bin` read in chunks up to `0x8000` to `0x40000000`, and the DMA sector dispatcher uses a full-sector read for destinations in `[0x40000000, 0x80000000)` regardless of the requested sub-sector count, so the first 512 bytes at RAM 0 are written whatever the file length | byte-read, window map sections 3 and 3.3 |
| ROM mode 2, only if entry is found | Raw block reads by DMA to `0x40000000` starting at SD block 2 | byte-read, window map sections 1 and 3.2 |
| Later boot stages and the running application | Not traced, because the first staging already overwrote the address | unproved for the region, moot for the answer |

The consequence for the byte values: after an ordinary boot the byte at `0x00000100` is the first record's byte at that offset, `0xff` in the pinned record (`private-data/analysis/x-m1-1.50/decode/boot-40048.bin` offset `0x100` is sixteen `0xff` bytes), not the payload's signature. Whether some later stage also writes the address is unproved and cannot restore the signature.

The region is not free RAM while the application runs either: the live `0x00000000` window captured in a maintenance boot holds the boot record's vector table with seven rewritten target words, which is the staged record's content, not spare space (hardware-observed, E-BOOTROM-EXTENT-150, window `00000000.bin`, SHA-256 `ceb2d5dc1c5431ead2e60c7d6abf4da496fd847e771ac21cbcd0f16878050ea7`).

The write is also not conditional on which outcome the attempt had: a failed entry boots normally and stages as above, and a successful entry halts with the signature in place but with no route to a reader. The one exit from a halted body is a battery pull, which removes power from DRAM and leaves retention unestablished for every address.

## 3. The reader and its prerequisites

The body's read primitive is opcode `0x200001` function 2, address at `+0x1c` and length at `+0x20`, acting only while adjustment index `0xf7` is nonzero ([transport README](../transport/README.md)). It read RAM `0x00000000`-`0x000000ff` in the ordinary boot of session `20261008T035458Z-probe-bootrom`, so the window at `0x00000100` is one more entry in a fixed list and needs no new capability, only a code change to the operation's window list (hardware-observed for the primitive and the page below it; static-inference for the extension to `0x100`).

Its prerequisite is the running application: the session runs under maintenance (jig) mode, which is an application surface, and the payload's halt is `wfi` with IRQ and FIQ masked, so a halted body serves no USB and no session (byte-read, card report section 4; the read path requires the running application, card report section 5).

## 4. What an observable payload would need

The ROM provides no write path to any medium. Its SD code is initialization plus reads: the command table at `0x800022e0`-`0x8000232f` carries CMD0, CMD8, CMD55, ACMD41, CMD2, CMD3, CMD7, CMD16, CMD55 and ACMD6, the block layer reads with `0x113a` at `base+0x0e`, and no data-write command appears in the 0x4000 window. Its internal-media code is the init `0x80000b74`, the page select `0x80000c2c`, the block read `0x80000d08` and the paged loader `0x80000dec`, all reads, whose failure paths write only MMIO `+0xe8` = `0x00030003`. The ROM's other writes are the config apply's register stores, the SRAM block, the halt and the handoff stub (byte-read, window map sections 1, 3, 8 and 9).

So a payload that leaves a trace must carry its own write engine and choose a target that survives both the next boot and the power removal, and the host must be able to read it.

| Channel | Survives power removal | Readable without the camera | What it needs |
| --- | --- | --- | --- |
| Card report sector, LBA 34816, outside the MBR partition | yes, plus card removal | yes, an ordinary card reader | A project-owned CMD24 helper: guard on `0xfff00434+0x10` = 1 and the CCS flag, the constant LBA, the argument scaling `lba << 9` or `lba`, the write command's two halfwords, the data-phase FIFO direction and CRC policy, busy and completion handling, a bounded wait with its own counter and distinct failure codes, one write with no retry, a bounded read-back of the sector, and the full MMIO write audit ([card report](diagnostic-sd-boot-card.md) section 6). The command words, FIFO direction and CRC policy are unproved; the ROM's own wait has no iteration bound. Only a correctly addressed write leaves the filesystem untouched, so a wrong argument or addressing-unit bug can reach the FAT, the root directory or the two files. New write-capable camera-path code needs one independent review and the owner's go |
| Any other RAM address | unproved | only through the camera, with the application running | An address whose no-writer argument holds from reset through the read, plus retention evidence across the battery pull. `0x00000100` has a refuted argument, and no address has retention evidence |
| An indicator register | not applicable | visible to an observer | An established register-to-visible-indicator binding. `0xfff88084` bit 0 is written by the ROM's init, config and failure paths, and no visible effect is bound to it (unproved) |
| Internal media | yes, and persistently | through a service read or the update path | A program sequence, which the ROM does not contain at all, so the payload would carry a write half derived from the application's writer ([UPDATE-01](update-validation-and-boot.md)). A persistent media change at the first write risk on the body |

The RAM channel is the one the current payload uses, and it is the one that cannot work: the failure is the address, not the halt. Writing the record somewhere the next boot does not touch and then halting, or not halting, both still need the missing ownership argument, and neither helps while the only reader needs the application.

## 5. What would change the answer

- A write path to the card in the payload, which needs no new address and is the design the reserved report sector exists for. The acceptance evidence for a future run is then the report sector's bytes read on a host card reader, not a camera readback. [The card-entry review](sd-boot-card-entry.md) section 4 specifies that reporting payload, its record fields and the outcome table: a valid record proves entry and execution, a normal boot proves refusal, and a dark body without a record stays inconclusive; the write-path blockers are named there and unchanged.
- A RAM address with a writer census showing no writer from reset through the read and an argument that its contents survive a battery pull.
- Any second channel out of a halted body. None is established.

## 6. Reproduction

- The record byte at the signature's future address: `od -A x -t x1 -j 0x100 -N 16 private-data/analysis/x-m1-1.50/decode/boot-40048.bin` reads sixteen `ff` bytes; the same offset in the container is `od -A x -t x1 -j 0x40148 -N 16 private-data/analysis/x-m1-1.50/payload.bin`.
- The load count: `od -A x -t x4 -j 0x48 -N 32 private-data/analysis/x-m1-1.50/payload.bin` shows `+0x18` = `0x00000002`.
- The live low RAM: `sha256sum private-data/sessions/20261008T035458Z-probe-bootrom/windows/00000000.bin` must read `ceb2d5dc1c5431ead2e60c7d6abf4da496fd847e771ac21cbcd0f16878050ea7`.
- The card itself: `devenv shell -- bazel test //host/firmware:boot-card-tests --test_output=errors`, and the `build` and `verify` subcommands of `//host/firmware:build-boot-card` ([reproduction](diagnostic-sd-boot-card.md#reproduction)).

Related records: E-SDBOOT-CARD-IMAGE-150 for the built image, E-SDBOOT-RAM-SIGNATURE-150 for this verdict, E-BOOTROM-EXTENT-150 for the low-RAM capture, E-SDBOOT-CARD-BUILDABLE-150 for the construction verdict, and [the recovery status](recovery-status.md) for the entry gate that stands.
