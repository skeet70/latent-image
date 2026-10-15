# AF observation session kit: E01 stock pilot (AF-OBSERVE-01)

Status: host-prepared session kit, `static-inference`. No observations and no camera authorization. This kit turns [the stock pilot protocol](stock-pilot-protocol.md), [its machine manifest](../manifests/benchmark-protocol.json) and [the observation plan](af-observation-plan.md) into one executable session for the owner's body. It accepts no gate and adds no research claim.

The session is ordinary photography plus external video: no USB, no jig mode, no service command, no settings, calibration or firmware write. It produces `hardware-observed` material for AF-OBSERVE-01: the repeatability and bracketability of the visible confirmation indication, focus quality at confirmation or deadline, physical cancellation and full-press behaviour, the physical-trigger-proxy-to-displayed-confirmation latency, and the settling-versus-confirmation correlation. Every quantity is a declared external proxy: the visible half-press onset is a proxy for the operator's action, not for AF acceptance inside the body, and the first visible confirmation indication is a proxy for the camera's report, not for the internal result. Nothing here dates an internal event; the internal bindings stay behind G4/G5.

The selected lens has no manual-focus ring and a fixed f/4.5 aperture, so no focus position can be commanded directly. Focus is set only by the camera's own AF at a staged plane and retained by holding S1 across the geometry change, as sections 3 and 5 specify.

