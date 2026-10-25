# AF observation session kit

This kit implements the owner-facing part of the [sensor-backed AF process](../spec/05-autofocus.md#110-stock-first-sensor-backed-iteration). It is a design, not a claim that the snapshot bridge or batch runner exists. The [first-session procedure](af-first-session-script.md) precedes the [stock pilot](stock-pilot-protocol.md). No camera operation is authorized here.

## Owner interaction

| Point | Owner action | Agent responsibility |
| --- | --- | --- |
| Setup | Place the body safely, identify the intended subject, confirm ordinary operation and battery | Prepare exact scope, source identities, closed operations, review/rehearsal evidence and stop/exit procedure |
| Start | Confirm the named run or an applicable standing scope | Run the explicit camera command and journal outgoing requests |
| Scene change | Move one household object, switch a room lamp or choose another subject when requested | Keep scene revisions separate and acquire bounded automated records |
| Unexpected behavior | Report it; handle the camera under the stated incident procedure | Stop further commands, retain uncertainty and investigate offline |
| Exit | Perform the scoped stock-return check | Verify available original bytes/state and report the exact cleanup boundary |

No per-attempt slate, hand-written identifier, copied command, manual JSON record, target-distance measurement or repeated focus staging is required. Before the automatic adapter exists, use only a few ordinary attempts to answer feasibility questions. Do not replace missing automation with the superseded multi-hour manual procedure.

## Equipment and scenes

Use the existing X-M1, mounted lens, data cable and an adequately charged battery or already available supply. A table or stack of books can stabilize the body without supporting it on the lens or blocking ventilation. No specialized photography tool is required. The Viltrox AF 28mm F4.5 firmware 1.05 has a fixed aperture and no manual-focus ring.

| Scene | Household setup | Control |
| --- | --- | --- |
| Bright detail | Book cover or package text in ordinary bright light | Fixed intended region, avoid clipping |
| Indoor texture | Fabric, a book page or a textured object | Record room/lamp state and actual exposure |
| Dim texture | The same class of subject with a lamp off | Do not substitute exposure/noise changes for focus gains |
| Low contrast | Low-contrast fabric or packaging | Independent same-plane detail outside the AF challenge if needed; record the relationship |
| Competing depths | Foreground object against a detailed background | The intended plane is chosen before the attempt; sharper background is a failure |

A setup image or exported preview records geometry and the intended region. Keep the composition fixed within paired blocks. Described lighting plus exposure/gain is sufficient; no invented lux or lens-to-metre conversion. An already available screen may supply repeatable development stimuli, subject to the [screen-stimulus limitations](../spec/05-autofocus.md#116-high-leverage-hypotheses-and-optional-features); it is not required and does not replace real multi-depth scenes.

## Settings and references

The runner records body/firmware/lens identity, AF mode/region, exposure policy and actual values, processing, assist/pre-AF behavior, source configuration and controller identity. The owner resolves only controls that cannot yet be read safely. A changed setting or scene creates a new recorded condition.

Automated starts and reference sweeps require the validated stock lens-control contract. Select safe interior positions, approach from both directions and repeat the best region without forcing endpoints. Unknown position or timing remains unknown. A stock AF lock after moving a target is not an independently calibrated optimum.

Use independent intended-region sharpness and target-plane correctness. The existing local-image `measure` tool supplies one gradient/acutance measure, not a universally validated focus metric. Calibrate its sensitivity to real defocus, exposure, noise, saturation and processing before acceptance use. If a candidate uses the same feature as its decision score, establish an independent quality measure rather than having the controller grade itself.

## Bounded batches

The frozen private batch specifies modes, scene/start combinations, attempt and movement limits, deadline, approved policy/configuration envelope, snapshot bounds and stop/exit behavior. The agent automatically assigns identities, stages focus, randomizes paired order, captures observations and retains every attempt. Host or emulator stalls do not hold the camera's acquisition callback open.

A named standing confirmation may cover reviewed H2 operations within its conditions and lapse under [governance](../spec/07-governance-and-handoff.md#34-standing-confirmations). It is not permission for future unreviewed mechanism bytes. Load once per session where the reviewed interface permits idle policy selection. Physical hot-unload is not assumed.

Cardless preview/statistic batches precede full-resolution capture-to-RAM. The latter needs its own fresh-exposure, output-lifetime and cleanup evidence. Normal card saving/playback and the unavailable SD recovery route remain separate constraints. Do not insert or format a card in the damaged slot.

## Existing host-only analysis tools

These tools remain useful for optional external recordings and independently obtained local images; none implements the proposed camera bridge. Supply absolute input/output paths through Bazel. Supported commands are `//af-session:af-session -- frames`, `extract`, `annotate` and `measure`, plus `//af-session:check-annotation` and `//af-session:check-exif`.

- `frames` reads video presentation timestamps and reports spacing/duplicate/nonmonotonic observations; `extract` exports selected frames.
- `annotate` records visible-event brackets and uncertainty. Confirmation remains a presentation proxy; barrel motion is not optical settling. Preserve the source recording and annotations.
- `measure` decodes a local image, converts to grayscale, crops the requested ROI and reports mean central-difference gradient magnitude and saturation. `--sweep-max` normalizes within that scene/domain only.
- `check-exif` validates its existing narrow saved-JPEG pilot profile. It does not validate RAM snapshots, all camera settings or arbitrary new scene profiles; do not force new observations into its old profile.

For example, with caller-supplied image and output paths:

```sh
devenv shell -- bazel run //af-session:af-session -- measure --image /absolute/path/frame.png --roi 0:0:256:256 --out /absolute/path/quality.json
```

## Records and handoff

All camera bytes, images, recordings, resolved settings and journals stay under ignored `private-data/`. The runner binds scene/session/attempt/pair identity, clocks, source configuration, start evidence, reference data, quality domain, all failures and cleanup outcome. Use existing trial/settings/artifact contracts where they represent the observation; version any new snapshot format before its consumers ship.

Report owner-attention time, usable observations, data loss and recovery interruptions alongside experimental findings. A short session is a success when it answers its question, not when its small sample is presented as a reliability result. Record durable findings in the evidence ledger and update open issues; no per-run public narrative is needed.
