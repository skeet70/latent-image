# Benchmark and acceptance protocol

## 12. Benchmark and acceptance protocol

### 12.1 Experimental design

**Pilot:** choose approximately 8–12 representative scene/start-condition combinations and about 10 repeated acquisitions per controller. This is enough to find measurement problems and large regressions, not to certify rare failure rates or precise tail latency. Use the pilot to estimate variance, refine fixtures, and set the confirmatory sample size.

**Confirmatory run:** freeze the candidate, scene list, settings, success definition, and analysis before collecting held-out results. Determine trial count from the observed variance and the desired confidence in speed and focus-success differences. A narrow failure-rate margin can require hundreds or thousands of trials; if that cannot be collected, report the interval and a narrower claim rather than treating an inconclusive test as a pass.

Collect development data in short automated household-scene batches under the [sensor-backed process](05-autofocus.md#110-stock-first-sensor-backed-iteration). A small feasibility block checks the instruments before an expanded pilot; neither is a rare-failure or p95 demonstration. Minimize owner actions through automated starts, references and records, not by weakening confidence requirements. No specialized photography equipment is required.

Pair stock and candidate by target, start state, lens, lighting, and aperture. Randomize order. Include trials starting substantially out of focus and near the correct focus. Keep pre-AF behavior, AF-assist illumination, frame rate, and exposure policy matched or report their changes as part of the feature's cost. Do not hide focus work performed before the timing start.

Store the randomization seed, attempt/pair IDs, scene/session grouping, and settings/build hashes with every trial. Reserve scenes and sessions for confirmation before tuning. Account for correlations within a scene/session when estimating uncertainty; repeated acquisitions from the same scene are not automatically independent evidence of generalization.

Predeclare a finite acquisition deadline and the treatment of failed/cancelled/invalid trials. Keep every attempt in the dataset. Report latency among successes alongside an all-attempt metric that includes a declared deadline penalty or censoring method; a lower median obtained by dropping slower failures does not count as improvement. Include fallback time in end-to-end latency and report fallback frequency separately. Log any exclusions and apply the same frozen rules to stock and candidate.

### 12.2 Required measurements

| Measure | Definition |
| --- | --- |
| Acquisition latency | Time from a reproducible AF request to independently acceptable focus; separately report camera confirmation latency. |
| Median and upper-tail latency | Report per scenario and pooled with prespecified weights; include uncertainty. |
| Focus quality | An independent image-sharpness/target measure and reference sweep, not solely the controller's own contrast score. |
| Failure rate | Timeout, wrong-plane focus, false confirmation, uncompleted acquisition, and fallback rate. Keep failures in the report. |
| Search cost | Measurement count, commanded movement, direction reversals, and total lens travel where measurable. |
| Runtime overhead | Target decision time, instrumentation overhead, peak stack/state memory, and interference with UI/capture. |
| Secondary effects | Visible hunting, noise from movement, energy/temperature observations if behavior changes, and capture reliability. |

Final-sharpness evaluation must use a defined ROI and target plane. A background that is sharper than the intended subject is a failure even if it produces a higher global contrast score. Large ROIs and small ROIs can behave differently; record the AF-region size.

A saved photograph can establish final quality but cannot by itself timestamp the first acceptable focus. If continuous quality observations are unavailable, report confirmation latency plus independently verified final quality, or a bounded timing interval. Carry clock/video-frame uncertainty through the estimate rather than labeling a confirmation event as exact acquisition latency.

Preview, raw sensor, developed capture and saved-image quality are distinct domains. Validate the chosen metric against deliberately defocused and reference observations, exposure/noise variation and target-plane correctness before using it as independent quality. A coherent full-resolution RAM capture can support cardless quality evidence once its provenance and processing are established. Preview alone does not establish saved-photo sharpness, and neither RAM capture nor a cardless exit check establishes normal save/playback compatibility.

### 12.3 Provisional performance target

Use the following as a starting product target, fixed or revised in a recorded decision after stock characterization and before confirmatory tests:

- At least **20% lower median acquisition latency** across the declared primary scenario mix.
- No more than **10% worse p95 acquisition latency** in adequately sampled required scenarios.
- No material final-sharpness loss. Define the noninferiority threshold using repeatability of the independent sharpness measurement.
- Focus-success rate no more than **2 percentage points below stock**, with confidence intervals reported. Insufficient data for that margin leaves the reliability claim unproved.
- No repeated camera hangs, unexplained persistent changes, uncontrolled lens motion, or failure to restore stock operation during accepted tests.

The default product goal includes an accuracy improvement, not only speed with noninferior accuracy. After stock characterization, predeclare difficult-scene false-confirmation, wrong-plane or unacceptable-focus rate as an accuracy endpoint and freeze its minimum meaningful improvement and confidence rule. A faster/noninferior controller is an intermediate result; claim both faster and more accurate only when both endpoints pass. Keep the provisional success-loss guardrail above separate from this positive improvement requirement.

Host-in-the-loop latency includes transport and host execution and is not target AF latency. Validate finalists with local on-camera control and bulk extraction outside timed acquisitions. Record stock versus instrumented overhead separately. Research crashes trigger investigation; repeated battery-removal recovery is not acceptable operation for an accepted feature.

These are project acceptance choices, not a forecast. A smaller but robust gain in one well-defined scenario may still be worth retaining as an experimental mode; label it accordingly. A speedup limited to bright static targets cannot be presented as an overall AF improvement.

Define the estimator, scenario weights, confidence level, latency/quality bounds, and noninferiority comparison in the frozen protocol. Gate decisions are pass, fail, or inconclusive; an under-sampled p95 or success-rate margin remains inconclusive. G6 cannot pass merely because a point estimate meets the target. See [the trial schema](../schemas/benchmark-trial.schema.json) for the initial per-attempt record.

### 12.4 Minimum regression coverage

The final scoped feature must cover ordinary near/far starts, already-near-focus starts, low contrast, dim light, repeated half-press, half-press cancellation, invalid/stale measurements, unsupported mode selection, and graceful removal/reload. Test unsupported modes using unchanged stock behavior and verified hand-back. Continuous/video AF performance is not claimed until separately benchmarked.

Mock faults belong in the host harness first. Do not simulate a low battery by cutting power during a persistent write, or test a lens disconnect by creating an electrically hazardous condition. Hardware tests should address remaining real uncertainties after host tests have passed.
