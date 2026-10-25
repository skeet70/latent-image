# X-M1 1.50 preview and AF-statistic snapshot producers

## Scope and decision

This is host-only research for [#107](https://github.com/skeet70/latent-image/issues/107), using the decoded X-M1 1.50 images and the existing idle cardless shooting-screen Run-3 capture. The [snapshot contract](../spec/08-artifact-and-runtime-contracts.md#camera-snapshots-and-live-experiment-sessions) remains the acceptance boundary. Static producer bindings and pixels recovered from a non-atomic capture do not demonstrate a fresh coherent observation after a scene change.

The smallest useful AF export is a completed 112-byte stock evaluation result, with its 120-byte input descriptor and explicit validity/provenance, copied once into owned export storage. Reading the live result globals directly is not a coherent snapshot: stock retains no host extraction lease. A result-copy point is bound below; it is a design for independent mechanism review, not implemented hook bytes or camera authorization. A preview/statistic pair additionally needs acquisition correlation and an evidenced source lifetime, not matching host read times.

## Evidence inputs

The decoded image-4 input is `private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin`, SHA-256 `f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16`; image 5 is `lz/id5-00bf1a00.bin`, SHA-256 `3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a`. They load at `0x0061c400` and `0x00bf1a00`. The full DAT identity and installed-byte limitations are in the [static map](autofocus-static-map.md#scope-and-evidence).

Run 3 is `private-data/sessions/20261025T050355Z-read-bulk/ram-00000000-05aa0000.bin`, 95,027,200 bytes, SHA-256 `fa933fcf723ff46d435161cfecf48edc4a29cc4dc4c0ff5232fd0c70e7a6fee2`. File offsets are physical DRAM addresses. The `0x40000000` uncached alias maps to physical `0x00000000`; it is not an additional bank. The [bulk-read report](ff80-bulk-read.md#64-run-3-outcome) owns the collection and exit evidence. Its 91 chunks were read while firmware continued running, so descriptor and payload reads can belong to different times. This report adds no camera or USB operation and does not expand the physical-read bound.

## AF statistic intake, computation and completion

### Hardware bank destinations and rotation

The stock continuous producer uses two statistic banks, `0x5ffb0dc0` and `0x5ffb3770`, separated by `0x29b0`. Initializer `0x00edf558` registers both extents with that size. `0x00ee39c4` initializes bank selectors to 1, installs completion callback `0x00edb25c` through `0x00ee0804` and `0x00d9f0b0`, and starts the continuous state. These addresses are not in Run 3. The bank catalogue alone is not evidence that read-bulk can reach them.

`0x00ee074c` builds the nine destination pointers from a bank base; `0x00d9e88c` writes them through the image-4 setters. The layout is:

| Destination register | Bank-relative offset | Bounded window extent |
| --- | --- | --- |
| `0xfff18500` | `0x0000` | `0x800` |
| `0xfff18504` | `0x0800` | `0x50` |
| `0xfff18508` | `0x0850` | `0x1000` |
| `0xfff1850c` | `0x1850` | `0x140` |
| `0xfff18510` | `0x1990` | `0x400` |
| `0xfff18514` | `0x1d90` | `0x400` |
| `0xfff18518` | `0x2190` | `0x20` |
| `0xfff1851c` | `0x21b0` | `0x200` |
| `0xfff18520` | `0x23b0` | `0x200` |

The offsets follow the X-M1 `0x00ee04f0` dispatcher and its destination setters `0x0067a80c/930/aa54/ab78/ac9c/adc0/aee4`. Extents match the bounded windows of the retained X-M1-selected emulator producer, but their numerical statistic laws remain declared synthetic there. Adjacent offsets bound storage; they do not establish the optical meaning of each bank. `0x00ff5fc4` obtains format-3 offset `0x850` and copies from the completed bank selected at `0x016f6c04` into `0x5ffec1b4`; a second copy rearranges one quarter of that output. Do not equate that transformed AF input with an untouched hardware bank.

The source-side callback `0x00ed8634` alternates next-bank selector `0x016994ec`, obtains the next destination tuple through `0x00ee074c`, and programs it through `0x00da035c(...,0x3f,...)`. It separately rotates the corresponding `0xd8`-byte configuration snapshots. This is reuse, not host retention.

### Hardware completion and its consumers

Image-4 driver `0x0067b878` registers logical event `0x30` through `0x0064e918`. The X-M1 table row at `0x007c9364` is `{0x2e,0x0d,3}`, so this is physical IRQ `0x2e`, not IRQ `0x30`. Handler `0x0067bfdc` snapshots callbacks `0x0095669c/a0`, acknowledges bit 0 at `0xfff188f4` and `0xfff189f4` through `0x0067bdec(channel,8)`, and waits until those bits clear through `0x0067bec8(channel,8)` before invoking the callbacks. The paired ACK precedes software delivery. Its real latency and relation to exposure completion remain unmeasured.

`0x00edb25c` invokes its downstream callback with `(0,current_bank,configuration_pointer,mode_bit)` before switching `0x016992a0` between the two banks and publishing the old base at `0x016992a4`. `0x00ee7c60` maps bank number 1 or 2 to the corresponding configuration snapshot at `0x01699334 + (bank-1)*0xd8`. The callback is a concrete candidate for a bounded raw-statistic copy before rotation. Its uninterrupted bank lifetime across the other source-edge callback and CPUs still needs proof before treating the whole `0x29b0` region as immutable during a new copy.

Run 3 binds `0x0095669c = 0x00edb25c`, `0x009566a0 = 0`, continuous state `0x01698f50 = 1`, downstream callback `0x01698f54 = 0x00fe95c8`, producer-started flag `0x0169929c = 1`, current bank `0x016992a0 = 0x5ffb0dc0` and previous bank `0x016992a4 = 0x5ffb3770`. The metadata bridge's retained source at `0x016f6c04` is also `0x5ffb0dc0`. These are captured configuration and retained bank identities, not a simultaneous observation of MMIO, a payload or a measured frame cadence.

Downstream `0x00fe95c8` serializes the completed bank base, a copy of the `0xd8` configuration bytes and mode into message `0x3f07` with `0xe0` argument bytes; error completion instead sends `0x3f0a`. Bridge handlers `0x00ff42fc/0x00ff43fc` call `0x00ff6810`, which retains that `0xe0` event block starting at `0x016f6c04` and builds bridge metadata. The later AF intake and evaluation are separate consumers below. This chain provides a bank/configuration association; it does not yet bind an acquisition clock or acquisition-time lens state.

### Three separate products

Do not merge hardware-produced statistic banks, the bridge's copied statistic/metadata records, and the AF worker's computed result. Each has a different producer and lifetime.

| Product | Producer and destination | Consumer and format boundary |
| --- | --- | --- |
| Bridge statistic copies | `0x00ff6b4c(0)` calls `0x00ff5fc4(0x5ffec1b4,0x1000)` and `0x00ff6050(0x5ffed1b4,0x10)`; case 1 writes `0x5ffebe8c` and `0x5ffec08c`; case 4 writes `0x5ffee518` | `0x00f40944` binds AF input pointers at `0x016c0f78/7c/80/98` to those four addresses; these are source data, not the final AF score |
| Bridge metadata snapshot | At `0x00ff6f80`, `0x00ff6b4c(kind)` finishes by copying exactly `0x5c4` bytes from `0x016f6640` into `0x016f496c + kind*0x5c4` | `0x00fea7ac -> 0x00ff5680 -> 0x00ff6f94` copies exactly `0x5c4` bytes from a selected stored record to the caller; `0x00f40944` requests kind 1 |
| Computed AF evaluation | Worker `0x00f49890` calls `0x00f873cc(input,result)`; result extent is `0x70`, input descriptor extent `0x78`; worker result is stack-local | Completion callback `0x00f3c104` sends operation `0x89` to `saclac`; selected state handlers call `0x00f44fec`, which accepts score fields into AF state |

The `0x5ffe...` addresses must not be translated by subtracting `0x40000000` and treated as captured low DRAM. They lie outside Run 3 and outside the operation's admitted physical bound after alias translation. Their placement does not authorize a bulk read. The low-DRAM input descriptor contains pointers to these buffers, not their payloads.

### Format and accepted result

`0x00f88dcc` consumes the contrast source as two planes of 64 little-endian 32-bit words, separated by `0x100` bytes. For selected-area input modes 2 or 5, it indexes `(x>>1) + (y>>1)*8`, with each shifted coordinate below 7. Other paths aggregate or weight the 64 positions. This establishes consumer packing and indexing, not a measured contrast transfer function, optical scale or complete sensor-to-region geometry.

`0x00f873cc` copies the 120-byte descriptor into `0x016d6058` and zeroes its 112-byte output. Input word `+8`, copied into `0x016d6060`, decides whether to compute or reuse the prior result. Zero branches to `0x00f87b84`, which copies `0x016d6ba8` back to the output. Computation and cached paths both finish at `0x00f87ba8`, copying the output to `0x016d60d0`. Completion of this function is therefore not proof of a new acquisition. At intake, `0x016c0f48` is set from `(snapshot_word0 & 5) == 5`; the worker validity branch does not independently establish sample age or lens settling.

`0x00f44fec` reads the result pointer in the received event at `0x0169d384`. Its first six words supply the paired values accepted into `0x016c1160/64/68/6c/70/74`; key `0x8778` selects whether the first, third and fifth accepted fields are pair sums or individual words. It also copies metadata from offsets `0x28..0x6e`. These are parameter-dependent computed values, not raw image pixels. A snapshot should preserve the full stock result and parameter/configuration identity rather than flattening it into an unqualified scalar.

### Completion and lifetime

`0x00f3d500` marks evaluation in flight at `0x016c2200`, then requests worker operation `0x3c09` through `0x00f4c400`. The request contains the callback and a copy of the 120-byte descriptor. `0x00f49890` returns from calculation before invoking the callback with `(0,result_pointer)` at `0x00f498c0`. AF result handlers, including `0x00f0ced8`, clear the in-flight field before calling `0x00f44fec`. In-flight clearing, a callback, and acceptance are software processing boundaries, not exposure timestamps. `0x00f0ce6c` and `0x00f3ebd4` separately gate evaluation against movement-request and sample-processing state.

The stack result is valid for the worker callback's stock lifetime. The persistent caches at `0x016d60d0` and `0x016d6ba8`, the bridge metadata records and accepted AF globals can be rewritten by later stock work. No USB lease or producer pause is established for them. Repeated pointer/header/checksum reads cannot extend that lifetime.

### What Run 3 retains

The input descriptor at `0x016c0f40..0x016c0fb7`, accepted primary result fields at `0x016c1160..0x016c119f`, worker descriptor/current-result region at `0x016d6058..0x016d613f`, and prior result at `0x016d6ba8..0x016d6c17` are zero. Search state `0x016c0fc0` is `0x0d`; in-flight and movement-request words `0x016c2200/04` are zero. This idle capture does not contain a completed ordinary AF evaluation or a calibrated AF sample. It does not show that the hardware statistic producer was inactive.

## Processed preview buffers

### Actual Run-3 image destinations

Use the producer's committed descriptors, not the display pool catalogue, to select pixels. `0x00ee71a0` commits configuration `0x016996b8` through `0x00d9fd54`. Mask `0x800` programs image-bank instance 0 at `0xfff12000`; mask `0x1000` programs instance 1 at `0xfff13000`. Setter `0x006734d4` writes the destination at instance-relative `+0xc0` and pixel stride at `+0xc4`. `0x006724a8` and `0x00672c84` program the geometry fields. The captured descriptors are:

| Output | Destination word | Geometry word | Pixel stride | Bounded active raster |
| --- | --- | --- | --- | --- |
| Instance 0 | `0x016996ec = 0x031cc700` | `0x016996d4 = 0x01aa0280`, 640 by 426 | `0x016996f0 = 640`, 1280 bytes | `[0x031cc700,0x03251900)`, `0x85200` bytes |
| Instance 1 | `0x01699724 = 0x03386000` | `0x0169970c = 0x01e002d0`, 720 by 480 | `0x01699728 = 720`, 1440 bytes | `[0x03386000,0x0342ec00)`, `0xa8c00` bytes |

The first output lies 27 rows into a 640-by-480 allocation starting at `0x031c4000`, matching the `+0x8700` address adjustment in `0x00edc9b8`. This distinguishes active image geometry from allocation geometry. Both extents are inside the hardware-read Run-3 coverage. Nothing here licenses reading every address in a firmware pool.

The X-M1 CPU conversion fallback `0x006e2050` reads each four-byte pair as `{Y0,Y1,Cb,Cr}` with unsigned luma and chroma centered at 128. It uses full-range coefficients approximately `{R=Y+1.402Cr, G=Y-0.34414Cb-0.71414Cr, B=Y+1.772Cb}` and advances each row by twice the declared pixel stride. This is same-model evidence for the packed 8-bit 4:2:2 interpretation, not a format inferred from a sibling. The host PNG conversion uses FFmpeg's full-range BT.601 conversion and is a visualization, not a bit-exact reimplementation of stock rounding.

These are processed image-bank outputs upstream of display composition, not Bayer raw sensor data or a JPEG. The MOVDC processing builders `0x00edccd8/0x00edd120` configure film simulation through `0x00eee958`. Sensor mode, exposure and complete processing settings still need acquisition-bound export metadata. The display-mode selector `0x008fa5fc` is 1 in Run 3; the mode-3 branch of `0x006dd884` must not be assumed merely because USB attach used mode 3 in a different namespace.

### Completion and software buffer ownership

Image-4 `0x00673c28` installs logical events `0x2e/0x2f`, mapping through the X-M1 interrupt table to physical IRQs `0x2c/0x2d`, with handlers `0x00674254/0x00674290`. They acknowledge their instance through `0x0067409c(1,instance)`, then invoke callbacks at `0x0095ca50/60`. Run 3 contains `0x00ed848c/0x00ed844c` in those slots. These set completion bits `0x100/0x200` in `0x01698d48`; when that equals required mask `0x01698d4c`, they clear the observed mask and call `0x00ed8368`. The captured required mask is `0x300`.

`0x00ed8368` invokes the preview consumer at `0x01698b54`; Run 3 binds it to `0x01031b78` with continuous state `0x01698b50 = 1`. That consumer advances software frame counter `0x01703084`, updates the two buffer groups through `0x0103fd38`, then schedules message `0x401a` to task `0x30` through `0x0103a7dc`. It is a completed processed-output boundary, not a calibrated exposure clock.

`0x0103f190` selects stride-`0x18` descriptor groups. In the ordinary branch, output 0 uses five records at `0x016fedd0`; output 1 uses seven at `0x016fed28`. Each begins with buffer address, state, width and height, followed by two more words. `0x0103fb50` acquires a state-0 slot and changes it to state 1, or state 2 in its special path; `0x0103f1fc` advances state 1 to 2 on completion and recycles older state-2/3 records under its additional conditions. Other consumers use additional state values, so these are observed transition meanings, not a complete enum.

Run 3 retains output-0 records `{0x43098000,state0,640,480}`, `{0x4312e000,state3,640,480}` and `{0x431c4000,state2,640,480}`. Output-1 records include `{0x43386000,state2,720,480}`, `{0x4342ec00,state5,720,480}`, `{0x434d7800,state3,720,480}` and `{0x43580400,state0,720,480}`. Cached/uncached aliases name the same physical rasters. The descriptors and image bytes were read in different bulk chunks; their agreement is a binding lead, not proof they described the same instant.

There is no evidenced host retain/release path for those stock slots. Neither state 2 nor consumer selection gives USB ownership until extraction finishes. The next stock acquisition/recycle can reuse a slot. The display-ready, producer-complete and USB-export lifetimes remain distinct.

### Live pool rotation and state-binding limits

**Hardware-observed, E-PREVIEW-POOL-ROTATION-LIVE-150:** a single seven-window control read on 2026-10-25 found both preview destinations and pool states different from Run 3. The body was reported at the ordinary shooting screen in mode S, without a card. The geometries remained 640 by 426 and 720 by 480, with pixel strides 640 and 720.

| Output | Current committed destination | Matching allocation record, zero-based slot | Observed state |
| --- | --- | --- | --- |
| 0 | `0x03136700` | Slot 1, `0x4312e000`; active raster starts at physical base plus `0x8700` | 0 |
| 1 | `0x0342ec00` | Slot 1, `0x4342ec00` | 2 |

The Run-3 output-0 allocation `0x431c4000` was now state 3, and output-1 allocation `0x43386000` was state 5. Completion callbacks remained `0x00ed848c/0x00ed844c`, continuous state was 1, consumer was `0x01031b78`, observed/required completion masks were `0/0x300`, and the software counter was `0x1de82`. These sequential windows support rotating pool identities across captures, not a timed rotation sequence or simultaneous ownership. The fixed-address observation stopped at these controls; no new pixel read or PNG followed.

**Source-inspected state bindings:** shared acquisition helper `0x0103fb50` treats state 0 as free/acquirable and normally sets it to state 1, pending producer completion. However, with `0x0170306c == 1` it sets state 2 directly at acquisition. State 2 is therefore not an unconditional completion marker. Completion helper `0x0103f1fc` advances state 1 or `0x1b` to 2 and, under retention flag `0x0170319c`, preserves an older state-2 slot as state 3. Output-0 consumer selector `0x010401f0` selects state 3; output-1 selector `0x01040190`, used by `0x0102fd28`, selects state 5. Output-1 transition `0x0103fd88` promotes state 2 or 3 to 5 according to `0x0170327c`, retires prior state 5 through 6, and recycles it. `0x0104006c` can also force a selected slot to state 5. State 4 remains unbound in the inspected pool-management paths; the states are not a closed 0-5 enum.

Route qualification must account for pool selector `0x016fec4c`, special-acquisition flag `0x0170306c`, comparison counter `0x01703080` against `0x01703084`, retention flag `0x0170319c`, and completion/consumer route flag `0x0170327c`. The control read did not capture these branch inputs. The consumer selectors return the first matching slot, not a proved chronological maximum across all routes, so no source-qualified “most recent completed” fallback was established.

A fixed address or a state-only read cannot establish a completed frame. A coherent preview export needs a route-qualified completion predicate plus an evidenced source lifetime and DMA visibility, or the [owned-copy design](#preview-only-copy-point). Neither matching pre/post controls nor selecting state 3 or 5 supplies that lifetime.


### First camera-derived preview visualizations

Two bounded PNGs were decoded into ignored private storage:

| Private output | Geometry | SHA-256 |
| --- | --- | --- |
| `private-data/analysis/x-m1-1.50/snapshot-producer/run3-preview-03386000-720x480.png` | 720 by 480 | `7b7863eff896793701d3037803d615e677c1d61595efed136ac1b4b39c209c1d` |
| `private-data/analysis/x-m1-1.50/snapshot-producer/run3-preview-031cc700-640x426.png` | 640 by 426 | `59745dc9ecda50c988888fed2857b3e544dd227820173222ca1f259676c24369` |

Both rasters are almost black, with small pixel variations and no useful scene detail. The first source words are `00 01 80 80`, consistent with near-zero paired luma and neutral chroma. Do not infer a lens cap, exposure, scene, sharpness or color calibration from that appearance. These are camera-derived preview visualizations from a known captured source extent, not synthetic emulator images. They are not coherent frames: each raster crosses a 1 MiB capture-chunk boundary, and firmware continued producing/recycling buffers throughout the read. A scene-change freshness result remains open.

## Smallest owned snapshot design

### Statistic-only export first

A candidate minimal copy point is the existing single-word `BL` at `0x00f498ac`, stock word `0xeb00f6c6`, to `0x00f873cc`. A reviewed call wrapper can preserve the original call, then copy its completed `[result,result+0x70)` before the callback uses it or the worker releases its stack. Copy the corresponding 120-byte input descriptor at entry, not after another worker may have changed global scratch. Preserve the cached-result indicator and reject it as a new acquisition. This replaces no AF decision and adds no lens command. The copy point does not resolve hardware bank ownership or acquisition metadata by itself.

Use one fixed-size export slot initially. A bounded versioned descriptor carries session epoch, monotonically increasing export ID, source domain `stock-af-evaluation`, payload lengths, stock format revision, producer completion identity, raw producer/sample/configuration identifiers where bound, processing/region configuration, and validity/reuse/drop flags. Clock IDs, acquisition time, acquisition-time lens command, position and completion remain explicitly unknown until independently bound. Do not substitute the export counter or callback time for a sensor frame ID. The owned payload is the 120-byte input descriptor plus 112-byte result, not the pointed-to external buffers.

The lifecycle is `free -> copying -> published -> released/expired -> free`. Writer publication follows payload completion and memory ordering; a retained slot is never overwritten. A busy slot drops the new observation and increments a loss counter, without retrying or waiting in the callback. Release must identify both epoch and export ID. Define a finite session and explicit bounded expiry; expiry cannot race an accepted bulk read. The simplest first session keeps one published slot until explicit release or session shutdown and admits no replacement during extraction. Reconnect/reset invalidates the epoch. New camera-side release/session operations would be closed, reviewed operations, not arbitrary memory forwarding.

Allocate the slot only within evidenced owned DRAM, access its data consistently through the uncached alias and publish with the reviewed target barrier/cache protocol. The existing bulk-read DMA masks aliases to the physical source; it does not lock a source. Read only the rounded, 64-byte-aligned owned extent through `read-bulk`, with padding inside the allocation. The allocator, installation/suspended-context safety, CPU/callback ownership, copy cost, visibility and session-ending behavior require mechanism review and emulator evidence before any separately authorized body run. No proposed payload is implemented here.

For raw-statistic replay rather than stock-score analysis, the producer-side alternative is the successful bank/configuration callback boundary in `0x00edb25c`, before the downstream `0x00fe95c8` message send and bank toggle. The bounded nine-window envelope is `[bank,bank+0x25b0)`, 9648 bytes, plus the `0xd8` configuration snapshot, 9864 payload bytes before the export header. This omits the bank allocation's trailing space but retains all nine configured windows until a narrower selected-mode dependency set is proved. Copying only format 3 would omit the contrast planes consumed through `0x00fe988c`. First prove the bank cannot be rearmed during the copy and bound the callback's additional time; otherwise this copy point is not admitted. Do not add this raw-bank hook merely to duplicate the smaller completed-result export.

### Preview-only copy point

The smallest preview proposal is a fixed-capacity, at most 128-by-128 luma crop, 16 KiB, in processed-output coordinates, not an unproved sensor or AF region. At the joint-completion consumer `0x01031b78`, capture the completed output's committed geometry, source address, stride and software descriptor identity before `0x0103fd38` changes ownership or `0x0103a7dc` permits later scheduling. Read the packed source pair and retain only its luma bytes. A wrapper must preserve the stock callback and all register/state effects. No hook bytes or installation authorization are supplied here.

This is a candidate copy boundary, not yet an admitted one. Prove that no image-bank interrupt, second CPU or already-armed DMA overwrites the selected output during the bounded copy, and measure the worst-case completion-path delay. The software counter at `0x01703084` is an export provenance observation, not an exposure ID. Source state 2 alone cannot establish the required interval. If the interval fails review, first bind a stock hold/release mechanism rather than running a timed USB read or stopping the preview producer.

Publish the crop into the same owned-slot protocol, with separate `stock-processed-preview` domain, active/allocation geometry, ROI origin, byte stride, packed-source format, retained luma format, processing configuration and explicitly unknown acquisition fields. Retain its bytes unchanged until host release or an evidenced expiry that cannot race an accepted read. Do not attach the latest AF score as though it came from the crop's exposure. A stock buffer with a proved existing lease would remove this hook; none is established by the inspected transitions.

### Paired preview and statistics

A paired observation must refer to one evidenced acquisition relationship. It is not enough to copy the latest image and latest statistic into one envelope. Bind the sensor/configuration identity and the lens command active at acquisition, or expose a statistics-only or preview-only observation with explicit unknown correlation. A bounded intended-region luma crop is preferable to a full-frame transfer during feedback. Copy only after producer completion while stock still owns a stable source, before its evidenced release/rearm boundary. If that interval is not bounded, retain no claim of a coherent crop and do not freeze the sensor or wait for USB to manufacture one.

The first physical validation remains a visible scene change with paired producer identity, stale/torn detection and measured overhead against stock. The one existing non-atomic dump cannot meet that acceptance criterion. Ordinary AF under the current card-error refusal, lens completion/settling and full-resolution capture remain distinct work in [the observation plan](af-observation-plan.md#cardless-s1-result-and-decision-options).

## Reproduction

All commands below are offline and consume owner-supplied private inputs. Run from the repository root in the pinned `devenv` environment. Hash the exact three inputs listed above with `sha256sum`. Inspect captured words using `xxd -g4 -s ADDRESS -l LENGTH CAPTURE`; little-endian word values are not the printed byte-order groups.

```sh
S=private-data/sessions/20261025T050355Z-read-bulk/ram-00000000-05aa0000.bin
xxd -g4 -s 0x016c0f40 -l 0x120 "$S"
xxd -g4 -s 0x016c1160 -l 0x100 "$S"
xxd -g4 -s 0x016c21f8 -l 0x24 "$S"
xxd -g4 -s 0x016d6058 -l 0xe8 "$S"
xxd -g4 -s 0x016d6ba8 -l 0x70 "$S"
xxd -g4 -s 0x016996b8 -l 0x80 "$S"
xxd -g4 -s 0x016fed28 -l 0x120 "$S"
xxd -g4 -s 0x0103f1f0 -l 0xc "$S"
devenv shell -- llvm-objdump -d --triple=armv7a-none-eabi \
  --adjust-vma=0xbf1a00 --start-address=0x40552c --stop-address=0x4055c0 \
  private-data/analysis/x-m1-1.50/lz/id5.elf
devenv shell -- llvm-objdump -d --triple=armv7a-none-eabi \
  --adjust-vma=0xbf1a00 --start-address=0x357e90 --stop-address=0x357ee0 \
  private-data/analysis/x-m1-1.50/lz/id5.elf
devenv shell -- llvm-objdump -d --triple=armv7a-none-eabi \
  --adjust-vma=0xbf1a00 --start-address=0x3959cc --stop-address=0x395a20 \
  private-data/analysis/x-m1-1.50/lz/id5.elf
devenv shell -- llvm-objdump -d --triple=armv7a-none-eabi \
  --adjust-vma=0xbf1a00 --start-address=0x396168 --stop-address=0x3961c0 \
  private-data/analysis/x-m1-1.50/lz/id5.elf
```

The 720-by-480 visualization is reproduced with the following bounded first-frame decode. For the companion, use input size `320x426`, offset `52217600`, output scales `640:426`, and the companion output filename.

```sh
devenv shell -- ffmpeg -v warning -f rawvideo -pixel_format rgba \
  -video_size 360x480 -skip_initial_bytes 54026240 -i "$S" \
  -filter_complex "[0:v]extractplanes=r+g+b+a[y0][y1][u][v];[y0]scale=720:480:flags=neighbor[a];[y1]scale=720:480:flags=neighbor[b];[a][b]blend=all_expr='if(mod(X,2),B,A)'[y];[y][u][v]mergeplanes=0x001020:yuv422p,scale=in_range=full:out_range=full:in_color_matrix=bt601[out]" \
  -map '[out]' -frames:v 1 -pix_fmt rgb24 -update 1 \
  private-data/analysis/x-m1-1.50/snapshot-producer/run3-preview-03386000-720x480.png
```

The disassembler's start/stop selectors above are ELF offsets before the VMA adjustment. Source inspection uses the matching eight-hex-address slices under `private-data/analysis/x-m1-1.50/af/functions/`; decompiler-omitted forwarded registers must be checked in the byte listing, as for the `0x5c4` metadata-copy lengths. No sibling-model address, pixel format, completion event or timing is evidence for this report.
