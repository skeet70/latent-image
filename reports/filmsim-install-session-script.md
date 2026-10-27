# Film-simulation install session: capture, install, verify (X-M1 1.50)

Prepared 2026-10-09 for the owner's chosen film-simulation install after the AUTO_ACT experiment. No install or capture is authorized by this document. The owner runs every camera operation under [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs); the H3 go names the SD-card body firmware install and the image path and SHA-256 in section 2. The before/after service captures are separate named operations in the session's scope, not implied by permission to flash.

**Run 2026-10-09 (E-FILMSIM-INSTALL-150, hardware-observed).** The section 2 acts ran: the container was copied to a freshly formatted card as its only file, the upgrade prompt read 1.50 to 1.50, the owner confirmed, and the body boots with `SHOOTING MENU` to `FILM SIMULATION` showing seven entries against the stock five, including PRO Neg Std and PRO Neg Hi. The act outcomes: I0 to I8 ran; V0 is a normal first boot with no defect and no unexpected screen; V2 is the seven-entry list; V1 is the owner's report of manual settings, date, time and language intact, and no other setting is claimed. V3 to V5 and R0 to R6 have not run, so rendering, power-cycle survival and the byte readback stay open. The before half is on record as E-PREINSTALL-CONFIG-150, E-PREINSTALL-MEDIA-150 and E-PREINSTALL-ADJ-VALUES-150. The timing table below is unmeasured: no stopwatch timing was taken during the run, and the owner's process video of the upgrade is the source those columns wait on.

