# Independently acquired emulator inputs

This guide separates the public X70 reference implementation from its private historical inputs and explains how to acquire equivalent inputs from your own camera. It does not reproduce that camera's state. Credit for the inherited implementation belongs to jitl (justjake) and [x70view](https://github.com/justjake/x70view); the public retention decisions are recorded in `E-X70-REDISTRIBUTION-PERMISSION-150`, `E-MCU-OBSERVED-FIXTURE-PUBLIC-150` and `E-OWNER-SETTINGS-PUBLIC-150` in [the evidence ledger](../docs/evidence.jsonl).

## What is public, private and supported

The optional [owner-settings implementation](xm1-upstream/emulator/owner_settings.py) and [storage-profile implementation](xm1-upstream/emulator/storage_profile.py) retain their historical paths, addresses, hashes and metadata. The owner approved publishing this reference, not its private DRAM, preferences, calibration, configuration-bank or journal/cache payloads. The separately approved remembered sixteen-byte MCU packet remains a scoped observed fixture, not a general capture redistribution grant. [Licensing](../LICENSING.md) owns those distinctions.

**Your own captures cannot satisfy the historical private hashes or select unchanged `settings=owner`.** That option is an exact historical X70 1.10 fixture, not an import interface. It checks complete files, capture metadata, window digests, journal sequences, record equality and a particular set of active-versus-persistent differences. Copying your files to the historical paths or replacing only a digest does not create an equivalent validated profile. Keep the reference unchanged and prepare a separate profile with its own body/version binding.

