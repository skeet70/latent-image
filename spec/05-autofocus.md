# Autofocus research and implementation

## 11. Autofocus research and implementation work package

### 11.1 Establish the bottleneck before choosing an algorithm

E and B must produce a timeline for stock AF, with uncertainty where events are not directly observable:

1. AF request accepted and target area selected.
2. Measurement exposed/acquired and made available to AF code.
3. Search decision made.
4. Lens command sent, acknowledged, and movement completed where those signals exist.
5. New focus measurements become valid for the new lens state.
6. Focus confirmed, retried, or abandoned.

Measure how these activities overlap. Avoid assuming that total time equals a simple sum of CPU, motor, and sensor times. Quantify avoidable waits and unnecessary samples separately from physical lens travel and exposure time. The display or video frame rate is not a measurement of AF-statistics cadence.

If stock AF already performs very few iterations, reducing arithmetic may offer little benefit. If a high-quality measurement arrives late, an algorithm that needs fewer fresh observations may be valuable. If the lens dominates, preserve its verified control limits and look for a reduced travel path rather than commanding an unverified motor speed.

### 11.2 Candidate sequence

| Candidate | Purpose | Inputs and cost | Advance if |
| --- | --- | --- | --- |
| A0 — stock trace/controller reference | Make comparisons meaningful | Recorded stock states and measurements; selected native execution where mapped | Replay and observations agree within declared scope. |
| A1 — tuned coarse/fine search | Establish an inexpensive competing baseline | Existing contrast statistic, bounded lens commands | It reduces unnecessary positions or travel without quality loss. |
| A2 — adaptive steps and peak estimation | Reduce samples near the optimum | Recent measurements, position history, robust local fit | It behaves well on noise, shallow peaks, multiple depths, and direction reversals. |
| A3 — retained state and bounded search range | Avoid restarting from an implausible position | Recent successful position and explicit scene-change evidence | Benefits survive scene changes and cancellation; full-range fallback remains available. |
| A4 — calibrated two-frame prediction | Infer useful movement from change in blur/features | Correctly timed observations from known focus moves, lens-specific calibration | It generalizes to held-out scenes and remains cheaper than the search steps it removes. |
| A5 — small learned predictor | Explore larger improvements | Low-resolution crop/features, a calibrated training set, target inference implementation | Accuracy and measured target execution time justify complexity. |

A2's curve fit is local: a mixed-depth scene may have multiple peaks. A3 must not silently omit close subjects just to report faster focus. For A4, two scalar contrast values alone do not generally determine absolute focus distance or a globally correct move. Use an explicitly calibrated model or limited local search assumptions, with uncertainty and fallback. Learned blur prediction is image-based AF research; it does not add physical phase-detection pixels.

Treat the papers in [S11, S12] as methods to reproduce and adapt. Their headline gains are not an X-M1 estimate. The Google project offers ordinary-image experiments relevant to this sensor; use the RGB branch when assessing transfer. Code availability is per project and per component, not implied by a paper being public.

E's literature deliverable is a short reproducibility matrix, beginning with these concrete leads:

| Lead | Public starting material | Required reproduction and transfer check |
| --- | --- | --- |
| Wang et al., *Intelligent Autofocus* / *Deep Learning for Camera Autofocus* | Preprint; author repository with Keras estimator/discriminator, MATLAB calibration, and reinforcement-agent material. [S11](09-sources.md#s11) | Reproduce one documented configuration on the host; inventory data, weights, dependencies, and license. Separate sample-count reduction from measured end-to-end speed. Check calibration and image-domain assumptions against X-M1 data. |
| Herrmann et al., *Learning to Autofocus*, CVPR 2020 | Paper and focal-stack dataset; conventional-image and dual-pixel experiments. The project page does not provide a complete public training repository. [S12](09-sources.md#s12) | Reproduce an RGB-only baseline or the documented model with an independent implementation. Its reported focus-prediction error improvement is not a camera latency measurement. Inspect dataset size before downloading; start with the smallest useful subset. |
| Raspberry Pi `libcamera` AF controller | Public implementation with contrast-scanning logic and controller state. [S13](09-sources.md#s13) | Identify reusable search, settling, confidence, and retry ideas; document dependencies on frame metadata and lens units. Evaluate those ideas through the common interface, without importing unrelated hardware assumptions. |

Search forward from these papers for more recent primary work. Rank methods by available reproducible artifacts, ordinary-image inputs, number of new observations, lens calibration needs, target compute cost, and robustness. Conventional A1–A3 are engineering baselines, not assumed post-2013 inventions. Advance a learned method only after it beats those baselines on the project's measured conditions.

Reproduction may use the author's original Python/MATLAB tools in a pinned optional environment under [the upstream language exception](../docs/principles.md#implementation-and-build). Preserve data/weight/license provenance and hardware-input assumptions. No learned method is a prerequisite for a useful A1–A3 result.

`libcamera` [S13](09-sources.md#s13) is useful for reading a complete stateful controller, frame delays, ranges, and contrast scanning. Its phase-detection portions and hardware assumptions cannot be imported as X-M1 functionality. Build only the small independent pieces the project needs.

### 11.3 Lens-specific calibration and dataset

For every sequence record:

- Scene ID, target ROI, light level or controlled lighting setup, exposure/gain, aperture, focal length, lens model/firmware, camera firmware.
- Start position, commanded moves, observed positions/completion if available, movement direction, frame times, AF statistics, selected image crops, and independent final-sharpness measurements.
- Whether measurements came from raw/ISP statistics, live-view pixels, or saved photographs. These are different domains.
- Clock source, synchronization/uncertainty, sample age, command IDs, invalid samples, and dropped-frame/overflow events under the [trace contract](08-artifact-and-runtime-contracts.md#clocks-observations-and-controller-actions).
- A high-quality reference sweep obtained through ordinary bounded focusing. Manually acquired stacks can support static sharpness research but do not supply precise motor timing automatically.

Divide by scene and acquisition session, not neighboring frames from the same sweep. Keep entire scenes out of training/tuning. Initial learned models are scoped to one lens and the recorded aperture/processing conditions. Explain how other apertures, lens copies, exposure changes, and firmware versions affect calibration before broadening support.

### 11.4 Replay and simulation

Use two complementary tools:

1. **Recorded-path replay:** reproduce decisions with the same observed event stream. Useful for validating state interpretation, arithmetic, and instrumentation.
2. **Closed-loop lens/scene simulation:** answer candidate commands using measured focus sweeps plus a calibrated model of travel, settling, sensor delay, and measurement noise.

Recorded-path replay cannot answer every counterfactual command. Interpolation between measured positions is an assumption, especially around backlash, changing exposure, motion, and multiple depths. Tag simulated observations as such, preserve model uncertainty, and use hardware to validate winning candidates. Artificial Gaussian blur alone is insufficient evidence for real signed defocus behavior.

### 11.5 Target implementation

Use an explicit controller state and step function. Follow [the implementation policy](../docs/principles.md#implementation-and-build): Haskell owns host orchestration and analysis; new decision/execution code uses Haskell/Rust, with C limited to technically necessary engine or ABI boundaries. The existing C AF prototype requires migration or an explicit necessity argument. Host fixtures and target builds must exercise the same decision implementation; keep host tooling outside the camera-facing runtime.

- No hidden allocation in the real-time decision path unless the firmware's allocator contract is understood and justified.
- Explicit bounds on state, histories, frame crops, stack, and running time.
- A validated arithmetic strategy, with fixed-point if appropriate; do not silently assume an FPU, SIMD extension, cache size, or available CPU budget.
- Explicit signedness/overflow/rounding semantics when comparing the controller to native firmware. Compiler undefined behavior must not stand in for observed machine behavior.
- Stock lens limits and existing movement completion semantics remain the hardware contract.
- A stale/missing observation causes a controlled wait, validated fallback, or failure—not an unbounded extrapolation.
- The active firmware hook must be synchronized against in-flight callbacks/tasks. Account for instruction cache visibility when replacing code.
- Instrumentation can be disabled for final timing trials, with enough lightweight evidence retained to attribute the result.

Keep the first useful prototype scoped to the measured body/lens combination; record any requirement that would prevent extending it to another lens.
