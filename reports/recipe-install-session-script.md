# Recipe-candidate install session: capture, install, verify (X-M1 1.50)

**Withdrawn as an executable install procedure.** The historical `b1aabeb6…` container below is a pre-bootstrap composition, not a completed editable-bank image. It omits the bank's linked executable blob and boot-copy publication; the closing review did not establish installability. Do not prepare an install card or run the camera acts below from this record. A replacement procedure must bind a complete recipe-only image, early-boot/load-failure evidence, placement disposition and independent final-byte review. Recorder installation is not a prerequisite.

The sections below retain the historical 2026-10-11 inputs and proposed observations. Their paths, hashes, expected readbacks and claimed installable status do not clear a replacement image. Every camera operation still requires its own owner go under [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs).

**Byte-read:** the container carries the film-simulation unlock and the recipe bank in one image: the nine-row values, shooting-label and Q-label arrays and the hook at the 516-byte decode prefix `[0x0061c1fc, 0x0061c400)`, the two compiled-recipe conversion rows at `0x0077ac48`/`0x0077ac60`, the fourteen availability cells, the eight node pointer words, the 34 bank image-4 words, the nine image-5 words (the `case 0xfe` site instruction at `0x00d50a48`, the modal entry, cells and shim calls) and 85 plain-window writes (the label-class words, the menu, name-record and edit regions, the seven stock-mode film edit label quadruples); the compiled name `WARM STANDARD` sits in image 2. [The D3 patch report](recipe-patch-d3.md) and [the rebuild record](compiled-recipes-builder.md) own the write set. **Static-inference:** its expected menu list is the seven installed RC0 entries plus `WARM STANDARD` and a second Velvia row, nine in all. **Unproved:** the hook's instruction fetch from the prefix on hardware ([recipe-patch-d3.md](recipe-patch-d3.md) section 5.7), the compiled rows' on-body selection, rendering and persistence, and the whole install outcome until observed.

## 1. The install card

Use a dedicated ordinary camera-compatible SD card, not the raw diagnostic SD-BOOT card image. Preserve anything on it before formatting. Camera formatting is a separate state-changing act covered by the owner's session scope; it happens before the update file is copied, never during installation.

| Act | Owner action | Expected observation and record |
| --- | --- | --- |
| C0 | With the install card in the normally working camera, choose SET-UP → FORMAT, select OK and press MENU/OK. Wait for format completion, then switch off normally. | Formatted card; existing files erased. The vendor's instructions are byte-read, this card's outcome unproved. |
| C1 | Remove the card while the camera is off and insert it in the host reader. Copy only the section-2 source to its root as `FWUP0002.DAT`. | Correct root name, not `combined-candidate.DAT`, `FWUP0002 (1).DAT` or a hidden extra extension. |
| C2 | Run the host-only checks below against source and card copy. Record both lengths, both digests and the `cmp` exit status. | Both lengths 38,405,148; both SHA-256 values match section 2; `cmp` exits 0. Any mismatch stops preparation. |
| C3 | Flush and safely unmount/eject through the host, remount/reinsert the card in the host reader, and repeat the card length/hash/`cmp` checks. Inspect its root and directories, including hidden files. | The re-read after the write cache is gone still matches. No alternate payload remains. |
| C4 | Safely eject again. Only now may this prepared card go near the off camera. | The note sheet carries the verified digest and length of the card-side file, not just the host source. |

Replace `<card>` with the mounted card directory. These are owner-run host/card commands, not camera commands.

```sh
IMAGE="$PWD/private-data/analysis/x-m1-1.50/recipes/runtime/combined-candidate/install/FWUP0002.DAT"
CARD="<card>"
cp -- "$IMAGE" "$CARD/FWUP0002.DAT"
wc -c -- "$IMAGE" "$CARD/FWUP0002.DAT"
sha256sum -- "$IMAGE" "$CARD/FWUP0002.DAT"
cmp -- "$IMAGE" "$CARD/FWUP0002.DAT"
```

Keep `FWUP0002.DAT` as the only added payload. Empty filesystem directories left by camera formatting, such as `DCIM`, may remain; no photographs or host index/trash files are needed on this dedicated card. Remove the AUTO_ACT experiment's `DCAA` directory and script, `IMFIDX10/LX30A.S3`, `ffsys.bin`, `ffboot.bin`, `ffuser.bin`, alternative `FWUP0001.DAT`/`FPUPDATE.DAT`, any earlier `FWUP0002.DAT` or the film-simulation install card's contents, other firmware/lens updates and any diagnostic-card layout. This is a card-content restriction, not a claim that every forbidden file would execute. Do not include a second copy as a fallback.