| Work | Existing public support | Boundary |
| --- | --- | --- |
| Obtain a firmware update file | Fujifilm's model-specific download pages, below | Firmware remains proprietary; downloading is not installing |
| Decode the pinned X70 1.10 DAT on the host | Retained `tools/unpack_x70.py`, below | Exact version/hash and layout only; no camera capture or generic firmware support |
| Launch the X-M1 1.50 viewer | [`//emulation:xm1-viewer`](README.md#interactive-x-m1-firmware-viewer) | Requires your matching DAT and ROM; uses erased settings and synthetic scene inputs, not the X70 owner profile |
| Plan, rehearse and perform bounded X-M1 reads | [Read planner](../readplan/README.md), [closed transport](../transport/README.md) | Live operations require the camera-run procedure; fixed X-M1 addresses are not X70 support |
| Capture X70 normal-REC DRAM, logical banks and main/custom caches for a new profile | Layout and validation reference in the retained modules; acquisition method below | No supported public end-to-end acquisition command or generic profile importer is provided |

## Obtain and prepare firmware independently

Fujifilm currently lists [X70 version 1.10, `FPUPDATE.DAT`](https://www.fujifilm-x.com/en-us/support/download/firmware/cameras/x70/) and [X-M1 version 1.50, `FWUP0002.DAT`](https://www.fujifilm-x.com/en-us/support/download/firmware/cameras/x-m1/). Read the vendor's download agreement and applicable legal requirements before obtaining or analyzing the file. The agreements include a reverse-engineering restriction. This guide does not grant rights to firmware or instruct you to update a camera.

Save the file from the page for your model under ignored `private-data/`, not in the public source tree. Record its source URL, download date, version, length and SHA-256. The existing reference identities are:

| Model/version | File | SHA-256 |
| --- | --- | --- |
| X70 1.10 | `FPUPDATE.DAT` | `7dd5b358a66f38d1fc7dad899a77d35fe4e6ed0edd0ed1d04fbca512d5dc8fd6` |
| X-M1 1.50 | `FWUP0002.DAT` | `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490` |

For the exact X70 file, the retained [unpacker](xm1-upstream/tools/unpack_x70.py) is a host-only reachable preparation step. It has no path flags: it reads `firmware/original/FPUPDATE.DAT` relative to its source root and writes `firmware/mapped/` beneath that root. Run it only in a fresh private staging copy. From the repository root, after downloading your input and choosing a staging directory that does not exist:

```sh
R="$PWD/private-data/x70-own-inputs"
mkdir -m 700 -p "$PWD/private-data"
cp -R emulation/xm1-upstream "$R"
mkdir -m 700 -p "$R/firmware/original"
install -m 600 /ABS/PATH/TO/YOUR/FPUPDATE.DAT "$R/firmware/original/FPUPDATE.DAT"
devenv shell -- python3 "$R/tools/unpack_x70.py"
```

The script requires the X70 digest above, exports mapped sections and `manifest.json`, and records section digests and load addresses. Its manifest explicitly says the map is static, not validated by a camera RAM dump, and that it does not validate the firmware checksum. This command neither captures ROM nor prepares a usable own-body settings profile. Another firmware version requires its own format/layout review, not a changed hash constant.

For X-M1, place your matching DAT at `private-data/firmware/x-m1-1.50/FWUP0002.DAT`; the viewer stages and decodes its own runtime. Its ROM input is `private-data/analysis/x-m1-1.50/bootrom/bootrom-0x80000000-4000.bin`, exactly 16 KiB with SHA-256 `4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be`. Acquire that ROM from your own matching body through the reviewed X-M1 `probe-bootrom` operation, not by obtaining another person's dump. The operation captures more than this block; retain its journal. [The capture implementation](../transport/FujiAF/BootRom.hs) writes 256-byte files under `OUTDIR/windows/`: concatenate `80000000.bin` through `80003f00.bin` in ascending address order to obtain the first `0x4000` bytes. Exclude the repeated block, period probe, handoff window and gate readbacks. Check the resulting length and digest before placing it at the viewer input path. A different digest is a new input requiring investigation, not permission to disable validation.

These commands are host-only and do not acquire camera data:

```sh
devenv shell -- bazel run //emulation:xm1-viewer -- --help
devenv shell -- bazel run //transport:camera -- --help
devenv shell -- bazel run //transport:camera -- rehearse probe-bootrom
```

The X-M1 viewer has no `--settings` or arbitrary body-profile option. Do not pass the inherited X70 settings option to this launcher. See [the viewer instructions](README.md#interactive-x-m1-firmware-viewer) after obtaining the required matching inputs.

## Acquire equivalent state from your own body

### Establish the model, firmware and operation first

1. Record the camera model, installed firmware version, downloaded DAT digest and tool/source revisions. Keep serials and other identifying responses private. X70 1.10 and X-M1 1.50 have different addresses, layouts and evidence.
2. Identify each required input in the table below and trace its producer/consumer in your matching firmware. Separate persistent logical storage, mutable RAM caches and active UI preferences. A RAM copy of configuration is not proof of an intact persistent backup.
3. Prepare closed bounded transport operations with internally built bytes, explicit memory-region and response bounds, finite progress/deadline handling, outgoing-byte journaling and private outputs. Use existing X-M1 operations only within their documented scope; a new X70 operation needs its own implementation and review.
4. Follow [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs). The owner runs a command only after saying go. New bytes that change state require an independent review; persistent operations require a go naming the operation. A read that enables a debug flag is state-changing. Publication approval and an offline rehearsal are not live authorization.
5. Preserve this body's backups in two verified durable locations before any operation that might affect its persistent state. Do not restore another body's data. Use the reviewed stop/exit procedure for the specific operation; USB handle cleanup and a nominal read-only request do not establish a no-write session or safe restoration.

The [fffw source audit](../reports/source-and-transport-audit.md#readconfigdebug-packet-and-effect-matrix) documents the underlying read interfaces: configuration opcode `0x1a0002` function 2 uses little-endian `mb`, page, offset and length; debug RAM opcode `0x200001` function 2 carries an address and requested chunk length. Configuration function 3, debug enable at index `0xf7`, command mailboxes and service-bank loading are writes or state transitions, not an incidental permission of a read. These are source-inspected protocol facts, not a runnable X70 recipe.

Do not run the pinned upstream `ff80` CLI as this guide's acquisition tool. The [audit](../reports/source-and-transport-audit.md) records first-match device selection, eager experimental imports, unbounded/zero-progress read paths and incomplete cleanup/debug restoration. `learn-xff80.py` opens a device at import time. No upstream import, default `ram dump`, guessed configuration write, factory reset, calibration restore or End Setting operation is required by this guide.

### Input inventory and historical geometry

The following values identify the retained **X70 1.10 historical profile**. They are not an X-M1 address map, not universal camera read ranges and not a command list. Establish their equivalents against your own exact model/firmware before acquisition. Store resulting bytes and raw capture logs under ignored `private-data/` or outside Git.

| Input | Historical source/geometry | Acquire and record independently |
| --- | --- | --- |
| Normal REC DRAM and capture metadata | `dram.bin`, exactly `0x20000000` bytes (512 MiB); `capture.json` must identify `ui_mode=3`, `live_a3=0`, `live_f7=1` | A finite sequential capture in the reviewed normal-REC state, including start/end timestamps, address-to-file-offset mapping, chunk/transfer results and whole-file digest; it is not atomic |
| Configuration/calibration view | `cfgdata` at DRAM-file offset `0x02068000`, length `0x7e600` | Validate this RAM view against separately obtained logical bank bytes; retain their provenance and declare any state-dependent exclusions |
| Active preferences | `preferences` at offset `0x00d08354`, length `0x4c74` | Preserve the whole window, identify active payload and remembered MCU packet within it, and distinguish these from persistent records |
| Main and custom records | Offsets `0x00c28280` and `0x00be5c00`, each `0x1000` | Match records against your own captured journal blocks, validate integrity, and derive latest-record selection and sequences |
| Q layout and profile state | Offsets `0x00b56674`, length 136, and `0x00c9bef0`, length `0x170c` | Capture comparison windows and derive expectations from your own displayed/selected settings, not the historical owner's values |
| Logical configuration banks | Bank `0x3a3`, 40 blocks; bank `0x3cb`, one block; block size `0x20000`; managed partition starts at `0x37a` | Capture the service-loaded logical bank payloads plus descriptor metadata. These are not raw physical NAND or OOB/ECC captures |
| Main/custom journal caches | Each journal has 32 bytes of cache metadata and two complete `0x20000`-byte cache buffers | Capture both caches and metadata in the reviewed state, including unused slots, logical base ordering and record validation results |

[The storage module](xm1-upstream/emulator/storage_profile.py) retains two historical journal sources: `main-read` and `restored`. The latter checks a restoration log and readback-validation artifact in addition to main/custom cache metadata and buffers. Those filenames describe historical provenance, not a requirement to perform a restoration on your camera. An independently acquired profile should use its own capture/validation record without carrying out a write merely to imitate that history.

### Source contracts for an independent X70 acquisition implementation

The following contracts were inspected in the acquired x70view `40406daf52228ace99cb1a3e322f3193f96b0283` reference. The helper names identify missing public tooling, not programs bundled here or commands to execute. Their source-intended read purpose does not make their debug changes, callback redirection or storage-loader effects state-free. An independent implementation must rederive these contracts against its own matching firmware and complete the review above.

| Historical helper/source | Input and output contract | State and availability boundary |
| --- | --- | --- |
| `tools/x70_usb.py`, configuration backup | Checks configuration sizes `(0x420000, 0x100)` for both `mb` views and mapped section headers. One `mb=0` byte request invokes the loader of both complete logical banks. Five descriptor words at X70 `0x00aeab94` are compared with the decoded section; words 0, 1, 3, 4 must be `0x28`, `0x3a3`, `1`, `0x3cb`. Read bank 0 from `0x439c4000`, length `0x28 * 0x20000`; bank 1 follows at that base plus that length, with length `0x20000`. Outputs `storage-bank-0.bin`, `storage-bank-1.bin` and `configuration-backup.json` with descriptor, size and hash information. | Not bundled. Repeated small `mb=0` reads reload whole banks and are not equivalent to streaming the complete logical buffers. Temporary debug enable and loader effects require review. No raw physical NAND/OOB/ECC claim. |
| `tools/x70_usb.py`, RAM configuration/snapshot | The full configuration RAM view is `0x42068000`, length `0x420000`, not the shorter `cfgdata` window used by `owner_settings.py`. The backup helper outputs `cfgdata-ram.bin` with the original debug byte substituted at offset `0xf7` and records that transformation. Snapshot journal inputs include metadata `0x00c26c00`/`0x20`, buffers `0x00be6c00`/`0x40000`, descriptors `0x00be5a48`/`0x1b8` and record state `0x00be3a2c`/`0x201c`, read through the uncached alias. | Not bundled. Debug access may change `0xf7`. Record transformed-file versus live-byte provenance; a substituted backup byte is not an untouched capture. Merely reading the current shared cache does not prove both main and custom journals were loaded. |
| `tools/x70_bulk_capture.py`, `tools/x70_bulk_ram.py`, `research/bulk-ram/README.md` | The X70 stock `0x450006`, tag `0x80` path supplies the bulk-read pointer. Its E1 decoder accepts a big-endian offset; the read callback adds pointer and offset modulo 32 bits. The historical pointer was `0x4e7283c0`, so the source offset for target address `A` was `(A - 0x4e7283c0) & 0xffffffff`. The 512 MiB dump covers offsets `0..0x1fffffff` through uncached addresses `0x40000000..0x5fffffff` and produces `dram.bin` plus capture metadata. | Not bundled. Requires reviewed X70 identity, enabled debug access, stock callback and matching code spans. This offset mechanism is a version-bound finding, not an arbitrary-memory-read permission. A bounded runner must verify pointer/length behavior and finite progress before use. |
| `tools/x70_read_main_journal.py` | Uses the camera's own reader `0x0094d994` through callback slot `0x017d291c`, whose stock target is `0x010ef130`. The descriptor referenced at `0x00be3a30` is range-checked; the response must identify `0x450006` and point to main record `0x00c28280`. The helper reads `0x1000` bytes as `main-current-record.bin` and captures the journal cache. | Not bundled. Temporarily redirects a callback, changes eight output-state bytes at `0x017d548c` and may enable debug. These are camera RAM writes requiring operation-specific review even though the intended storage operation is a read. Source cleanup in `finally` is not a whole-session restoration guarantee. |

The historical main and custom readbacks reuse the same cache area: metadata at uncached address `0x40c26c00` and two buffers at `0x40be6c00`. Their descriptor pointers are at `0x00be3a30` (main) and `0x00be4a38` (custom), with current-record pointers `0x00c28280` and `0x00be5c00`. After the matching reader populates the cache, capture metadata, buffers and metadata again; reject a changed metadata read. Validate the two logical bases against the selected journal: main uses `{0x00ac0000, 0x00ae0000}`, custom `{0x00b00000, 0x00b20000}`. Preserve metadata ordering when assigning the two buffers. These contracts appear at acquired `x70_usb.py:124-148,246-253` and `x70_restore_settings.py:36-38,160-178`. The snapshot helper has no separately named custom-current-record region. A new read-only custom-journal operation must establish its descriptor/reader path rather than depend on the restoration helper.

`tools/x70_restore_settings.py` is a persistent-write helper, not an acquisition prerequisite. It stages payloads, invokes stock main/custom writers, appends records and readbacks, and carries the historical settings-restoration artifacts consumed by `source='restored'`. Do not run or reproduce it to obtain equivalent inputs. Its source also describes a volatile service-exit guard: stock JIG teardown can reset settings in a particular state. This is a reason to review lifecycle/exit behavior before a capture, not a recipe to set a guard or a guarantee that a different body's session is safe.

### Missing runnable acquisition boundary

The public closed transport supports X-M1 fixed probes, declared `dump-ram` segments, bounded config plans and the documented raw-media operation. It does not provide an arbitrary memory reader, a complete normal-REC X70 DRAM capture, a logical configuration-bank exporter or a main/custom-cache exporter matching the historical inputs above. X-M1 `read-config` is bounded to its documented configuration channel and `read-media` is not a substitute for X70 logical banks. Raw-media mode has its own shutdown/staging hazards and power-cycle requirement.

The retained profile modules validate and consume captures; they do not acquire them. The source-access limitation for the recorded x70view pin and unavailable private acceleration source is recorded in [the third-party source inventory](../third_party/README.md); remaining source-provenance work is tracked in [GitHub Issues](https://github.com/skeet70/latent-image/issues). An inaccessible historical acquisition script is not public tooling. The remaining implementation boundary is a reviewed, public, model/version-bound acquisition operation with the exact windows/cache producers and private provenance outputs above, followed by a separate profile integration. Until that exists, the table and protocol facts specify the obtainable data and review work, not an executable end-to-end X70 capture command.

## Prepare a separate body/version-bound profile

The output of independent acquisition is your own dataset, not a replacement for the reference's private files. Give it a distinct profile identity and leave the historical implementation and metadata intact. There is currently no generic importer, profile schema or CLI selector that turns an arbitrary capture directory into a validated settings profile. New project-authored acquisition/validation tooling follows the repository's Haskell/Rust policy; retained upstream Python is not a default for new tools.

A complete profile integration must establish:

1. **Identity and provenance:** exact body/model/version binding, firmware and ROM identities, tool/source revisions, capture state, sequential timing, every file's length/hash and the relationship between the whole capture and all extracted windows. Public metadata must exclude private identities and payload bytes unless separately approved.
2. **Storage geometry:** configuration-bank descriptors, cache metadata and logical block order; record lengths, latest-record selection, sequence wrap rules and integrity/parity validation from the matching firmware. Preserve all original journal slots. Do not reuse the historical sequences or assume one body's logical bank payload is another's calibration.
3. **Active versus persistent state:** reconcile preference payloads against validated records. The historical implementation accepts exactly three active/persistent differences and three configuration-view differences; these are not generic allowances. Derive and justify your own exclusions. Do not copy debug flags, tasks, pointers, lens/sensor caches or arbitrary RAM execution state into persistent input.
4. **Synthetic boundaries:** the reference maps logical data onto chosen physical blocks and synthesizes first/last-page managed tags. It does not capture actual physical placement, OOB/ECC or bad blocks. Any appended successor record is a host-generated fixture, not a camera-written journal record. Keep synthetic placement/parity and any inferred MCU reply explicitly labeled.
5. **Boot and observable behavior:** integrate a distinct reviewed selector, then exercise matching stock firmware on the host. Check ordinary REC entry, Q layout/selection, relevant saved fields and closed-menu state against independent expectations from your own body. Do not reuse `verify_owner_state()`'s fixed field values or navigation path as universal acceptance. Keep UI emulation, persistence, physical recovery, electrical timing and autofocus fidelity as separate claims.

The historical `owner_inputs()` appends one synthetic successor using the active preference payload while retaining existing slots. It also requires the captured main/custom records to match the restored caches, an erased append slot, a framed remembered MCU packet and the pinned firmware. A new profile needs its own accepted construction; removing these checks or renaming your directory does not meet that requirement.

Preserve your raw dataset privately and publish only the reusable method, reviewed profile code, non-private metadata and scoped findings permitted by [the publication policy](../docs/principles.md#publication-and-asset-boundaries). The public X-M1 erased-settings viewer remains available without obtaining anybody else's settings captures. Open tooling and integration work is tracked in [EMU-OWN-INPUTS-01](https://github.com/skeet70/latent-image/issues/1); `E-OWNER-INPUT-ACQUISITION-PUBLIC-150` records the guide's source-inspected scope, not a fresh capture or a successful independent reproduction.
