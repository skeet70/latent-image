# X-M1 1.50 live-RAM calibration

## Scope and result

E-BULK-READ-RUN3-150 is the hardware reference: physical `[0x00000000,0x05aa0000)`, 95,027,200 bytes, ordinary boot, RC0 application, stored `0xf7 = 1`, no card, S shooting screen, then ff80 attach. The capture is not atomic. Its source and authorization remain in [the bulk-read report](ff80-bulk-read.md#64-run-3-outcome).

Both engines cold-boot the clean RC0 DAT `update/prefix/FWUP0002-prefix.DAT` with `stored-f7/emulation/inputs/f7-1.nand`, no card, through their existing cardless bridge. Both render S, shutter 60, ISO 200, with current menu `(67,56,0)`, and enumerate ff80 before the dump. No RAM allocation, payload upload, physical camera operation or USB access occurs. The Haskell driver takes an emulator slot and reads paused memory in 64 KiB bridge requests. It does not replay the body's bulk-read traffic or its 16-second capture interval.

The retained dump differs from the body in **26,706,012 bytes**, FujiMod in **25,768,069**. These totals do not rank engine accuracy: their synthetic scene sources differ, and neither input NAND contains the owner's settings/calibration. The complete segment-table window matches on both. Image 4 differs in 80 bytes/44 words and image 5 in 7 bytes/5 words on each engine. All 846 plain-image differences are the recorder rev-2 remainder described below, not an engine defect.

No deterministic device-model defect is established by this comparison. No device model was changed, and `//emulation:verify-candidate` was not run. Unresolved bytes are retained explicitly rather than declared timing noise or model defects. This does not close #51, #53, #28 or a recovery gate.

## Regional headline numbers

Ranges are physical and half-open. Counts are differing bytes against the body, not differing bytes between engines. The rows partition the complete capture; task stacks below are an overlapping diagnostic view. The broad workspace rows describe address coverage, not a proof that every byte is allocated or initialized.

| Region | Range | Bytes | Retained differences | FujiMod differences |
| --- | --- | ---: | ---: | ---: |
| Low boot/scratch | `0x00000000..0x00040000` | 262,144 | 11 | 11 |
| Kernel A slot | `0x00040000..0x00060000` | 131,072 | 316 | 346 |
| Kernel A runtime/log | `0x00060000..0x000d0000` | 458,752 | 204,982 | 204,977 |
| Kernel B slot | `0x000d0000..0x000f0000` | 131,072 | 30 | 26 |
| Kernel B runtime | `0x000f0000..0x00130000` | 262,144 | 117,743 | 117,743 |
| Low resources/workspace | `0x00130000..0x0061c000` | 5,160,960 | 1,077,431 | 1,077,431 |
| Segment-table window | `0x0061c000..0x0061c400` | 1,024 | 0 | 0 |
| Image 4 | `0x0061c400..0x0089ec00` | 2,631,680 | 80 | 80 |
| Image-4 BSS/static stacks | `0x0089ec00..0x00a71a00` | 1,912,320 | 178,922 | 178,869 |
| Libc heap reservation | `0x00a71a00..0x00bf1a00` | 1,572,864 | 51,957 | 51,957 |
| Image 5 | `0x00bf1a00..0x010e3860` | 5,185,120 | 7 | 7 |
| Gap below plain image | `0x010e3860..0x01100000` | 116,640 | 116,617 | 116,617 |
| Plain image | `0x01100000..0x01460000` | 3,538,944 | 846 | 846 |
| Application heaps/stacks/workspaces | `0x01460000..0x021a1800` | 13,899,776 | 9,308,257 | 9,309,074 |
| Face workspace | `0x021a1800..0x021afc00` | 58,368 | 58,368 | 58,368 |
| Graphics/image workspace | `0x021afc00..0x03360000` | 18,547,712 | 12,604,585 | 12,269,621 |
| USB-fill range before display | `0x03360000..0x03700000` | 3,801,088 | 2,841,583 | 2,267,350 |
| Display/live-view windows | `0x03700000..0x04a00000` | 19,922,944 | 144,277 | 114,746 |
| USB-fill range after display | `0x04a00000..0x05aa0000` | 17,432,576 | 0 | 0 |
| **Total** | `0x00000000..0x05aa0000` | **95,027,200** | **26,706,012** | **25,768,069** |

The final 17,432,576-byte range is entirely `0xff` in all three dumps. The display window has 19,704,352 body bytes and 19,704,384 emulator bytes equal to `0xff`. The earlier fill range has subsequent image traffic: it is not expected to remain uniformly filled. Body frame data, the retained color-bar/gradient source and FujiMod's magenta source are different inputs.

The task-table decoder uses the retained X-M1 kernel contract: pointer/count cells `0x00054610` and `0x00054694`, TCB stride `0x38`, descriptor pointer at `+8`, descriptor entry `+8`, stack length/base `+0x10/+0x14`, task id `+0x20`. Null-entry placeholders are excluded. Each dump has **84 real task descriptors**, with the same core/id/entry/stack-length associations. Four 1 KiB stacks are permuted among task ids 102/104 on CPU0 and 93/103 on CPU1. This is consistent with cross-core allocation order, not proof of a scheduler defect. Body-derived distinct stack extents total **292,864 bytes**; 14,375 differ against retained and 14,213 against FujiMod. The tool compares the same physical extents, not relocated logical stack frames.

The libc heap starts at `0x00a71a00` ([heap initialization](filmsim-patch-01.md)); the body heap break at `0x00829a08` is `0x00a92000`, against `0x00a83000` on each emulator. The difference is `0xf000`. Neither equal heap breaks nor identical live allocation order follow from equal firmware when settings, calibration, lens/device inputs and elapsed time differ.

## Difference classification and device responsibility

The tool reports evidence-backed narrow classes and leaves the remainder unresolved. The `TimingDependent` stack class means a timing-dependent candidate; it is not a traced causal attribution for every stack byte. The classification rules are specific to this Run-3 comparison.

| Class | Retained bytes | FujiMod bytes | Evidence and scope |
| --- | ---: | ---: | --- |
| Timing-dependent candidates | 14,376 | 14,214 | Distinct body task-stack extents plus the task-id allocator counter at `0x00828094` |
| Configuration | 859 | 859 | 846 recorder-remainder bytes plus 13 calibration-interval bytes in the 36-row control table |
| Confirmed model defects | 0 | 0 | No deterministic disagreement isolated from inputs/timing |
| Unresolved | 26,690,777 | 25,752,996 | Includes initialized objects, allocation layout, scene data, logs and uninitialized/residual memory |

- **Kernel counters, waits and stacks:** timer/interrupt scheduling and cross-core ordering are responsible model boundaries, not demonstrated defects. The `0x00828094` firmware path at `0x0069cc18..0x0069cc68` consumes a task id, increments it and wraps after 104; body 7 versus emulator 63 is a counter, not a hardware-status disagreement. Kernel A's first differing word is `0x0004dad4`, body `0x49` versus retained `0x21`, not a blanket code mismatch.
- **Settings/calibration/input configuration:** the NAND inputs are synthetic templates with stored `0xf7 = 1`, not the owner's journals. The calibration interval words at control-row `+0x10`, table `0x00828120`, stride 28, differ. These concern NAND adjustment input and ADC/control input policy, not evidence that either NAND controller derives a wrong value. Image-5 differences are restricted to `0x00d7ee64/68/6c/74` and `0x010e3854`; their causes remain unbound.
- **Source/image/display buffers:** image DMA, source cadence, bank rotation and display conversion are the responsible device-model families. Different scene sources and nonaligned frames prevent a body-byte equality claim. The exact all-`0xff` suffix supports the observed fill result only, not DMA timing or pixel fidelity.
- **Untouched/residual DRAM:** the gap below plain is almost all `0xff` on the body and zero on the emulators; the face workspace is `0xff` in 58,367 of 58,368 body bytes and entirely zero on both engines. DRAM initialization/residue policy is the first suspect, but a snapshot does not identify the last writer. Setting all emulator RAM to `0xff` would not be an evidence-backed fix.
- **Heap layout and other object fields:** allocator results, configuration-derived sizes, lens/MCU input and scheduler history can all contribute. Their last writers and device responses are not recovered by a final snapshot. They remain candidates under #51/#53, not a fabricated model-defect class.

No defect issue is filed as though one of these hypotheses were proved. The calibration result and unresolved families are recorded on #51; #53 remains the timing/input measurement gate. Replacing a declared model law requires a value derived deterministically under matched inputs, not a lower aggregate difference count.

## Recorder rev-2 remainder after rollback

**Hardware-observed, E-RUN3-RECORDER-RESIDUE-150:** the body's complete `[0x0144a5e0,0x0144ad48)` span is byte-identical to the 1,896-byte emitted rev-2 recorder blob, SHA-256 `3badfd4640bce898fdbf634a047e08deccc430aa721615f5e7a8f193f6d0fd03`. This includes its zero-initialized state and trailer. It accounts for all 846 differences from the clean RC0 plain image, exactly the changed-byte count retained in the private rev-2 emitter record. The [recorder layout](af-observe-instruments-150.md#99-recorder-revision-2-bounded-scan-attempt-and-result-coverage-moved-placement) binds its separate plain-image placement. The installed application is RC0 after the block-14 repair; that did not replace the recorder's separate plain-image bytes. Persistence in installed flash is an inference from ordinary boot and the known plain-image load, not a new flash readback.

The comparison command returned zero with no output:

```sh
cmp --bytes=1896 --ignore-initial=21276128:0 \
  private-data/sessions/20261025T050355Z-read-bulk/ram-00000000-05aa0000.bin \
  private-data/analysis/x-m1-1.50/af-recorder/rev-2/af-recorder.bin
```

**Static analysis of captured bytes, E-RUN3-RECORDER-INERT-150:** the remainder is inert at the captured stock hook routes. The media receive hook at `0x0071feb4` holds stock `0xebffffdc`, `bl 0x0071fe2c`, not rev-2 `0xeb34a9c9`. The later post-id-6 activation site at `0x0073b564` holds stock `0xebffa12a`, `bl 0x00723a14`, not rev-3 `0xeb343d0d`. The old prefix location `0x0061c1fc` is zero, as is `0x0061c340`. The recorder's state/trailer remain identical to the never-run emitted initialization.

The Haskell static scan covers aligned A32 B/BL candidates in captured image 4 including its prefix, image 5 and plain, and matching-condition/register MOVW/MOVT constructor candidates within 64 bytes. It finds **zero incoming branch candidates and zero external constructor candidates** into the recorder span. The 28 branch hits and six constructor hits are all inside the recorder itself.

The raw pointer-pattern scan covers every byte offset of all 95,027,200 captured bytes, admitting only the physical and `0x40000000` DRAM aliases. It finds 176 numerical patterns, against 174 in the clean-RC0 emulated control. Image 4 and image 5 contain none; plain has two unchanged control patterns, at `0x011fb5ef` and `0x012b908c`, targeting `0x0144aaeb` and `0x0144ad23`. Their presence in clean stock data is not a recorder callback publication. The only body-only patterns are unaligned crossings at `0x00a66b0f` and `0x00a66b1b`: both combine the final zero byte of one word with the next aligned word `0x000144aa` in a repeated 12-byte record. They are not aligned callback pointers. Many shared patterns occur in the compressed staging/resource data, and a raw integer match is not a pointer consumer.

There is no evidence that a running stock path still reaches the recorder. “Inert” here is the captured hook/control-flow finding, not a proof excluding arbitrary computed targets, Thumb code, a transient overwritten reference, every future operating mode or every writer. No erasure or persistent cleanup is justified by this host-only observation. This finding belongs with the bounded repaired-state disposition in #75; #70 concerns segment-4 startup generalization and is not closed by it.

## Reproduction and private outputs

`//emulation:ram-calibration` is the Haskell capture/comparison tool. All dump, frame, bridge-state and analysis outputs stay under ignored `private-data/`. Capture outputs must be fresh directories. The retained path stages the supported runtime and reuses the cardless embedded-Haskell bridge; FujiMod reuses its existing bridge. Both use a fresh ordinary boot, not an installed-payload checkpoint.

```sh
export XM1_EMULATOR_SLOTS=8
B=--output_base="$HOME/.cache/bazel/latent-image-RamCompare"
DAT=private-data/analysis/x-m1-1.50/update/prefix/FWUP0002-prefix.DAT
NAND=private-data/analysis/x-m1-1.50/stored-f7/emulation/inputs/f7-1.nand
BODY=private-data/sessions/20261025T050355Z-read-bulk/ram-00000000-05aa0000.bin
devenv shell -- bazel "$B" run //emulation:ram-calibration -- capture retained "$DAT" "$NAND" private-data/ram-calibration-retained
devenv shell -- bazel "$B" run //emulation:ram-calibration -- capture fujimod "$DAT" "$NAND" private-data/ram-calibration-fujimod
devenv shell -- bazel "$B" run //emulation:ram-calibration -- compare "$BODY" private-data/ram-calibration-retained/ram-00000000-05aa0000.bin private-data/ram-calibration-fujimod/ram-00000000-05aa0000.bin private-data/ram-calibration-summary.json
devenv shell -- bazel "$B" run //emulation:ram-calibration -- references "$BODY" private-data/ram-calibration-recorder-references.json
```

`words BODY MODEL BEGIN_HEX END_HEX` emits exact differing aligned words for a chosen physical window. `references RAM OUTPUT_JSON` records the recorder scan, raw candidates, hook words and limits. Running it on the retained clean-RC0 dump supplies the numerical-pattern control. The JSON comparison includes zero/`0xff` counts, changed words, first changed addresses, decoded task extents and classification rules. It rejects wrong-sized input dumps instead of silently comparing a prefix.

Both capture routes, the regional comparison, recorder reference/control scans and the complete recorder `cmp` were exercised. Firmware/NAND identities and dump/analysis hashes are pinned by E-RAM-CALIBRATION-150 and the two recorder ledger entries. This work introduces no device-model fix and makes no verifier-route or silicon-fidelity acceptance claim.