## 2. The install bytes

| Field | Required value | Evidence |
| --- | --- | --- |
| Host source | `private-data/analysis/x-m1-1.50/recipes/runtime/combined-candidate/install/FWUP0002.DAT` | Emitted 2026-10-11 from the reviewed candidate, `install-artifact.json`. |
| File on card | Root `FWUP0002.DAT`, no directory and no suffix | Byte-read body lookup derived from this body's `0002` configuration bytes. |
| Length | 38,405,148 bytes | Record and `cmp`. |
| SHA-256 | `b1aabeb61f8ae19c83e4f76efcf0710687e8008daa998c625b7c0bf0f5ca0757` | Record; byte-identical to the reviewed container. |
| Body/version | X-M1, 1.50; version words remain `1` / `0x50` | Record and `rebuild-update verify`. |
| Main checksum word | `0x8f080f30` | `rebuild-update verify`, stored equals expected. |
| Verify | `contract fields: 42/42 checks pass` | `install/verify-run.txt`. |
| Expected offer list | PROVIA/STANDARD, Velvia/VIVID, ASTIA/SOFT, B&W, SEPIA, PRO Neg Std, PRO Neg Hi, `WARM STANDARD`, a second Velvia entry | Byte-read patch arrays; the compiled rows' on-body names, selection and rendering unproved. |

The host source basename is not the card basename. Do not rebuild, alter version words, substitute the earlier RC0 prefix container, substitute a one-row or matrix-only experiment, or copy the stock image under the approved image's name. **Byte-read:** both auxiliary restore lists are byte-identical to stock in this container; this matters independently of the main firmware checksum. The independent review of these bytes is E-FINAL-CANDIDATE-REVIEW-150 (`ReviewFinalCandidateSol`, correct, no findings) and carries to the emitted file because the copy is byte-identical.

## 3. First acts: record the body before the install

Use a normal photography card with no firmware, script or diagnostic files for these acts. Keep the install card out of the camera until section 5. Use the dummy-battery supply for maintenance captures, with the camera off before connecting USB. Nothing here calls `arm-auto-act`, End Setting, a configuration commit or a restore.

Record one row per act in a private note sheet under `private-data/`, with time, what happened and the saved file or printed session-folder path. Preserve serial-bearing screens, raw logs, photographs and captures privately. Section 8 specifies the handoff.

