# Fujifilm X-M1 recovery kit

Build an X-M1 SD BOOT candidate directly from a **known-good firmware DAT**, prepare its card with two complete readback checks, and retain the unchanged DAT for the ordinary updater. All commands run on the Linux host; none operates the camera.

**The general route is now exercised through persistent restoration in the host model, not demonstrated on a physical camera. This pattern _did_ recover an actually bricked camera, and was extrapolated into this more generic tool.** A firmware-derived 1.50 recovery card boots both CPUs with independently damaged installed applications, enters the ordinary updater, runs its real confirmation/write path and produces committed main media matching the unchanged restore DAT. A new process boots that media without the recovery card into shooting and working menus. Supplied 1.01, 1.20 and modified 1.50 inputs also generate source-bound cards; their complete restoration runs remain unverified.

## Start here

1. Preserve the failed firmware file, its original stock base, any last-known-good modified firmware, build/patch metadata and body-specific backups. Keep the original files unchanged. Record the camera model, installed version, modification and observed failure. If a write might still be running, do not interrupt it merely because the LCD is dark.
2. If the camera still reaches the ordinary firmware-update menu, prefer Fujifilm's documented update route with an authentic X-M1 firmware file. Follow the [official X-M1 firmware instructions](https://www.fujifilm-x.com/en-us/support/download/firmware/cameras/x-m1/). This kit does not disable model/checksum checks or establish downgrade compatibility. The 1.01 same-version RAM extension is documented below.
3. For a software failure with intact SD BOOT entry, use [firmware-derived RAM recovery](#firmware-derived-ram-recovery) with a known-good stock base or previously working modification. The failed DAT is useful for diagnosis but is not an input required to generate the card.
4. Preserve [firmware triage](#firmware-triage-for-other-failures) results if generation refuses the layout or native/loader bindings. Do not bypass those guards or select the recorder profile merely because the LCD is dark.
5. If you already have a recovery image independently reviewed for your exact failure, use [the two-PASS card preparation tool](#prepare-a-recovery-card-two-pass-checks). It accepts other image sizes and expected hashes; it does not establish that those images are safe to boot.

| Capability | Available now | Boundary |
| --- | --- | --- |
| Write an SD BOOT image and verify it before and after physical reinsertion | `//recovery:prepare-card` | Any nonempty sector-aligned image with an expected SHA-256; no automatic camera operation |
| Validate DAT checksums, inspect segment layout, compare firmware and extract analysis components | `//recovery:inspect-firmware` | Observed X-M1-style DAT layout; checksum validity is not vendor/model authenticity or installed-byte identity |
| Generate a RAM recovery image that requests the ordinary updater at startup | `//recovery:build-image restore` | Source-bound 1.01/1.20/1.50 updater families; unchanged restore DAT and visible confirmation; complete modeled restoration exercised for supplied 1.50 |
| Generate a diagnostic RAM boot image without requesting the updater | `//recovery:build-image ram-boot` | Derived components and RAM ranges; bound X-M1 native family and application-loader structures |
| Reproduce the demonstrated recorder-v2 rollback | `//recovery:build-image recorder-v2-v5` | Exact official X-M1 1.50 input; restores RC0 block 14, not complete stock firmware |
| Persistently restore damaged installed applications using the ordinary updater | Exercised in the full-system model for supplied 1.50 | Complete main-media comparison and independent no-card shooting/menu startup; physical recovery, unique-data preservation and cross-version restoration remain unaccepted |
| Recover corrupted calibration, damaged early boot metadata or physical hardware | Not established | May require genuine body-specific backups, factory tools or hardware repair |

## Host setup and files

Use a repository checkout containing this folder and its shared build dependencies, not a detached copy of the folder: the targets reuse the firmware parser, encoder, SD BOOT layout builder and target recovery sources. Build the explicit targets below; the recovery tools do not require the retained X70-derived emulator. Install Nix and devenv using their official instructions; [build integration](../build-support/README.md) documents the pinned Linux x86_64 toolchain.

From the repository root:

```sh
devenv shell
mkdir -p private-data
bazel build //recovery:build-image //recovery:prepare-card //recovery:inspect-firmware
```

Keep downloaded firmware, generated images, dumps, logs and backups under ignored `private-data/`. Body-specific backups need independent durable copies. A stock DAT contains vendor templates, not a backup of your camera's unique calibration. Never write a DAT, extracted component or logical firmware block directly to NAND or an SD card as though it were a recovery image.

`bazel run` commands below resolve firmware, image and output paths relative to the repository root. Output directories must be new, with an existing parent beneath `private-data/`. Help, builds and firmware-analysis commands do not access a card or camera. Card preparation explicitly uses a separate host reader and host `sudo`.

## Firmware-derived RAM recovery

Choose a **known-good** X-M1 DAT, preferably authentic stock firmware from the base of the failed modification. Do not give the generator the newly broken build and expect it to repair arbitrary program logic. A previously working patched DAT is allowed when its native family, standard loaders and declared layout pass the same bindings. Matching the installed base minimizes cross-version retained-data uncertainty; it is not proof of downgrade eligibility.

```sh
bazel run //recovery:build-image -- restore \
  private-data/known-good.DAT private-data/ram-recovery
```

No failed recorder DAT, RAM dump, extracted decode, body backup or patch manifest is required. The builder validates DAT checksums and media ranges, decodes the supplied applications with the strict decoder, derives their load/BSS and plain-image extents, binds loader instructions and generates the complete card. The output directory must not already exist. Stop on a failure and do not deploy partial output.

| Output | Purpose |
| --- | --- |
| `firmware-ram-recovery-sd.img` | Complete image for the two-PASS whole-card writer |
| `FFSYS.BIN` | Firmware-derived ROM configuration |
| `FFBOOT.BIN` | Linked 4-KiB bootstrap with checked region metadata |
| `FFRECOV.BIN` | Sequential RAM-loading body |
| `FWUP0002.DAT` | Byte-for-byte unchanged known-good input, included in the card filesystem |

The builder prints the complete image's SHA-256. Keep that value with the reviewed image and supply it to the writer:

```sh
bazel run //recovery:prepare-card
```

The bootstrap reads the supplied native records, both kernels, resources 2/3, decoded applications 4/5 and plain image into RAM, then clears the declared application BSS. Raw components retain their complete declared NAND-block windows. Sector padding is checked against meaningful ranges and trimmed from BSS coverage. CRC, complete-range preflight and disabled-data-cache guards apply before native entry.

Only the **RAM copies** are patched: native startup skips replacing preloaded code/table, the table advertises residency, application 4 skips the installed source reads for image 5 and resources 2/3, and the existing plain-image preload branch keeps its publication callbacks. The restore DAT is not patched. Load addresses and application call sites come from the supplied firmware, not reused 1.50 application addresses.

`restore` additionally requests the ordinary updater during RAM startup. It replaces only the held-DISP/BACK branch, retaining the permission bit and existing power, door, USB and JIG guards. It does not call the writer directly or skip identity, checksum or confirmation checks. For supplied 1.01, the RAM body-version comparison is extended from strictly greater to greater-or-equal, matching the inspected 1.20/1.50 body policy. The DAT version is not falsified. Changed or unrecognized updater code is refused.

The `restore` card uses a FAT32 volume with one-sector clusters. The stock application classifies FAT from the physical card's CSD capacity, so the small FAT16 diagnostic partition is unsuitable for updater use on ordinary SDHC cards. `ram-boot` retains that FAT16 diagnostic carrier and does not request the updater.

### Persistent restoration after SD BOOT entry

These are the ordinary-updater steps, including the persistent-write commitment and independent restart check. The complete 1.50 route has host-model evidence, not physical acceptance for the new image. Do not substitute the historical one-block rollback's entry/status signals for this bootstrap's outcome.

1. Generate from a known-good DAT and complete both card PASS checks. Preserve body-specific backups. Use stable power, close the card and battery doors, and disconnect USB. Keep the recovery card inserted throughout startup and updating; it also carries `FWUP0002.DAT`.
2. Enter SD BOOT using a method established for your body. The [historical entry observation](#camera-entry-and-result-demonstrated-profile-only) records our method and its scope. Neither a dark LCD nor the historical status-20 signal proves this new image has reached the updater.
3. Once the ordinary updater screen appears, inspect its BODY version and offered firmware. `restore` requests this screen in the same RAM session; do not power off and try ordinary DISP/BACK startup, which would boot the still-damaged installed application.
4. Select **BODY**, not LENS. Follow the displayed update and power/battery-cover confirmations. Before the final affirmative confirmation, an unexpected model, version, missing file, refused update or abnormal screen is a reason to stop without authorizing a write. Do not force eligibility or invoke a service writer.
5. The affirmative confirmation that starts updating is the persistent-write commitment. Do not press controls, open either door, remove the card, disconnect power or impose a timeout on a stationary progress screen. The ordinary writer erases the boot trailer before programming the main image and restores body-data fields through the DAT's auxiliary lists.
6. Wait for the updater's explicit successful completion and power-off instruction. An LED change, working RAM menu or displayed version is not completion. On error or ambiguous progress, preserve power and record the screen; do not automatically retry.
7. After explicit successful completion, switch OFF, remove the recovery card while unpowered, and start once without the card or boot buttons. Verify ordinary menus, photography, normal-card save/playback and retained settings. Persistent recovery requires this no-card startup, not just a usable RAM application.

A stock DAT supplies templates, not your unique calibration. This route depends on readable compatible retained data and the stock updater's preservation machinery. Cross-version compatibility, damaged adjustment storage and custom firmware-specific dependencies require separate evidence. See the mapped [update phases and preservation limits](../reports/install-procedure.md#2-what-the-camera-does-when-inside-the-update).

### Acceptance and remaining dependencies

The bootstrap handoff is exercised for supplied 1.01, 1.20, 1.50 and a modified 1.50 input through modeled FAT reads of `FFRECOV.BIN` and raw reads of the **complete generated card**, preserving the captured caller frame. Protocol fixtures also exercise CRC corruption, invalid/overlapping ranges, short/error reads and cache-enabled refusal.

The complete restoration smoke uses supplied 1.50, a complete FAT32 card and an independent installed-media fixture with the first application-4 and application-5 NAND blocks zeroed. That fixture fails ordinary cold boot. Starting with the real bootstrap's terminal CPU0/RAM state and a separately cold CPU1 reaches the rendered updater. Stock ADC button pulses select BODY and confirm the update; no writer event, application checkpoint or updater state is injected. The stock writer reaches stage 4/status 0. Exported committed main media, including the completion trailer and capacity padding, matches the unchanged DAT. An independent ROM-start process retains only that NAND backing, with no card or recovered RAM, and reaches the first-boot prompts, shooting display and working shooting menu.

The fixture contains vendor data, not a captured unique calibration backup. It does not prove preservation of a real body's calibration/settings, bad-block recovery or cross-version compatibility. The model renders the updater title, selections, confirmations and processing display, but its body/version/completion text is incomplete; the physical success wording is not visually verified here.

Application startup still includes NAND-manager initialization and table-driven supplemental reads outside the standard component loaders. The inspected 1.50 fixed logical-block `0xd9` call passes a transfer count of zero, so it does not itself preload block data; the manager and lock dependencies remain. Supplemental reads have not been replaced with guessed vendor templates. Custom patched firmware can add further dependencies. Damaged retained data, mapping state or calibration can therefore remain a blocker even when the supplied code is healthy.

Do not treat host completion as a tested physical field-recovery prescription. New bytes need independent review and an owner-approved bounded camera run under the [camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs). Physical acceptance must include updater completion, no-card startup and retained body data. No new physical entry sequence or success LED packet has been established for this bootstrap; the historical profile's status-20 packets do not apply.

Failures before SD BOOT entry, damaged controller configuration, incompatible native records, unsupported loader structures, corrupted body-specific data and hardware faults are not solved merely by supplying another DAT. A stock DAT cannot reconstruct unique calibration. Unsupported inputs fail closed; they are a binding investigation, not a reason to edit the hash or copy old addresses.

### Host verification

```sh
bazel test //recovery:ram-rescue-tests //host/firmware:boot-card-tests
bazel build //recovery:ram-rescue-engine
bazel-bin/recovery/ram-rescue-engine \
  private-data/ram-recovery/FFBOOT.BIN private-data/ram-recovery/FFRECOV.BIN \
  fat private-data/fat-recovery.ram none
bazel-bin/recovery/ram-rescue-engine \
  private-data/ram-recovery/FFBOOT.BIN private-data/ram-recovery/firmware-ram-recovery-sd.img \
  raw private-data/raw-recovery.ram none
```

RAM output and companion `.state.json` files must be new; input files must remain unchanged during execution. The engine executes the real linked bootstrap with captured ROM MPU regions and coherent RAM aliases. ROM readers consume regular-file bytes. The companion records terminal CPU0, SRAM and modeled MPU state for independent native continuation. Cache/barrier timing, NAND, CPU1 and native continuation are not modeled by this engine. `status=handoff` means native entry was reached and stopped before execution.

Full-system continuation is optional research, not a dependency of these recovery tools. The retained upstream continuation tool, `emulation/xm1-upstream/tools/continue_rec_scanout.py`, accepts `--recovery-ram RAM_FILE --cold-profile PROFILE --sd-image CARD --installed-media INDEPENDENT_MEDIA --menus`; it validates the CPU0 companion and starts CPU1 cold. Use its `nand NEW_FILE` command only after native completion to export committed storage. Start a separate process with `--profile PROFILE --installed-nand NEW_FILE`, without `--recovery-ram` or `--sd-image`, for the no-card check. A healthy application checkpoint is not recovery evidence. The emulator's [x70view credit and terms](../LICENSING.md#inherited-source-and-public-research) apply.

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

Addresses are file-declared fields, not runtime observations. The root segment-table pointer selects the stock or compact layout before rows are read; unsupported pointers are refused. The segment-6 load/BSS words do not give the plain-image loader's destination. The inspector derives that separate destination word's media address from the validated layout: `0x1401a8` for stock or `0x140158` for the compact host control.

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

If generation refuses a binding, preserve the inputs and analysis rather than trying older experimental cards. The information needed to develop a new persistent repair is listed in [adding another repair profile](#adding-another-repair-profile). A damaged application with intact early boot is different from damaged boot metadata, calibration, a lens failure or an electrical fault. See [failure classification](../spec/04-recovery.md#103-required-failure-classification).

## Demonstrated repair: `recorder-v2-v5`

### Applicability and effect

This profile repairs the specific boot failure caused by this project's AF recorder revision-2 activation before payload residency. It restores the seven changed compressed-image bytes in logical/physical block 14 to the prior RC0 film-simulation build. The recorder payload in block 71 remains inert; this is not a complete stock reinstall or removal of every modification.

The failure input was the exact recorder DAT with SHA-256 `72a78c5595f40f88af26c4ed429529a756f759aa9b8e5c5704a745613f9ed0a8`; the known-good RC0 reference was `acb0fbbb9507421f48a6254920cafb5ce20e1fd6bf821bc80c912c1f99c29ca8`. The target code checks the expected recorder call, validated mappings and byte-exact recorder block under the storage lock before submitting one stock lower-layer program operation. It requires both initial mappings to physical block 14. These guards do not establish compatibility with another failure or guarantee recovery on another body.

Native startup executes before the repair. The one-block repair bound is not a guarantee of settings or other retained-state preservation outside the observed result.

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
3. Enter the SD BOOT image location and its expected SHA-256 from its build/review. The hash is required for every image, including the historical v5 image. **Do not compute a hash of an untrusted image and treat that as a safety review.**
4. Unmount all partitions of that card using the desktop or `umount`, without ejecting it, then press Enter. The tool refuses mounted/swap-active, read-only, non-disk, undersized or non-512-byte-sector devices and checks the image's alignment, size and hash. It does not automatically unmount another filesystem or retry a failure.
5. It writes with `dd ... conv=notrunc,fsync`, flushes host buffers and compares the entire image-length prefix. `PASS: image readback matches` appears only after successful comparison.
6. Physically remove the SD card from the reader and reinsert it, then press Enter. Enter its mount point or whole device again; its device name may have changed. If it has no mount point, use the whole device. Do not mount it merely to obtain a path. Reject format/repair prompts.
7. Unmount any automatically mounted partitions without ejecting, then press Enter. The tool rechecks the image and card, flushes buffers and performs a second complete **read-only** comparison. The second PASS verifies the reinserted card contains the expected image. It does not write again.

Stop on any error. A first PASS without the second PASS is incomplete preparation. Enter acknowledges physical reinsertion; software cannot prove you performed it. Both PASS results establish host preparation, not camera compatibility or successful recovery.

After the second PASS, remove the card. Do not format, repair or copy files onto its SD BOOT layout. Disable desktop automount if it would change the prepared filesystem. The demonstrated card uses a raw/FAT dual-path research layout; copying `ffboot.bin`/`ffsys.bin` onto a normally formatted card does not reproduce it.

For an image-only check with no card access:

```sh
bazel run //recovery:prepare-card -- --verify-image \
  private-data/ram-recovery/firmware-ram-recovery-sd.img \
  --sha256 EXPECTED_64_HEX_DIGIT_SHA256
```

This checks supplied identity, not whether the image is bootable, reviewed or appropriate for your firmware.

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

Interpret the mapping fields above for successful status 20/21. Earlier refusal packets can carry unvalidated/default fields; do not treat those counts as physical-map evidence.

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
