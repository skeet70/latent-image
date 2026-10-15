# Fujifilm X-M1 recovery kit

Tools and instructions for preparing SD BOOT recovery cards, diagnosing firmware differences and reproducing supported repairs. Intended for owners of a bricked X-M1 and for future recovery work on stock or modified firmware. All commands in this folder run on the Linux host; none connects to or operates the camera.

**This is not yet a universal unbricker.** Card preparation and firmware analysis are reusable. The only demonstrated persistent repair implemented here is the `recorder-v2-v5` profile for this project's exact X-M1 1.50 recorder revision-2 failure. An official firmware DAT alone cannot tell a tool what failed, what is actually installed or which physical NAND blocks are safe to overwrite.

## Start here

1. Preserve the failed firmware file, its original stock base, any last-known-good modified firmware, build/patch metadata and body-specific backups. Keep the original files unchanged. Record the camera model, installed version, modification and observed failure. If a write might still be running, do not interrupt it merely because the LCD is dark.
2. If the camera still reaches the ordinary firmware-update menu, prefer Fujifilm's documented update route with an authentic X-M1 firmware file. Follow the [official X-M1 firmware instructions](https://www.fujifilm-x.com/en-us/support/download/firmware/cameras/x-m1/). This kit does not bypass the normal updater's eligibility or downgrade checks.
3. If you installed this project's exact recorder revision-2 build on 1.50, read [the demonstrated repair](#demonstrated-repair-recorder-v2-v5).
4. For another modification, firmware version or unknown failure, use [firmware triage](#firmware-triage-for-other-failures) before selecting a repair. Do not try the recorder profile merely because the camera is an X-M1 or has a black screen.
5. If you already have a recovery image independently reviewed for your exact failure, use [the two-PASS card preparation tool](#prepare-a-recovery-card-two-pass-checks). It accepts other image sizes and expected hashes; it does not establish that those images are safe to boot.

| Capability | Available now | Boundary |
| --- | --- | --- |
| Write an SD BOOT image and verify it before and after physical reinsertion | `//recovery:prepare-card` | Any nonempty sector-aligned image with an expected SHA-256; no automatic camera operation |
| Validate DAT checksums, inspect segment layout, compare firmware and extract analysis components | `//recovery:inspect-firmware` | Observed X-M1-style DAT layout; checksum validity is not vendor/model authenticity or installed-byte identity |
| Generate the demonstrated recorder-v2 recovery image from official firmware | `//recovery:build-image recorder-v2-v5` | Exact official X-M1 1.50 input; restores RC0 block 14, not complete stock firmware |
| Generate a safe persistent repair for arbitrary stock/patched firmware | Not implemented | Requires an identified failure, version-specific bindings, preserved calibration, bounded writer and independent review |
| Recover corrupted calibration, damaged early boot metadata or physical hardware | Not established | May require genuine body-specific backups, factory tools or hardware repair |

## Host setup and files

Use the complete repository, not a detached copy of this folder: the targets reuse the existing firmware parser, encoder, SD BOOT layout builder and target recovery sources. Install Nix and devenv using their official instructions; [build integration](../build-support/README.md) documents the pinned Linux x86_64 toolchain.

From the repository root:

```sh
devenv shell
mkdir -p private-data
bazel build //recovery:build-image //recovery:prepare-card //recovery:inspect-firmware
```

Keep downloaded firmware, generated images, dumps, logs and backups under ignored `private-data/`. Body-specific backups need independent durable copies. A stock DAT contains vendor templates, not a backup of your camera's unique calibration. Never write a DAT, extracted component or logical firmware block directly to NAND or an SD card as though it were a recovery image.

`bazel run` commands below resolve firmware, image and output paths relative to the repository root. Output directories must be new, with an existing parent beneath `private-data/`. Help, builds and firmware-analysis commands do not access a card or camera. Card preparation explicitly uses a separate host reader and host `sudo`.

## Firmware triage for other failures

Keep three roles distinct:

- **Stock base:** the authentic official firmware from which a modification was built.
- **Failed input:** the exact DAT installed before the failure, if available. A file you intended to install is not proof of the camera's actual bytes.
- **Known-good restore reference:** firmware that previously worked, possibly your own patched build. This need not be stock; the demonstrated repair restores RC0 and retains its film simulations.

Inspect each input:

```sh
bazel run //recovery:inspect-firmware -- inspect private-data/stock-base.DAT
bazel run //recovery:inspect-firmware -- inspect private-data/failed.DAT
bazel run //recovery:inspect-firmware -- compare \
  private-data/known-good.DAT private-data/failed.DAT
```

The inspector verifies the three declared checksum ranges, bounds the segment table and extents, and prints the firmware hash, raw version words, identity-record hash and load/BSS/flash layout. Comparison reports identity/restore-list differences, early boot/configuration/kernel equality, adjustment-template differences and differing bytes per logical 128-KiB firmware block. It does not read the camera, decompress the applications or produce a flash plan. Even zero DAT differences do not rule out a failed write, physical storage failure or corrupt body state.

The inspected 1.01, 1.20 and 1.50 inputs have equal early configuration, boot records and kernel/loader bytes, but different application sizes, BSS boundaries and image-5/plain-image locations. That equality does not make the 1.50 application's addresses reusable on another version. The older inputs' checksum consistency does not establish vendor authenticity. See [the scoped comparison](../reports/firmware-extraction.md#supplied-101-and-120-comparison).

Extract components for host analysis without relying on private scripts:

```sh
bazel run //recovery:inspect-firmware -- extract \
  private-data/stock-base.DAT private-data/stock-analysis
bazel build //lz:fflzd
bazel-bin/lz/fflzd private-data/stock-analysis/segment-4.bin 0 \
  private-data/stock-analysis/decoded-4.bin
bazel-bin/lz/fflzd private-data/stock-analysis/segment-5.bin 0 \
  private-data/stock-analysis/decoded-5.bin
```

Extraction produces `rom-configuration.bin`, `native-boot-records.bin`, `section-table.bin` and `segment-0.bin` through `segment-7.bin` in media-byte form. The strict decoder takes the compressed segment file, record offset zero and a new output file. These are **analysis components, not bootable recovery images or physical NAND backups**. An unrecognized layout, out-of-file extent or bad checksum stops inspection/extraction.

If no supported repair matches, preserve the inputs and analysis rather than trying older experimental cards. The information needed to develop a new repair is listed in [adding another repair profile](#adding-another-repair-profile). A damaged application with intact early boot is different from damaged boot metadata, calibration, a lens failure or an electrical fault. See [failure classification](../spec/04-recovery.md#103-required-failure-classification).

## Demonstrated repair: `recorder-v2-v5`

### Applicability and effect

This profile repairs the specific boot failure caused by this project's AF recorder revision-2 activation before payload residency. It restores the seven changed compressed-image bytes in logical/physical block 14 to the prior RC0 film-simulation build. The recorder payload in block 71 remains inert; this is not a complete stock reinstall or removal of every modification.

The failure input was the exact recorder DAT with SHA-256 `72a78c5595f40f88af26c4ed429529a756f759aa9b8e5c5704a745613f9ed0a8`; the known-good RC0 reference was `acb0fbbb9507421f48a6254920cafb5ce20e1fd6bf821bc80c912c1f99c29ca8`. The target code checks the expected recorder call, validated mappings and byte-exact recorder block under the storage lock before submitting one stock lower-layer program operation. It requires both initial mappings to physical block 14. These guards do not establish compatibility with another failure or guarantee recovery on another body.

**Observed on one body:** exact v5 rollback reported status 20 and mapping fields 1/1. Ordinary startup after power removal, photography, normal-card operation/playback and retained film simulations/settings, including C-slot color +2, were subsequently owner-reported working. Detailed lens/AF and new recipe acceptance remain separate feature work. The [durable result](../reports/sd-boot-card-entry.md#260-physical-rollback-readback-status-20) retains the evidence scope. Preserve an already successful recovery card; the working camera does not need another recovery run.

This closes the recorder rev-2 incident and the reported recovery-adjacent checks. It does not claim formal R4 acceptance, universal recovery, or detailed lens/AF and new-recipe acceptance; [current principles](../docs/principles.md#scope-and-evidence) and [recovery status](../reports/recovery-status.md) own those distinctions.

### Generate the image

Obtain the official X-M1 1.50 `FWUP0002.DAT` yourself. Required SHA-256: `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`. Other stock versions, models and modified DATs are refused for this profile.

```sh
bazel run //recovery:build-image -- recorder-v2-v5 \
  private-data/FWUP0002.DAT private-data/recorder-v2-recovery
```

The image is `private-data/recorder-v2-recovery/card/physical-block14-recovery-sd.img`. The command decodes stock image 4, applies [the checked-in RC0 patch metadata](rc0-patch-set.json), re-encodes/rebuilds the exact RC0 container, reconstructs the four known recorder word substitutions and invokes the existing repair-card builder with the compiled v5 stages. A failed-recorder DAT, camera dump and private build manifest are not needed. Intermediate decoded images and containers are removed; final component artifacts stay private beside the image.

The build must finish with PASS and produce exactly **17,826,304 bytes**, SHA-256 **`15d0cc28fc76d7d78ecf59a455284761333cd0e345929d3f88393fe991f5f343`**. This is the complete successful image, not merely an equivalent-looking layout. Changed inputs, source or toolchain output that changes the image fails the final identity gate. Do not deploy partial output after failure.

## Prepare a recovery card: two PASS checks

Requirements: an expendable SDHC card, separate host card reader, the checked-in environment and working host `sudo`. Keep the camera disconnected. Back up card contents first. This overwrites the **whole disk's partition/filesystem bytes**, not a file inside its mounted filesystem. Only the image-length prefix is written; the remaining card capacity is unchanged.

```sh
bazel run //recovery:prepare-card
```

1. Enter the expendable card's current mount point, such as `/run/media/you/CARD`, or whole device, such as `/dev/sdb`. A mount point resolves through its partition to the parent disk. The tool displays size, model and mounted children. Identify the card yourself; never reuse an old device name blindly.
2. Confirm erasure by typing the exact displayed whole-disk path. All partitions on that disk are affected.
3. Enter the SD BOOT image location and its expected SHA-256 from its build/review. For the demonstrated `recorder-v2-v5` image, leave the hash empty to use the pinned hash above. For another reviewed recovery image, supply its own expected hash. **Do not compute a hash of an untrusted image and treat that as a safety review.**
4. Unmount all partitions of that card using the desktop or `umount`, without ejecting it, then press Enter. The tool refuses mounted/swap-active, read-only, non-disk, undersized or non-512-byte-sector devices and checks the image's alignment, size and hash. It does not automatically unmount another filesystem or retry a failure.
5. It writes with `dd ... conv=notrunc,fsync`, flushes host buffers and compares the entire image-length prefix. `PASS: image readback matches` appears only after successful comparison.
6. Physically remove the SD card from the reader and reinsert it, then press Enter. Enter its mount point or whole device again; its device name may have changed. If it has no mount point, use the whole device. Do not mount it merely to obtain a path. Reject format/repair prompts.
7. Unmount any automatically mounted partitions without ejecting, then press Enter. The tool rechecks the image and card, flushes buffers and performs a second complete **read-only** comparison. The second PASS verifies the reinserted card contains the expected image. It does not write again.

Stop on any error. A first PASS without the second PASS is incomplete preparation. Enter acknowledges physical reinsertion; software cannot prove you performed it. Both PASS results establish host preparation, not camera compatibility or successful recovery.

After the second PASS, remove the card. Do not format, repair or copy files onto its SD BOOT layout. Disable desktop automount if it would change the prepared filesystem. The demonstrated card uses a raw/FAT dual-path research layout; copying `ffboot.bin`/`ffsys.bin` onto a normally formatted card does not reproduce it.

For an image-only check with no card access:

```sh
bazel run //recovery:prepare-card -- --verify-image \
  private-data/recorder-v2-recovery/card/physical-block14-recovery-sd.img
```

For another reviewed image:

```sh
bazel run //recovery:prepare-card -- --verify-image \
  private-data/other-recovery.img --sha256 EXPECTED_64_HEX_DIGIT_SHA256
```

The second command checks supplied identity, not whether the image is bootable, reviewed or appropriate for your firmware.

## Camera entry and result: demonstrated profile only

**Booting the recorder-v2 image can write persistent internal media.** Prepare stable power, preserve body-specific backups and record the LED continuously. An owner must knowingly approve the operation for their camera. This project's research runs additionally follow the [camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs), including independent review of new write-capable bytes. The host tool performs no camera operation and grants no approval.

The following is the empirical entry method used for the demonstrated profile, **not a universal X-M1 boot key or a procedure for other images**:

1. Start unpowered, with USB disconnected and the two-PASS verified spare card inserted. Use a stable battery or dummy-battery supply that will not be disconnected once recovery starts. Do not use the normal photography card.
2. Hold **Fn plus the inward sub-command-dial press**. Do not add full shutter. Use warm OFF/ON switch transitions at approximately 0.75 seconds per transition, with at most 40 transitions and a 60-second entry bound, ending ON. Do not remove the battery between transitions.
3. Stop switching immediately on an LED response. If it starts while OFF, make only the next ON transition and stop. On the solid entry marker, release the held buttons and leave power/card unchanged. The marker binds bootstrap entry, not completed repair.
4. Record a complete repeating status cycle and both mapping fields. Do not cut power, format the card, navigate settings, connect USB or invoke the ordinary updater while programming/status is unresolved. A dark LCD or visible UI is not the success test. The 60-second entry bound is not a timeout for an in-progress write or its slow status packets.
5. If entry is not obtained within the bound, stop toggling rather than continuing a button sweep. An absent marker does not identify the failure stage. If a write may have started or the signal is ambiguous, preserve power and evidence for diagnosis rather than blindly restarting. Stop immediately for abnormal heating, odor or other physical danger.

The v5 status emitter repeats three packet types. Distinguish them by the long synchronization interval, relative to a single data interval; count the following short intervals, not an arbitrary ten-second blink sample. The observed physical LED polarity was inverted relative to source helper names, so the sync appeared **dark**. Record the entire pattern rather than assuming a long bright interval identifies a packet.

| Sync length | Packet | Meaning |
| --- | --- | --- |
| Four data-delay units | Status count | 20: writer returned zero and complete RC0 readback matched without remap; 21: complete RC0 readback after stock remap, host-verified but not the demonstrated hardware outcome |
| Eight data-delay units | Boot mapping field | Physical block index minus 13; field 1 means physical block 14 |
| Twelve data-delay units | Final mapping-delta field | Absolute change in physical index plus one; field 1 means unchanged |

Only status **20 or 21** is the profile's successful write/readback result. Our demonstrated result was **20, 1, 1**. Other counts, fast equal blinking, a partial packet, UI alone or card-not-initialized text are not success. Preserve power and evidence on a refusal/error or uncertain decode; do not automatically retry. The payload's [status implementation](../host/firmware/media-view-diagnostic.S) and [result](../reports/sd-boot-card-entry.md#260-physical-rollback-readback-status-20) provide the detailed interpretation.

After an unambiguous successful readback and an approved return-to-normal decision, switch OFF, disconnect all battery/external power, remove the recovery card while unpowered and wait 30 seconds before reconnecting power. Start once without buttons or a card. Check ordinary startup, then photography, normal-card save/playback and retained settings/film simulations. Do not induce another failure to test the kit. Preserve the successful recovery card and image privately.

## Adding another repair profile

The entry bootstrap, image layout code, card writer, checksum inspector and extraction tools are reusable. **The persistent repair policy is not inferred from a firmware version or a diff.** A new profile must establish:

1. **Inputs and failure:** exact stock base, failed installation artifact and intended known-good restore reference; what is known about actual installed bytes and what remains inferred. A stock corruption or failed install may not match its DAT.
2. **Boot dependencies:** target-specific ROM/configuration and loader correspondence, storage initialization and an independent entry route. Do not borrow addresses from sibling Fujifilm models. Equal early code across versions does not accept their applications or this body's entry timing.
3. **Repair boundaries:** logical firmware extents, validated physical remap/ECC/bad-block handling, body-specific calibration/settings exclusions and exact before/after identities. DAT offsets are not physical NAND addresses. Cross-version reinstall can change restore lists and settings policy.
4. **Executable bindings:** each native call/ABI, code and data residency, fragment/reference lifetime through startup/log/IRQ paths, cache/peer behavior and stable power. The v2/v3/v4 repair candidates failed at these boundaries; changing only the four delta words is insufficient.
5. **Host proof and review:** deterministic build, exact input/linked-stage/image gates, canonical FAT/raw reconstruction, emulator execution of positive/refusal paths and complete media readback. Independently review new write-capable bytes before their first camera run.
6. **Owner-run acceptance:** a bounded entry operation, interpretable stage/write/readback signals and ordinary startup/functional checks for the stated failure class. A modeled writer or passing scaffold is not demonstrated camera recovery.

Relevant implementation homes: [entry bootstrap](../host/firmware/persistent-repair-bootstrap.S), [v5 target stage](../host/firmware/media-view-diagnostic.S), [repair-card builder](../host/firmware/BuildPersistentRepairCard.hs), [SD BOOT layout](../host/firmware/BootCard.hs), [container contract](../reports/update-container-rebuild.md) and [startup-lifetime host proof](../emulation/README.md#physical-block-14-recovery-startup-lifetime). Keep new profiles explicit and fail closed for unsupported inputs. The host triage/extraction workflow above supplies inputs for this work without requiring someone else's private camera artifacts.

## Distribution and limits

Distribute source, checked-in patch metadata, build rules and this guide. Users supply firmware locally; generated recovery images and body data stay private. **Do not put firmware-derived images in Git LFS:** LFS is still distribution of those bytes. Keep durable private copies of proven images and original backups.

Original project software uses AGPL-3.0-only; [LICENSING.md](../LICENSING.md) records upstream obligations and unresolved release decisions. That license does not grant permission to redistribute Fujifilm firmware, dumps or generated firmware-derived recovery images. No authentic X-M1 factory SD BOOT package or general calibration/boot-metadata recovery is provided here.