| Act | Owner action | Expected observation and record | Evidence |
| --- | --- | --- | --- |
| B0 | Start the note sheet. Record body identifier, lens and lens firmware, supply arrangement, original dial position and current film simulation. Power on normally, without USB. | Menus and live view respond; record any existing defect before the install. | Hardware-observed method from prior ordinary sessions; this session's result unproved. |
| B0v | Switch off, then switch on holding DISP/BACK with the no-firmware photography card. Photograph BODY VERSION and lens version; switch off, then boot normally without held keys. Do not enter an upgrade. | BODY VERSION 1.50; retain the displayed lens version and both screen photographs. Any different body version stops this procedure. | Byte-read vendor version-check procedure; body 1.50 hardware-observed in prior owner reports, this read unproved. |
| B1 | Photograph or transcribe date/time, language, the C-slot values without saving over the C slot, and the shooting/SET-UP values listed below. Do not reset anything. | A before record of settings that an update or a wrong maintenance ending could lose. | Hardware-observed settings-reset risk, E-JIG-CYCLE-D-150. |
| B2 | Use ordinary still M for the film-menu comparison. Photograph every page of SHOOTING MENU → FILM SIMULATION, scrolling without accepting a new choice. Record the dial position. | Seven entries, the installed RC0 list: PROVIA, Velvia, ASTIA, B&W, SEPIA, PRO Neg Std, PRO Neg Hi, in that order. Retain the actual punctuation and icons in the photographs. | Byte-read installed arrays; the seven-entry outcome hardware-observed in the 2026-10-09 install and render sessions. |
| B3 | Take one normal shot of a fixed scene and play it back. Preserve the original image file, its filename and the displayed settings. | Normal capture/playback before the firmware change; a reference for the same-scene check later. | Hardware-observed method; this result unproved. |
| B4 | Switch off normally and wait for shutdown. With USB connected, hold the shutter fully down and Up, switch on, then release both when the screen is blank and the green LED is lit. | Maintenance mode, blank screen and green LED; host device `04cb:ff80`. | Hardware-observed entry, E-JIG-OBSERVED and E-PRESS-MAP-150. |
| B5 | Run the full reviewed adjustment read below. Record its printed folder and terminal result. | `read-adjustment-42000-v1`, 1,309 windows; no refusal or uncertainty. This records the adjustment mirror, update stamp, signature, C cache, develop tables and stored gate values before the install. | Byte-read plan; hardware-observed read method, E-ADJ-READ-150. |
| B6 | After B5 succeeds, run the reviewed `read-media` command below, once. Record the printed folder and terminal result. Do not take a shot or enter playback in this maintenance boot. | Two area-2 raw banks, 131,072 bytes each, with map/precondition, gate and staging records. These preserve the ordinary-settings ledger before the change. | Hardware-observed method, E-AREA2-BANKS-150; static-inference frame-pool/storage-driver constraints. |
| B7 | After the command has ended, cut the dummy-battery supply. Do not use the camera POWER switch to end maintenance. Unplug USB only after power is gone; set the POWER switch to OFF while unpowered. | Maintenance boot ended without its POWER/OFF defaults record. | Hardware-observed settings-preserving cut control, E-JIG-CYCLE-D-150; future outcome unproved. |
| B8 | Retain both session folders, including their journals and outgoing records. Have the host-side integrity and before-state checks completed before continuing. Restart normally without USB and confirm the watched settings still match B1. | No failed window, unexpected status, missing bank, uncertain cleanup or unexplained settings change. | Byte-read checks on the new captures; results unproved until performed. |

```sh
devenv shell -- bazel run //transport:camera -- read-config "$PWD/private-data/read-plans/adjustment-42000.json"
devenv shell -- bazel run //transport:camera -- read-media
```

