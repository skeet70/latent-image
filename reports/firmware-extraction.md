# X-M1 firmware findings and restart points

The exact public X-M1 1.50 update is preserved privately. Header parsing, a byte-complement export and bounded framing metadata are verified on that file. A [content survey](#payload-content-survey) segmented the complemented payload: plain little-endian ARM code (plus an ASCII settings-name table in its data) can be read directly, and two hardware-compressed records (~7.8 MB) decode with an inferred byte-oriented LZ grammar that passes every static check. The boot load path, load addresses and main/auxiliary checksum ranges are now read from stock code and data; [UPDATE-01](update-validation-and-boot.md) maps model/version validation, the writer boundary and selected boot halt branches. Hardware correspondence of the decoded images and of the installed body is established over the artifact extents apart from 781 bytes, none of them in an AL-condition word (317 in 41-90% AL-density data blocks; E-RAM-PROBE-150, E-DUMP-RAM-150); exact CPU/ABI remains static inference. G2 and recovery remain open; G3 was accepted 2026-10-08 on the dump evidence recorded in [the evidence ledger](../docs/evidence.jsonl).

## Preserved input and framing

The [official support page](https://www.fujifilm-x.com/en-us/support/download/firmware/cameras/x-m1/) observed on 2026-09-28 associated version 1.50 (released 2017-11-21) with [FWUP0002.DAT](https://dl.fujifilm-x.com/support/firmware/x-m1-150-oqijkawt/FWUP0002.DAT). Acquisition provenance and exact input are in [the firmware manifest](../manifests/firmware.json), with source-page HTML/download headers retained beside the private file. No vendor digest/signature was verified. Installed correspondence is established over the artifact extents apart from 781 bytes, none of them in an AL-condition word (317 in 41-90% AL-density data blocks; E-RAM-PROBE-150, E-DUMP-RAM-150); this update is neither a storage-chip image nor a body-specific calibration backup. Only one private preservation location is recorded.

| Bytes | SHA-256 | Meaning |
| ---: | --- | --- |
| 38,405,148 | `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490` | Original DAT, `private-data/firmware/x-m1-1.50/FWUP0002.DAT` |
| 38,404,616 | `8c6f806ef4554128e5be887a76e438a91b7d6cb030c940fc1c8fd6d32d5926c0` | Length-correct complemented payload |
| 38,405,148 | `46a82a39213751764f4d6e4cac9c9f1d669459240ece15fdb6d0f999c88148e9` | Untouched upstream readable export, including 532 synthetic trailing ff bytes |

**Earlier updates.** The user's body showed BODY VERSION 1.00 on 2026-10-03 and later ran 1.50 before the recorder failed-boot incident. Archived copies of the old download page (fujifilm.com/support/digital_cameras/software/firmware/x/xm1/download.html, Wayback captures 2013-12 to 2018-01) link five keyed paths under download.fujifilm.co.jp/pub/tools/:

| Path key | Linked |
| --- | --- |
| `xm19qx4jvrc` | 2013-12 |
| `xm1EAjX4j29` | 2014-08 to 2015-09 |
| `xm1kssUk82D` | 2015-11 to 2016-02 |
| `xm13ayuh6qz` | 2016-04 to 2017-07 |
| `x-m1-150-oqijkawt` | from 2017-12 |

The first four probably correspond to 1.01, 1.10/1.20, 1.30 and 1.40 by date. That mapping is unverified, because the page text wasn't parsed per version. On 2026-10-03 all four returned HTTP 403 from both download.fujifilm.co.jp and dl.fujifilm-x.com, and the Wayback Machine holds no capture of any of the DATs. No 1.00 update was ever published. A 1.00 image would therefore come only from the body, through a reviewed RAM/storage read.

The export adapter's framing contract is `[0,532)` (outer header) and `[532,38405148)` (complement-export payload). Type word 4 selects a 512-byte field plus the remaining header words. Upstream calls this field code; the native updater copies it for model-identity matching. The updater also reads 72 bytes of range metadata after the outer header, for a total staging header of 604 bytes. Little-endian words at offsets 516/520 are 1/0x50, at 524 the main checksum `0x8f0ab18f`, and at 528 the value 1 used to select the native writer's block classes. [UPDATE-01](update-validation-and-boot.md#container-file-path-and-validation) owns those consumer semantics. Container byte order and file offsets alone do not establish instruction endianness or load addresses.

The exact source-checked upstream readable transform is payload byte XOR ff. Its allocator retains the original container length, causing the synthetic tail; new probes must use the clipped payload or original DAT. **Complement is not decompression.** The original and clipped exports remain private.

### Supplied 1.01 and 1.20 comparison

The owner supplied two copies from tiredboffin on 2026-10-12. The raw header version words are `1/0x01` and `1/0x20`; all three declared checksum ranges pass on each input. These checks establish internal consistency, not vendor authenticity. Both compressed records in each file decode to their exact declared sizes with the existing `//lz:fflzd`. E-OLDER-XM1-FIRMWARE-COMPARISON records the inputs and host evidence.

| Input | File length | Decoded id 4 size | Id 5 load / decoded size | Plain-image destination |
| --- | ---: | --- | --- | --- |
| `private-data/firmware/x-m1-1.50/XM1-0101-FWUP0002.DAT` | 38,405,156 | `0x27e660` | `0x00bed1c0` / `0x4e1a60` | `0x010e0000` |
| `private-data/firmware/x-m1-1.50/XM1-0120-FWUP0002.DAT` | 38,405,148 | `0x280660` | `0x00bef3e0` / `0x4efce0` | `0x010e0000` |
| Official 1.50 | 38,405,148 | `0x282800` | `0x00bf1a00` / `0x4f1e60` | `0x01100000` |

**Recovery-relevant byte equality:** the first `0x1000` media bytes, both complete `0x10000` boot-record slots at flash `0x40000`/`0x50000`, and both complete `0x20000` kernel/loader slots at `0x80000`/`0xa0000` are identical across all three versions. A separate `cmp` of the original DAT spans reproduces the equality without the extraction helper. The old copies therefore supply no different early boot code or stock ROM configuration. They do not include the mask ROM, establish physical recovery entry or demonstrate restoration.

**Adjustment-preservation lead:** 1.20's identity field and both auxiliary restore lists are byte-identical to 1.50's. 1.01's identity and auxiliary-2 list also match, but its auxiliary-1 list is `0x190` rather than `0x188` bytes: 100 entries rather than 98. Relative to 1.01, 1.50 removes `(0x1d71,0x24)`, `(0xe1e8,0x4b0)`, `(0xe699,0x4b0)` and `(0xebdb,0xfa2)`, adds `(0x277,2)` and `(0x1d77,0x1e)`, and reduces unique short-list coverage from 7,880 to 1,474 bytes. Auxiliary 2 contributes the same 13 bytes in every version. The meaning of the additional old preserved regions remains unbound; do not substitute an old list into a 1.50 container.

**Historical-template lead:** over the factory adjustment image at media `[0xe0000,0x122000)`, 1.01 differs from 1.50 in 6,249 bytes, while 1.20 differs in 72. The prior captures still differ in 14,340 bytes between 1.00 and 1.50; 6,241 of those changed positions match the supplied 1.01 template's old value, and 72 match 1.20's. This narrows historical default attribution without turning 1.01 into a captured 1.00 firmware image. The compared develop tone-family region at adjustment `[0xcb77,0xccfd)` is byte-identical in all three templates.

**Address limit:** image 4 retains load `0x0061c400`, but its size/BSS and the image-5/plain load addresses change. Existing 1.50 application addresses and patch sets are not reusable on the old builds without correspondence work.

Reproduce container identity, checksum/layout inspection, comparison and component extraction with [`//recovery:inspect-firmware`](../recovery/README.md#firmware-triage-for-other-failures), then decode extracted compressed segments with `//lz:fflzd`. Supply your own DATs beneath ignored `private-data/`. Historical adjustment-window and archived-template comparisons additionally require owner-supplied captures and the stated field/range contract; the inspector does not reproduce those private capture comparisons. These are host-only findings, not old-build native execution or recovery-gate acceptance.

#### Menu and code expansion implications

**Byte-read layout, not runtime ownership:** the three plain-image extents preserve relative starts `0`, `0xc0000` and `0xe0000`, while their lengths change and the whole plain-image destination moves upward by `0x20000` in 1.50. Each inter-extent gap below is entirely zero in its shipped image. The common normalized portions occupied by the current 1.50 gaps are zero in both older images too.

| Version | Gap after first plain extent | Gap after second plain extent | Image-4 decoded end / BSS start |
| --- | ---: | ---: | --- |
| 1.01 | 88,864 bytes | 13,696 bytes | `0x0089aa60` |
| 1.20 | 83,776 bytes | 13,184 bytes | `0x0089ca60` |
| 1.50 | 80,480 bytes | 13,152 bytes | `0x0089ec00` |

**Static inference:** the shrinking gaps and shifted destinations establish build-dependent packing boundaries, not fixed patch reserves. In each version image 4 ends exactly where its BSS begins, and that BSS ends exactly at the image-5 load: there is no intervening growth reserve on these boundaries. Extending image 4 upward would collide with BSS unless the BSS geometry and linked references were reconciled. Existing 1.50 code cannot inherit the older build's smaller RAM use merely by copying its table values.

**Placement leads:** 1.50's plain gaps `[0x011ac5a0,0x011c0000)` and `[0x011dcca0,0x011e0000)` remain candidates for loaded carrier bytes, not accepted runtime storage or early executable targets. Whole-image load coverage, execution permissions, every consumer/writer and the first call's position after residency remain separate requirements. RECIPE-PLACEMENT-01 remains open. The recorder rev-2 failure is the concrete timing counterexample: carried bytes were not yet resident when its early hook ran.

**Prefix evidence:** the table preload region corresponding to RAM `[0x0061c200,0x0061c400)` is zero in all three inputs. This corroborates layout stability around the existing small decode-prefix design, whose corrected 1.50 window is `[0x0061c1fc,0x0061c400)` (516 bytes). Its stronger ownership argument remains the retained-table boundary and copy-before-decompression ordering, not cross-version zero equality; [prefix widening](filmsim-prefix-widening.md) retains the runtime-writer limits.

This static layout census used a fixed-input private probe; there is no public one-command census reproducer. The addresses, extents and comparison method above remain public findings. The supported inspector extracts the inputs but does not validate a new placement.

**INSTAX is a bounded vendor-extension example.** [Fuji's release history](https://www.fujifilm-x.com/en-us/support/download/firmware/cameras/x-m1/) names instax SHARE SP-1 compatibility as a 1.20 addition from 1.10. Byte reads of the supplied builds find the setup parent by its stable header `{id 0x3f, type 0, property 5, kind 7, flag +0x18 = 1}` and walk its terminated values pairs and parallel label quadruples:

| Version | Setup parent | Values / labels arrays | Setup rows | INSTAX child / label id |
| --- | --- | --- | ---: | --- |
| 1.01 | `0x00785858` | `0x00784ea0` / `0x00783044` | 18 | absent from this list |
| 1.20 | `0x00787264` | `0x007866b4` / `0x007845f4` | 19 | `0x0078110c` / `0x3` |
| 1.50 | `0x00789228` | `0x00788678` / `0x007865b8` | 19 | `0x007830d0` / `0x5` |

The added values pair has token `0xc` and a child pointer, inserted before FORMAT. Both later child headers have id `0x7d`, property `0xc0`, kind 6 and a parent pointer back to their respective setup list. All 18 pre-existing setup labels shift by 15 between 1.01 and 1.20, then by 2 between 1.20 and 1.50. Many existing row tokens also shift by one between 1.01 and 1.20; the child addresses and both arrays relocate. This is table growth and relinking, not occupancy of a fixed spare row. The list walker tolerates the vendor's additional row without a changed parent kind; a custom extension still needs its own action dispatch, availability and navigation contracts.

The English-language label directory is at slot-relative `0x3704` in 1.01 and `0x3758` in both later builds. A separate DATE/TIME text-to-row-to-directory anchor resolves that directory in every build. In 1.01 id 1 names SET-UP; in 1.20 id 1 names PRINTER CONNECTION INFORMATION and id 3 names PRINTER CONNECTION SETTING; in 1.50 ids 1/2 name TELE. CONV. VERSION / TELE. CONV., and the printer setting is id 5. DATE/TIME moves `0x44` → `0x53` → `0x55`, while FORMAT moves `0x7f` → `0x8e` → `0x90`. New IDs were inserted ahead of existing IDs. Importing an old numeric label id into 1.50 changes its meaning; extending labels through a patch-owned accessor class avoids renumbering every stock reference.

**Practical inference:** menu growth should relocate complete terminated values/label arrays and repoint the parent, with the new child and its callbacks explicitly owned. INSTAX supplies a real format/reference example for that operation. Reclaiming its code or buffers would require removing every consumer and writer, including its worker lifetime; hiding the row alone supplies no such proof. Cross-version data improves candidate selection and reference binding, but adds no accepted large code cave or RAM allocation.

E-OLDER-XM1-MENU-LAYOUT records the layout and menu conclusions. Reproduction requires walking the stated parent/value/label structures in owner-supplied extracted images; there is no public one-command menu census. These are static file walks, not native menu execution or an observed old-build UI.

## Available source and execution boundary

[Source pins](../manifests/sources.json) and [selected inspected blobs](../third_party/firmware-source-inventory.json) retain provenance.

| Source | Useful finding |
| --- | --- |
| fffw `bedc091e0b54a1a34aaf6929dd08e1db36d13b08` | ffun DAT Python is a help stub; public Go paths depend on absent pycheck/assets/fflz code. Generated CLI glue cannot supply those implementations. No public dat2elf or X-M1 manifest exists at this pin. ffindex is an indexer, not an extractor. |
| Fujihack `754acff9aa0322df19446cf7eca699b221c3330d` | XF1 RAM-range copying and sibling model headers/Unicorn examples supply no X-M1 DAT decoder/map. No X-M1 port/header is present. |
| Patcher `0e81431c32f2fc908d9626d255e1d758ebfe8a79` | Exact util.js/patcher.js parse/readable paths expose header/complement only. Patch compilation adjusts a checksum by changed-byte deltas without validating the original absolute range/algorithm. CLI/assembler/UI/updater and mounted-media copy paths are excluded. |

The [export adapter](../host/firmware/export_payload.cjs) runs only hash-checked upstream util.js/patcher.js through a restricted VM with in-memory input and inert UI hooks. Source evaluation/parsing has one-second VM bounds; readable export has 30 seconds. It independently checks exact size/hash/header, caps regular input at 64 MiB, checks lstat and nofollow/nonblocking opened descriptors, bounds growth, and creates only a new output below resolved ignored `private-data/`. It has no camera/network/device API. Existing outputs, special files, symlinks and replacement FIFO races reject; filesystem failure can leave partial private output. Ordinary kernel/filesystem progress remains a trusted-host assumption.

Haskell export checks retain eight synthetic guards and twelve exact-input/full-byte/boundary checks. The C framing helper requires the exact supported DAT hash and uses 256 MiB address space, 10 CPU seconds, 15 wall seconds, finite marker work and at most 32 offsets per marker; its deadline also kills its hash child. [Tool commands](review/README.md) provide current entry points. No derived firmware bytes enter Git or public caches.

## Bounded framing observations

| Observation | Exact result and scope |
| --- | --- |
| Raw container byte sum modulo 2³² | `0xd6c1ba08` |
| Raw payload sum | `0xd6c1545d` |
| Complemented payload sum | `0x70f6b19b` |
| Checksum | `0x8f0ab18f` equals the bitwise complement of the 8-bit byte sum (mod 2³²) of the complemented payload over payload [0x48, 0x24a0068), i.e. DAT [604, 38,404,732). See [Checksum](#checksum). |
| RIHR, LENGTH=, PTBL, A9262, gzip 1f8b08, XZ fd377a585a00 | Zero occurrences in the entire raw container and entire complemented payload; excludes only these patterns in these views |
| ND1 / IPL | Zero raw hits; one each in complemented payload at 7,813,482 / 8,710,042, adding 532 for original-file offsets; short strings do not establish sections or addresses |

The framing probe inferred no codec/token grammar. Later data-inferred decoding is scoped under [decompression grammar](#decompression-grammar); no round-trip is used as independent output evidence.

## Payload content survey

Survey of the clipped complemented payload (`8c6f806e…26c0` above), 2026-10-02/03. Static inference on that file only. Offsets are payload offsets; add 532 for DAT offsets. An independent review re-derived each row and re-verified the corrections applied here, except where a row says otherwise.

| Observation | Exact result and scope |
| --- | --- |
| `gzip -1` output/input per 1 MiB block | Highest 79 %; fill blocks ~0 %. **This metric did not detect the compressed records below**, which gzip to 77–80 % as whole records. Don't use it to rule out compression. |
| A32 `bx lr` (`1e ff 2f e1`, little-endian) | 3,175 hits, 2,810 of them 4-byte aligned, from payload offset 269,056 to 9,324,892 (about 100 aligned hits fall inside compressed records). Big-endian `e1 2f ff 1e`: 0 hits. Chance expectation for one fixed word in 38 MB is ≪1. |
| Sampled code at payload offset 0x69cacc | Decodes as ordinary little-endian ARM: frame-pointer prologues/epilogues and `movw`/`movt` pairs (ARMv6T2 or later), one forming the data address `0x014ea5ac` (`e30a35ac`, `e340314e`) used as a `strh` base. |
| Plain ASCII string table, payload offsets 6,030,480–~6,031,700 | 4-byte aligned, NUL-padded: Wi-Fi labels, `D-RANGE`, `COLOR`, `HIGHLIGHTTONE`, `SHADOWTONE`, `SHARPNESS`, `FILM SIM.` (6,030,880), `PROVIA`, `ASTIA`, `Velvia`, `PRO Neg. Std`, `PRO Neg. Hi`, `MONOCHROME`, `MONOCHROME+R/+Ye/+G`, `SEPIA`, white-balance/metering/scene-mode names, `instax-`. |
| Pointer search for that table | No literal word or pointer array found that references it. |

### Segmentation

After a 0x48-byte prefix, top-level regions begin at 0x48 + k·0x10000. Internal fill splits some regions into sub-extents at other offsets. Rows marked ✓ were rechecked by the coordinator as well as the reviewer.

| Payload range | Content |
| --- | --- |
| 0x48–0x8048 | Header block: `55 aa ff 00` at 0x48, then bytes `67 45 23 01 ef cd ab 89` at 0x4c. Role not established. |
| 0x40048, 0x50048 | Two A32 vector tables with identical `ldr pc,[pc,#0x18]` ×8 but different literal targets (0x1800… vs 0x11800/0x3fff00xx…), each followed by different boot code (0x2b00 / 0x3480 bytes). |
| 0x60048 | 0x20-byte record (`0x22b, 0x22b`, …), uninterpreted. |
| 0x80048, 0xa0048 | Loader records `{0, declared load, size, entry, 0x40000000\|(load+size), …, 0xaaaaffff}`: load 0x00040000 / 0x000d0000, size 0xdae0 / 0xdaa0, code start at load+0x20. Literal pools resolve inside each record at its declared load. The two bodies differ. ✓ (0x80048 header) |
| 0xe0048–0x111e08 | Three extents, uninterpreted. |
| 0x140048 | Probable segment table: 9-word entries from 0x140088 (stride 0x24, indexes 0–7, 0xffffffff end) whose offset and size fields match the extents, e.g. entry 3 is the font, entries 4/5 the compressed records (0x61c400/0x282828 and 0xbf1a00/0x4f1e60, flag 1), entry 2 is 0xca0048 (0x130000/0x24bac8). A second list at 0x1401f0 gives 0x01100000/0xac5a0, 0x011c0000/0x1cca0 and 0x011e0000/0x26a5e0, whose sizes and spacing match the three plain-code extents. Field semantics not established. |
| 0x160048, 0x2c0048 | Compressed (probably LZ-style) records: cleartext opcode and string fragments in a stream about half its declared output. Header `{0x282800 / 0x4f1e60, length 0x146567 / 0x2c66d1 = extent length, ceil(length/0x4000), 0x4000, mode byte 4}`, stream from +0x14. Word 0 is probably the decoded size, but the segment table lists 0x282828 for the first. Decoded; see [decompression grammar](#decompression-grammar). ✓ (headers) |
| 0x5a0048–0x64c5e0, 0x660048–0x67ccd1, 0x680048–0x8ea627 | Plain A32 code and data, holding the ASCII settings table. ~29,000 adjacent `movw`/`movt` pairs: 59 % of high halves in 0x0144–0x0170 (0x014e most common, used as data bases), 25 % in 0x0100–0x0143, the rest 0xff00+ and other. Linked for 0x01100000; see [boot load path](#boot-load-path-and-compression). |
| 0x8ea627–0xb40048 | Zero then 0xff fill. |
| 0xb40048–0xeebb03 | Tables/data, including `FLSNW001…DEFAULT` (0xb55e48), two equal-length (0x17b48) non-identical records (0xbe0048, 0xc40048) and segment-table entry 2 (0xca0048). |
| 0x1060048–~0x10dd019 | `FDL Font` resource (size 0x7cfd4 per the segment table). |
| 0x1100048–0x1560048 | 35 UTF-16LE language slots at 0x1100048 + k·0x20000 (0 Japanese, 1 English, 2 German, 3 French, 34 Cyrillic). Each is a headerless stream of NUL-terminated, 4-byte-aligned strings with private-use glyph codes, interleaved with unexplained binary records; about 2.3k strings per slot with unequal counts. The string-table author found no index table and no menu grouping (not independently reviewed), so no string IDs are established. The English slot (0x1120048) holds the five menu labels, e.g. `<U+F06A><U+F06F>/STANDARD` (0x11210d0) and `B&W` (0x1122430) ✓, and also `PRO Neg. Std` (0x112248c ✓), `PRO Neg. Hi` and `MONOCHROME+R/Ye/G FILTER`. `PRO Neg. Std` occurs 3 times in each of the 35 slots (105 hits ✓). |
| 0x1600048–0x1ab325c | Seven more `FDL Font` resources. |
| 0x1c20048–0x1c41f53 | A JPEG/Exif image. |
| 0x24a0048–0x24a0208 | Trailer (`0x024a0000, 0x024a0000, 0x20, …, 0x61c400, 0x282828, 0xaaaaffff`). |

Menu, film-processing or custom-settings code may sit inside the compressed records, so absence from plain code proves nothing.

The user's body (BODY VERSION 1.00 on the firmware-update screen, 2026-10-03) offers only `PROVIA/STANDARD`, `Velvia/VIVID`, `ASTIA/SOFT`, `B&W` and `SEPIA`, and Fuji's 1.01–1.50 release notes add no film simulation. PRO Neg labels appear both in the ASCII table and in every UTF-16 UI slot. Their use context is unknown, and neither location shows that PRO Neg or filtered-monochrome processing exists or is reachable in this build. [Film recipes](../spec/10-film-recipes.md) depend on that question.

Survey commands were `dd`, `gzip`, `strings`, `grep -obUaP`, `od`, the pinned `llvm-objdump` and ad hoc private C scanners; no derived bytes enter Git. Still open: exact CPU core and revision (family in [CPU profile](#cpu-profile)), Thumb use, the header block's role, and the compressed extents' contents beyond the two records the grammar decodes. Segment-table semantics are mapped in [the segment-table section](#segment-table-structure-and-the-extent-done-flags), apart from the entry-6 duplication; installed correspondence is evidenced at the probe windows ([E-RAM-PROBE-150](../docs/evidence.jsonl)).

### Boot load path and compression

Read from the stock boot code at payload 0x40048 (base 0, reset 0x1800), 2026-10-03, and independently reproduced by the reviewer with separate listings and scanners. Static inference on this file only. Flash offset = payload offset − 0x48. Private notes: `private-data/analysis/x-m1-1.50/decode/` and `review/`.

| Observation | Result and scope |
| --- | --- |
| Segment table | The boot reads flash 0x140000 to RAM 0x0061c000 and looks entries up by id until −1. Entry fields the boot uses: `{id, load, size, bss, bss_len, flash_off, flash_len, …}`; words 7/8 (0x20, probable compressed flag) aren't confirmed by use. Flash (probably NAND) is read per 128 KiB block with a bad-block skip list. |
| Stages 0/1 | A `mrc p14` DBGDSAR test against 0x4003, probably a per-core discriminator, selects the path. One path copies ids 0/1 to 0x00040000/0x000d0000, decompresses id 4 and targets 0x40020; the other targets 0xd0020. Both pass r0 = `*(0x61c40c)`, a word inside decoded id 4. The boot also programs the ARM MPU region registers (PMSA environment); see [CPU profile](#cpu-profile). |
| Image 4 (payload 0x160048) | The only image the boot decompresses: staging 0x42e40000, destination 0x0061c400 (via the 0x40000000 alias), bss 0x89ec00 + 0x352e00 zeroed by a fill/copy engine (probably DMA) at 0xfff02000. The boot halts unless the trailer (payload 0x24a0048) halfword +0x1e is 0xaaaa, or if that read fails. Image 5 and the plain image are loaded later, by image 4's `flashloader` task [(who loads them)](#who-loads-image-5-and-the-plain-image). |
| Decompressor | A hardware engine at MMIO 0xfff43000: src +0x40, length +0x44 (`len & 0x7ffff`, bit 24 set iff mode byte ≠ 4), dst +0x48, start +0x04 = 2, status/ack +0x88, output count +0x4c. Input is streamed in 0x4000-byte chunks after the 0x14-byte header. In the literal and `mvn` forms the base occurs only in the boot (7 aligned hits at 0x43c30–0x43d58; 4 unaligned hits inside record 0x2c0048 are data); image 4 also carries a relocated copy of the driver that forms the register addresses with `movw`/`movt` pairs (`0x6843c8`, `0x684308`), which the earlier scan could not see. No software decoder was found (heuristic search). |
| Load addresses | Loader at 0x00040000 (its literal pools, a string reference, and boot calls into its division routines). The plain image (payload 0x5a0048) is linked for 0x01100000, per the sub-list at 0x1401f0: of 48,903 unconditional BLs, 1,829 land on five loader function entries (1,420 prologue landings at this base, ≤1 at ±0x1000 or +4), and 3,789 pointer words hit prologues against ≤556 at ±0x1000. It also makes 6,737 BLs into [0x61c400, 0x89ec28) and 581 into [0xbf1a00, 0x10e3860), against 12 into bss. Which stock code loads it there and where are in [who loads image 5 and the plain image](#who-loads-image-5-and-the-plain-image); whether the address is physical or virtual remains open. |

The second supplied boot record at payload `0x50048`, mapped at base `0x10000`, initializes peripherals and uses the same segment table for an id 1 handoff. The reciprocal synchronization supports companion processor initialization rather than redundant recovery banks; [UPDATE-01](update-validation-and-boot.md#second-supplied-boot-record-companion-initialization-and-shared-image-handoff) owns the branch evidence and limits. Reset/ROM loading order and independent SD entry remain open.

The compressed images can't be decoded by emulating stock code, because the boot only drives the hardware engine. The user approved one exception to the "do not guess tokens" rule ([Phase 2](../spec/03-phases-and-gates.md#phase-2--preserve-inputs-and-map-firmware)): a grammar inferred from the data counts only if decoded output passes exact-length, stage-header and cross-image call-target checks, and is later compared with a camera RAM read when that access exists (both done: E-RAM-PROBE-150 and E-DUMP-RAM-150 compare the decoded images with the camera's RAM over the artifact extents).

### Who loads image 5 and the plain image

Static inference from the exact 1.50 images, 2026-10-07, host-only and spot-checked from the bytes at each named address.

- The boot records load neither. Boot record 1 looks up segment-table ids 0, 1 and 4 only (`0x1d18` mov r0,#0, `0x1d24` mov r0,#1, `0x225c` mov r0,#4, each `bl 0x4174`); ids 0 and 1 are copied by the 128 KiB block reader `0x231c`, and id 4 is decompressed through the shared streaming loader (`0x3d3c` setup, engine helpers `0x3c54`/`0x3cbc`/`0x3ce4`/`0x3d08`). Neither loader image calls into the boot block (no BL target in `0x0000`-`0x3fff`), and boot record 2 calls none of its own helper copies.
- Image 4 loads both, from its `flashloader` task `0x73b3a0` (created by `0x73ac7c`, mailbox 0x3c). Its dispatch table at `0x73b47c` maps command words to cases: case 0 at `0x73b4ac` calls `0x6a96c0`, case 3 at `0x73b554` calls `0x6a9844`.
- Image 5, `0x6a96c0`: segment-table id 5 (`0x6a96cc` mov r0,#5, `bl 0x657114`), entry at payload `0x14013c` = `{5, load 0x00bf1a00, size 0x4f1e60, bss 0x01460000, bss_len 0x002bbe40, flash 0x002c0000, flash_len 0x002e0000, 0x20, 1}`. It zeroes the bss (`0x6a9820` `bl 0x66609c`), opens a decompression session on the destination (`0x6a9778` `bl 0x6ae360`), reads logical blocks `0x16`-`0x2c` into staging `0x43e40000` (`0x6a9798` `bl 0x6a8f1c`, drain callback `0x6ae500`) and finishes at `0x6ae3f4`. The compressed record at flash `0x2c0000` carries the header `{0x4f1e60, 0x2c66d1, 0xb2, 0x4000, 4}` at payload `0x2c0048`.
- The plain image, `0x6a9844`: segment-table id 6 (`0x6a9850` mov r0,#6), entry at payload `0x140160` = `{6, load 0x00bf1a00, size 0x4f1e60, bss 0x01460000, bss_len 0x002bbe40, flash 0x005a0000, flash_len 0x00360000, 0x20, 0}`. The destination used is not the entry's load field but the sub-list word at `0x61c1a8`: `0x6571a0` returns `[0x61c004]` = `0x4061c1a4` and `0x6a9890` loads `[+4]`, which the probe capture shows as `0x01100000`. The read is a raw storage read with no engine and no CPU copy loop: `0x6a98ac` `bl 0x6a8f1c` over blocks `0x2d`-`0x47`, 0x1b blocks covering `0x360000` bytes, with progress events through `0x73b924`.
- Image 4 also carries a relocated copy of the engine driver (`0x6843c8` writes `0xfff43040`/`0xfff43044`/`0xfff43048`/`0xfff43080`, `0x684308` writes `0xfff4308c`/`0xfff43088`, wrappers at `0x684760`/`0x6847d8`/`0x684828`/`0x684880`), so the "engine base occurs only in the boot" statement holds only for the literal and `mvn` forms scanned then.

Entry indices and fields, the staged order and the delivered destinations are static inference from the decoded image and the payload; the 2026-10-07 probe shows both images resident but does not time the loads, and the companion core's part is not assigned. The two staging constants are hard-coded per caller and derived from nothing: the boot stages image 4 at `0x42e40000`, and the id-5 loader stages at `0x43e40000` while image 4, 5 and 6 code uses `0x42e40000` as a buffer elsewhere; the id-5 loader is the only user of `0x43e40000`. Detail: `private-data/analysis/x-m1-1.50/loaders/NOTES.md`.

### Segment-table structure and the extent-done flags

Read from the payload table (flash `0x140000`) and the live probe copy, 2026-10-07.

The table base holds three pointers with the `0x40000000` alias: `0x4061c040` to the entry list, `0x4061c1a4` to the extent-list descriptor and `0x4061c184` to the last entry. The entry list is eight 9-word entries from `0x61c040` (payload `0x140088`), `{id, load, size, bss, bss_len, flash_off, flash_len, 0x20, compressed}`, and each field is bound by a consumer: the boot getter `0x4174` and image 4's `0x657114` walk by id with stride `0x24` and `0xffffffff` terminator; the block readers use `flash_off`/`flash_len` in byte offsets, which the loaders shift right by 17 for logical blocks (`0x231c` for ids 0/1, `0x6a8f1c` for ids 5/6); the id-4 path uses `load` as the engine destination and `bss`/`bss_len` for its fill; `0x6a96c0` uses id 5's `load` the same way. Entries: 0 and 1 the loader stages, 2 a tables image at `0x00130000`, 3 the FDL font at `0x003bc000`, 4 image 4, 5 the compressed image 5, 6 the plain-image source with `flash 0x005a0000`/`0x00360000` and last word 0, 7 a source-only region at `flash 0x00b40000` with load and size zero.

Entry 6's `load` and `size` fields repeat entry 5's (`0x00bf1a00`/`0x004f1e60`) while its `flash_off` names the plain image, and the id-6 consumer never reads the `load` field: `0x6a9844` takes the destination from the extent descriptor instead, so that field is unused by the load path. Why the vendor table repeats it is not established.

The extent descriptor at `0x61c1a4` is `{pointer to the first extent entry, destination 0x01100000, total 0x00360000}`. Its entries are six words (stride `0x18`) from `0x61c1b0`, terminated by `0xffffffff`, and hold `{id, base, size, ?, done, resource}`: the three base/size pairs are the plain image's linked extents `0x01100000`/`0xac5a0`, `0x011c0000`/`0x1cca0` and `0x011e0000`/`0x26a5e0`, and the `resource` words are `0x44`, `0x46` and `0x47`.

The `done` words are the three bytes the probe found changed from the payload. Image 4's progress reporter `0x73b924` keeps a high-water mark at `0x914834`, ignores a report at or below it, walks the extent entries, skips any whose `done` word is nonzero and, when the reported copy position passes an entry's base plus size, logs and posts event `0x6f00` through `0x73b760`. The `0x6f00` handler `0x73bc54` walks the same entries, and for an entry whose `done` word is zero and whose base plus size the position has passed, it invokes the callbacks in the entry's base word list, sets that entry's `done` word to 1 and releases the entry's resource word through `0x724c00`; the same function takes those resources through `0x724b68` before the copy starts. So the three observed flags mark copied plain-image extents.

The word at `0x61c188` is the boot's own publication, not a mystery write. Boot routine `0x2058` stores `0x4310` into RAM `0x4804` and zeroes the count word at `0x4310`; the handoff code at `0x1f60`-`0x1f78` then copies `[0x4804]` into `[[0x61c00c] + 4]` = `0x61c188`, which is why the payload slot is zero and the live slot reads `0x00004310`. The structure at `0x4310` is the flash block skip list: `0x209c` walks it to map a logical block index past every entry at or below it, `0x20f0` reports its remaining capacity against 20, and `0x2118` returns its count, which the stage loader feeds to the same walk. No image 4, 5, 6 or loader code reads `0x4804`, `0x4310` or `0x61c188` by literal reference, so the table slot is a publication whose reader is not in the exported images.

The extent completion callbacks are the words at each extent's base. The `0x6f00` handler calls `0x6571bc(id)`, an extent-list lookup by the entry's first word, and for an unflagged entry whose end the copy position has passed it walks the entry's base word as a NULL-terminated function list, invokes each pointer, sets the flag and releases the resource. Extent 0 begins `{0x0110f9d8, 0}`, extent 1 begins with zero and so has no callbacks, and extent 2 begins `{0x011e00f8, 0x0125a260, 0x0125f16c, 0x01274628, ...}`. The callbacks are per-extent init hooks with one shape: `0x0110f9d8` calls `0x110f960`, which issues `0x724cdc(0x21, {0x30, 0x14})` and then hands `{0x21, 0x110f9f0, 9, 0x1000, 0x1109130, 0}` to `0x725dd8`, and returns; the hook spins forever if the return is nonzero, and `0x011e00f8` does the same through `0x11e0080`. So each extent arms its own subsystem once its bytes land, and an initialization failure halts the boot. The artifact's offsets for those bases are 0, `0xc0000` and `0xe0000`.

Entry 6's repeated load, size and bss words are dead in this build: `0x6a9844` is the only reader of entry 6 and it uses only the flash fields and the extent descriptor, and the loader stages never call the entry getter. Why the vendor table repeats them is not derivable from the images.

### The log store

Loader A's writer `0x42188`, found through the image-5 load's `MAIN_S`/`MAIN_E` calls, appends to a fixed store at `0x400a0800`. Its initializer sets the capacity halfword to `0x1ffc` at `+0x1c`, zeroes the next-index halfword at `+0x1e`, clears the 8188 entry slots from `+0x34` to `0x400c07f4`, and hands the dump path the store's `0x1fff4` bytes; the writer wraps the index to zero when it reaches capacity. Entries are 0x10 bytes from `+0x34` and hold `{type byte, thread byte, delta halfword, tick word, one eight-character string prefix}`: the type byte is `0x72` on this path, the thread byte is the low byte of TPIDRURW or zero when its sign bit is set, the delta is `[0xFFF91004] - [0xFFF91000]`, the tick word is `*(0x400c0800)` (the same shared tick cell the loader initializers expose), and `+0x3c`/`+0x40` hold the two words of the one string argument, the writer copying up to eight characters into a single local. The initializer sets the same layout for a second store at `0x40063460`; no writer to that one is identified. The whole writer body is gated on `*(0x4004f25c) == 1` at `0x421d0`-`0x421e0`, and of the six id-5 loader calls only `0x6a96f0` (`MAIN_S`) and `0x6a9834` (`MAIN_E`) are unconditional: `0x6a9758` branches past `LMAIN_S`, `LMAIN_E`, `DMAIN_S` and `DMAIN_E` when `0x657184` returns nonzero; the live log carries more strings than these call sites name (next section). `read-log` reads the store whole, 8188 slots in 512 windows, plus the tick cell at `0x400c0800` read twice in a row before the store and once after it; the first store window carries both header halfwords.

Still open: the `0xFFF91000` counter's own rate, which fixes the tick's unit in seconds and which the next run's tick samples measure, whether a stock boot's own log survives long enough to be read, and whether the drain at `*(0x409c1e2c)` reaches this base; the structure itself names itself `syslog Ver 2.3`, the tick is the counter divided by 1875 (E-LOG-TICK-SOURCE-150), and the maintained boot's timeline is in the next section.

### The boot log observed

The reviewed `read-log` session `20261007T203100Z-read-log` ran in maintenance mode on 2026-10-07. All 34 captured files match the SHA-256 values in the journal, no request was refused, both gate readbacks hold (index `0xf7` = 1 after enable, 0 after restore) and the host session took 9.01 s, 0.258 s per window. The store's first bytes are ASCII `syslog Ver 2.3` and a zero byte, then the fields the static analysis predicted: capacity halfword `0x1ffc` at `+0x1c` and next index `0x0333` at `+0x1e`. The ring had not wrapped and 819 of its 8188 slots were occupied; the fixed prefix held 508 complete slots, every one non-zero, and its last twelve bytes are the start of slot 508.

The prefix carries the whole image-5 load. The six loader strings of `E-LOG-STORE-150` are present, and seven more appear that the static writer-call list had missed: `DECE_W`, `DECE`, `WaitLoad`, `UPDATE_R` (11 lines), `IBW_S`, `IBW_E` and `LoadEndE`. Ticks are the word at `0x400c0800`, deltas the `[0xFFF91004] - [0xFFF91000]` halfword:

| Line | tick | delta |
| --- | --- | --- |
| `MAIN_S` | 32 | 28077 |
| `LMAIN_S` | 32 | 27692 |
| `LMAIN_E` | 54 | 24949 |
| `DMAIN_S` | 54 | 24942 |
| `DECE_W` | 54 | 24932 |
| `DECE` | 54 | 23791 |
| `DMAIN_E` | 54 | 23784 |
| `MAIN_E` | 54 | 23775 |
| `WaitLoad` | 59 | 7580 |
| `IBW_S`, `IBW_E` | 62 | 21399, 21367 |
| `UPDATE_R` (11 lines) | 61 to 86 | 27619 at 61, 2570 at 64, 7775 at 86 |
| `LoadEndE` | 133 | 22144 |

`MAIN_S` to `MAIN_E` spans 22 tick units and 4302 delta units, about 195 delta units per tick unit; the tick's own unit is unproved, and milliseconds would make the span 22 ms and the delta clock about 195 kHz. The phases order as flash read (`LMAIN_S` to `LMAIN_E`, 2743 delta units), destination (`DMAIN_S` to `DMAIN_E`, 1158), decompression (`DECE_W` to `DECE`, 1141), and `LoadEndE` follows 79 tick units after `MAIN_E`. The `L` and `D` lines exist, so the writer's gate `*(0x4004f25c)` was 1 and the `0x657184` branch returned zero on this boot, both of which the static review had left open.

Indices 508 to 818, the rest of that boot's syslog, were not read.

### The second boot log

The reviewed session `20261008T020826Z-read-log` ran the 64-window list on 2026-10-08. All 66 files match the journal, no request was refused, both gate readbacks hold, and the host session took 17.49 s. The header is again `syslog Ver 2.3` with capacity `0x1ffc`, but its next index reads `0x0fd2`, so that maintenance boot had logged 4050 slots, four times what the list could carry; its tail stayed unread and the list now covers the whole store.

The load repeats the first session's interval. `MAIN_S` is at tick 31 and `MAIN_E` at tick 53, 22 tick units, with `LMAIN_S`, `LMAIN_E`, `DMAIN_S`, `DECE_W`, `DECE` and `DMAIN_E` between them, `WaitLoad` at 59, then `LoadEnd` at 101 from thread `0x02`, a string the first session's list did not carry, and `LoadEndE` at 132 from thread `0x04`, seen at 133 in the first session. The delta span for the same interval differs between the two boots, 3759 units here against 4302, so the delta source follows a clock that changes between boots and only the tick compares across them.

Two field facts come from this capture. The tick is a 16-bit counter that wraps: the first four entries carry `65511` and the fifth `21`, 46 units later on the same axis. And the loader's strings are a minority of the ring: of the 1020 slots read, 989 are type `0x0f` records holding a finer 32-bit counter at `+0x38`, a small word at `+0x3c` and the tick at `+0x40`, ten are type `0x71` records with register-like words, four are type `0x09`, and seventeen are the type `0x72` loader strings. Only the `0x72` type carries text, so the producers share the ring and each type has its own field layout.

### The whole store

The reviewed session `20261008T024241Z-read-log` read all 8188 slots on 2026-10-08, in the same boot as the second session. All 514 files match the journal, no request was refused, both gate readbacks hold, and the host span was 156.97 s at 0.305 s per window. The header reads capacity `0x1ffc` and next index `0x0fd7`, and the capture holds 4073 non-zero slots against 4115 zeros with no wrap.

Eighteen of the non-zero slots sit past that index because they were written while the read was running, which is what makes the tick measurable: they carry ticks 7286 to 12288 and lie in the two windows the read reached 65.9 and 66.1 s after its first, so the tick advances at least 75 units per second of host time. The image-5 load's 22 tick units therefore bound it at 0.29 s or less, and milliseconds would put it at 22 ms. The twelve bytes past the store's end that the last window reaches, `0x400c07f4` to `0x400c07ff`, are zero, so the overshoot the byte review flagged reads as unclaimed padding.

The store's strings are the loader's eleven markers once each, eight of them the `MAIN_S` to `MAIN_E` sequence, plus `UPDATE_R` 45 times, `IMGSTB_S` and `IMGSTB_E` 37 times each, and `IBW_S` and `IBW_E` 4 times each. The load's own span is again 22 tick units, `MAIN_S` at 31 to `MAIN_E` at 53, with a delta span of 3434 units, a third distinct value after 4302 and 3759, so the delta clock differs per boot while the tick does not. Type `0x0f` records keep their tick at `+0x40` with data at `+0x38` and `+0x3c`, unlike `0x72` where `+0x38` is the tick.

### Decompression grammar

Inferred from the data under that exception, 2026-10-03. Independently re-derived: the reviewer wrote a separate decoder and branch scanner from the written description, before reading the author's code, and reproduced every result and both output hashes. The grammar is consistent with every static check. The 2026-10-07 RAM probe bound it on hardware at seven windows of both decoded images ([E-RAM-PROBE-150](../docs/evidence.jsonl)).

| Item | Result |
| --- | --- |
| Header | 5 LE words `{out_size, total_len (incl. header), ceil(total_len/0x4000), 0x4000, mode 4}`; stream from +0x14 to total_len, no terminator. |
| Tokens | Byte-aligned, no flag bytes. T = 0x01–0x7f: copy the next T bytes literally. T = 0x80–0xff: with the next byte b1, v = ((T&0x7f)<<8)\|b1, distance = v>>4 (1..2047), length = v&0xf (1..15), copied byte by byte. T = 0x00, zero distance/length and modes other than 4 never occur, so their meaning is unknown. |
| Pages | No token crosses a 0x4000 boundary counted from the record start (header included), and a token ends exactly on every interior boundary (81/81, 177/177). Counted from other origins, tokens do cross. |
| Exact sizes | Id 4 consumes 0x146567 of 0x146567 and yields 0x282800; id 5 consumes 0x2c66d1 of 0x2c66d1 and yields 0x4f1e60. The table/trailer value 0x282828 ends inside [0x89ec00, 0xbf1a00), which the boot zeroes (started before decompression, awaited after), so the extra 0x28 bytes are probably zero-initialized data. |
| External cross-check | A published account of this grammar for the X100F 2.12 ([S17](../spec/09-sources.md#s17)) agrees element for element: 0x14-byte header in the same five-word order, the same literal and two-byte back-reference tokens with a 4-bit length and an 11-bit distance, and a window running across the 0x4000 feed chunks. A host decoder written from that account alone reproduces both images byte for byte and token for token from the same input (`private-data/analysis/x-m1-1.50/lz/fflzsa.c`, run 2026-10-08). Scope: a different model and firmware version, so it corroborates the grammar and supplies no X-M1 fact; its no-op or clamp handling of control 0x00, zero distance or length and a back-reference before the output start differs from this decoder's rejection, and none of those cases occurs in the records. |
| Stage headers | Id 4 starts `{0, 0x61c400, 0x282828, 0x69cdf8, 0, 0, 0, 0xaaaaffff}`. Word 3 is a function entry in both images (0x69cdf8, 0xd985a4); its role (probably the entry point) is not established. |
| Cross-image calls | Of the plain image's unconditional BLs, 6,737 into id 4 and 581 into id 5. About 98 % and 90 % land on an evident function entry (prologue, or preceded by a return/branch) at the table loads; at shifts of ±4, ±8 or ±0x1000 that falls to ≤27 %. |
| Content | Strings decode whole (e.g. `ERR_SYSTEM_HALT`, `NIGHTSCAPE_PORTRAIT`); aligned `bx lr` density is ~1,480/MiB (id 4) and ~610/MiB (id 5). |
| Film-sim names | `PROVIA`, `Velvia`, `ASTIA`, `B&W`, `PRO Neg`, `FILM SIM` occur 0 times in either decoded image (ASCII and UTF-16LE). The labels live only in the plain image's ASCII table and the UI language slots, so the menu probably references them indirectly. |

Private outputs, the grammar cross-check record (`rederivation.md`) and the RAM-comparison manifest (address ranges, stable sub-range hashes, per-4 KiB block hashes) are in `private-data/analysis/x-m1-1.50/lz/`. Output SHA-256: id 4 `f2e2acab…dcff16`, id 5 `3cd501c0…fd892a`. Who loads image 5 and when is unknown.

### The encoder

Remaining work 3 asked whether a modified image can be stored uncompressed (segment-table word 8, engine mode bit) or needs a matching encoder. It needs one: no code path accepts a stored image 4 or 5, and both images would outgrow their flash windows uncompressed. Static-inference from the pinned 1.50 bytes, read 2026-10-08 ([E-LZ-ENCODER-150](../docs/evidence.jsonl)); the encoder reproduction is a host run, 2026-10-09.

| Question | What the code does |
| --- | --- |
| Mode values | The mode byte has one use in both engine-driver copies: bit 24 of the engine length register is (mode != 4) (boot kick `0x3c54` writes `0xfff43044`; the relocated copy compares byte `+0x10` with 4 at `0x684414`–`0x684430` before the same write). The driver accepts any value without validation, nothing branches on it further, and no code treats any value as stored or uncompressed. What the engine does with bit 24 is hardware and unproved. |
| Segment-table word 8 | No consumer in any mapped load path. The boot's id-4 path `0x2250` reads words 1, 3, 4, 5, 6; the id-0/1 path (`0x1d08`, `0x231c`) reads 1, 4, 5 plus the loaded loader record's own size word; image 4's id-5 loader `0x6a96c0` reads 1, 3, 4, 5, 6 and chooses the decompression session against a raw staging read through the gate `0x657184` (word `+8` of the struct `0x6570f4` returns), not a table word; the id-6 loader `0x6a9844` reads 5 and 6 only. Ids 0, 1 and 6 are already stored and copied raw; ids 4 and 5 are hard-wired to the engine, so a stored image 4 or 5 would reach the engine as a token stream. |
| Size growth | Uncompressed, image 4 is 0x282800 bytes against its 0x160000-byte flash window and image 5 is 0x4f1e60 against 0x2e0000, so a stored form would not fit the container even if a path accepted it. |

What an encoder must guarantee, beyond producing a decodable stream:

| Item | Requirement | Where it comes from |
| --- | --- | --- |
| Header words | `{out_size, total_len, ceil(total_len/0x4000), 0x4000, 4}` with total_len = 0x14 + stream length and no padding: the loader feeds the stream for total_len − 0x14 bytes and no others, so a short or padded total_len feeds garbage or drops tokens. | Boot `0x3ea0`/`0x3dc0` and image 4's `0x6ae500`/`0x6ae18c`, which read only header `+4`, `+0xc` and `+0x10` |
| Block-count arithmetic | The `+8` page count is read by neither driver copy but by the strict decoder and the container contract, so it stays `ceil(total_len/0x4000)`. | Driver reads above; `fflzd.c` |
| Feed-block boundaries | Every token must end at each interior record offset that is a multiple of 0x4000: the kick arithmetic targets only those offsets (the target is the cumulative fed position `min(avail, total_len)` rounded down to a multiple of the chunk word minus 0x14 stream bytes; each kick delivers its growth toward the target and can span several pages when the reader delivers faster), and a token split across kicks is untested. Tokens are contiguous, so this is the same statement as the stock observation that a token ends at every interior boundary (81/81 and 177/177). | Feed arithmetic `0x3f64`–`0x3fa0`; both stock streams |
| Window | The back-reference window runs across feed blocks; matches may reach 2047 bytes back across a boundary, with no window reset. | [E-LZ-GRAMMAR](../docs/evidence.jsonl) |
| Token domain | Literal runs 1..127 bytes; match distance 3..2047 with length 1..15 and length ≤ distance. The grammar admits distance 1..2047 and overlapping copies, but the stock streams use distance ≥ 3 and never length > distance (0 of 317,995 and 0 of 623,410 matches), so the encoder keeps the stock domain. Control `0x00`, distance 0 and length 0 stay unemitted. | Stock stream statistics; `fflzd.c` refuses the unknown forms |
| Size fields elsewhere | The segment entry's load/bss/bss_len/flash_off/flash_len words still bound the load (destination, fill, 128 KiB block count). The entry size word is unread by the load path; id 4 keeps 0x282828 against out_size 0x282800 with the extra 0x28 inside the bss fill, and id 5's equals its out_size. | [Segment-table structure](#segment-table-structure-and-the-extent-done-flags) |
| Flash window | The record must fit the segment entry's flash_len. The stock records already use 92 percent (0x146567 of 0x160000) and 96 percent (0x2c66d1 of 0x2e0000) of theirs and a patch grows the stream, so the encoder takes the window and fails with `WindowExceeded`, and the image-replacement step must pass the entry's flash_len. | Segment entries 4 and 5; [the encoder](#the-encoder) |
| Failure shape | The engine's produced count (`0xfff4304c`) is accumulated but never compared with out_size, and the engine error bits spin the boot forever (`0x3d28`), so a wrong record hangs the boot instead of failing it. | `0x3dc0`, `0x3d08` |

The encoder is `//lz:lz-codec` (`FujiAF.Lz.Encode` over `FujiAF.Lz.Format`), and it follows the stock stream policy: greedy longest match at every output position with the length capped at the distance and at 15, length 3 minimum, nearest distance on ties; literal runs of unmatched bytes capped at 127; and the feed-page fill the stock streams show, where a run clamps to a page end or to 127 and continues after it, a run that would strand one byte of page absorbs the next output byte so the run fills the page, a match with four bytes of page left becomes a one-byte literal run plus the match tail (same distance) ending at the page, and a match with three left becomes a two-byte literal run ending at the page plus the tail at the next page.

Three guards sit around that policy, each closing one defect class without touching the matcher's choices or the stock fill shapes. A literal run that reaches the 127 cap with 129 bytes of page left flushes one byte short of the cap so the fill guard closes the page, because a full-cap flush there leaves one byte of page where no token fits and the old code answered with a zero-length run; a zero-length run flush is now a hard error. A stream whose total_len lands on a feed boundary is refused (`FeedAlignedTotalLen`): the final kick would be a full chunk, a geometry no stock record shows and whose failure shape is a boot hang. Any record larger than the flash window handed to the encoder is refused (`WindowExceeded`), and the encoder re-walks its own tokens before returning one (`InvalidTokenization`) so no record can leave with a zero-length run, a token start at feed room one, or a token crossing a feed boundary. The fixes are local to the defects they close, and the pinned byte-for-byte reproduction below still passes on both images with them applied.

The recorded 2026-10-09 host results below use the strict decoder and exact pinned stock inputs. Public synthetic checks now run with `devenv shell -- bazel test //lz:lz-synthetic-round-trip`; they do not reproduce stock bytes. The separate manual target `//lz:lz-round-trip` requires the owner-supplied decoded images and payload in `lz/pinned-stock-images.txt`, or explicit `--pinned IMAGE PAYLOAD OFFSET WINDOW` inputs, and refuses absent inputs. `--synthetic` explicitly selects public-only mode and cannot be combined with pinned-input flags.

- Re-encoding the two pinned decoded images reproduces both stock records byte for byte: image 4 (`lz/id4-0061c400.bin`, 0x282800 bytes) yields the 0x146567-byte record at payload 0x160048, and image 5 (`lz/id5-00bf1a00.bin`, 0x4f1e60 bytes) the 0x2c66d1-byte record at 0x2c0048, each inside its flash window (92 and 96 percent used). That is the strongest available check: the encoder's output is the vendor's own stream, not merely something the decoder accepts. It is a hard gate on the fixes, and it still passes on both images with them applied.
- Both the stock records and the re-encoded records decode back to the pinned images under `fflzd`.
- A 189-case synthetic corpus (constant runs, period-5 motifs, sparse fills, mutated repeated blocks, pointer-like 4-byte words, uniform noise and zeros at 27 sizes around the 127-byte run cap, the 2047-byte window and the 0x4000 feed geometry) round-trips through `fflzd`, and every record passes the structural checks: header arithmetic, contiguous legal tokens, no token crossing a feed boundary, a token ending at every boundary, matches inside the stock domain, and short matches only as page-edge tails.
- A 513-case swept page-geometry family (a noise prefix of 0..512 bytes before a 25,000-byte match-free region, the sweep that found the room-one defect) round-trips the same way. The feed-aligned sweep (1,001 noise lengths) shows the `FeedAlignedTotalLen` refusal live and every accepted record off the feed grid, and the window check refuses oversized records.

What the byte-for-byte reproduction buys, and what it does not: for the two stock inputs the encoder is the vendor's encoder, which is strong evidence that the grammar, the header arithmetic and the policy model are right. For a modified input it is evidence of model fidelity only. The engine has been observed on three streams: the two stock records, and the stream this project's encoder produced for the installed RC0 prefix container, which the body boots with its patched arrays live in an ordinary menu (E-FILMSIM-INSTALL-150, hardware-observed, 2026-10-09). A patch shifts every later stream position onto feed-page offsets the stock streams never visited, and that geometry is now exercised on hardware once rather than extrapolated; every other patched geometry stays unobserved. Acceptance for one record is a strict `fflzd` decode back to the patched image, not to the stock one and not the decoder merely accepting the record, with the structural invariants re-checked on that record (no token crossing a feed boundary, no zero control byte, contiguous tokens, total_len at the exact record length, the stock token domain) and the flash-window fit; the container diff carries its own explanation. A camera run's failure shape is a hang, so the 2026-10-09 boot is one observed acceptance and not a general engine test.

What stays unproved: the engine's meaning for mode values other than 4 (bit 24 is hardware); the grammar's unused corners (control `0x00`, distance 0, length 0, overlapping copies, distances 1–2, and what the engine does with them); token behaviour across a kick boundary and interior pages that do not fill to their end (the stock streams never exhibit either and the encoder never produces them); the engine's end-of-stream behaviour when the final kick is a full chunk (no stock record lands its total_len on a feed boundary, and the encoder refuses the geometry, so the refusal is a guard against an untested shape rather than a test of it); and the camera accepting a rebuilt container, which was answered once on 2026-10-09 for the RC0 prefix container (E-FILMSIM-INSTALL-150) and stays open for every other rebuilt container, including the D3 and name images.

### Checksum

`0x8f0ab18f` (DAT offset 524) equals `~Σ mod 2³²`, where Σ is the 8-bit byte sum of the complemented payload over payload [0x48, 0x24a0068). The unreduced sum is 6,190,091,888, so 32-bit wrap-around is required. The 0x48 prefix and the 0x1a0-byte table after the trailer are excluded. The range ends with the 0x20-byte trailer, whose first two words, 0x024a0000, equal the length of [0x48, 0x24a0048).

The pinned patcher (`0e81431c…`) adjusts the stored value by Σ old plain bytes − Σ new plain bytes over each changed range. That is consistent with this rule but doesn't fix the range. An 8,250-test matrix of CRC-32 variants, Adler-32, Fletcher, XOR, ones'-complement and wider sums matched nothing else. No additional checksum was identified in the examined image-record headers. The 8 bytes `67 45 23 01 ef cd ab 89` at payload 0x4c are a constant marker, not a digest.

The match was initially found by search and independently recomputed from the raw DAT. [UPDATE-01](update-validation-and-boot.md#stock-code-binds-all-three-checksum-ranges-on-this-dat) now binds the exact endpoint to the native parser's main-length field and checksum helper: raw DAT `[0x25c,0x24a027c)` is complemented and summed. Two separately checked raw-byte auxiliary ranges cover the remaining `0x1a0` bytes, ending at the exact file length. Independent review reproduced all three checksums. A second official DAT remains useful for format generalization; it is no longer needed to infer this file/path's main endpoint. This establishes neither installed 1.00 updater behavior nor acceptance of a modified image.

### CPU profile

Static inference from the exact 1.50 boot records, loaders and decoded images, 2026-10-03. Independent review CPU-REVIEW-01 confirmed the conclusion and verified the corrections applied here. Addresses are offsets within each listed image. Listings: `llvm-objdump -d --triple=armv7a-none-eabi --mattr=+neon,+vfp4,+mp,+virtualization,+hwdiv-arm,+trustzone` over each whole image, a deliberately permissive decoder so that absent features read as absent from the bytes rather than from the tool. Linear-sweep decodes include data words; the rows separate code sequences from data decodes.

| Evidence | Location | Inference |
| --- | --- | --- |
| `mcr p15,0,rX,c6,c2,0` then `c6,c1,4` / `c6,c1,0` / `c6,c1,2` | boot `0x1a84–0x1a98`; id4 `0x300–0x31c` | PMSA MPU: region number, then access control, base and size/enable. The layout is shared by PMSAv6 and PMSAv7; the barrier and cache-ID rows make it v7. No VMSA core has these registers. |
| IFSR/DFSR reads, then `mrc p15,0,r0,c6,c0,2` / `c6,c0,0` on abort paths | id4 `0x35c`, `0x410` (also `0x3e8`) | Fault status/address reads. The same encodings exist under VMSA, so this row shows abort handling, not PMSA. |
| No TTBR0/TTBR1/TTBCR (`c2,c0,{0,1,2}`) or DACR (`c3,c0,0`) access | all images | No MMU setup. The remaining CRn=`c2`/`c3` decodes in id4, id5 and img6 are data words or cond=0xF MCR2/MRC2 encodings. |
| `mcr p15,0,rX,c15,c5,0` (invalidate all data cache) and `c9,c1,1` (TCM region) | boot `0x1a70`, `0x18c4`; loader `0x2718` | Cortex-R4/R5 implementation-defined registers |
| `dsb sy`, `isb sy` | boot `0x1a6c`, `0x1a7c` | ARMv7 barrier instructions, undefined on ARMv6 cores such as ARM1176. The nearby `wfi` (`0x1abc`) and `cps #0x13` (`0x183c`) are ARMv6K/ARMv6 and do not discriminate. |
| CLIDR/CSSELR/CCSIDR walk (`p15,1,c0,c0,1`, `p15,2,c0,c0,0`, `p15,1,c0,c0,0`) | loader `0x272c–0x275c` | ARMv7 cache identification |
| VFP context save/restore of `{d0–d15}` plus FPSCR, gated on CPACR and FPEXC | loader `0x2454–0x24e4` | 16 double registers: VFPv3-D16. id4 also has VFPv3 `vmov.f` immediates in unconditional code. |
| `movw`/`movt`, `ubfx`/`bfi` throughout | all images | ARMv6T2+ code |
| No A32 `sdiv`/`udiv` in code. Three id5 data words decode as conditional SDIV (`0x4a4b28`, `0x4a67dc`, `0x4ac1f0`), each between `svc`/undefined/unrelated decodes. | all images | No A32 hardware divide, matching the loader's software division routines and Cortex-R4, which lacks A32 divide |

Conclusion: an ARMv7-R core with PMSAv7 and VFPv3-D16, which matches Cortex-R4F. MIDR and the other `c0,c0–c7` ID registers are never read (only the cache ID registers are), so the core and revision are unobserved; an R5F whose code avoids A32 divide would also fit the bytes. The sibling [EXR II CPU report](https://github.com/tiredboffin/fffw/wiki/EXR-II-CPU) names Cortex-R4 but supplies no X-M1 evidence. The wiki's X-M1 row names EXR Processor II Cortex-R4F and SoC FF4227 directly (E-XM1-PLATFORM-LEAD, reported scope), and the boot-ROM window match (E-BOOTROM-PROBE-150) is consistent with both, still with no CPUID read. FujiHack's "ARM11 SoC" note and Cortex-A5/A7 compiler flags concern other bodies and payload builds, and ARM1176 cannot execute this code.

Unicorn 2.1.4 has no Cortex-R4 model; the native runner uses `UC_CPU_ARM_CORTEX_R5F` (ARMv7, PMSA with 16 regions, VFPv3-D16, constant ATCM/BTCM and no-op `c15,c5,0`). Its additional A32 divide, multiprocessing ID and region count are excluded by [the R4F-only rule](../docs/principles.md#scope-and-evidence). For listings, `--triple=armv7r-none-eabi --mcpu=cortex-r4f` decodes the loader's VFP code and leaves A32 divide undecoded.

### Matching C recompilation lead

**Reported:** jitl states that about 55% of his decompiled X70 functions compile byte-identically with an older GCC toolchain. This is a rate over his decompiled subset, not demonstrated coverage of the whole firmware or an X-M1 result (E-X70-MATCHING-C-LEAD).

**Source-inspected, sibling-only:** the fetched X70 checkout at `ebc7869731ab574d1fa0755caf6fa65923faf00e` has a substantial matching-C workflow, not just a compiler-identification spike (E-X70-MATCHING-C-FETCHED). Its current build defaults to GCC 4.6.4 and links emitted functions at stock addresses. `tools/build.py:1453` uses unmasked equality against the stock prefix over the emitted `.text` section; a shorter emission can match while trailing stock bytes remain. Nonmatching functions also retain stock bytes, so `image == stock` alone does not prove complete C coverage. The older `compiler-id/cmp.py` masking is separate from this current verifier. The upstream native toolchain is macOS-specific and documents host-sort compatibility patches; neither its compiler choice nor its X70 match rate establishes an X-M1 result.

**Host-executed, source-derived:** running the fetched `tools/progress.py --no-log` produced 38,737 of 47,439 functions (81.66%) and 5,559,796 of 10,221,224 bytes (54.39%). This default mode counts source definitions without `NONMATCHING` markers, not freshly compiled matches. The checked-in log's latest build-derived entry records 34,605 functions and 4,362,700 bytes; no native compiler rebuild was performed here.

**Next, host-only:** MATCHING-C-01 is a fixed ten-function pilot, two small functions from each of loader A, loader B, image 4, image 5 and the plain image, including a patch-relevant call boundary. Freeze the sample before compiler trials; determine compiler/flags per region, preserve the target ABI and R4F instruction limits, and compare fully linked bytes at original addresses including relocations and literal pools. Account separately for emitted bytes, stock padding and retained stock code/data; do not call a truncated-prefix match whole-function identity. Record exact and nonmatching outcomes separately. Obtain tools through the checked-in Nix/Bazel environment rather than copying the sibling's macOS installation procedure. This is not a whole-firmware rewrite or a new RC release gate.

### Permission-fault loading and live iteration lead

**Reported, sibling-only:** jitl describes changing X70 software over USB without reboot, and seeking the same workflow on X100VI. Tiredboffin warns that disabling protection indiscriminately may bypass abort-driven loading on X-E2. The reported one-byte-per-region change and sibling behavior do not establish an X-M1 patch or its safety (E-EXR-LIVE-ITERATION-LEAD).

**Source-inspected, X-M1 1.50:** the captured vector map points to prefetch/data-abort wrappers `0x0061c73c` and `0x0061c7f0`. Original image-4 instructions mask fault status with `0x40f`, compare with 13 and pass the fault address to `0x0073bfec`. The matching-address branches transfer through literal `0x0061c8c8 = 0x0061c8a4`, whose continuation calls `0x0073c064` and restores saved state. The decompiled range predicate clears address bit 30 and searches six-word records provided through `0x006571a0`; `0x0073c064` logs `WaitLoad`, invokes `0x0004208c`, waits through `0x00724b68` and logs `LoadEndE`. This corroborates a fault-triggered load/wait path, not its complete region lifetime, kernel-service semantics or behavior after permission changes (E-XM1-FAULT-LOAD-LEAD-150).

**Decision:** do not propose a blanket protection patch. MPU-LOADER-01 first closes this specific abort/classifier/kernel-service chain against original instructions and maps its region permissions before/after loading. Any later live-iteration proposal must separately bind code-write and cache visibility, execution, return, transport lifetime and an observable camera surface. The existing [service-mode findings](service-shoot-feasibility.md#answer) still exclude the ordinary menu path in stock maintenance mode; host-driven imaging jobs are a separate candidate with unproved exposure, not a demonstrated hot-testing workflow. No new camera bytes or operation are prepared, and neither lead blocks the review-closed recipe candidate.


## Decoder research and restart points

The plain code region and two inferred decoded images are available for mapping. The source-availability findings below explain the independent decoder route; hardware binding is established over the artifact extents apart from 781 bytes, none of them in an AL-condition word (317 in 41-90% AL-density data blocks; E-RAM-PROBE-150, E-DUMP-RAM-150).

The bounded source search found no applicable public decoder or known-good X-M1 block. Negative scope is the advertised refs and reachable history inspected on 2026-09-28, not all deleted/private code:

- fffw main and experimental `469c3adbe00b7c0c02bc5c66c7787000aa0d8cda` provide no decoder implementation. The [historical fflz README](https://github.com/tiredboffin/fffw/blob/8e12daea55e517607ec195c0d9868fb6b31ae758/ffun/ffcompress/fflz/README.md) discusses an LZ implementation and inferred 16 KiB page/padding behavior; code is absent and those notes are not an X-M1 token specification. The public releases page exposed no decoder asset at inspection.
- [Route 0xff80 notes](https://github.com/tiredboffin/fffw/wiki/Route-0xff80), inspected as an unbound web page, report related-camera LZ/hardware decompression, an X-A2 RAM comparison and private ffun work. These are reported leads, without token grammar or an X-M1 known-good pair. Wiki HEAD alone does not bind a browsed page.
- [YI/LEN sources](https://github.com/protyposis/yi-mirrorless-firmware-tools/tree/b90b96c124a3b076fa0283404e6595a4a40d20d0) at `b90b96c124a3b076fa0283404e6595a4a40d20d0` expect 256-byte ASCII LENGTH headers, zero padding/2048-byte alignment and heuristic LZSS termination/allocation. Exact static files were firmware.js SHA-256 `69889a7e5c02d7f4e1c64f163ac53f8cb20560f61aa53f7c13d8ac9df426c8f1` and lzss.js `1ce2e8883bf1e940ca243b588017c11e1282dfe2674a243cc793ed5ea531430d`. No matching X-M1 LENGTH framing exists in the scanned views; no imports/build/execution occurred. Revisit only after independent format applicability evidence.

Current mapping homes: [film simulation](film-simulation.md), [custom settings](custom-settings.md), [autofocus](autofocus-static-map.md), [update validation and boot](update-validation-and-boot.md) and [container rebuilding](update-container-rebuild.md). The encoder is required and reproduces both pinned stock streams ([encoder](#the-encoder), E-LZ-ENCODER-150); installed-byte correspondence is established over artifact extents with the stated 781 data-byte differences (E-DUMP-RAM-150). The delivered stock area-3 validation closure remains a bounded [native case](../emulation/README.md#stock-native-case-150-area-3-record-validation), not general firmware execution. [GitHub Issues](https://github.com/skeet70/latent-image/issues) owns remaining compiler, loader, recovery and feature work.
Rehash owner-supplied DATs and pinned source inputs at restart. Preserve the established installed-byte correspondence and distinguish it from unproved loader/MPU and silicon behavior. Rebuilding absent upstream fflz paths or repeating broad marker scans supplies no new target evidence.