**Byte-read:** the independently accepted container is data-only RC0: it adds PRO Neg Std and PRO Neg Hi to the existing offer list, changes their availability cells and relocates the shared arrays into a 192-byte decode prefix. It adds no executing code. The D3 recipe image is not this image and is outside this session. The byte review is `agent://ReviewPrefixContainer2`; the [patch report](filmsim-patch-01.md#5-decode-prefix-extension-and-container-contract) owns the bytes and placement argument.

**Static-inference:** an interruption between trailer erase and completion of the firmware program leaves an invalid application with no demonstrated repair. **Hardware-observed:** documented strap-entry presses did not select SD boot on this body. **Static-inference:** the diagnostic SD card has no known actuation here, the S-record route is rejected, cold reset provides no repair route, the other update filenames have no reader, and the stored AUTO_ACT enable byte is zero, so a card file alone cannot run it. The [recovery status](recovery-status.md), [hardware routes](recovery-hardware-routes.md), [S-record report](srecord-route.md) and [AUTO_ACT map](auto-act-script.md) carry those bounds. A stock DAT and the retained adjustment captures are not a working recovery system.

**Byte-read:** E-RECOVERY-EMUL-150 records 36 of 36 passing native-harness interruption cases at the declared media-model boundary. **Static-inference:** these establish the modeled erase/program ordering, not hardware recovery. **Unproved:** device physics, bad-block persistence and power loss inside one persistence step remain outside that model. **Unproved:** the hardware LZ engine has consumed two observed vendor streams, not this third stream produced by our encoder. **Static-inference:** the prefix survives if no table-block reload or other effective writer follows decompression; a reload predicts missing new entries on an otherwise working body, not a crash. Absence of every later writer remains **unproved**.

## 1. First acts: record the body before the install

Use a normal photography card with no firmware, script or diagnostic files for these acts. Keep the install card out of the camera until section 3. End the AUTO_ACT experiment by its prescribed supply cut first; do not carry its armed RAM state into these captures. Use the dummy-battery supply for maintenance captures, with the camera off before connecting USB. Nothing here calls `arm-auto-act`, End Setting, a configuration commit or a restore.

Record one row per act in a private note sheet under `private-data/`, with time, what happened and the saved file or printed session-folder path. Preserve serial-bearing screens, raw logs, photographs and captures privately. Section 8 specifies the handoff.

| Act | Owner action | Expected observation and record | Evidence |
| --- | --- | --- | --- |
| B0 | Start the note sheet. Record body identifier, lens and lens firmware, supply arrangement, original dial position and current film simulation. Power on normally, without USB. | Menus and live view respond; record any existing defect before the install. | Hardware-observed method from prior ordinary sessions; this session's result unproved. |
| B0v | Switch off, then switch on holding DISP/BACK with the no-firmware photography card. Photograph BODY VERSION and lens version; switch off, then boot normally without held keys. Do not enter an upgrade. | BODY VERSION 1.50; retain the displayed lens version and both screen photographs. Any different body version stops this procedure. | Byte-read vendor version-check procedure; body 1.50 hardware-observed in prior owner reports, this read unproved. |
| B1 | Photograph or transcribe date/time, language, the C-slot values without saving over the C slot, and the shooting/SET-UP values listed below. Do not reset anything. | A before record of settings that an update or a wrong maintenance ending could lose. | Hardware-observed settings-reset risk, E-JIG-CYCLE-D-150. |
| B2 | Use ordinary still M for the film-menu comparison. Photograph every page of SHOOTING MENU → FILM SIMULATION, scrolling without accepting a new choice. Record the dial position. | Five entries: PROVIA, Velvia, ASTIA, B&W and SEPIA, in that order. Retain the actual punctuation and icons in the photographs. | Byte-read stock arrays; five-entry stock outcome hardware-observed in prior owner sessions. |
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

**Byte-read:** the existing plan has 1,309 windows, covers adjustment `[0,0x42000)` plus `[0x400000,0x40fd00)`, and includes the `X-M1` oracle at `+0x15c` and `55 aa` signature at `+0x202`. Check the new capture's oracle/signature, page-zero `0xa2`/`0xf7`, `+0x1e8`/`+0x1e9`, `+0x204`/`+0x205` and `+0x275`/`+0x276`. Previous 1.50 captures read `0xa2 = 0`, `0xf7 = 0`, update stamp `01 54`, boot-build stamp `01 78`, and pre-erase controls `01 0a`; these are comparison expectations, not values to write. Unexpected values are a stop for host analysis.

**Static-inference:** the full adjustment read supports the restore-list comparison and records this body's calibration mirror before refresh; `read-media` supplies the two persistent ordinary-settings banks and their record history. Neither is a full persistent-storage backup or a demonstrated restore. `read-media` does not read the C-slot's area-3 banks; the ordinary-menu record and adjustment-cache read capture its visible/cache state only. The preserved 1.00 S03 archive in two verified locations remains separate evidence. Do not invent additional raw reads, expand offsets or issue a backup-restore command for this session.

## 2. The install bytes

| Field | Required value | Evidence |
| --- | --- | --- |
| Host source | `private-data/analysis/x-m1-1.50/update/prefix/FWUP0002-prefix.DAT` | Byte-read accepted container. |
| File on card | Root `FWUP0002.DAT`, no directory and no suffix | Byte-read body lookup derived from this body's `0002` configuration bytes. |
| Length | 38,405,148 bytes | Byte-read accepted container. |
| SHA-256 | `acb0fbbb9507421f48a6254920cafb5ce20e1fd6bf821bc80c912c1f99c29ca8` | Byte-read independent review. |
| Body/version | X-M1, 1.50; version words remain `1` / `0x50` | Byte-read identity/version fields, unchanged from stock. |
| Expected offer list | Stock five, then PRO Neg Std and PRO Neg Hi | Byte-read patch arrays; hardware outcome unproved. |

The host source basename is not the card basename. Do not rebuild, alter version words, substitute D3, substitute the earlier matrix-only experiment, or copy the stock image under the approved image's name. **Byte-read:** both auxiliary restore lists are byte-identical to stock in the accepted container; this matters independently of the main firmware checksum.

## 3. Prepare and re-read the install card

Use a dedicated ordinary camera-compatible SD card, not the raw diagnostic SD-BOOT card image. Preserve anything on it before formatting. Camera formatting is a separate state-changing act covered by the owner's session scope; it happens before the update file is copied, never during installation.

| Act | Owner action | Expected observation and record |
| --- | --- | --- |
| C0 | With the install card in the normally working camera, choose SET-UP → FORMAT, select OK and press MENU/OK. Wait for format completion, then switch off normally. | Formatted card; existing files erased. The vendor's instructions are byte-read, this card's outcome unproved. |
| C1 | Remove the card while the camera is off and insert it in the host reader. Copy only the section-2 source to its root as `FWUP0002.DAT`. | Correct root name, not `FWUP0002-prefix.DAT`, `FWUP0002 (1).DAT` or a hidden extra extension. |
| C2 | Run the host-only checks below against source and card copy. Record both lengths, both digests and the `cmp` exit status. | Both lengths 38,405,148; both SHA-256 values match section 2; `cmp` exits 0. Any mismatch stops preparation. |
| C3 | Flush and safely unmount/eject through the host, remount/reinsert the card in the host reader, and repeat the card length/hash/`cmp` checks. Inspect its root and directories, including hidden files. | The re-read after the write cache is gone still matches. No alternate payload remains. |
| C4 | Safely eject again. Only now may this prepared card go near the off camera. | The note sheet carries the verified digest and length of the card-side file, not just the host source. |

Replace `<card>` with the mounted card directory. These are owner-run host/card commands, not camera commands.

```sh
IMAGE="$PWD/private-data/analysis/x-m1-1.50/update/prefix/FWUP0002-prefix.DAT"
CARD="<card>"
cp -- "$IMAGE" "$CARD/FWUP0002.DAT"
wc -c -- "$IMAGE" "$CARD/FWUP0002.DAT"
sha256sum -- "$IMAGE" "$CARD/FWUP0002.DAT"
cmp -- "$IMAGE" "$CARD/FWUP0002.DAT"
```

Keep `FWUP0002.DAT` as the only added payload. Empty filesystem directories left by camera formatting, such as `DCIM`, may remain; no photographs or host index/trash files are needed on this dedicated card. Remove the AUTO_ACT experiment's `DCAA` directory and script, `IMFIDX10/LX30A.S3`, `ffsys.bin`, `ffboot.bin`, `ffuser.bin`, alternative `FWUP0001.DAT`/`FPUPDATE.DAT`, other firmware/lens updates and any diagnostic-card layout. This is a card-content restriction, not a claim that every forbidden file would execute. Do not include a second copy as a fallback.

## 4. Power before the final confirm

**Byte-read, vendor instructions:** Fujifilm requires a fully charged battery and says never to turn off or operate the camera while firmware updating. The [vendor procedure](https://www.fujifilm-x.com/en-us/support/download/procedure-x-interchangeable/) gives approximately five minutes maximum as generic X-series guidance. **Unproved:** neither battery runtime for this image nor this body's flash-load supply behaviour has been measured.

For a battery-powered install, use a genuine, healthy, freshly fully charged battery of the supported type. A partial charge, a low-battery indication or uncertain battery health is a pre-confirm stop. Do not plan to change batteries, attach a charger or switch to the dummy battery mid-update. USB is not the install path or a substitute for the power requirement.

For the owner's existing dummy-battery supply, secure the connector and lead, keep the battery/card door closed, use a stable mains-backed source not on a shared switch, and place the camera and supply where nobody can bump or switch them. Do not rely on a laptop staying awake. **Hardware-observed:** idle/read-only maintenance supply cuts have preserved settings on this body. **Unproved:** voltage/current margin, sag during erase/program, transient response and uninterrupted mains availability remain unmeasured; those cuts establish nothing about safety during firmware writes. Do not treat the dummy supply as proven safer under flash load than a fully charged battery.

Choose the power arrangement before installation entry and record it. If it is uncertain, stop before confirmation. Leave the lens mounted, remove USB while power is off, secure the card door, leave the camera on a stable surface and reserve uninterrupted time beyond the vendor's five-minute estimate. From the final confirm to the completion screen, no switch, cable, card, door or battery is touched.

## 5. Camera acts: enter, inspect, confirm, wait

The screen descriptions below come from the vendor's illustrated procedure (**byte-read documentary text/images**) and the mapped body-update route (**static-inference**). The wording and intermediate screen order for this equal-version X-M1 candidate remain **unproved**. Do not advance an unfamiliar screen by guessing. The vendor shows several OK presses, including a final battery-cover/power warning with BACK CANCEL. Treat the first confirmation of an upgrade as the operational commitment if the body skips intermediate screens.

| Act | Owner action | Expected observation and record |
| --- | --- | --- |
| I0 | With camera OFF, insert the re-read verified install card; close the door. Start an external screen/LED video and a clock if available. Record C3's length/hash and the chosen supply. | Camera off, USB absent, power and card secured. |
| I1 | Hold DISP/BACK, switch ON, then release when the firmware screen appears. Do not hold Fn or the shutter/Up maintenance combination. | FIRMWARE UPGRADE/version display with body 1.50. DISP/BACK entry is byte-read/static-inference; an upgrade screen has been hardware-observed on this body. Record the whole display. |
| I2 | Inspect the screen before pressing OK. | Correct body route and 1.50 information; no wrong model, lens route, file error, low battery or refusal. If the equal-version update is not offered, abort here. Equal-version body comparison is byte-read; complete UI eligibility unproved. |
| I3 | Press MENU/OK once to reach item selection, if shown. Select BODY, not LENS, and press MENU/OK once. Photograph the item and any current/new version display. | BODY selected; both intended versions 1.50. Vendor illustrated item selection is byte-read; this body's wording unproved. |
| I4 | At FIRMWARE UPGRADE OK?, inspect the choice. CANCEL is the vendor's initial selection. Select OK only if all prior checks hold, then press MENU/OK once. If the warning with BACK CANCEL appears, pause there without pressing its final OK. | Vendor sequence: confirmation, then DO NOT OPEN A BATTERY COVER NOR TURN OFF THE CAMERA. No processing has begun on that illustrated sequence. If PROCESSING starts instead, immediately follow I6 and do not cancel. |
| I5 | At the last warning/confirm screen, record its photograph/time, keep the camera untouched while deciding, then press its OK once only under the owner's named go. | This OK is the visible point of no return. It leads to PROCESSING in the vendor sequence; do not double-press. |
| I6 | Let the camera finish. Watch and record only. No buttons, USB, card removal, door opening, battery movement, POWER switch or supply cut. | Vendor illustration shows PROCESSING and progress squares. A pause, unchanged squares, blank display or unexpected message is recorded without interrupting power. Per-phase display correspondence is unproved. |
| I7 | Wait for an explicit FIRMWARE UPGRADE COMPLETED / TURN OFF THE CAMERA instruction. Record the complete screen and elapsed time before acting. | Completion, not a stopped-looking progress bar, ends the hands-off interval. Displayed VERSION NO. should still be 1.50. |
| I8 | Only after I7, switch OFF as instructed and wait for shutdown. With power off, remove the update card and use the normal photography card for verification. | Ordinary shutdown after the update, not a supply-cut maintenance ending. Keep the install card and its bytes as the session input until the evidence is secured. |

**Byte-read:** internally, staging and the three checksum checks are RAM/card reads only, unless the cached-success byte bypasses them. The first persistent mutation is S1, erase of raw trailer block `0x125`, whose boot marker is at media `0x24a0000` with halfword `0xaaaa` at `+0x1e`. The erase precedes all firmware block programs. **Static-inference:** once it lands, restarting cannot run the old application updater. This is the invisible point of no return. No UI indication locates it, so no post-confirm pause can be used to guess that aborting is still safe.

### Phase durations and pauses

Do not use these figures as cut-power deadlines. The visible-screen boundaries and the hidden S1–S5 phases cannot be timed individually from a recorded screen alone.

**Measured column, 2026-10-09 (hardware-observed, one video frame precision):** the source is the owner's process recording, `private-data/VID_20261009_105432.mp4`, 3,383 frames at 30 fps over 112.71 s, SHA-256 `7c092a3c0196323f906bca44cdd25964d8df0a736738866887f9942e1d350f3a`. Screen switches are read from the decoded frames, and the progress row is tracked per frame by its ten squares (a finished square holds black, the square in progress blinks), so the boundaries below are frame timestamps quoted to 0.1 s. The recording opens with the confirm already displayed, so the acts before it are not on the timeline, and it starts at the owner's act rather than at S0; times are video time, not camera time. Supporting analysis and the source digests are E-INSTALL-TIMING-150.

| Phase | Duration known before this run | Measured 2026-10-09 | What a pause means | Evidence |
| --- | --- | --- | --- | --- |
| Entry, header read, model/version eligibility | No measured X-M1 duration; owner-controlled time at prompts | Not on the video: the recording opens with the confirm already on screen | Waiting at an unconfirmed prompt is allowed. Refusal stops before the write route. | Static-inference route; timing unproved. |
| S0: read 38,405,148-byte container ranges, stage, three sums | No measured duration | Inside [17.0 s, 30.7 s], the stretch that shows no progress; not separable on screen | Progress without visible motion is not evidence of a hang or of write entry. | Byte-read range/ordering; screen cannot bind this phase. |
| S1: trailer erase | No measured duration and no distinct screen binding | Invisible. Inside [17.0 s, 30.7 s] if segment 1 marks the start of S3, otherwise inside [17.0 s, 74.3 s] | Invisible first write, not an operator pause point. | Byte-read ordering; screen binding unproved. |
| S2: optional pre-erase and deliberate sleep | Last capture's controls `0x276 = 0x0a`, `0x275 = 0x01` predict ten full-span erase passes, then one 1,000 ms sleep. Erase time itself is unknown. Re-check B5's controls. | Same window as S1; the passes and the sleep are not separable on screen | A delay before block-by-block progress has a mapped cause. The sleep is once after the passes, not one second per block. | Byte-read controls/code; the 13.7 s unlit stretch is consistent with this phase but does not count its passes. |
| S3: firmware program/verify, possible bad-block handling | 294 logical blocks (`0` through `0x125`), 64 pages of `0x800` per block; no measured wall-clock duration | Inside [30.7 s, 74.3 s]; visible only as the ten segment activations in the timeline below | Callback progress advances per completed block, not every page. An unchanged indicator is not a safe-abort signal. | Byte-read counts/progress; segment count and timing do not bind blocks to segments. |
| S4: configuration refresh, restore and commit | No measured duration; commits logical indices `[0x29,0x4a)` to allocator-selected blocks in physical pool `[0x380,0x400)`, media `[0x7000000,0x8000000)` ([bounded mapping](update-validation-and-boot.md#native-writer-boundary-and-preservation-limits)) | At most the [72.3 s, 74.3 s] tail; unproved | Firmware marker arrival does not mean settings work is finished. Do not stop at apparently full firmware progress. | Byte-read logical extent and allocator pool, not a fixed physical write span; screen cannot bind this phase. |
| S5: update-session commit | No measured duration; chain-A row 13/state 0 selects callback `0xec4618`, which registers the separate directory-journal step ([selection contract](update-validation-and-boot.md#commit-step-and-service-writers)) | Order relative to S1-S4 and screen binding unproved; no final-tail duration assigned | Wait for the explicit completion instruction, not an inferred last write. | Byte-read selection; no established total order against the body writer or screen binding. |
| Whole update | Vendor image gives approximately 90 seconds; current vendor text gives approximately five minutes maximum | **57.3 s measured**: confirm at 17.0 s to the completion screen at 74.3 s | These are generic examples, not measurements or an enforced timeout for this image. Longer than five minutes means record/report while leaving power connected, not restart. | **Hardware-observed** screen boundaries for this recording; the S0-to-S5 split stays unmeasured. |
| First candidate boot and later service reads | Candidate boot duration unmeasured; commands have their own transport deadlines | LCD dark at 83.7 s, lit again at 90.3 s, live view complete at about 91.1 s. The power-on press is off frame, so the boot itself is not timed | Do not transpose the update estimate onto boot or USB captures. A command ending reports its own success/refusal. | **Hardware-observed** screen boundaries; candidate boot timing still unproved. |

The [native writer analysis](update-validation-and-boot.md#native-writer-boundary-and-preservation-limits) binds the ten passes, single sleep and per-block callback. Its original provenance is private `private-data/analysis/x-m1-1.50/update/natw-nativewriter-report.md`, section 2. The measured column now carries the video's screen boundaries; nothing supplies seconds for the individual S0 to S5 phases, because no screen element identifies them.

**Screen timeline (video time, one frame precision):**

| Event | Time |
| --- | --- |
| Confirm (`OK YES` / `BACK CANCEL`) displayed at the recording's start and unchanged | 0.0 s |
| `MENU/OK` accepted; `PROCESSING...` replaces it, ten squares, none lit | 17.0 s |
| No visible change: all ten squares unlit | 17.0 s to 30.7 s (13.7 s) |
| Segment 1 lights | 30.7 s |
| Segments 2 to 10 light | 41.2, 49.6, 52.8, 55.5, 58.5, 61.3, 64.4, 67.2, 70.3 s |
| The last segment stops blinking and holds black | in [72.3 s, 72.8 s] |
| `FIRMWARE UPGRADE COMPLETED` with `TURN OFF THE CAMERA` and `VERSION : 1.50` | 74.3 s |
| Completion screen while the owner reads it and switches off | 74.3 s to 83.7 s (9.5 s) |
| Camera dark: power off, then the card swap and the power-on, both off frame | 83.7 s to 90.3 s |
| LCD lit again; live view complete with the shooting overlay and the frame cross | about 91.1 s |
| `SHOOTING MENU` (MANUAL) | 93.1 s |
| `FILM SIMULATION` entry screen, then the whole seven-entry list | 96.3 s |
| Recording ends | 112.7 s |

**Longest pause: 13.7 s.** From `PROCESSING...` at 17.0 s until the first lit segment at 30.7 s nothing on the screen changes: all ten squares stay unlit, the header text is static, and the frame-to-frame differences stay at panel-refresh level. That is the number an operator needs, since it is what a frozen-looking screen is worth without being abnormal: 13.7 s of no change opens this image's processing, the second segment then takes another 10.6 s, and the remaining eight arrive about every 3 s. A pause is still not a safe-abort signal at any length, because no mapped act stops the writes once they start.

**Ten-pass prediction against the display.** The progress row holds ten squares and ten segment activations were observed, but that count does not confirm the ten pre-erase passes: the measured segment lengths are 10.6, 8.4, 3.1, 2.7, 3.0, 2.8, 3.1, 2.8, 3.1 and 3.9 s, while ten identical full-span erase passes should be near-equal, and the unlit 13.7 s before the first segment is the only stretch an erase phase can occupy. A fixed ten-segment bar matching `min(10, 0x276)` numerically is a coincidence. So the `01 0a` reading of ten passes stays **static-inference** and is neither confirmed nor refuted here; the display cannot count passes.

**Unrecoverable window, bounded.** S1 has no screen binding. The confirm at 17.0 s precedes any media write and the completion screen at 74.3 s follows the last one, so the longest interval in which the body could not have booted its application is [17.0 s, 74.3 s], and the whole `PROCESSING...` display lies inside it. S1 precedes S2 and S3 in the mapped order, so if segment 1 marks the start of S3 the old marker died within [17.0 s, 30.7 s]. The new marker is programmed in the first page of block `0x125`, last in the program loop, and the final segment stops blinking within [72.3 s, 72.8 s] and holds black to the completion screen, so the bootable state returned within [70.3 s, 74.3 s]. **Unproved:** which instant inside those bounds destroyed the marker and which restored it; no screen element marks either.

### Abort rules

- Before any upgrade-confirm OK: wrong image/hash, file/model/version mismatch, no equal-version offer, low/uncertain power, unfamiliar screen or any unexpected behaviour means cancel if available, switch off, then cut the supply or remove the battery after shutdown. Stop and report; do not correct it in the same boot.
- At the vendor's final warning, BACK CANCEL is still shown, but only use it while the warning remains and PROCESSING has not begun. If screen order differs, the first accepted upgrade confirmation is the commitment; do not assume another cancel screen will follow.
- After confirmation has started processing: there is no mapped operator cancel or repair act. Let it finish. If the display seems frozen or errors, preserve power, photograph/video it, record elapsed time and report. Do not press BACK, retry, attach USB, or cut power. A claimed timeout is not permission to interrupt.
- After any aborted prewrite attempt, make a real power cut before another attempt. **Byte-read:** checksum-success byte `0x15c1e58` is set to 1 after successful staging/sums and has no mapped clear; another attempt in the same powered session skips reread and checksum work. A menu exit, card swap or second OK does not clear it. A fresh attempt needs a new owner go and repeats the card checks.
- If power fails after the confirm, do not attempt repeated starts, generic battery-removal tricks, a stock reflash, SD-BOOT presses, S-record or AUTO_ACT as a rescue. Record the last display, exact interruption and card/supply state, preserve the artifacts, and stop camera operations. No repair is demonstrated.

## 6. Verify normal operation, then seven entries

| Act | Owner action | Expected observation and record |
| --- | --- | --- |
| V0 | With no USB and a normal photography card, boot normally once. Record video/time from switch-on to menus. | No halt/hang and no first-run setup prompt. A nonworking body follows section 7, not a menu test. Candidate first boot is unproved until observed. |
| V1 | Compare date/time, language, C-slot values and watched controls with B1; photograph BODY VERSION using the ordinary version-check entry if needed, then leave it by ordinary shutdown/restart. Do not reset or overwrite the C slot. | Watched settings retained, BODY VERSION 1.50. Byte-read version getter is not proof of installed-image identity. A setup prompt or lost setting is a stop for host analysis, not permission to restore defaults and hide the difference. |
| V2 | In the same ordinary still M used at B2, open SHOOTING MENU → FILM SIMULATION and photograph/scroll the whole list without accepting a new choice. | Seven offered choices in order: PROVIA, Velvia, ASTIA, B&W, SEPIA, PRO Neg Std, PRO Neg Hi. Byte-read patched arrays; on-body names/icons/selectability unproved until recorded. |
| V3 | Select PRO Neg Std, leave the menu, reopen it to verify the selected value, take one fixed-scene test shot and play it back. Repeat for PRO Neg Hi. Record selection screen, filenames, playback and exposure/WB/DR settings. | Both new entries selectable and usable for ordinary capture. Do not infer a required visual difference from any one scene. Hardware processing/output remains unproved until captured. |
| V4 | Return to the original stock film simulation and the B3 scene/settings; take/play back one stock-mode shot. Switch off normally, boot again and photograph the seven-entry M list again. | Stock mode still works; seven entries survive a normal power cycle. Save originals from all shots. |
| V5 | Record current values and any remaining differences before the post-install byte captures of section 8. | No unexplained regression; selected simulations and any temporary scene-setting changes are distinguished from update effects. |

**Static-inference:** the new availability cells copy the ASTIA/Velvia seven-column pattern, not every dial position. Use M, which is the ordinary comparison mode here, and ensure it offered selectable ASTIA and Velvia before installing. A special mode with greyed-out ASTIA/Velvia is not a valid seven-entry failure test. The patch's shared shooting/Fn/type-9 nodes do not establish seven entries on every Q, bracketing or RAW-conversion surface; this session does not change Fn assignment or use those as acceptance criteria. The [patch's surface coverage](filmsim-patch-01.md#1-surface-coverage-one-relocation-serves-all-three-nodes) owns the scope.

**Hardware-observed from the process recording (2026-10-09), the acts this table's outcomes rest on:** the first ordinary boot after the install shows no setup prompt. The LCD lights at 90.3 s and is a complete shooting screen by about 91.1 s with the frame cross, `M` and `100` (manual, 1/100 s), `F4.5` (the mounted lens) and `6400` in the ISO position. The shooting menu opened at 93.1 s reads ISO 6400, IMAGE SIZE `L 16:9`, IMAGE QUALITY `F+RAW`, DYNAMIC RANGE `DR100`, FILM SIMULATION `STD` and an empty FILM SIMULATION BKT row. From 96.3 s the film-simulation list scrolls as PROVIA/STANDARD, VELVIA/VIVID, ASTIA/SOFT, B&W, SEPIA, PRO Neg. Std and PRO Neg. Hi, with each selection's help text, so all seven entries are present, in the byte-read order, the two new ones included, the list wraps at the end, and no entry is greyed out. That is four names of the watched list (IMAGE SIZE/QUALITY, film simulation, dynamic range and ISO) plus the exposure mode and the lens reading, and it satisfies V0, the menu half of V2 and the ISO/size/quality part of B1-to-V1. White balance and its R/B shifts, colour, sharpness, highlight/shadow tone, noise reduction, long-exposure NR, focus mode, face detection, Fn assignment, date/time, language and the C-slot values are on no recorded screen and rest on the owner's report alone; the C-slot and Fn items stay owner-reported.

## 7. Missing entries versus a nonworking camera

If the body boots, menus respond and capture/playback work but V2 still shows five entries, do not reflash, raise version words or try D3. Photograph the list, mode and all greyed entries; retain the completion video and proceed only to the already scoped reviewed reads in section 8. **Static-inference:** zeroed/overwritten prefix arrays are the placement residual's predicted functional failure. A stock array/pointer readback, wrong shooting-mode gate or different loaded image can also explain missing entries; the photograph alone cannot select among them. Report a working-camera unlock failure, not successful installation of the unlock.

If the body does not boot or cannot reach normal menus after an explicit update completion and one first boot, stop camera operations. Do not attempt the maintenance reads, repeated power cycles, card-trigger experiments or another update. **Static-inference:** an invalid application or LZ-engine/load failure can stop before the updater or service command exists. **Unproved:** which failure happened on this body must remain unresolved without new evidence. The prefix-overwrite residual predicts missing entries, not this nonworking-camera outcome.

If settings/calibration differ unexpectedly, a setup prompt appears, capture/playback fails or readback mismatches outside the intended change set, stop normal use and report. **Static-inference:** interruption of S4 can leave firmware that boots with defaults or torn adjustment bytes; repeating an update restores from the then-current cache and cannot recreate this body's lost bytes. Do not treat boot success or 1.50 on screen as a calibration check.

## 8. Readback and evidence handoff

Run this section only with a normally functioning body, and only under the owner's scope for these reviewed operations. Remove the firmware card while power is off. Use the dummy-battery maintenance arrangement, with no image capture/live view/playback in flight. Do not arm AUTO_ACT or use POWER/OFF to leave maintenance.

| Act | Owner action | Expected observation and record |
| --- | --- | --- |
| R0 | Enter maintenance as at B4. | Blank screen, green LED, one supported `04cb:ff80` device. |
| R1 | Run `dump-ram id4-image-0` below once; record folder/result. | 1,048,576-byte segment starting `0x0061c000`, including the new arrays at `0x0061c340`/`0x0061c380`. Byte-read fixed range; candidate readback unproved. |
| R2 | After R1 succeeds, run `dump-ram id4-image-1` below once; record folder/result. | 1,048,576-byte segment starting `0x0071c000`, including all six node pointer words and fourteen availability cells. Byte-read fixed range; candidate readback unproved. |
| R3 | After R2 succeeds, repeat B5's full `read-config` command; record folder/result. | Same 1,309-window plan and oracle/signature. Supplies the before/after restore comparison and the update stamp. |
| R4 | After R3 succeeds, repeat B6's `read-media` command last; record folder/result. | Two area-2 banks bind the post-install ordinary settings to the stored ledger. No normal camera use until the following power cycle. |
| R5 | After commands end, cut the supply, then disconnect USB and set POWER OFF while unpowered. Boot normally once and check settings, shot/playback and the seven-entry M list again. | Settings-preserving maintenance ending; ordinary operation restored after `read-media` shut down its storage driver. Hardware-observed prior method; this outcome unproved. |
| R6 | Hand the session folders, note sheet, images and screen/video evidence to host analysis. | Results become recorded observations, not a chat-only report. No success claim until the checks below are complete. |

```sh
devenv shell -- bazel run //transport:camera -- dump-ram id4-image-0
devenv shell -- bazel run //transport:camera -- dump-ram id4-image-1
devenv shell -- bazel run //transport:camera -- read-config "$PWD/private-data/read-plans/adjustment-42000.json"
devenv shell -- bazel run //transport:camera -- read-media
```

Run one command at a time and stop on the first refusal, unexpected reply, disconnect or cleanup uncertainty. Use R5's supply-cut ending after a failed maintenance command; do not run the remaining commands or retry in that boot. These are the existing reviewed fixed segments/plan, not a new arbitrary-address reader.

**Byte-read:** host analysis checks every journaled request/status/hash against its actual file, complete segment/bank sizes, unique terminal success, outgoing bytes, no refusal/uncertainty, and the gate readbacks/restores. Check the 192-byte prefix against `private-data/analysis/x-m1-1.50/update/prefix/id4-patched.bin` bytes `[0,192)`. Its dump offset is `0x340` = 832 in `id4-image-0.bin`. This host-only comparison has no camera I/O:

```sh
dd if="<R1 folder>/id4-image-0.bin" bs=1 skip=832 count=192 status=none | cmp -n 192 - private-data/analysis/x-m1-1.50/update/prefix/id4-patched.bin
```

**Byte-read:** compare all 68 manifest words, not merely the prefix: six pointers and fourteen matrix cells live in R2's segment; address-to-file offset is address minus `0x0071c000`. The schema-3 `private-data/analysis/x-m1-1.50/filmsim/patch-01/patch-set.json` and accepted decoded image supply intended bytes. Preserve both gate readback files as well as the segment files. A prefix match in maintenance is evidence of survival to that maintenance boot, not proof that no writer exists in every ordinary-mode transition. Bind it to the ordinary-menu photographs before and after the read.

**Byte-read:** compare the pre/post adjustment windows with the intact auxiliary restore lists. The lists cover 1,487 bytes; the previous body/template census found 224 body-specific restore-covered differences and nine stamp bytes. Do not compare only the nine develop tables. Oracle and signature must hold; restore-covered body bytes must retain their before values, allowing only named boot-stamp writers where applicable. **Static-inference:** with 1.50 performing the update, `+0x1e8`/`+0x1e9` should move from the previously observed `01 54` to `01 78`, if B5 still read `01 54`. It is the executing-updater stamp, not proof that the new image landed. `+0x204`/`+0x205` is a boot-load stamp and cannot substitute for the prefix/word comparisons. Explain every other difference through the [adjustment census and update stamps](adjustment-and-service-interfaces.md#adjustment-read-coverage-and-update-stamps), promoted from private `private-data/analysis/x-m1-1.50/adj-read-01/findings.md`, and [update mapping](update-validation-and-boot.md#native-writer-boundary-and-preservation-limits), or stop.

**Static-inference:** the before/after area-2 banks carry the ordinary-settings commit history, with record selection and bank erasure accounted for by the [settings instrument](settings-log-map-session-script.md#1-capacity-read-this-first). They do not read firmware storage or prove prefix identity. The RAM word readback binds the running candidate; the card digest binds its input; configuration/read-media bind the observed body state. None alone proves all three.

The private handoff contains the named image/path and verified card digest/length; owner go and operation scope; before settings/menu/shot; B5/B6 and R1–R4 printed folders with their journals, outgoing records, files and hashes; the confirm/completion photographs and external video with elapsed times; first-boot and final-boot results; seven-entry photographs including PRO Neg selection marks; original Std/Hi/stock test images and filenames/settings; the readback/config/bank comparisons; and every error or deviation. The public report/evidence ledger records only scoped results and private artifact references, with **hardware-observed** for owner acts/results, **byte-read** for checked capture bytes, **static-inference** for interpretations and **unproved** for unresolved mechanisms. Do not call the unlock observed from the host harness or accept it from the displayed version alone.

## 9. State-changing operations in this session

| Operation | Effect | Class and evidence |
| --- | --- | --- |
| Ordinary controls, M comparison, Std/Hi/stock selection, test shots and ordinary shutdown | Shooting settings and their ordinary commit records may change; shots write the photography card. No C-slot save/reset is requested. | Owner-operated ordinary settings/capture; hardware-observed prior methods, this session's effects unproved until recorded. |
| Format dedicated install card | Erases its existing files and creates the ordinary card filesystem. | Owner-operated persistent card change; byte-read vendor procedure, outcome unproved. |
| Host copy/removal of unwanted files | Changes only the dedicated card's contents, not camera internal media. | Host/card preparation; byte-read required image and filename. |
| Maintenance entry and C0/C4 session framing | Session/request/reply RAM state. C4 does not exit maintenance. | H1 for read-config; byte-read framing, static-inference effect scope. |
| `read-config` with the existing full plan | Function 2 reads only; no configuration function-3 write or persistence command. | H1; byte-read transport contract, hardware-observed method. |
| `dump-ram id4-image-0` and `id4-image-1` | Reviewed `0xf7 = 1`, readback, fixed RAM reads, `0xf7 = 0`, readback; gate changes are RAM only. | H2; byte-read fixed bytes, hardware-observed method. |
| `read-media` | Reviewed RAM gate enable/restore, staging fields at `0x81`–`0x8a`, trigger `0x80 = 0x35`, disarm `0x80 = 0x33`, original-byte restores, bounded raw area-2 reads; shuts down storage driver. No internal-media program/erase request. | H2; byte-read closed operation, static-inference effect scope, hardware-observed bank-read method. |
| Maintenance supply cut | Clears volatile gates/arm state and restarts the body after read-media; no POWER/OFF defaults record is requested. | Physical ending; hardware-observed prior settings-preserving control, this ending unproved. Never applied to an in-progress update. |
| SD BODY firmware upgrade of the section-2 image | S1 trailer erase; S2 optional span erases; S3 program/verify/remap; S4 configuration refresh/restore/commit; possible S5 directory-journal commit. | H3, owner's go naming operation and image required; byte-read reviewed container/order, static-inference residues, hardware outcome unproved. |

No configuration/calibration restore, persistent gate enable, `0x80 = 0x22` whole-cache commit, End Setting, S-record writer, AUTO_ACT arm, D3 hook, raw SD-BOOT install or intentional interruption belongs to this session. A successful install is seven selectable entries surviving power cycles, working capture/playback and unchanged watched body state, bound to the reviewed image by the word readback and the retained before/after evidence. **Unproved:** a demonstrated recovery remains absent even after a successful installation.