Do not launch the second command if the first refuses or stops. Both commands already exist and use their reviewed internally built bytes; their contracts are in [transport/README.md](../transport/README.md#operations-and-their-bytes). After a capture refusal, disconnect or uncertain cleanup, stop the sequence, use B7's supply-cut ending, and report. No retry in that boot and no flash while the before capture is unresolved.

The watched settings are IMAGE SIZE/QUALITY, film simulation, dynamic range, ISO, white balance and its R/B shifts, colour, sharpness, highlight/shadow tone, noise reduction, long-exposure NR, focus mode, face detection, Fn assignment, date/time, language and the C-slot recipe values. Read their current values, not an assumed defaults list. Record any change needed to use the fixed scene or M separately from the original value.

**Byte-read:** the existing plan has 1,309 windows, covers adjustment `[0,0x42000)` plus `[0x400000,0x40fd00)`, and includes the `X-M1` oracle at `+0x15c` and `55 aa` signature at `+0x202`. Check the new capture's oracle/signature, page-zero `0xa2`/`0xf7`, `+0x1e8`/`+0x1e9`, `+0x204`/`+0x205` and `+0x275`/`+0x276`. **Static-inference:** this body has run the film-simulation install since the last full capture, so the expected before values are oracle `58 2d 4d 31`, signature `55 aa`, update stamp `01 78` (the 1.50-time constants; the observed move `01 54` to `01 78` is E-FILMSIM-READBACK-150), boot-build stamp `01 78`, pre-erase controls `01 0a` and page-zero `00`/`00`. These are comparison expectations, not values to write. Unexpected values are a stop for host analysis before the install.

**Static-inference:** the full adjustment read supports the restore-list comparison and records this body's calibration mirror before refresh; `read-media` supplies the two persistent ordinary-settings banks and their record history. Neither is a full persistent-storage backup or a demonstrated restore. `read-media` does not read the C-slot's area-3 banks; the ordinary-menu record and adjustment-cache read capture its visible/cache state only. The preserved 1.00 S03 archive in two verified locations remains separate evidence. Do not invent additional raw reads, expand offsets or issue a backup-restore command for this session.

## 4. Power before the final confirm

**Byte-read, vendor instructions:** Fujifilm requires a fully charged battery and says never to turn off or operate the camera while firmware updating. The [vendor procedure](https://www.fujifilm-x.com/en-us/support/download/procedure-x-interchangeable/) gives approximately five minutes maximum as generic X-series guidance. **Unproved:** neither battery runtime for this image nor this body's flash-load supply behaviour has been measured.

For a battery-powered install, use a genuine, healthy, freshly fully charged battery of the supported type. A partial charge, a low-battery indication or uncertain battery health is a pre-confirm stop. Do not plan to change batteries, attach a charger or switch to the dummy battery mid-update. USB is not the install path or a substitute for the power requirement.

For the owner's existing dummy-battery supply, secure the connector and lead, keep the battery/card door closed, use a stable mains-backed source not on a shared switch, and place the camera and supply where nobody can bump or switch them. Do not rely on a laptop staying awake. **Hardware-observed:** idle/read-only maintenance supply cuts have preserved settings on this body. **Unproved:** voltage/current margin, sag during erase/program, transient response and uninterrupted mains availability remain unmeasured; those cuts establish nothing about safety during firmware writes. Do not treat the dummy supply as proven safer under flash load than a fully charged battery.

Choose the power arrangement before installation entry and record it. If it is uncertain, stop before confirmation. Leave the lens mounted, remove USB while power is off, secure the card door, leave the camera on a stable surface and reserve uninterrupted time beyond the vendor's five-minute estimate. From the final confirm to the completion screen, no switch, cable, card, door or battery is touched. The one-time cached checksum-success byte `0x15c1e58` is not cleared by a menu exit, card swap or new confirmation; after any aborted prewrite attempt, make a real power cut before another scoped attempt, and after processing begins never cut power or retry ([install-procedure.md](install-procedure.md) section 7).

## 5. Camera acts: enter, inspect, confirm, wait

The screen descriptions below come from the vendor's illustrated procedure (**byte-read documentary text/images**) and the mapped body-update route (**static-inference**); the 2026-10-09 RC0 install supplies the equal-version observations for the screens this body actually shows. **Unproved:** the on-screen wording and intermediate screen order for this image until recorded. Do not advance an unfamiliar screen by guessing. The vendor shows several OK presses, including a final battery-cover/power warning with BACK CANCEL. Treat the first confirmation of an upgrade as the operational commitment if the body skips intermediate screens.

| Act | Owner action | Expected observation and record |
| --- | --- | --- |
| I0 | With camera OFF, insert the re-read verified install card; close the door. Start an external screen/LED video and a clock if available. Record C3's length/hash and the chosen supply. | Camera off, USB absent, power and card secured. |
| I1 | Hold DISP/BACK, switch ON, then release when the firmware screen appears. Do not hold Fn or the shutter/Up maintenance combination. | FIRMWARE UPGRADE/version display with body 1.50. DISP/BACK entry is byte-read/static-inference; an upgrade screen has been hardware-observed on this body. Record the whole display. |
| I2 | Inspect the screen before pressing OK. | Correct body route and 1.50 information; no wrong model, lens route, file error, low battery or refusal. **Hardware-observed 2026-10-09:** an equal-version 1.50 container was offered, not refused. If the equal-version update is not offered, abort here. |
| I3 | Press MENU/OK once to reach item selection, if shown. Select BODY, not LENS, and press MENU/OK once. Photograph the item and any current/new version display. | BODY selected; both intended versions 1.50. Vendor illustrated item selection is byte-read; this body's wording unproved. |
| I4 | At FIRMWARE UPGRADE OK?, inspect the choice. CANCEL is the vendor's initial selection. Select OK only if all prior checks hold, then press MENU/OK once. If the warning with BACK CANCEL appears, pause there without pressing its final OK. | Vendor sequence: confirmation, then DO NOT OPEN A BATTERY COVER NOR TURN OFF THE CAMERA. No processing has begun on that illustrated sequence. If PROCESSING starts instead, immediately follow I6 and do not cancel. |
| I5 | At the last warning/confirm screen, record its photograph/time, keep the camera untouched while deciding, then press its OK once only under the owner's named go. | This OK is the visible point of no return. It leads to PROCESSING in the vendor sequence; do not double-press. |
| I6 | Let the camera finish. Watch and record only. No buttons, USB, card removal, door opening, battery movement, POWER switch or supply cut. | Vendor illustration shows PROCESSING and progress squares. A pause, unchanged squares, blank display or unexpected message is recorded without interrupting power. Per-phase display correspondence is unproved. |
| I7 | Wait for an explicit FIRMWARE UPGRADE COMPLETED / TURN OFF THE CAMERA instruction. Record the complete screen and elapsed time before acting. | Completion, not a stopped-looking progress bar, ends the hands-off interval. Displayed VERSION NO. should still be 1.50. |
| I8 | Only after I7, switch OFF as instructed and wait for shutdown. With power off, remove the update card and use the normal photography card for verification. | Ordinary shutdown after the update, not a supply-cut maintenance ending. Keep the install card and its bytes as the session input until the evidence is secured. |

**Byte-read:** internally, staging and the three checksum checks are RAM/card reads only, unless the cached-success byte bypasses them. The first persistent mutation is S1, erase of raw trailer block `0x125`, whose boot marker is at media `0x24a0000` with halfword `0xaaaa` at `+0x1e`. The erase precedes all firmware block programs. **Static-inference:** once it lands, restarting cannot run the old application updater. This is the invisible point of no return. No UI indication locates it, so no post-confirm pause can be used to guess that aborting is still safe.

### Confirm-to-completion timing, and what a pause means

**Hardware-observed, 2026-10-09 (E-INSTALL-TIMING-150, one video frame precision):** the RC0 install's visible span from the accepted confirm to the completion screen was **57.3 s** (video 17.0 s to 74.3 s), with a **13.7 s longest pause** (17.0 s to 30.7 s, all ten progress squares unlit) that looks like a hang and is not, and the ten progress segments lighting at 30.7, 41.2, 49.6, 52.8, 55.5, 58.5, 61.3, 64.4, 67.2 and 70.3 s, the last holding black within [72.3 s, 72.8 s]. The completion screen read `FIRMWARE UPGRADE COMPLETED` / `TURN OFF THE CAMERA` / `VERSION : 1.50`. This image is a different container of the same size through the same updater and its own timing is **unmeasured**; the S0-to-S5 split stays unmeasured for both, the vendor's generic text allows about five minutes maximum, and no mapped edge supplies a timeout. A longer run is recorded with power connected, never interrupted. The [install procedure](install-procedure.md) section 2 and [the RC0 session's timing table](filmsim-install-session-script.md#phase-durations-and-pauses) own the phase mapping.

### Stop conditions

- Before any upgrade-confirm OK: wrong image/hash, file/model/version mismatch, no equal-version offer, low/uncertain power, unfamiliar screen or any unexpected behaviour means cancel if available, switch off, then cut the supply or remove the battery after shutdown. Stop and report; do not correct it in the same boot.
- At the vendor's final warning, BACK CANCEL is still shown, but only use it while the warning remains and PROCESSING has not begun. If screen order differs, the first accepted upgrade confirmation is the commitment; do not assume another cancel screen will follow.
- After confirmation has started processing: there is no mapped operator cancel or repair act. Let it finish. If the display seems frozen or errors, preserve power, photograph/video it, record elapsed time and report. Do not press BACK, retry, attach USB, or cut power. A claimed timeout is not permission to interrupt.
- After any aborted prewrite attempt, make a real power cut before another attempt, under a new owner go and repeated card checks.
- If power fails after the confirm, do not attempt repeated starts, generic battery-removal tricks, a stock reflash, SD-BOOT presses, S-record or AUTO_ACT as a rescue. Record the last display, exact interruption and card/supply state, preserve the artifacts, and stop camera operations. No repair is demonstrated.

## 6. Verify normal operation, then the nine entries

| Act | Owner action | Expected observation and record |
| --- | --- | --- |
| V0 | With no USB and a normal photography card, boot normally once. Record video/time from switch-on to menus. | No halt/hang and no first-run setup prompt. A nonworking body follows section 7, not a menu test. This install's first boot is unproved until observed. |
| V1 | Compare date/time, language, C-slot values and watched controls with B1; photograph BODY VERSION using the ordinary version-check entry if needed, then leave it by ordinary shutdown/restart. Do not reset or overwrite the C slot. | Watched settings retained, BODY VERSION 1.50. Byte-read version getter is not proof of installed-image identity. A setup prompt or lost setting is a stop for host analysis, not permission to restore defaults and hide the difference. |
| V2 | In the same ordinary still M used at B2, open SHOOTING MENU → FILM SIMULATION and photograph/scroll the whole list without accepting a new choice. Repeat once at the Fn menu and once at the Q menu's film-simulation cell. | Nine offered choices in order: PROVIA/STANDARD, Velvia/VIVID, ASTIA/SOFT, B&W, SEPIA, PRO Neg Std, PRO Neg Hi, `WARM STANDARD`, a second Velvia entry; the Q cell scrolls the same nine. The four RC0/PRO Neg names are the installed-list precedent; the two recipe rows' names and selectability are unproved until recorded. |
| V3 | Select `WARM STANDARD`, leave the menu, reopen it to verify the selected value, then read the eight parameter fields. Take one fixed-scene test shot and play it back. Repeat for the second Velvia entry. Record selection screens, filenames, playback and the parameter readouts. | Both recipe rows selectable; the eight parameter fields take the compiled values through the same path a manual change takes (recipe A: DR 200 %, highlight medium hard, shadow medium soft, colour high, sharpness standard, NR medium low, WB auto, ISO 400; recipe B: DR 100 %, highlight standard, shadow standard, colour medium high, sharpness medium soft, NR low, WB auto, ISO 200, menu names per [recipe-patch-d3.md](recipe-patch-d3.md) section 6.1). Do not infer a required visual difference from any one scene: the compiled rows' rendering is unproved and the measured nine-shot rendering is the stock three modes'. |
| V4 | Select any other film row, then return to the original film simulation and the B3 scene/settings; take/play back one stock-mode shot. Switch off normally, boot again and photograph the nine-entry M list again. | Stock mode still works; the nine entries survive a normal power cycle; the recipe token clears when another film row is selected (byte-read `0x00d4fc74` case behaviour). Save originals from all shots. |
| V5 | Record current values and any remaining differences before the post-install byte captures of section 8. | No unexplained regression; selected simulations and any temporary scene-setting changes are distinguished from update effects. |

**Static-inference:** the new availability cells copy the ASTIA/Velvia seven-column pattern, not every dial position, and the two recipe rows are reachable only where the film list already appears. Use M, which is the ordinary comparison mode, and ensure it offered selectable ASTIA and Velvia before installing. A special mode with greyed-out ASTIA/Velvia is not a valid nine-entry failure test. The FILM 1-3 bracket children keep their own five stock rows and cannot select a recipe ([recipe-patch-d3.md](recipe-patch-d3.md) sections 6.2 and 6.4). The Q cell offers the same nine rows through its own Q label array, and each recipe row draws its base mode's Q glyph; the Q-cell drawing is unproved until photographed.

## 7. Missing or extra entries, selection failures, versus a nonworking camera

If the body boots, menus respond and capture/playback work but the film list does not read as section 6's nine entries, do not reflash, raise version words or try another container. Photograph the list, mode and all greyed entries; retain the completion video and proceed only to the already scoped reads in section 8. **Static-inference:** zeroed or overwritten prefix arrays, a wrong shooting-mode gate, a partial image-4 program or a different loaded image can each explain a wrong list; the photograph alone cannot select among them. A wrong list is a reported install outcome, not a reason for a same-day second flash.

If the body does not boot or cannot reach normal menus after an explicit update completion and one first boot, stop camera operations. Do not attempt the maintenance reads, repeated power cycles, card-trigger experiments or another update. **Static-inference:** an invalid application or an LZ-engine/load failure can stop before the updater or service command exists; which failure happened must stay unresolved without new evidence.

If settings/calibration differ unexpectedly, a setup prompt appears, capture/playback fails or the readback mismatches the intended change set, stop normal use and report. **Static-inference:** interruption of S4 can leave firmware that boots with defaults or torn adjustment bytes; repeating an update restores from the then-current cache and cannot recreate this body's lost bytes. Do not treat boot success or 1.50 on screen as a calibration check.

## 8. Post-install byte readback and evidence handoff

The byte-level verification is a separate prepared session: [the recipe post-install readback session](recipe-postinstall-session-script.md). It runs the existing reviewed read-only operations in one maintenance boot, compares every installed extent against the deliverable's own recorded bytes, and ends by cutting the dummy-battery supply. Run it only with a normally functioning body and only under the owner's scope for those operations. Its act commands, in order:

```sh
devenv shell -- bazel run //transport:camera -- dump-ram id4-image-0
devenv shell -- bazel run //transport:camera -- dump-ram id4-image-1
devenv shell -- bazel run //transport:camera -- dump-ram id5-image-0
devenv shell -- bazel run //transport:camera -- dump-ram id5-image-1
devenv shell -- bazel run //transport:camera -- dump-ram plain-0
devenv shell -- bazel run //transport:camera -- read-config "$PWD/private-data/read-plans/adjustment-42000.json"
devenv shell -- bazel run //transport:camera -- read-media
```

Run one command at a time and stop on the first refusal, unexpected reply, disconnect or cleanup uncertainty; use the readback session's supply-cut ending after a failed maintenance command and do not run the remaining commands or retry in that boot.

**What the owner observes, and what the host verifies.** The owner observes and records the card-prep checks' outputs, the pre-install settings/menu/shot, the firmware screens and the whole confirm-to-completion interval, the first boot, the nine-entry list and the Q cell, the recipe selections and parameter readouts, the test shots and the power-cycle survival. The host verifies the card digest and `cmp`, the before/after session journals and gate readbacks, the readback comparisons of the post-install session (image-4 decode, image-5 words, plain-window writes, segment-table words), the adjustment mirror against the before capture, and the area-2 banks against the before ledger. A visible list or a version screen is an observation until the host comparisons name which container's bytes are running.

The private handoff contains the named image/path and verified card digest/length; owner go and operation scope; the before settings/menu/shot and the B5/B6 printed folders with their journals, outgoing records, files and hashes; the confirm/completion photographs and external video with elapsed times; first-boot and final-boot results; the nine-entry photographs including the recipe rows' selection marks and parameter readouts; original stock/recipe test images and filenames/settings; the readback/config/bank comparison results; and every error or deviation. The public report/evidence ledger records only scoped results and private artifact references, with **hardware-observed** for owner acts/results, **byte-read** for checked capture bytes, **static-inference** for interpretations and **unproved** for the named gaps.

## 9. State-changing operations in this session

| Operation | Effect | Class and evidence |
| --- | --- | --- |
| Ordinary controls, M comparison, recipe/stock selection, test shots and ordinary shutdown | Shooting settings and their ordinary commit records may change; recipe selections write the ordinary block or the C-slot RAM record; shots write the photography card. No C-slot save/reset is requested. | Owner-operated ordinary settings/capture; hardware-observed prior methods, this session's effects unproved until recorded. |
| Format dedicated install card | Erases its existing files and creates the ordinary card filesystem. | Owner-operated persistent card change; byte-read vendor procedure, outcome unproved. |
| Host copy/removal of unwanted files | Changes only the dedicated card's contents, not camera internal media. | Host/card preparation; byte-read required image and filename. |
| Maintenance entry and B4/B7 session framing | Session/request/reply RAM state. B7 does not exit maintenance with the POWER switch. | H1 for read-config; byte-read framing, static-inference effect scope. |
| `read-config` with the existing full plan | Function 2 reads only; no configuration function-3 write or persistence command. | H1; byte-read transport contract, hardware-observed method. |
| `read-media` | Reviewed RAM gate enable/restore, staging fields at `0x81`-`0x8a`, trigger `0x80 = 0x35`, disarm `0x80 = 0x33`, original-byte restores, bounded raw area-2 reads; shuts down storage driver. No internal-media program/erase request. | H2; byte-read closed operation, static-inference effect scope, hardware-observed bank-read method. |
| Maintenance supply cut | Clears volatile gates/arm state and restarts the body after read-media; no POWER/OFF defaults record is requested. | Physical ending; hardware-observed prior settings-preserving control, this ending unproved. Never applied to an in-progress update. |
| SD BODY firmware upgrade of the section-2 image | S1 trailer erase; S2 optional span erases; S3 program/verify/remap; S4 configuration refresh/restore/commit; possible S5 directory-journal commit. Replaces the installed RC0 container's images and plain window. | H3, owner's go naming operation and image required; byte-read reviewed container/order, static-inference residues, hardware outcome unproved. |

No configuration/calibration restore, persistent gate enable, `0x80 = 0x22` whole-cache commit, End Setting, S-record writer, AUTO_ACT arm, raw SD-BOOT install or intentional interruption belongs to this session. A successful install is the nine listed entries surviving power cycles, working capture/playback, unchanged watched body state and a readback that matches the reviewed image; the compiled rows' rendering and the hook's hardware fetch remain **unproved** until observed, and **unproved:** a demonstrated recovery remains absent even after a successful installation.
