# Stock autofocus pilot protocol

Status: offline design, no camera authorization and no new performance observation. The [AF process](../spec/05-autofocus.md#110-stock-first-sensor-backed-iteration), [benchmark requirements](../spec/06-benchmarks.md) and [runtime contracts](../spec/08-artifact-and-runtime-contracts.md) are authoritative. The [machine manifest](../manifests/benchmark-protocol.json) is a draft expanded-pilot example, not a frozen run or a prerequisite for the first useful observation.

## Scope and readiness

Use one observed X-M1 body/installed-firmware/lens combination, initially static AF-S and a single intended region. Confirm the actual baseline rather than assuming pristine official bytes. The current lens is Viltrox AF 28mm F4.5 firmware 1.05; it has no manual-focus ring and a fixed aperture.

Separate three collection stages:

1. A short question-driven feasibility block under the [first-session procedure](af-first-session-script.md): stock AF with ordinary USB, same-boot trace, and fresh coherent observations. It is not a quality or speed benchmark.
2. A development pilot after observation/start/reference validation: short automated household-scene batches that diagnose measurement failures and estimate variation. The checked-in example uses two repeats of eleven scene/start combinations, 22 attempts across separate short sessions if needed.
3. A frozen confirmatory comparison whose independent scenes, sessions and attempt count come from pilot variation and the desired confidence. The pilot's count is not a statistical acceptance target.

Before collection, freeze a private snapshot of the actual operations, source/build identities, scene/region, settings, lighting description, start recipes, data domain, clocks/uncertainty, limits and exit procedure. Unknowns relevant to the observation remain blockers, not invented defaults. Camera access follows the [camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs); a reviewed bounded batch can use a suitably scoped standing confirmation.

## Fixtures and starts

| Scene | Household subject | Starts in the expanded pilot |
| --- | --- | --- |
| P01 | Bright book/package detail | near, far, near-focused |
| P02 | Indoor textured object | near, far |
| P03 | Dim textured object under a recorded lamp state | near, far |
| P04 | Low-contrast fabric or packaging | near, far |
| P05 | Intended foreground object against a detailed background | near, far |

No tripod, printed target, light meter, measured distance or high-speed recording is required. Keep composition stable on an ordinary support, identify the target plane/ROI once and record exposure/gain. Hunting is an observed outcome, not a property assumed for P05.

After bounded stock focus control is validated, select repeatable safe interior starting positions that visibly defocus the intended region, plus a near-optimum start independent of the timed result. Record raw units, direction/history and available position evidence; do not force endpoints or infer metres. Before that control exists, limit collection to feasibility rather than demanding manual target staging for every attempt.

Keep pre-AF work, AF assist, region, processing and exposure policy matched. Do not hide focusing before the stopwatch. If automatic exposure is used, record its actual values and exclude or model only through frozen rules. A changed scene/settings condition receives a new revision.

## Timing and quality domains

Prefer synchronized source-bound camera events and coherent image observations. Large USB extraction occurs after timed acquisition. Host-controlled experiments report their transport/host timing separately and cannot establish final on-camera speed.

Acquisition latency starts at the declared reproducible request and ends at independently acceptable focus only when continuous quality observations establish that event. Otherwise report confirmation latency plus final quality, or a bounded interval. Message sends, predicted position, actual lens completion, optical settling and UI presentation are distinct events.

Optional external video uses presentation-timestamp event brackets, not nominal frame count. Preserve clock/visibility uncertainty and do not subtract unsynchronized clocks. It is a corroborating or fallback instrument, not a mandatory rig.

Use an unoverlaid intended-region image and an independently validated quality measure. Calibrate against real staged defocus, clipping, noise, exposure and processing changes. AF statistics cannot grade themselves. A metric identical to the candidate's own decision feature needs independent validation or replacement. Wrong-plane focus fails even when the background is sharper.

Preview quality, full-resolution raw/developed RAM capture and saved-photo quality are different claims. Validate format, acquisition provenance and processing before scoring any exported frame. Cardless full-resolution export can support optical quality if established; it does not establish normal saving/playback. A later capture cannot establish quality at confirmation if another focus cycle or lens movement intervened.

## Independent reference acquisition

Use a finite sweep through validated stock lens operations at fixed geometry/settings. Start with a bounded coarse coverage, refine the best region and capture it repeatedly from both approach directions. Freeze position/movement/time bounds before the batch. An edge maximum, direction disagreement beyond repeatability or unstable quality prevents accepting the reference; revise offline rather than extending indefinitely.

Normalize quality only within the same scene, region and processing domain. Freeze acceptable-plane/quality thresholds from the independent reference and repeated captures before inspecting timed outcomes. Keep reference images and their uncertainty. A stable scalar maximum does not alone establish correct target selection.

## Attempts, deadlines, failures and fallback

Each batch has finite acquisition, observation, movement and session bounds. The draft example retains a 10-second administrative acquisition deadline and 15-second attempt window; these are collection limits, not predicted AF performance. Staging and reference work have separate recorded bounds. No late result becomes an on-time success.

Retain every initiated attempt using the existing trial outcomes: success, timeout, wrong-focus, false-confirmation, incomplete, cancelled and protocol-error. Success needs on-time completion and independently acceptable intended-plane quality. Preserve secondary outcomes, missingness and invalidity; a replacement gets a new ID linked to the original.

Fallback remains on the original stopwatch and deadline. Report its reason/frequency separately. Use the frozen all-attempt penalty/censoring policy alongside latency among successes; dropping failures cannot create a speedup. If focus onset is unobserved, keep acquisition latency unavailable and label the confirmation-proxy result.

## Order, pairing and storage

The expanded draft uses the manifest's seed and SHA-256 ordering, with one complete combination block per repeat. [The generated order](../manifests/stock-pilot-order.txt) is an offline example. It is not permission to execute all scenes in one session. A frozen actual batch records its chosen combinations, grouping, seed and order; scene batching must preserve randomized paired controller order and matched conditions.

For comparisons use contemporaneous stock/candidate pairs with matched scene/start/settings/history, restaging both. A historical stock-only pilot is not the stock control for a later candidate. Keep all scene/session groups intact in uncertainty estimation; repeated frames are not independent scenes.

Store the resolved protocol/settings, source domains and configuration, attempt/pair IDs, start evidence, acquisition/delivery clocks, raw observations, reference data, quality/plane results, errors and cleanup outcome under ignored `private-data/`. Existing trial v1 can bind extra grouping/interval/provenance through hashed settings and raw artifacts; the future snapshot producer needs its own versioned contract. No hardware origin is assigned to a synthetic fixture.

## Splits, analysis and decisions

Pilot scenes/sessions are development data. Reserve wholly new scenes and sessions before tuning; never split adjacent frames from one sweep between training and confirmation. Adaptive experiment selection may choose development captures where candidates disagree, but it must not inspect the confirmatory holdout. If holdout results influence a change, retire that holdout.

Report per-scenario and prespecified weighted results, failure causes, missingness, fallback, quality, median and upper tail with uncertainty. Two repeats per combination establish neither p95 nor a narrow failure margin. Freeze a paired scene/session-aware confidence method, timing-interval sensitivity, sample size and stopping rule before confirmation. Insufficient independent clusters or power means inconclusive.

Retain the specification's provisional speed/noninferiority guardrails. Also predeclare a positive difficult-scene accuracy endpoint, its minimum meaningful effect and confidence rule after stock characterization. Faster with noninferior quality is an intermediate result, not proof of both faster and more accurate AF. Neither host feedback nor cardless preview accepts untethered operation and ordinary capture/save/playback.

## Offline validation and handoff

`//tests:research-contracts` validates the public draft schema, cross-field consistency and evidence records. The schema accepts bounded pilot sizes rather than pinning the old 110-attempt plan. The [session kit](af-session-kit.md) names existing optional local-image/video tools and separates them from the proposed automatic camera adapter. Host checks establish no camera performance or permission.