Section links: [owner decisions](#0-owner-decisions), [equipment](#1-equipment), [recording rig](#2-recording-rig), [fixtures](#3-fixtures-and-measurable-placement), [settings freeze](#4-body-settings-freeze), [reference acquisition](#5-reference-acquisition-and-quality-metric), [attempt order](#6-attempt-order), [attempt steps](#7-per-attempt-operator-steps), [auxiliary blocks](#8-auxiliary-observation-blocks), [pilot subset](#9-pilot-subset), [storage](#10-file-naming-and-storage), [annotation workflow](#11-annotation-workflow), [handoff](#12-evidence-after-the-session).

## 0. Owner decisions

| # | Decision | Default if the owner says go |
| --- | --- | --- |
| D1 | Run this session now, later, or not at all | Nothing runs until the owner says go in chat; the kit is inert |
| D2 | Which lens the session reports | The mounted Viltrox AF 28mm F4.5 on firmware 1.05, the only lens with X-M1 evidence on this body (E-S02-IDENTITY-QUERY, E-JIG-OBSERVED). It has no manual-focus ring and a fixed f/4.5 aperture, so the AF-stepped procedures of sections 3 and 5 replace any manual sweep. If the owner prefers a conventional manual focus-position sweep, an independently focusable X-mount lens is a missing prerequisite and a different session identity |
| D3 | Which blocks run | Full: five reference acquisitions, 6-attempt pilot subset, 110 timed attempts, OBS-CANCEL-01 (5 procedures by 3 repeats), SETTLE-01 (Step 0 plus 10 attempts, or Step 0 alone when no feature tracks focus motion). Reduced: reference acquisitions, pilot subset and OBS-CANCEL-01 only. The pilot subset is the gate for the full set |
| D4 | One day or two | Two sessions if time is tight: P01-P03 then P04-P05, each with its own session ID. Fixture settings must not change between the parts |
| D5 | Effort accepted | Roughly 3-4 h for the full set including setup, reference acquisitions, staging, verification captures and the auxiliary blocks; the video and photo storage is 20-60 GB |
| D6 | Exposure values | The aperture is the lens's fixed f/4.5; the kit pins the shutter and ISO procedure per scene and the owner confirms the recorded values before attempt 1 |

The session changes no persistent state, so the camera-run procedure's H2/H3 review does not apply. The protocol's private snapshot still applies: identity, lens and camera firmware, focal length, aperture, exposure, AF mode and region, assist and pre-AF behavior, processing, lighting, staging distances, target plane and ROI, and the external video configuration all enter the private session file before the first timed attempt.

## 1. Equipment

- X-M1 body `XM1-body-01`, official 1.50, settings retained (E-STOCK-150-OWNER-REPORT), on the dummy battery supply (E-DUMMY-POWER-SUPPLY) or a fresh battery set, plus the SD card formatted empty.
- Viltrox AF 28mm F4.5, firmware 1.05 (E-S02-IDENTITY-QUERY, E-JIG-OBSERVED), mounted and confirmed on the day. The lens has no manual-focus ring and a fixed f/4.5 aperture, so it offers no direct focus control and no aperture choice; the reference acquisition of section 5 and the staging of section 3 are built on the camera's own AF. If the owner wants a manual focus-position sweep, a focusable X-mount lens is a missing prerequisite (decision D2).
- One rigid tripod for the body; the camera is not moved once a scene's geometry is set, except for the scene switch of section 3.
- One external video camera with a constant, user-selectable frame rate: 60 fps minimum, 120 fps preferred, and the highest available rate for the barrel recording of section 8. An action camera or a phone that exposes manual frame rate and manual exposure is suitable. The rate is chosen before the session and never changed inside a scene.
- A second video camera or a second tripod position if the primary rig cannot show the shutter button, the LCD and the lens barrel at once.
- Target boards: matte prints mounted flat on rigid board, one per print listed in section 3, plus a textured background board for P05.
- Target stand or cart that slides along a marked line toward and away from the camera, with a pointer at one defined board face.
- Tape measure (2 m) and floor/table tape for the marks of section 3; a plumb line or square to place marks.
- Plane marker: a rigid card or printed scale fixed at the intended plane and visible in at least the setup photograph of each scene.
- Fixed lamps on stands with marked positions, plus whatever realizes the P03 low light state as one repeatable step (a switched second lamp, a lamp moved to a marked second position, or a neutral-density sheet taped over the lamp). No scene may depend on unrecorded daylight.
- A light meter or a phone lux application, if available. If not, record the lamp model, positions and settings and the exposure; do not invent a lux value.
- Large-text attempt-ID cards and a marker pen: one card per attempt ID, or a whiteboard.
- A visible count for the deadline, section 7: a phone or timer placed in the video frame showing elapsed time, or the operator's silent ten-count with the actual time taken from the video later.
- Storage: SD card capacity for the 110 timed captures, the 110 staging verification captures, the reference acquisitions and the auxiliary blocks, and video card capacity for roughly 40 min per scene at the chosen rate.

## 2. Recording rig

One recording per scene, started before the first attempt of that scene and stopped after its last attempt. Never stop and restart the recording inside a scene: one continuous file per scene keeps every bracket in one clock, and the protocol forbids subtracting timestamps across unsynchronized recordings.

The primary camera framing must show, legibly and simultaneously:

1. the operator's finger and the shutter button, so the half-press onset is a visible frame event;
2. the rear LCD, so the AF box and its confirmation change are visible;
3. the lens barrel side, for the SETTLE-01 Step 0 feature check of section 8 and for any visible focusing motion.

Lock the video camera's exposure, white balance and focus manually, disable any in-camera stabilization, and use a fixed frame rate, not a power-saving or variable-rate mode. Verify all of this with a 10 s test clip before the session: read the AF box on the recorded frames, and confirm the frame rate from the frames listing.

Frame-rate choice bounds the annotation sampling only:

- With one frame bracketing the onset and one frame bracketing confirmation, the latency interval is about two frame periods wide: 33 ms at 60 fps, 17 ms at 120 fps, 8 ms at 240 fps.
- The comparison of interest is a 20% change in median latency. A stock median in the 300-800 ms band makes the smallest difference of interest 60-160 ms, so 60 fps brackets consume up to half of it and 120 fps a quarter.
- 60 fps is the minimum for the timed attempts. For the barrel recording, use the highest fixed rate the camera offers; at 60 fps the last-motion bracket is 17 ms wide.
- The frame rate bounds how finely the brackets can be read and nothing else. The measured interval also contains the unknown internal request delay, the AF decision and the display delay: the static map places at least two message hops and one UI-loop iteration between the AF result and the presentation, with no measured display-refresh or OSD-present bound ([static map](autofocus-static-map.md) §5). No frame-based correction exists for that, and the frame rate is not a measurement of AF-statistics cadence. A bracket that overlaps or runs backwards means only that the events are unresolved at the recording's frame rate.

Audio on the video camera is optional and secondary: the AF beep and the shutter sound can be bracketed in the audio stream, but they are neither synchronized nor a substitute for the visible brackets.

Attempt slating: hold the attempt-ID card in front of the video camera for about one second before every attempt, then remove it into a fixed out-of-frame place. The card is the only way to segment a continuous scene file later, so it is mandatory: an attempt without a preceding card is a recording deviation.

## 3. Fixtures and measurable placement

Five scene fixtures, each with a marked intended plane, a staging recipe and a fixed illumination state. The scene categories and starts come from [the protocol](stock-pilot-protocol.md); the numbers below are starting points for a 28 mm lens at about one metre, verified on the body and recorded, not facts about the X-M1.

| Scene | Intended target | Starts | Illumination | Camera-to-plane D | Staging offset |
| --- | --- | --- | --- | --- | --- |
| P01 | high-contrast print: large bold text plus strong edges | near, far, near-focused | bright: two fixed lamps, positions marked | about 1.0 m | about 0.15 m |
| P02 | fine-texture print: detailed small text or a textured page | near, far | indoor: the P01 lamps at their P02 marks, plus room light if it is stable | about 1.0 m | about 0.15 m |
| P03 | the P02 target class under fixed low illumination | near, far | same target as P02, one recorded low-light step (lamp switch, lamp position or ND sheet) | about 1.0 m | about 0.15 m |
| P04 | low-contrast print: mid-grey paper with a slightly darker print, measured contrast noted | near, far | as P02 | about 1.0 m | about 0.15 m |
| P05 | P02-class target on the intended plane with a textured background board about 1.0 m behind it | near, far | as P02 | about 1.0 m | about 0.15 m |

Placement procedure, per scene:

1. Mount the body on the tripod, aim at the target stand and set the working distance D from the lens front to the intended target plane with the tape; record the measured value and the two object positions.
2. Tape the intended mark D, the near mark D minus the offset, the far mark D plus the offset and the reference planes of section 5 on the table or floor, in one line along the optical axis. The stand carries a pointer; every placement is by pointer on mark.
3. Place the plane marker at the intended plane, at the edge of the frame so it does not occlude the target; it appears in the setup photograph and covers the geometry record.
4. Square the target board to the axis and record any tilt that remains.
5. Verify the staging offset with a staged test capture (the staging recipe below): with the lens focused at the staging plane and the board back at the intended mark, the intended ROI must be visibly soft at 100% review. If it is not, increase the offset, re-check and record the value used. The offset, not a lens coordinate, is the start-state evidence.
6. Define the ROI before the reference acquisition: a rectangle inside the intended target with strong edges and no saturation; tape it on the board or record its pixel coordinates from the first reference image. One ROI per scene, unchanged afterwards.
7. Set the illumination as the table states, mark the lamp positions, and record the lamp state and, if measured, the lux at the plane. No scene is lit only by a window.

### Staging recipe (AF-only lens)

The lens cannot be focused by hand, so every start state is produced by the camera's own AF at a staged plane and retained by holding S1 across the geometry change. One staging action, bounded:

1. Slide the board to the staging mark: the near mark for `near`, the far mark for `far`, the intended mark for `near-focused`.
2. Half-press and hold. Wait for the visible confirmation indication within a 10 s staging window. No confirmation, an ambiguous indication, or a search that keeps hunting past the window is a staging abort: release, log it, and do not trigger the attempt.
3. With S1 still held, slide the board back to the intended mark. The lens cannot refocus while S1 is held, so the staged focus is retained by construction.
4. Check the LCD before releasing: the AF box sits on the intended target, the frame is the intended composition, and for near or far the intended target is visibly out of focus. A failed check is a staging abort.
5. Still holding S1, full-press to take the staging verification capture, then release. This is the objective start-state evidence: for `near` and `far` the intended ROI must measure clearly below the scene's reference maximum, and for `near-focused` it must measure at the reference level.
6. Pause about two seconds. The timed attempt then begins with its own fresh half-press; nothing touches focus, mode or geometry in between. The recording covers the whole staging action, so any focus movement between the staging release and the timed half-press visible on the video invalidates the attempt: when Step 0 of section 8 shows tracking motion, motion in that interval is a required check, and when it does not, the staging verification capture is the start-state evidence instead.

A staging abort leaves the attempt uninitiated. It is logged as a schedule deviation with its reason, its own `E01-stage-abort-<scene>-<start>-<n>` ID and the video segment, and the attempt slot is rescheduled as a replacement with a new ID (section 6). At most two staging attempts precede a given timed attempt; after a second abort, the slot moves to the end of the scene's list and the abort is reported.

### Retention checks (ordered around the reference acquisition)

The staging recipe retains focus by holding S1, and that retention is asserted until it is checked. The checks run in two stages, because a quantitative reference level does not exist until the reference acquisition has been measured.

Before the reference acquisition, once per scene, three acquisitions, one per start, judged qualitatively from the saved images:

1. Far: run the staging recipe at the far mark to the verification capture. The intended target must be visibly defocused at 100% review.
2. Near: the same at the near mark, with the same qualitative defocus criterion.
3. Near-focused: the same at the intended mark. The intended target must look at least as sharp as during the live-view focus check on that plane.

An image that is sharp where defocus is expected, or soft where intended-plane quality is expected, means the hold or the full press re-ran focus: stop, revise or simplify the recipe, and repeat the preflight. A scene whose retention cannot be established is not run.

The reference captures themselves are exempt from the timed-start defocus check and from any reference-level check: they produce those levels, and they are expected to run from sharp to defocused as the planes move.

After the reference acquisition has been measured, evaluate the retained preflight captures and the staging verification captures of both passes against the reference maximum, and freeze the quantitative start criteria: the normalized band a near or far start must fall in, and the band a near-focused start must reach. A preflight capture outside its required band triggers a revision of the offset or the plane set and a repeat of the reference acquisition, and the timed attempts do not start before the criteria are frozen.

What the checks do not establish: the state at the moment of the timed half-press. The start evidence for a timed attempt is the verification capture taken while S1 was held, and the interval between that capture and the timed half-press is covered only by the rule that nothing touches focus, mode or the body. When SETTLE-01 Step 0 shows a feature that tracks focus motion, the recording's check that no motion occurs in that interval closes the gap; when no feature is observable, the interval stays unmeasured and the start state at the timed trigger is a proxy, not a verified state.

`near` stages the board at the near mark and focuses there. `far` stages at the far mark. `near-focused` stages at the intended mark itself, so the start state is the camera's own converged position at the intended plane, not an independently measured optimum; that distinction is recorded with the attempt.

Two execution modes for the fixture set, because the manifest order visits five scenes per repeat block:

- Co-installed stations: all five fixtures stand simultaneously and the tripod pans between them, with a marked head setting per scene. This follows the manifest order exactly and is the only mode that preserves the block randomization.
- Scene-batched: one fixture at a time, scenes visited in the order of their first appearance in the order file, and each scene's attempts taken in the file's within-scene order. This is a recorded order deviation; the analysis then treats execution order as recorded metadata, not as randomization.

The kit's default is scene-batched, recorded as a deviation, unless the room already holds five verified stations.

## 4. Body settings freeze

Every row names where the record shows the item. `1.50` means the owner's 1.50 record asserts it; `1.00` means the 1.00 record shows the item and 1.50 is unasserted, so it is verified on the body at preflight; static-map rows cite the bound 1.50 node.

| Setting | Session value | Record and status |
| --- | --- | --- |
| Body version | 1.50 | `BODY VERSION` in [the 1.50 record](../docs/camera-menus-human-recorded-1.50.txt), 1.50 |
| Image size | L 3:2 | confirmed 1.50 |
| Image quality | FINE | confirmed 1.50 |
| Dynamic range | 100% | confirmed 1.50 |
| Film simulation | PROVIA/STANDARD | confirmed 1.50 |
| White balance | one fixed option, shift 0/0, matched to the lamps; CUSTOM or FINE or INCANDESCENT rather than AUTO | confirmed 1.50, options `1.00` |
| Color, sharpness, highlight tone, shadow tone, noise reduction | the middle value: COLOR 0, SHARPNESS 0, HTONE 0, STONE 0, NR 0 | confirmed 1.50 |
| Long exposure NR | OFF | confirmed 1.50 |
| Self timer | OFF | `1.00`, SHOOTING MENU 2 |
| Face detection | OFF | `1.00`, SHOOTING MENU 4; 1.50 static property `0x1e8` (E-AF-MENU-MAP) |
| Focus mode | AREA: one selectable AF area, placed over the intended target and verified before every staging action and attempt | `1.00`, SHOOTING MENU 4; 1.50 node offers MANUAL/MULTI/AREA/CONTINUOUS/TRACKING (E-AF-MENU-MAP) |
| AF illuminator | OFF | `1.00`, SHOOTING MENU 4; 1.50 static property `0x1a` |
| MF assist | leave as found; unused, because the lens cannot be focused by hand | `1.00`, SHOOTING MENU 4 |
| AE/AF-LOCK button | leave untouched; the option list is not recorded, and no lock facility is assumed | `1.00` lists the item only |
| Exposure mode | manual exposure on the mode dial, with shutter and ISO set by the dials and fixed per scene | the mode dial is visible in the record; verify the M position on the body |
| ISO | one value from the recorded list, chosen so the JPEG target is mid-tone and unclipped | `1.00`, Q menu and SHOOTING MENU 1 list the values |
| Aperture | the lens's fixed f/4.5, confirmed from the first saved image | the lens has no aperture control and no ring |
| Silent mode | OFF, so the AF beep is audible as a secondary marker | `1.00`, SET-UP MENU 1 |
| Image display | 0.5 SEC | `1.00`, SET-UP MENU 1 |
| Monitor sunlight mode | OFF | `1.00`, SET-UP MENU 1 |
| LCD brightness | 0 | `1.00`, SET-UP MENU 1 slider |
| Auto power off | OFF | `1.00`, SET-UP MENU 2 |
| IS mode | record what it is; leave as found | `1.00`, SHOOTING MENU 3 and Q menu |
| Shoot without lens | OFF | `1.00`, SHOOTING MENU 3 |
| Date and time | set before the session; jig cycles reset it | E-JIG-OBSERVED |
| Release priority | no such item exists in either recorded menu; nothing is pinned | absence, both records |
| Pre-AF | no such item exists in either recorded menu; no hidden pre-focus is claimed, and the start state is the documented staging action | absence, both records |

Preflight steps:

1. Set every row, then photograph the FOCUS MODE, FACE DETECTION and AF ILLUMINATOR screens and the shooting-information screen with the phone as settings evidence.
2. Record whether the AF box can be moved over the target. If it cannot, move the target stand laterally so the fixed box sits on the intended target, record the lateral mark, and keep the camera untouched.
3. Confirm no lens firmware or body setting changes during the session; any change ends the scene and starts a new revision.

The private snapshot that resolves the manifest's nulls, written before the first timed attempt as `private-data/sessions/<session>/settings/session-snapshot.json`:

```json
{
  "schema_version": 1,
  "session_id": "<UTC>-af-pilot-E01",
  "protocol": {"path": "manifests/benchmark-protocol.json", "sha256": "<measured>"},
  "order": {"path": "manifests/E01-stock-pilot-v1-order.txt", "sha256": "05d509d2c1c8ff12e6b312b90e1895a6708aeb080f52fc1ac9c4af10e4470f9b"},
  "provenance": {
    "source_revision": "<git rev-parse HEAD>",
    "devenv_lock_sha256": "<sha256sum devenv.lock>",
    "module_bazel_lock_sha256": "<sha256sum MODULE.bazel.lock>",
    "protocol_sha256": "<sha256sum manifests/benchmark-protocol.json>",
    "order_sha256": "<sha256sum manifests/E01-stock-pilot-v1-order.txt>",
    "tool_build": "<bazel --version and the //af-session:af-session target revision>"
  },
  "seed": "E01-stock-pilot-v1",
  "body": "XM1-body-01",
  "camera_firmware": "1.50",
  "lens_model": "Viltrox AF 28mm F4.5",
  "lens_firmware": "1.05",
  "focal_length_mm": 28,
  "aperture": "f/4.5 (fixed by the lens, confirmed from the first saved image)",
  "exposure_mode": "manual",
  "shutter_time_s": "<recorded per scene>",
  "gain_iso": "<recorded per scene>",
  "af_mode": "AREA",
  "af_region": "<box position and target relationship>",
  "af_assist": "off",
  "pre_af": "not offered in the recorded menus",
  "processing": "FINE L3:2, DR100, PROVIA, color/sharpness/HTONE/STONE/NR 0, long-exposure NR OFF",
  "video": {"camera": "<model>", "frame_rate_hz": "<chosen>", "file": "<per scene>"},
  "scenes": {"P01": {"staging_distance_m": null, "lighting_setup": null}, "P02": {}, "P03": {}, "P04": {}, "P05": {}}
}
```

Host-only collection for the provenance block, run once before the first attempt:

```sh
git rev-parse HEAD
sha256sum devenv.lock MODULE.bazel.lock manifests/benchmark-protocol.json manifests/E01-stock-pilot-v1-order.txt
```

Each attempt also binds its own settings artifact, `trials/<attempt-id>.settings.json`, written by the operator from the template below. It is what the trial record's `settings` field points at, and it is the join key between a scheduled combination, its annotation and its photographs:

```json
{
  "schema_version": 1,
  "attempt_id": "E01-r01-P02-near",
  "pair_id": "E01:P02:near:01",
  "combination_id": "P02-near",
  "scene_id": "P02",
  "start": "near",
  "repeat": "01",
  "session_id": "<UTC>-af-pilot-E01",
  "protocol_sha256": "<session snapshot protocol hash>",
  "order_sha256": "05d509d2c1c8ff12e6b312b90e1895a6708aeb080f52fc1ac9c4af10e4470f9b",
  "snapshot_sha256": "<sha256sum settings/session-snapshot.json>",
  "stage_abort_ids": [],
  "replacement_of": null,
  "staging_verification_shot": "DSCF0000",
  "timed_capture_shot": "DSCF0000",
  "annotation": {"path": "annotations/<attempt-id>.json", "sha256": "<measured>"}
}
```

`replacement_of` names the original attempt ID when a staging abort or an invalid attempt is replaced, so replacements are findable from both ends. `stage_abort_ids` lists the abort records of section 3 that preceded this attempt.

The scene entries are filled with the measured distances, the offset, the illumination state, the reference plane set and the ROI before the scene's first timed attempt. Nulls that stay unknown stay null.

### Host-only EXIF comparison checker

`//af-session:check-exif` checks an existing JPEG against the fixed S01-S03 X-M1/Viltrox photographic comparison profile. This profile is **not the E01 settings freeze above**: it requires 4896 × 2760 (16:9), FINE+RAW, PROVIA/STANDARD, manual exposure and FocusPixel `3015 1060`. It must not be used to accept an E01 3:2 capture.

```sh
devenv shell -- bazel run //af-session:check-exif -- /absolute/path/reference.JPG /absolute/path/pilot-settings.json
devenv shell -- bazel test //af-session:check-exif-test
```

The caller-owned settings JSON contains positive numeric `gain_iso`, `aperture`, `shutter_time_s` and `focal_length_mm`, a nonempty `camera_firmware` string, and these exact profile fields:

```json
{"exposure_mode":"M","lens_model":"Viltrox AF 28mm F4.5","processing":"FINE+RAW, PROVIA/STANDARD, 16:9, rest default","af_region":"one up and one right of centre, 100% size (EXIF FocusPixel 3015 1060)"}
```

Without an explicit settings path, the checker uses `pilot-settings.json` in the parent of the frame's directory. ISO, aperture, focal length and camera firmware must match the supplied values. The shutter tolerance is strictly within ±2%; the fixed film mode, image size, focus pixel, focus warning, lens model and exposure mode must also match. Missing metadata, extraction errors and unsupported profiles fail with a nonzero exit.

Bazel supplies ExifTool and jq from the locked Nix packages. The checker reads local files, performs no camera I/O and writes no photographic artifacts. A passing result covers EXIF fields only: lens firmware, AF mode/size, AF assist, FINE+RAW and remaining defaults still require operator readback. The synthetic metadata test covers matching, mismatching, missing and malformed fields; it does not establish real-camera metadata correspondence. Photographs and caller settings remain private inputs.

## 5. Reference acquisition and quality metric

The lens has no focus ring, so the reference is acquired by stepping the *target distance*, not a focus position. It is an AF-produced, image-evaluated reference: the camera's own AF converges at each plane and every judgement is made on the saved images. It is not an independent lens-position optimum, and an independently focusable lens remains the prerequisite for one (decision D2).

Bounds, stated separately:

- Planes: at most 7 distinct marked planes, D plus k times the reference step for k from -3 to 3, the step being one third of the staging offset. A plane the stand cannot reach is dropped and recorded.
- Acquisitions: at most 15 across both passes, which maps the protocol's 15-position cap onto AF acquisitions; with seven planes each pass uses seven, so fourteen.
- Repeat captures: exactly three, at the selected best plane, outside the acquisition bound.

Procedure, per scene, before any timed attempt, with the scene's exposure frozen:

1. Mark the planes and record which are reachable.
2. Pass 1, ascending distance: at each plane run the staging recipe of section 3 with that plane as the staging plane and capture its verification image. The recipe is unchanged for planes near the intended mark: the return move there is short or zero, S1 is still held across it, and the verification capture is still taken at the intended mark, so every acquisition ends at the same geometry. These captures are not judged by the timed-start defocus criterion: they produce the levels that criterion is frozen from.
3. Pass 2, descending distance: the same planes in reverse order.
4. Acquisition history for this lens is the sign of the target-distance change that precedes an acquisition, ascending or descending, because with no manual focus the lens can only be positioned by an AF acquisition starting from wherever the previous acquisition left it. It conditions the acquisition, and it is neither an observed lens convergence direction nor a manual approach direction; the camera's internal convergence is not observable.
5. Best plane: the plane with the highest intended-ROI acutance across both passes.
6. Repeats: three independent acquisitions at the selected best plane, each returning the board to the intended mark and ending in a verification capture. They share one acquisition history, so their spread bounds the metric's noise and not a direction effect.
7. Direction agreement, evaluated after the repeats: compare the ascending and descending samples at the best plane against the repeat spread. If they differ by more than that spread, or the best plane sits at an edge of the plane set, record the reference as history-dependent or unstable and stop and revise the plane set or the scene. The repeats bound the metric's noise; they do not repair a history disagreement.
8. Set the acceptance threshold from the repeat spread and the visibly acceptable intended-plane level, before any timed outcome is inspected, and write the plane set, the acutance values, the repeat spread, the direction comparison and the threshold into the session snapshot.

What the reference establishes: the same-scene acutance maximum for the intended plane, a defocus curve over measured distances, and a repeatability bound for the metric. What it does not: a commanded or measured lens position, motor timing, or an optimum independent of the camera's own AF. The distances, not lens units, are the independent variable.

The frozen metric definition, implemented by `af-session measure`:

- Decode the saved JPEG unchanged, convert to grayscale, then crop the ROI, so the crop is pixel-exact; a nonzero `saturated_fraction` means the ROI is clipped and the value is not a valid quality measure for that frame.
- Value: the mean over ROI interior pixels of the central-difference gradient magnitude `(|I(x+1,y)-I(x-1,y)| + |I(x,y+1)-I(x,y-1)|) / 2`, reported in 8-bit units with six decimals, computed in one linear pass.
- Normalized value: the raw value divided by the same-scene reference maximum. The normalized value is the trial record's `quality_score`; the raw value and the reference maximum stay with the reference images.
- Plane correctness is checked separately by eye: a sharper background never rescues an unsharp intended subject, and in P05 the background board lies outside the ROI.

## 6. Attempt order

The checked-in order is [manifests/E01-stock-pilot-v1-order.txt](../manifests/E01-stock-pilot-v1-order.txt): 110 lines of `RR:combination_id`, block order 01 to 10, within a block sorted by the manifest's SHA-256 rule. Its SHA-256 over the exact file bytes is `05d509d2c1c8ff12e6b312b90e1895a6708aeb080f52fc1ac9c4af10e4470f9b`, which is the schedule hash the protocol already published.

Regenerate and verify it exactly as the manifest's `randomization` section states:

```sh
seed=E01-stock-pilot-v1
for rr in 01 02 03 04 05 06 07 08 09 10; do
  jq -r '.design.combinations[].id' manifests/benchmark-protocol.json | while read -r combo; do
    d=$(printf '%s|%s|%s' "$seed" "$rr" "$combo" | sha256sum | cut -d' ' -f1)
    printf '%s %s\n' "$d" "$combo"
  done | sort -k1,1 | awk -v rr="$rr" '{print rr":"$2}'
done > /tmp/order.txt
sha256sum /tmp/order.txt
```

The attempt ID is `E01-r<RR>-<combination_id>` and the pair ID is `E01:<scene_id>:<start>:<two-digit repeat>`. Each combination is visited once per repeat block, ten blocks in all: 110 timed attempts, P01 with its three starts by ten and every other scene with two starts by ten. Every initiated attempt, including retries after a failed or invalid one, gets its own record; replacements never overwrite originals, and cancellations and protocol errors stay in the all-attempt accounting the protocol defines.

Execution: follow the file, or the recorded scene-batched deviation of section 3. Deviations from the listed order, staging aborts and their replacements are noted in the session log and in the attempt's settings artifact as they happen, with the reason.

## 7. Per-attempt operator steps

Per attempt, in this order:

1. Check the scene state: the recording is running, the stand is on the intended mark, the lamps are at the marked positions, nothing occludes the box or the marker.
2. Show the attempt-ID card to the video camera for about one second; put it out of frame.
3. Run the staging recipe of section 3 for this attempt's start: board to the staging mark, half-press and hold, bounded wait for confirmation, board back to the intended mark with S1 still held, LCD check, full-press the staging verification capture, release. A staging abort ends the attempt before the trigger and is logged as a deviation.
4. Note the verification capture's shot number on the log line, and pause about two seconds without touching the camera or the stand.
5. Trigger: half-press and hold. This is the S1 onset the video brackets, and in the recording it is the second half-press of the attempt segment.
6. On the first visible confirmation indication (the focus box change on the LCD, with the beep as a secondary marker), full-press to capture at confirmation, then release. The capture must not re-run focus: if the lens visibly moves or the box re-enters a searching state after the full press, record the observation in the log and the attempt's capture as invalid.
7. Deadline: if no confirmation has appeared by about ten seconds of search, full-press to capture at the deadline. If the body does not fire within about two seconds, release and record a timeout with no final capture. Do not start a second AF cycle to force a capture.
8. Restage before the next attempt, including after a failure. Never carry a start state across an attempt.
9. Write one line in the session log: attempt ID, order position, start, the staging verification shot number, the timed capture shot number, whether the confirmation was seen, whether the capture fired, and any deviation.

The operator does not use a stopwatch as the time base. The video is the clock; the deadline is assessed from the annotated brackets afterwards. A silent count is enough to decide when to attempt the deadline capture, and its overshoot is then measured, not assumed.

Outcomes use the manifest's vocabulary: success, timeout, wrong-focus, false-confirmation, incomplete, cancelled, protocol-error. `success` requires an on-time completion and an independently acceptable intended-plane quality; the quality assessment comes after capture, from the saved photograph, not from the camera's own indication.

Capture-recipe validation, before the pilot subset: three attempts at P02 in which the pattern is half-press to confirmation, hold, full-press and capture. Confirm on the video that the full press does not restart focus (no visible focus change and, when Step 0 of section 8 succeeds, no lens motion after the capture trigger) and that the saved image is at the intended plane. If the recipe re-runs AF or loses the confirmation state, stop and revise the capture recipe, then repeat the validation. Nothing in the recorded menus provides a release-priority item, so this validation is the only evidence that the capture preserves the confirmed focus.

## 8. Auxiliary observation blocks

Both blocks sit outside the 110 timed attempts. Their attempts get their own IDs, their own trial records and their own reporting; they are not pooled into the timed statistics.

### OBS-CANCEL-01: cancellation and full-press behaviour

Runs once per session, at the P02 fixture after its timed attempts, at the same geometry and settings as P02, with the staging recipe of section 3 and the same slating and recordkeeping as a timed attempt. Five procedures, three repeats each, fifteen attempts, IDs `E01-obs-P02-<procedure>-<repeat>` with pair IDs `E01:P02:<procedure>:<repeat>`. The declared windows are 2.0 s, measured in the recording.

Protocol validity is recorded per attempt and kept separate from the behavioural outcome:

| Validity | Meaning |
| --- | --- |
| `complete` | every needed bracket and window is readable within one recording and no frozen rule was broken |
| `indeterminate` | a needed bracket, window or observation is unreadable, or the frame spacing cannot resolve the event |
| `protocol-error` | staging was not restaged, the recording was stopped or restarted inside the scene, the camera or stand was touched inside a declared window, or another frozen rule was broken |
| `cancelled` | operator interruption |

`protocol-error` and `cancelled` attempts are retained and never counted as success. Unreadable brackets are a validity statement only; they never produce a pass.

Behavioural classification, predeclared per procedure and judged only from `complete` attempts:

- `full-press-confirmed`, `full-press-held`, `full-press-late`: pass only when the tool's `capture_window` verdict is `within` (the widened full-press-to-capture interval inside the 2 s bound), the whole sequence stays inside the pilot's 10 s acquisition deadline measured from the first trigger, and the saved image is valid with no visible refocus after the full press. Fail on verdict `after`, no image, an invalid capture or visible refocus; verdict `unresolved` is indeterminate, not a pass.
- `cancel-search`, `cancel-confirmed`: no aggregate pass or fail, because the static map leaves the expected cancellation behaviour open and an aggregate would encode an assumption. Report three separate classifications: `continued_hunting` (visible focusing motion continues inside the 2.0 s release window), `stale_confirmation` (a confirmation indication appears inside the release window after the release), and `repress_outcome` = `confirmed` | `deadline` | `none`.

| Procedure | Steps | Declared window | Repeats |
| --- | --- | --- | --- |
| `cancel-search` | stage `far`, half-press and hold, release S1 during visible search before any confirmation | 2.0 s from the release, camera untouched; then re-press and hold to confirmation or the 10 s deadline | 3 |
| `cancel-confirmed` | stage `far`, half-press to visible confirmation, release S1 | 2.0 s from the release, camera untouched; then re-press and hold | 3 |
| `full-press-confirmed` | stage `far`, half-press to visible confirmation, full press immediately | the shutter must fire within 2.0 s of the full press, else timeout | 3 |
| `full-press-held` | stage `far`, half-press to confirmation, hold about 1 s, full press | as above | 3 |
| `full-press-late` | stage `far`, half-press to visible confirmation, hold 1.0 s, release S1 fully, wait 1.0 s, half-press again and hold to visible confirmation or the 10 s deadline, hold the confirmed state 1.0 s, full press | shutter within 2.0 s of the full press, else timeout | 3 |

Recorded per attempt: the `s1_release`, `s2_press` and `capture` brackets, whether visible focusing motion continues inside the window (`continued_hunting`), whether any confirmation indication appears inside the window after the release (`stale_confirmation`), whether the next half-press restarts or resumes the search (`focus_restart_observed`), the `repress_outcome`, whether the shutter fired on time, and the saved image. The `capture` bracket is the first visible external capture proxy after the full press, an LCD blackout or the visible shutter action; audio is secondary evidence only and needs a recorded audio-to-video synchronization contract with its uncertainty before it can carry the bracket. The tool emits the `s2_press` to `capture` interval as `capture_window`, widened by the declared uncertainty, with verdict `within`, `after` or `unresolved` against the 2 s bound, so a pass is the `within` verdict rather than an eyeballed stopwatch.

These are physical cancellation and full-press behaviours only: the static map reads `0x00df1d8c` and `0x00df2200` as request wrappers (opcodes `0x0f` and `0x0e`), and makes no invocation, binding or cleanup claim about them, so this block settles nothing about those sites.

Trial-record mapping for these fifteen attempts, applied in order: the first matching rule fixes the outcome, and no later quality reading can improve an outcome an earlier rule has already fixed.

1. Protocol validity first. Validity `indeterminate` gives `incomplete` with the reason in `exclusion_reason`; `protocol-error` or `cancelled` give the value of the same name. Nothing below overrides validity.
2. Deadline or window failure, whatever the image looks like afterwards. A `capture_window` verdict of `after`, or the sequence leaving the pilot's 10 s acquisition deadline measured from the first trigger, gives `timeout`. A saved late image is retained as an artifact and reported as a secondary observation; it never becomes an on-time success and never improves the classification.
3. Capture preservation. Visible refocus after the full press, or any evidence that the capture did not preserve the confirmed state, gives `incomplete` with the reason recorded and `focus_restart_observed` set: the relationship between the indication and the captured image cannot be established, so quality at confirmation is not assessed from that image. The image itself is retained as a secondary observation.
4. Independent quality and plane on the preserved image. Image acceptable and on time gives `success`; the indication accepted but the plane or quality failing gives `false-confirmation`; the intended plane not reached without a false indication gives `wrong-focus`; no image at all gives `incomplete`.

| Behavioural result | Independent image assessment | Trial outcome |
| --- | --- | --- |
| validity `indeterminate` | any | `incomplete` with `exclusion_reason` |
| validity `protocol-error` or `cancelled` | any | the same value |
| `capture_window` = `after`, or the 10 s deadline left | any, including an acceptable image | `timeout` |
| shutter fired, visible refocus after the press | image present and otherwise acceptable | `incomplete` with the refocus reason |
| shutter on time, capture preserved | image present, intended plane acceptable | `success` |
| shutter on time, capture preserved | indication accepted but plane or quality fails | `false-confirmation` |
| shutter on time, capture preserved | intended plane not reached without a false indication | `wrong-focus` |
| shutter on time, capture preserved, no image saved | no image | `incomplete` |
| cancel procedure, `repress_outcome` = `confirmed` | as the preceding four rows | as those rows |
| cancel procedure, `repress_outcome` = `deadline` | any | `timeout` |
| cancel procedure, `repress_outcome` = `none` | no image | `incomplete` |

The procedure classifications (`continued_hunting`, `stale_confirmation`, `repress_outcome`) are reported separately and never stand in for the trial outcome; `success` means only that the attempt was on time, the capture preserved the confirmed state and the saved image passed the independent check, not that any cancellation or full-press behaviour was correct.

### SETTLE-01: settling versus confirmation

Step 0, before any settling attempt, and mandatory: establish that a visible feature tracks the focusing motion of this lens. Record a short high-frame-rate pass over two reference planes, the near and far marks, using the staging recipe of section 3, and check whether the chosen feature (lens front edge, barrel seam, any visible scale) moves with the plane changes and is resolvable at that frame rate. This lens has no focus ring and an internally focusing barrel, so no feature may move at all. If no feature tracks, or the motion is not resolvable, record focusing motion as unobservable, report this correlation as unresolved and stop the block. Do not spend the settling attempts on an unframeable motion.

If Step 0 succeeds, runs once per session at P02 with the high-frame-rate camera framing the chosen feature and the LCD together:

- Five near-to-far acquisitions and five far-to-near acquisitions, staged and triggered exactly like the timed attempts, IDs `E01-obs-P02-settle-near-<repeat>` and `E01-obs-P02-settle-far-<repeat>`.
- Annotate each with the motion bracket (last frame with visible motion, first frame without it) and the confirmation bracket.
- Report the signed confirmation-minus-motion-end interval and the relation the tool derives from it: `trails`, `leads`, `overlaps` or `unresolved`, per direction, and list the trials where the two overlap.
- Overlap and unresolved are indeterminate order, never evidence that the lens was unsettled or that the internal result preceded the motion; a trailing indication is not evidence of settling. Never state a settling relation across a bracket too coarse for the interval of interest.
- The presentation limitation of section 2 applies: the confirmation indication can trail visible motion while the internal result was produced during it, so this block orders two visible things and cannot date `0x3f0d`, the internal result or optical settling. A continuous intended-ROI stability measure would be needed to make optical settling an objective; until one is validated, that part stays unmeasured and G4/G5-gated.

## 9. Pilot subset

Before the full set, run six timed attempts at P02: `01:P02-near`, `01:P02-far`, `02:P02-far`, `02:P02-near`, `03:P02-near`, `03:P02-far`, as the order file lists them for the three blocks. Run the P02 reference acquisition, the staging validation of the offset, and the capture-recipe validation first, because the threshold freeze needs them.

The pilot subset is a measurement check, not a result: it validates the recording rig framing, the slate segmentation, the staging recipe with its verification capture, the deadline behaviour, the extraction and annotation commands, and the acutance tool. Continue to the full set only when:

1. every one of the six attempts produces a valid annotation record, checked against the schema, with the staging verification capture present for each;
2. the onset and confirmation brackets are identifiable in at most three review passes per attempt, and the second (timed) half-press is unambiguous in the segment;
3. the normalized acutance separates the reference maximum from a staged (defocused) verification capture clearly enough for the frozen threshold to bind;
4. no attempt needs a capture recipe revision.

If any of the four fails, fix the rig or the fixture, and repeat the subset. The six subset attempts count as the P02 attempts of blocks 01-03; do not repeat them in the full set.

## 10. File naming and storage

All raw material goes under ignored `private-data/sessions/<UTC>-af-pilot-E01/`, following the existing session directory convention:

```text
private-data/sessions/<UTC>-af-pilot-E01/
  session-log.md                     paper-log transcription, deviations, order as executed
  settings/session-snapshot.json     the resolved private snapshot with the provenance block
  settings/menu-photos/              preflight settings evidence
  setup-photos/                      geometry, marks, plane marker, lamp positions
  video/<scene>-primary.mp4          one continuous file per scene
  video/<scene>-barrel.mp4           SETTLE-01 recordings
  frames/<recording>.frames.json     af-session frames output per recording
  frames/<recording>/review/*.png    af-session extract output for bracket review
  annotations/<attempt-id>.json      af-session annotate output, schema-checked
  trials/<attempt-id>.json           benchmark-trial v1 records
  trials/<attempt-id>.settings.json  per-attempt binding: combination, snapshot and annotation hashes, shot numbers
  deviations/<stage-abort-id>.json   staging-abort records with reason and video segment
  photos/<scene>/DSCF####.JPG        the saved captures, staging verification and timed, one pair per attempt
  reference/<scene>/plane-<k>.JPG    reference acquisition captures per marked plane
  reference/<scene>/measure/*.json   af-session measure output per reference capture
```

Attempt IDs are `E01-r<RR>-<combination_id>`; pair IDs are `E01:<scene>:<start>:<RR>`; staging aborts are `E01-stage-abort-<scene>-<start>-<n>`. Photo-to-attempt binding is by the shot numbers in the attempt's settings artifact and the session log, entered at capture time; every attempt has both a staging verification capture and a timed capture.

After the session: copy the raw video and photographs to a second durable location, record the SHA-256 of each raw file and verify both copies. The annotation and trial records are small JSON and stay with the session directory; they name raw files by relative path and hash. Public outputs use the alias `XM1-body-01`, the lens model and hashes only.

## 11. Annotation workflow

The host tool is `//af-session:af-session` (Haskell). It reads video and images only; it never opens a camera or USB device. `bazel run` does not inherit the shell's working directory, so every path in these commands must be absolute; set `S` once:

```sh
S="$PWD/private-data/sessions/<UTC>-af-pilot-E01"
mkdir -p "$S"/{settings/menu-photos,setup-photos,video,frames,annotations,trials,deviations,reference}
mkdir -p "$S"/photos/{P01,P02,P03,P04,P05}
devenv shell -- bazel run //af-session:af-session -- frames \
  --video "$S/video/P02-primary.mp4" \
  --out "$S/frames/P02-primary.frames.json"
```

The tool creates the parent directory of any `--out` it writes, so a nested annotation or measure path needs no extra setup.

`frames` reports the container's presentation timestamps, the actual frame spacing, duplicate and nonmonotonic timestamp counts, gaps above 1.5 times the median spacing, the video's SHA-256 and the stream parameters. It refuses a video it cannot decode and a recording with fewer than two frames. A nonzero duplicate or nonmonotonic count is a recording defect: treat the affected attempts as protocol errors and do not annotate them. A 40-minute 60 fps scene lists about 144,000 frames, a listing of roughly 20 MB; the tool caps the listing it will read at 64 MiB.

Review loop, per attempt:

```sh
devenv shell -- bazel run //af-session:af-session -- extract \
  --video "$S/video/P02-primary.mp4" \
  --frames 12340-12360 --out "$S/frames/P02-primary/review"
```

`extract` writes PNGs named by frame index, so a file name is the index the listing timestamps. Locate, for each event, the last frame without it and the first frame with it:

- start: the visible onset of the half-press on the shutter button, taking the second half-press of the segment (the timed one, after the staging verification capture);
- confirmation: the first visible change of the focus indication on the LCD;
- motion end: the last frame with visible motion of the SETTLE-01 feature and the first frame without it, only when Step 0 of section 8 established that a feature tracks focus motion;
- `s1_release`, `s2_press` and `capture` for OBS-CANCEL-01: the release, the full press and the first visible external capture proxy after it (LCD blackout or visible shutter action; audio needs a recorded synchronization contract and uncertainty). The tool emits the full-press-to-capture interval as `capture_window` with the `within`/`after`/`unresolved` verdict against the 2 s bound.

Then record one attempt:

```sh
devenv shell -- bazel run //af-session:af-session -- annotate \
  --video "$S/video/P02-primary.mp4" \
  --frames "$S/frames/P02-primary.frames.json" \
  --session-id <UTC>-af-pilot-E01 --trial-id E01-r01-P02-near --pair-id E01:P02:near:01 \
  --scene-id P02 --start near \
  --start-before 12345 --start-after 12346 \
  --confirmation-before 12393 --confirmation-after 12394 \
  --barrel-before 12390 --barrel-after 12391 \
  --sync-uncertainty-ms 20 --out "$S/annotations/E01-r01-P02-near.json"
```

Rules the tool enforces: indices must exist in the listed recording; the last-without frame must precede the first-with frame; the confirmation bracket must follow the start bracket; a capture bracket that precedes the full press is refused, and a capture whose first-with frame is not after the press's first-with frame gives the unresolved verdict rather than an exact interval; a repeated flag is refused instead of silently taking the first value; a frames listing that names a different video, carries no valid digest, disagrees with its own pts column, or has duplicate or nonmonotonic timestamps is refused; the recording is re-hashed and refused when its bytes no longer match the listing; an ambiguous event withholds timing instead of guessing.

The latency is the protocol's interval, computed from the brackets and widened outward by the declared uncertainty: lower bound `max(0, confirmation.before - start.after)`, upper bound `confirmation.after - start.before`, each then widened by `--sync-uncertainty-ms`, rounded outward to 1e-6 ms. It is the physical-trigger-proxy-to-displayed-confirmation latency and nothing else: it contains the unknown internal request delay, the AF decision and the display delay, and the frame rate bounds only how finely the brackets can be read. The declared uncertainty covers clock and visibility doubt: bracket ambiguity, motion blur on the event, the video camera's frame rate and any known synchronization limitation. When the operator cannot state a bound, the event is marked with `--ambiguous-<event>` and timing is withheld, which is the conservative outcome the protocol requires.

Barrel relation: with the motion bracket [m0,m1] and the confirmation bracket [c0,c1], the tool reports the signed confirmation-minus-motion-end interval `[c0-m1, c1-m0]` widened by the declared uncertainty, and derives the relation from its sign: `trails` when the widened interval is strictly positive, `leads` when strictly negative, `overlaps` when the brackets interleave, and `unresolved` when the bracketed order is trails or leads but uncertainty leaves the sign spanning zero. Never read a trailing indication as evidence of optical settling, and never read an overlap or unresolved result as evidence that the lens was still moving when the result was produced.

Secondary flags carry the observations the timing cannot: `--hunting`, `--focus-restart`, `--wrong-plane-suspected`, plus `--invalid-reason` for an attempt that must not count. Boolean flags are read wherever they appear in the argument list.

Trial v1 binding, so the per-attempt record and the annotation cannot drift apart:

| Trial field | Value |
| --- | --- |
| `session_id`, `trial_id`, `pair_id`, `scene_id` | as passed to `annotate` |
| `controller` | `stock`, `controller_build` null |
| `evidence_label` | `hardware-observed` |
| `settings` | the attempt's `trials/<attempt-id>.settings.json`, which carries the protocol, order and snapshot hashes, the combination and start, the replacement link and the annotation artifact |
| `outcome` | the operator's classification after quality review |
| `acquisition_latency_ms` | null always: no acceptable-focus onset is observed in this kit, and visible motion stopping is not optically acceptable focus |
| `confirmation_latency_ms` | the annotation's `latency_ms.upper_ms`, the conservative end of the physical-trigger-proxy-to-displayed-confirmation interval |
| `timing_uncertainty_ms` | `upper_ms - lower_ms` from the annotation |
| `quality_score` | the normalized acutance of the intended ROI |
| `exclusion_reason` | null for included attempts; a frozen reason otherwise |
| `raw_artifacts` | the video, the frames listing, the annotation JSON, the staging verification capture, the timed capture, the SETTLE-01 recording and the reference captures where relevant |

Validate every annotation before it leaves the session directory, using the pinned checker through the workspace (the devenv shell has no standalone check-jsonschema):

```sh
devenv shell -- bazel run //af-session:check-annotation -- "$S/annotations/E01-r01-P02-near.json"
```

The acutance tool:

```sh
devenv shell -- bazel run //af-session:af-session -- measure \
  --image "$S/reference/P02/plane-07.JPG" --roi 1830:1210:900:600 \
  --out "$S/reference/P02/measure/plane-07.json"
```

`measure` decodes the image through ffmpeg, converts to grayscale and then crops the ROI so odd origins and dimensions are pixel-exact, and reports the frozen metric and the saturated fraction in one linear pass. Add `--sweep-max <value>` to stamp the normalized value once the scene's reference maximum is known.

Test coverage for the tool and the annotation contract: `devenv shell -- bazel test //af-session:session-tests`. Everything is asserted through the command line against fixtures made by the pinned ffmpeg: a synthetic 25-frame recording for the frames listing and the video digest, extracted frames checked against ground-truth colours, annotation intervals including uncertainty widening, signed barrel relations, unresolved and refusal cases, boolean flags at the end of the argument list, replaced-video and tampered-listing refusals, a 900x600 ROI with an analytically known acutance under a runtime bound, and odd-origin, odd-size JPEG crops that distinguish the requested ROI from a shifted one. Annotation records are validated against `schemas/af-session-annotation.schema.json`.

## 12. Evidence after the session

- Each camera observation gets a `hardware-observed` ledger entry bound to the observable counterpart of the static claim it bears on: confirmation indication, quality at confirmation, physical cancellation and full-press behaviour, the request-proxy-to-visible-indication latency, and the settling correlation. Entries carry the body/lens/firmware, the brackets, the uncertainty and the artifacts' hashes; they do not generalize beyond this body and lens.
- Their contribution to the §11.1 timeline is limited to the external endpoints: the visible half-press onset proxy, the first visible confirmation indication, the capture, and the bracketed intervals between them. Nothing here dates an internal event. The map's `0x00defa30` chain is not timed and is not identified in the recording, and the presentation delay stays unmeasured.
- Record observations and their scope in the relevant report and evidence ledger; track remaining observation work in [AF-OBSERVE-01](https://github.com/skeet70/latent-image/issues/2). Internal bindings (S1 producer, statistic cadence, direct completion timing, physical lens units) stay open and G4/G5-gated.
- The analysis keeps every attempt, uses the scene as the correlation unit, reports intervals with uncertainty and treats the pilot subset as measurement calibration, not as a result.
- If any single observation contradicts a static claim, record the contradiction where the claim lives; the static map remains the reading of the firmware, and the observation bounds its behavioural meaning.
