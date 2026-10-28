# Recipe-only install procedure: complete recipe image (X-M1 1.50)

**Prepared, blocked on the candidate gate in section 2.** This replaces the withdrawn combined recipe/recorder install procedure. It covers one recipe-only image: the two compiled recipe rows, their Quick-menu entries and seven editable named slots, with the stock C custom setting preserved. A recipe-rows-only image is useful for isolating a problem, but it is not this feature. Nothing here authorizes a camera operation.

**Status, 2026-10-26.** No complete recipe-only candidate has passed the preinstall admission in section 2. Completion of the image is open in [#11](https://github.com/skeet70/latent-image/issues/11), placement in [#17](https://github.com/skeet70/latent-image/issues/17), boot publication in [#13](https://github.com/skeet70/latent-image/issues/13) and exact-candidate update acceptance in [#37](https://github.com/skeet70/latent-image/issues/37). The package builder emits its candidate record with `installable: false` and names seven unaccepted items: gap1 lifetime/ownership, nonallocator carve writer exclusion, heap exhaustion with the 64 KiB reduction, source fetch coherence, publication core ownership, independent final-byte review and camera behavior. The last is postinstall acceptance; the rest are preinstall admission. Because it emits no admission classification, its output is a stop. The reviewed admission classification and true flag are a #11 prerequisite, not an operator waiver. No candidate is admitted and no card is prepared.

## 1. Scope

The image this procedure installs must provide all of:

- the two compiled recipe rows in the film-simulation list and in the Q film cell;
- the seven editable named slots C1 to C7, with Save Current, selection or Recall, rename, edit and reset;
- the stock C custom setting unchanged, including its payload, its mode field and the CUSTOM SET behavior;
- only X-M1 1.50 settings the body already offers: base film simulation, dynamic range, highlight tone, shadow tone, colour, sharpness, noise reduction, white balance and ISO.

It does not install a recorder, an autofocus change, Classic Chrome or another RC2 processing change, a recipe-rows-only isolation image, or a stock reinstall. Those are different operations with their own review and their own go.

## 2. Candidate binding gate

The procedure binds exactly one candidate record produced by the build of the exact image to be installed. A historical container, a re-download, a hand-edited manifest or an image emitted for another feature is not a candidate. The record must carry the following, and a missing, stale or inconsistent field stops the procedure before the card.

| Required field or check | Required condition |
| --- | --- |
| `schema` | `recipe-executable-package-1` |
| `firmware`, `firmware_sha256` | Exact output DAT path and digest (DAT file); the file prepared for the card is byte-identical |
| `stock_sha256` | `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`, the pinned official X-M1 1.50 DAT |
| `inputs` | Every builder input with its digest: the stock DAT, the bank and D3 specs, the linked kernel, the boot fragments, the linker scripts and every source file the build used |
| `installable`, `unaccepted` | `installable` is `true`, emitted by the reviewed builder or candidate admission path. A false value is a stop. The record also carries the builder's admission classification of every unaccepted item as preinstall admission or postinstall acceptance. Every preinstall item is cleared; postinstall items remain obligations for section 9. The flag does not prove a postinstall item, and camera behavior is not pre-proven by it. The current builder's hardcoded false flag and unclassified list are a #11 prerequisite. An operator may not set the flag or delete an item by hand. |
| Feature declaration | The record states the film-list surfaces it covers, whether the Fn path is included, and confirms the persistent SELECT contract of [the recipe design](../reports/recipe-design.md) sections 18.1 and 18.5. A one-shot Recall substitute is not the complete bank. |
| `source_start`, `source_end` | The boot-publication source extent in the plain image's runtime RAM, covered by a readback segment |
| `runtime_start`, `runtime_end` | The runtime-RAM carve destination, with no overlap with a stock writer, heap object or live allocation beyond the declared heap-capacity loss |
| `executable_start`, `executable_end`, `executable_sha256` | The linked bank executable extent in runtime RAM and its digest |
| `source_package_sha256` | Digest of the published source package in the plain image |
| `publication.json` | Every published runtime-RAM address and value in order, with its old and new word |
| `loader_descriptor_address`, `loader_descriptor_words` | The runtime-RAM loader descriptor and its wait/release resource |
| `status_cell`, `name_base`, `name_records` | The runtime-RAM refusal status cell and the name records for the compiled row and the seven slots |
| `heap_upper_bound`, `heap_bound_site`, `heap_bound_current_word`, `heap_bound_new_word`, `heap_capacity_loss_bytes` | The runtime-RAM heap carve and the exact word change |
| Entry and site words | The runtime-RAM addresses `presenter_*`, `root_draw_*`, `failure_graphics_*`, `name_editor_*`, `redraw_poll_*`, `current_value_preview_*`, `key_left_*`, `key_right_*` and `editor_cancel_cell_*`, each with its stock word and its replacement |
| Container checks (DAT and media) | Every current exact-candidate verifier check passes: `rebuild-update verify`, `verify-stock` or an equivalent full check, round-trip equality with the matching encoder, write-window limits, and `diff` against stock with every changed byte explained; both auxiliary restore lists checked against this body; main checksum and record lengths reproduced |
| Changed-extent table | The table of section 3, complete for this candidate |
| Independent final-byte review | One recorded review of this exact candidate by a reviewer who is not the author, with no open finding, under [the operation classes](../spec/07-governance-and-handoff.md#32-operation-classes) |
| Placement | [#17](https://github.com/skeet70/latent-image/issues/17) resolved for this candidate, with the accepted location and the accepted residual risk stated |
| Boot publication | [#13](https://github.com/skeet70/latent-image/issues/13) integrated for this candidate: exact covered length, bounded termination, destination validation, and the truncated, unterminated, out-of-range and checksum-preserving corrupt packages exercised |
| Emulator | `//emulation:flash-boot-check` boots this image to the stock menus, the full-system visual and behavioral checks are recorded, and no boot-path prerequisite is open |
| Owner go | A per-run H3 go naming the operation and this DAT digest, under [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs) |
| Recovery | No R4 acceptance for this recipe image or interrupted-update class; historical recorder-only R4 supplies no current rescue path through the damaged SD slot. Restate the owner's accepted residual for an unrecoverable interruption for this image and digest |

A candidate that shows the requested menu rows but does not carry all of the above is not installable under this procedure, whatever it renders in an emulator.

### 2.1 Admission versus postinstall acceptance

Preinstall admission is evidence that can be established without installing this image on the body: the exact candidate composition and inputs, the verifier checks, the changed-extent table, placement, boot publication, the source-ownership, heap, fetch and core items, the independent final-byte review, the owner go and the recovery restatement.

Postinstall acceptance is behavior that only the installed image can show: `camera behavior`, the declared list and Q surfaces, selection and application, the seven slots, the stock C observable state, stock-mode output and the normal-operation return.

The current unaccepted item `camera behavior` is postinstall acceptance. It is not a preinstall gate unless a named preinstall trial covers it, and no such trial is recorded for this complete image. The current builder hardcodes `installable: false` and does not emit an admission classification, so its output is a stop. The reviewed builder or candidate admission classification and a true flag must exist before first-run use; that is a #11 prerequisite, not an operator waiver. Do not move an item, set the flag or clear an item without the builder and review evidence. Camera behavior remains a postinstall obligation and is not pre-proven by this gate. A gated procedure with missing preinstall data is not an available install.

## 3. Write ranges and identities

The updater writes the firmware image and configuration area; only the extents the candidate declares differ from stock. The candidate's changed-extent table is the authoritative list, and the operator transcribes it into the session note sheet before the card is prepared. The table must give, for every changed extent:

- a stable name for the extent;
- the offset and length in its address space;
- the expected bytes or their digest;
- the source artifact and input digest the bytes come from;
- the address space of each offset and length;
- the readback operation and address that reads it back, or an explicit statement that no existing operation covers it.

The four address spaces are not interchangeable:

| Space | What it names | How it is checked |
| --- | --- | --- |
| DAT file offset | A byte offset in the `FWUP0002.DAT` container | Card length, SHA-256 and `cmp`; `rebuild-update diff` against stock |
| NAND/media offset | A persistent media byte the updater writes | The S1 to S5 phases in [the install procedure](install-procedure.md#2-what-the-camera-does-when-inside-the-update); never inferred from a DAT offset |
| Decoded image address | A linked address in image 2, 4, 5 or the plain image | Patch manifests and decoded-image comparisons |
| Runtime RAM address | A live address after the load, including the carve and publications | The fixed `dump-ram` segments and `read-config` |

The classes in the current design are the image-4 decode prefix at `[0x0061c1fc, 0x0061c400)` for the unlock arrays and the D3 hook, the image-4 body conversion rows and availability cells, the image-5 menu commit site words, the image-2 compiled and bank name records, the plain-extent carrier for the bank menus, label records, storage kernel and publication source, the boot carve for the runtime image, publications, status cell and heap-bound word, and the segment-table words, record lengths and main checksum. The candidate record supplies the exact offsets for its own build; none of these is assumed from this list.

The candidate's patch diff describes the DAT and decoded-image changes and the plain carrier. It is not the updater's complete persistent S1 to S5 write set: the updater erases and programs whole blocks and rewrites the configuration area, including bytes the candidate does not change. The readback compares the declared changed extents and the named post-update state; it does not claim that the candidate diff enumerates every media write.

This report deliberately carries no frozen range table. Copying the historical combined container's offsets or an unreviewed candidate's offsets would bind a different image. The operator transcribes the bound candidate's table into the session record, and that completed table is part of the install evidence.

The rebuild starts from the pinned official stock DAT. It does not assume that the previously installed container is intact, and it does not treat the restoration of physical block 14 as establishing the rest of a container's identity. A changed extent that is not in the table, or a table that disagrees with `rebuild-update diff`, is a refusal.

## 4. Card identity and readback

1. Use a dedicated ordinary camera-compatible SD card, not the diagnostic SD-BOOT card. Preserve anything on it first.
2. Before formatting, record in the private note sheet a physical mark that distinguishes this card, its capacity, and any stable hardware identity the host exposes, such as an SD CID or reader identity. The note sheet is private; do not copy a raw serial or CID into a public report.
3. Format the card in the camera through `SET-UP` > `FORMAT` as a separate owner-scoped state change. Do not format it from the host. After formatting, record the capacity and the volume identity again. Formatting can change a volume identity while leaving a hardware CID unchanged.
4. With the camera off, copy only the candidate DAT to the card root as `FWUP0002.DAT`, with that exact name and no directory.
5. Re-read the card copy on the host. The length, SHA-256 and `cmp` result must match `firmware` and `firmware_sha256`, and the card mark, capacity and stable identity must match the recorded values. A changed device path is not a changed card, and a matching file digest alone does not bind the card.
6. Flush and safely unmount, remount and repeat the card identity, capacity, length, digest and `cmp` checks. Inspect the root and any hidden entries. Only `FWUP0002.DAT` may be added; empty camera-created directories may remain.
7. Remove the AUTO_ACT script and `DCAA` directory, `ffsys.bin`, `ffboot.bin`, `ffuser.bin`, any other `FWUP0002.DAT`, `FWUP0001.DAT`, `FPUPDATE.DAT` and every diagnostic-card layout. Do not keep a second copy as a fallback.
8. Immediately before inserting the card, check the physical mark again and, when the reader can still read it, the stable identity. The card must be the one recorded. Reject a different card, a card whose identity cannot be distinguished from the recorded one, or any capacity change.
9. Record the verified card-side length and digest and the card identity privately, not only the host source.

The host may expose the capacity and a volume identity through its block-device tools, for example `lsblk -o NAME,SIZE,PTUUID,MODEL <device>`. The device path is not the identity. Record the values, not just the path.

```sh
IMAGE="<the candidate record's firmware path>"
CARD="<the mounted card directory>"
cp -- "$IMAGE" "$CARD/FWUP0002.DAT"
wc -c -- "$IMAGE" "$CARD/FWUP0002.DAT"
sha256sum -- "$IMAGE" "$CARD/FWUP0002.DAT"
cmp -- "$IMAGE" "$CARD/FWUP0002.DAT"
```

## 5. Camera-specific backup and starting state

Record the starting state before any install act, on a normal photography card with no firmware or diagnostic files. Keep the install card out of the camera until section 7.

| Act | Owner action | Expected and recorded | Evidence |
| --- | --- | --- | --- |
| B0 | Start the note sheet. Record the body identifier, lens and lens firmware, supply arrangement, dial position and current film simulation. Boot normally without USB and record any existing defect. | Menus and live view respond; every pre-existing defect is recorded before the change. | Hardware-observed method; this session's result unproved. |
| B0v | Switch off, then boot with DISP/BACK held and the no-firmware card. Photograph BODY VERSION and the lens version, then leave by the normal entry and boot normally without held keys. Do not enter an upgrade. | BODY VERSION 1.50 and the declared target model. A different version or model stops the procedure. | Byte-read vendor version check; this read unproved. |
| B1 | Record date/time, language, image size and quality, film simulation, dynamic range, ISO, white balance and its R/B shifts, colour, sharpness, highlight and shadow tone, noise reduction, long-exposure NR, focus mode, face detection, Fn assignment and every visible C-slot value. Read the C slot without saving over it. | A before record of the visible settings and C values. The C cache bytes are compared from B5, not from this record. Do not reset anything to reach a baseline. | Hardware-observed settings-reset risk, E-JIG-CYCLE-D-150. |
| B2 | In ordinary still M, photograph every page of `SHOOTING MENU` > `FILM SIMULATION`, scrolling without accepting a choice. Record the dial position and the actual list. | The installed list before the change, with its real punctuation and icons. | Byte-read installed arrays where RC0 is present; this session's list unproved. |
| B3 | Take one fixed-scene shot with the recorded settings and play it back. Preserve the original file, filename and displayed settings. | A before reference for the stock-mode comparison and the bank parameter checks. | Hardware-observed method; this result unproved. |
| B4 | Switch off. With USB attached, hold the shutter fully down and Up, switch on, and release both when the screen is blank and the green LED is lit. | Maintenance mode; host device `04cb:ff80`. | Hardware-observed entry, E-JIG-OBSERVED and E-PRESS-MAP-150. |
| B5 | Run the reviewed full adjustment read once and record its printed folder and terminal result. | `read-adjustment-42000-v1`, 1,309 windows, no refusal or uncertainty. It records the adjustment mirror, update stamp, signature, C cache and develop tables. The C cache bytes from this capture are the raw C comparison for section 9. | Byte-read plan; hardware-observed read method, E-ADJ-READ-150. |
| B6 | After B5 succeeds, run the reviewed `read-media` once and record its printed folder and terminal result. Do not take a shot or enter playback in this maintenance boot. | Two area-2 raw banks of 131,072 bytes each, with map, gate and staging records. | Hardware-observed method, E-AREA2-BANKS-150. |
| B7 | After the command ends, cut the dummy-battery supply. Do not end maintenance with the POWER switch. Unplug USB after power is gone and set POWER to OFF while unpowered. | A settings-preserving maintenance ending. Never apply this ending to an in-progress update. | Hardware-observed control, E-JIG-CYCLE-D-150; this ending unproved. |
| B8 | Retain both session folders and their journals. Complete the host integrity and before-state checks, boot normally without USB and confirm the watched settings still match B1. | No failed window, unexpected status, missing bank or unexplained settings change. | Byte-read checks; results unproved until performed. |

```sh
devenv shell -- bazel run //transport:camera -- read-config "$PWD/private-data/read-plans/adjustment-42000.json"
devenv shell -- bazel run //transport:camera -- read-media
```

Both commands are existing reviewed operations. After a refusal, a disconnect or uncertain cleanup, stop, use B7's supply-cut ending and report. No retry in that boot, and no flash while the before capture is unresolved.

The before captures are the body-specific backup for this procedure. Keep them in two verified durable locations. They are a capture of the adjustment area and the area-2 banks, not a full persistent backup, not a restore route, and not a demonstrated recovery. `read-media` does not read the C slot's area-3 banks, so the C state is recorded from its visible values and the B5 cache capture only; do not describe that as a raw C backup. No restore command is part of this procedure.

## 6. Power

The supply must be uninterrupted from the final upgrade confirm through the completion screen. Use a genuine, healthy, freshly fully charged battery, or the owner's mains-backed dummy supply with its connector secured, the lens mounted, the card door closed, USB absent, the camera on a stable surface and no shared switch on the supply.

Neither arrangement is measured under this image's flash load. Voltage and current margin, transient response, grounding and erase/program behaviour remain unproved. The vendor's generic X-series maximum is about five minutes; the 2026-10-09 RC0 install measured 57.3 s for a different container, and this image's timing and phase split are unmeasured. Do not treat that measurement as a bound.

Do not change the battery, attach a charger or switch to the dummy supply during the update. If the supply is uncertain, stop before the confirm.

## 7. Camera acts

| Act | Owner action | Expected observation and record |
| --- | --- | --- |
| I0 | With the camera off, insert the re-read install card and close the door. Start an external screen or LED video and a clock. Record the card digest and the chosen supply. | Camera off, USB absent, card and supply secured. |
| I1 | Hold DISP/BACK, switch on and release when the firmware screen appears. Do not hold Fn or the shutter/Up maintenance combination. | BODY VERSION 1.50 and the candidate's version. A wrong model, version, file error, low battery or unfamiliar screen stops here. |
| I2 | Inspect the screen before pressing OK. Select BODY, not LENS, and press MENU/OK once. Photograph the item and the version display. | The offered update is for this body at 1.50. If the equal-version update is refused, abort; nothing has been written. |
| I3 | At the upgrade confirmation, inspect the choice. CANCEL is the vendor's initial selection. Select OK only after every check above holds, then press MENU/OK once. If a battery-cover warning with BACK CANCEL appears, pause there without pressing its final OK. | No processing begins before the final accepted confirmation. If PROCESSING starts instead, follow I4 and do not cancel. |
| I4 | At the last warning or confirmation, record the photograph and time, then press its OK once only under the owner's named go. | This is the visible point of no return. Press once; do not double-press. |
| I5 | Let the camera finish. Touch nothing: no button, card door, USB, battery, POWER switch or supply cut. | Record pauses, unchanged progress, blank display or unexpected messages without interrupting. Per-phase display correspondence is unproved. |
| I6 | Wait for the explicit completion screen that instructs the camera to be switched off. Record the whole screen and elapsed time. | Completion, not a stopped-looking progress bar, ends the hands-off interval. |
| I7 | Switch off as instructed and wait for shutdown. Remove the install card and insert the normal photography card. | An ordinary shutdown after the update, not a supply-cut maintenance ending. Keep the install card and its bytes until the evidence is secured. |

## 8. Abort points and the unrecoverable interruption

The last abort with a working camera is before the final accepted upgrade confirmation. Staging and the three checksum checks are RAM and card reads only, so a stop before the first persistent write leaves the old firmware intact.

The first persistent write is S1, the erase of raw trailer block `0x125` holding the boot marker at media `0x24a0000` with halfword `0xaaaa` at `+0x1e`. After that erase lands, the old application cannot boot again, and no mapped operator cancel exists. S2 erases the optional span, S3 programs blocks `0` to `0x125`, S4 refreshes and restores the configuration area, and S5 rewrites the directory-journal pair. An interruption between S1 and the end of S3 leaves a body that does not boot; an interruption in S4 or S5 leaves the residues in [the install procedure](install-procedure.md#8-interruption-residues-and-which-are-recoverable-by-repeating-the-update).

Recovery is not a guarantee for this image. [R4 is accepted only for the historical recorder-v2-v5 block-14 repair](recovery-status.md#historical-recorder-failure-r4-decision-2026-10-27), with block-71 residue retained, not this recipe image or interrupted updates. The damaged SD slot makes that route unavailable now. The owner's 2026-10-09 acceptance of an unrecoverable interruption covered the RC0 prefix container, not this image, so section 2 requires the residual to be restated for this digest. A successful install earns no recovery credit.

After any aborted prewrite attempt, make a real power cut after shutdown before any newly scoped attempt. The one-time cached checksum-success byte `0x15c1e58` is not cleared by a menu exit, card swap or second confirmation. After processing begins, do not cut power, retry, attach USB or attempt a rescue through a stock reflash, SD-BOOT, S-record or AUTO_ACT. Preserve the last display, the interruption point, the card and the supply state, and stop camera operations.

## 9. Complete-feature regression plan

Run one bounded ordinary session after the first boot. Use the fixed scene, the recorded settings and exposure, and ordinary still M for the comparisons unless an act names another mode. Do not reflash within this session; a failure stops the sequence and goes to host analysis.

Any manual setting change needed for the test, including assigning Fn to FILM SIMULATION, selecting P/S/A/M, setting the fixed scene, or choosing a film row, is an owner-scoped ordinary settings change. Record it separately from the install, keep it within the supported X-M1 settings, and restore it before the final normal-operation check. The procedure does not change settings on the owner's behalf.

| Act | Owner action | Expected observation and record | What it establishes |
| --- | --- | --- | --- |
| V0 | With no USB and the normal photography card, boot normally once. Record the time from switch-on to menus. | No halt, hang or first-run setup prompt. A nonworking body follows section 8, not a menu test. | The image boots on the body. |
| V1 | Compare date/time, language, watched settings and every visible C-slot value with B1. Photograph BODY VERSION. Do not reset or overwrite the C slot. | The watched settings and visible C values are retained; BODY VERSION is 1.50. A setup prompt or lost setting stops the sequence for host analysis. | The update preserved the body state and the observed stock C state. |
| V2 | In ordinary still M, photograph every page of `SHOOTING MENU` > `FILM SIMULATION` and the Q film cell, which the complete feature requires. Open the Fn film list only when Fn is assigned to FILM SIMULATION; record the assignment and do not change it for this test. Scroll each without accepting a choice. | The stock rows plus PRO Neg Std and PRO Neg Hi where the image carries them, plus the two compiled rows with their compiled names, on the shooting menu, the Q film cell and any other declared surface. A surface the candidate does not declare is not a failure; a missing declared surface is. | The complete list on the required and declared surfaces. |
| V3 | Select each compiled row in turn, leave the menu, reopen it and read the eight parameter fields. Take one fixed-scene shot, play it back and read its recorded settings. | Both rows are selectable; each row's film code and eight parameter values match the candidate's definition; the compiled name renders as list-row text; the image's recorded settings match the selection. | D3 behavior and recorded-settings acceptance. |
| V4 | Select each compiled row from the Q film cell and repeat V3's parameter and recorded-settings read. If the candidate declares a Q bank selector, select C1 to C7 and NOTHING SELECTED from it. | The Q selection applies the same recipe as the list selection; the declared Q bank selector changes only the effective bank selection and shows the effective value independently of the cursor. | Quick-menu exposure and the bank's Q surface. |
| V5 | Open `EDIT/SAVE CUSTOM SETTING` and `SELECT CUSTOM SETTING`. Save Current into a named slot, rename it on camera with OK/CANCEL, edit each bound parameter with per-item OK/CANCEL, Recall or select it, and Reset a slot after its confirmation. Repeat once with a cancel. | Both entries appear once; seven slots exist; Save Current captures the record's bound fields; rename, per-item edit, Recall and Reset behave as the candidate declares; a cancel leaves live values and the C position unchanged; no slot auto-updates. | The seven editable slots and their UX. |
| V6 | After the bank acts, compare the visible C values with B1. Open `CUSTOM SET` and `CUSTOM RESET` and leave or cancel without confirming. Attempt a recipe selection and a bank selection with the dial at C. | The visible C values match B1; selection at C is refused or disabled and writes nothing; both C flows open and cancel with the observable state unchanged. Do not execute either commit. The cache-byte equality and the cache unchanged assertion are decided in section 10 against B5. The stored area-3 C record is not read or backed up raw. | The stock C state is untouched at the observable level; no destructive C commit is exercised. |
| V7 | Switch off normally, boot again, and repeat the list, compiled-row selection and slot read. Read the selected bank (NOTHING SELECTED or C1 to C7). | The compiled rows and slot payloads survive; the selected bank resumes, and its effective value is not inferred from the menu cursor or from equal field values. The C observable state remains as in V6. | Slot and selected-bank persistence. |
| V8 | Return to the B3 scene and settings. Take and play back one shot for each stock film row the body offers. Compare with the B3 reference. | Stock modes render and record settings as before; no other menu or watched setting regresses. Record any difference rather than assume none. | Stock-mode regression. |
| V9 | Remove the install card, boot normally without USB, take one ordinary shot and play it back, then switch off normally. | Menus, capture, playback and shutdown work; no unexplained change remains. | A clean return to normal operation. |

The bank acts use only the X-M1 settings listed in section 1. A row, value or glyph that the stock body does not offer is a regression, not an approximation to accept. Record designed, built and installed as separate states for D3, the bank and the stock C setting; a host or emulator result is not an installed result.

The persistent SELECT contract is the one in [the recipe design](../reports/recipe-design.md) sections 18.1 and 18.5. A candidate that substitutes one-shot Recall for it is refused at section 2, not accepted by this plan.

The candidate's existing source and native regression evidence for `CUSTOM SET` and `CUSTOM RESET` is separate from this session. This procedure opens and cancels those flows only. It does not execute their destructive commit behavior and does not claim that behavior as exercised.

## 10. Post-install byte readback

Run this only with a normally functioning body and only under a separate owner scope for the read operations. It is one maintenance boot with the reviewed read-only operations, ending with the supply cut.

| Read | Covers |
| --- | --- |
| `dump-ram id4-image-0` | The image-4 decode prefix and the start of the image-4 body |
| `dump-ram id4-image-1` | The rest of the image-4 body, including conversion rows, availability cells and pointer words |
| `dump-ram id5-image-0`, `dump-ram id5-image-1` | The image-5 site words and the surrounding menu code |
| `dump-ram id4-bss-3` | The boot carve, runtime image, publications and status cell in the current design |
| `dump-ram plain-0` or the covering plain segment | The publication source package and the bank carrier bytes |
| `read-config` with the full plan | The adjustment mirror, stamps, signature and C cache |
| `read-media` | The two area-2 banks and their record history |

```sh
devenv shell -- bazel run //transport:camera -- dump-ram id4-image-0
devenv shell -- bazel run //transport:camera -- dump-ram id4-image-1
devenv shell -- bazel run //transport:camera -- dump-ram id5-image-0
devenv shell -- bazel run //transport:camera -- dump-ram id5-image-1
devenv shell -- bazel run //transport:camera -- dump-ram id4-bss-3
devenv shell -- bazel run //transport:camera -- dump-ram plain-0
devenv shell -- bazel run //transport:camera -- read-config "$PWD/private-data/read-plans/adjustment-42000.json"
devenv shell -- bazel run //transport:camera -- read-media
```

The candidate's changed-extent table decides the exact segment and address for each extent. Run one command at a time and stop on the first refusal, unexpected reply, disconnect or uncertain cleanup, using the supply-cut ending and no retry.

Compare every covered extent against the candidate's own expected bytes, not against a recomputation that repeats the builder's logic. Name every written extent the fixed segments do not cover, such as the image-2 name record or the segment table, and do not claim byte-level confirmation for it. Screen out only the runtime-written cells the candidate record or the owning report names; do not invent an exclusion to make a comparison pass.

The `read-config` C cache comparison is against the B5 capture, not against a photograph; this is the cache-byte equality deferred from V6. The area-3 C banks remain unread, so the procedure does not establish a raw or byte-identical stored C record.

End the maintenance boot by cutting the dummy-battery supply, then boot normally once and confirm the settings and the film list again before the ordinary closing shot. A visible list or a version screen is an observation until the host comparisons and the record readbacks agree.

## 11. Rejection and stop rules

Reject the run, before the card or at the first affected act, when any of the following holds:

- the candidate's `installable` is not true, the builder admission classification is missing or inconsistent, any preinstall item is not cleared, or the exact-candidate review, placement or boot-publication prerequisite is missing; postinstall acceptance items may remain listed and are decided in section 9;
- the DAT's length, SHA-256, name or card copy differs, the card carries an extra payload, or the mounted re-read does not match;
- the body is not the declared model or BODY VERSION is not 1.50, the starting settings or C state do not match B1, or the fixed scene or supply arrangement changes;
- the update is not offered, the version or model on screen differs, the battery is low or the supply is uncertain, or an unfamiliar screen appears;
- the film list, Q surfaces, seven slots, rename, edit, Recall, Reset, C behavior or recorded settings differ from the candidate's declared behavior, or a stock row or another setting regresses;
- the body does not boot, a setup prompt appears, C or calibration bytes change unexpectedly, capture or playback fails, or the readback mismatches outside the declared change set and named runtime cells.

A wrong list or a selection failure on a booting body is an install outcome, not a reason for a same-day second flash. A nonbooting body stops all camera operations. Host work continues.

## 12. Acceptance state and open prerequisites

The procedure is complete for a candidate that passes section 2. It is not executable today because no candidate does.

- **Complete image.** [#11](https://github.com/skeet70/latent-image/issues/11) still owes the composed bank code, boot loading, placement approval, final-byte review and full-system visual and behavioral checks. A recipe-rows-only image does not satisfy the feature.
- **Placement.** [#17](https://github.com/skeet70/latent-image/issues/17): no placement is accepted, and the remaining marker-0 lifecycle, heap-demand and core-visibility observations are open.
- **Boot publication.** [#13](https://github.com/skeet70/latent-image/issues/13): the builder integration, store fields and `RB_STORE_ENTRY`, the covered length, bounded termination, destination validation and the corruption cases still need the listed checks.
- **Update image.** [#37](https://github.com/skeet70/latent-image/issues/37): each candidate needs its input manifest, carried ranges and restore list, checksum and layout results, and readback accepted against its exact inputs.
- **Review and go.** No independent final-byte review of a complete recipe-only image exists, and no H3 go names one. The owner's earlier RC0 interruption acceptance is not this image's acceptance.
- **Recovery.** No R4 acceptance covers this recipe image or interrupted updates. Historical recorder-only R4 supplies no present rescue assurance through the damaged SD slot. No result in this procedure demonstrates a repair or a recovery guarantee.

The current package builder emits its record with `installable: false` and no admission classification. That is a stop and a #11 prerequisite, not a waiver. `camera behavior` is postinstall acceptance under section 2.1; the source-ownership, nonallocator writer, heap, fetch, core and independent-review items are preinstall admission. No candidate is admitted until the reviewed builder or candidate classification exists, the preinstall items are cleared and the flag is true.

This document establishes none of those prerequisites, authorizes no run and accepts no installed behavior.
