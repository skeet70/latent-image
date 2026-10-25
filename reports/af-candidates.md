# Host AF candidate pipeline

This report supports [#109](https://github.com/skeet70/latent-image/issues/109), [the candidate specification](../spec/05-autofocus.md#112-candidate-sequence) and the measured-data dependency [#108](https://github.com/skeet70/latent-image/issues/108). Source inspection is distinct from reproducing a trained model. The supplied comparison is simulated, uses original synthetic scalar focus curves and has no camera-time or X-M1 performance interpretation.

## Reproducibility matrix

Primary pages and repository files were fetched on 2026-10-25. Availability below means availability in the inspected artifacts, not proof that no other implementation exists. No upstream code, weights, dataset or firmware is incorporated into the candidate library.

| Lead and exact source | Inputs, artifacts and redistribution | Calibration and target cost | Bounded reproducible baseline | Hypothesis and rejection condition |
| --- | --- | --- | --- | --- |
| S11: Wang et al., [*Intelligent Autofocus*, arXiv:2002.12389v1](https://arxiv.org/abs/2002.12389v1), and *Deep Learning for Camera Autofocus*, IEEE TCI 7 (2021), DOI `10.1109/TCI.2021.3059497`; [author repository revision `fbbf0c58b25bf0503e317cfad3fc466768d143b4`](https://github.com/ChengyuWang1007/Deep-Learning-for-Camera-Autofocus/tree/fbbf0c58b25bf0503e317cfad3fc466768d143b4). | Complete recursive repository tree and relevant source fetched. Keras estimator/discriminator definitions, MATLAB calibration and PyTorch RL material are present. Tree lists `step_1_model.hdf5` (8,023,064 bytes), `rl-agent/autofocus.pth` (2,657,748 bytes) and two calibration JPEGs (1,277,624 and 1,224,355 bytes). These are availability/file-size claims, not a successful load or validated weight format. No licence file is present; public access does not authorize redistribution of code, weights or images. The RL branch also needs DAVIS frames and external depth data; their size/rights are not established here. | Estimator input is 512x512 grayscale, with camera-specific blur and magnification calibration. MATLAB uses gamma 2.4, disk radius 26 through 28 and scale 1.017 through 1.019 on the bundled focus-1400/defocus-1700 pair. A float32 input alone occupies 1,048,576 bytes by multiplication. Weight-file sizes are not peak RAM. Published workstation inference is not an X-M1 budget; target operations, resident weights, peak scratch, stack and execution time are unmeasured. | Smallest upstream reproduction is the nine-cell MATLAB calibration on the two bundled images, in a separately pinned optional upstream environment with MATLAB/Image Processing Toolbox rights. Compare its resynthesized crop to the observed defocused crop. Next probe the supplied estimator on a small rights-cleared calibrated ordinary-image stack, against A1/A2 and a blur-radius prior. These are proposed experiments, not runs claimed by this report. | Calibrated image features may save fresh observations. Reject transfer if held-out lens/domain error or failure rate exceeds A1/A2 at matched final quality, or preprocessing/inference costs exceed the saved observation cost. Reject a calibration configuration that cannot discriminate a unique grid optimum or whose held-out reconstruction error exceeds the predeclared tolerance. |
| S12: Herrmann et al., [*Learning to Autofocus*, CVPR 2020, pp. 2227-2236](https://openaccess.thecvf.com/content_CVPR_2020/html/Herrmann_Learning_to_Autofocus_CVPR_2020_paper.html), [arXiv:2004.12260v3](https://arxiv.org/abs/2004.12260v3), [project page](https://learntoautofocus-google.github.io/) and [dataset README](https://storage.googleapis.com/cvpr2020-af-data/LearnAF%20Dataset%20Readme.pdf). | Actual project page and dataset README fetched. Page provides dataset links and code contact, not a training/inference repository or pretrained weights. Both conventional-image and dual-pixel formulations are described. Page labels test 89 GB and seven training archives totalling 681 GB; the test object's actual HTTP headers report 108,233,864,056 bytes, generation `1635796159070210`, ETag `ca7583a53c27e5a7381ded1bcb6ead1c`. No corpus downloaded. No dataset redistribution licence is stated in the inspected page/README; obtain rights before including samples. | Five-camera Pixel 3 rig, 49 physical focus settings, depth-derived labels, focus breathing and processing differ from X-M1. README warns reconstructed depth scale is not guaranteed metric-correct. Model is a modified MobileNetV2 with ordinal regression; training/inference closure and X-M1 compute/memory remain unmeasured. Even a float32 128x128 single-channel patch is 65,536 bytes before network state; full-stack/channel-position encodings and width multiplier increase this. Do not use generic MobileNetV2 memory as this model's budget. | A1/A2 on the published synthetic curves are the current executable comparator. For a paper-domain reproduction, obtain one rights-cleared ordinary-image 49-slice stack and another held-out scene as a bounded subset, implement the paper's conventional-input contrast/full-stack metric baseline, then the ordinal model independently if code remains unavailable. Do not label scalar synthetic replay or dual-pixel accuracy as the RGB model reproduction. Full-training reproduction is not available from the page alone. | Ordinary-image prediction might reduce observations at matched accuracy. Reject if held-out final error/failure does not beat A1/A2, or measured target preprocessing/inference plus extra observations costs more than search. The dual-pixel result and lower focal-index prediction error alone do not accept X-M1 latency or intended-plane quality. |
| S13: Raspberry Pi `libcamera`, revision `957f0f4bd06a163466ff1f3adda66f64fb66cd64`, [`af.cpp`](https://github.com/raspberrypi/libcamera/blob/957f0f4bd06a163466ff1f3adda66f64fb66cd64/src/ipa/rpi/controller/rpi/af.cpp) and [`af.h`](https://github.com/raspberrypi/libcamera/blob/957f0f4bd06a163466ff1f3adda66f64fb66cd64/src/ipa/rpi/controller/rpi/af.h). | Actual C++ controller and state definitions fetched. Contrast branch consumes ISP focus statistics; hybrid controller also consumes phase/confidence and AWB statistics. No learned weights. Neither fetched file supplies a replay dataset. Both declare BSD-2-Clause and Raspberry Pi copyright; preserve applicable notice and licence if copying. The complete libcamera dependency closure has additional licences. | Defaults explicitly assume IMX708 and the standard Raspberry Pi V3 lens. Dioptres map through a calibrated piecewise-linear hardware mapping. Frame skips depend on sensor/ISP cadence, with updates capped at 30 Hz. Source reserves 32 scan records, each four doubles, plus dynamically sized region weights and other state. This is not a hard allocation bound. X-M1 execution time, compiled size, stack and memory fit are unmeasured; no 30 Hz to X-M1 timing conversion is valid. | Rebuild the pinned upstream controller in its supported C++ environment for a literal reproduction; isolate the contrast-only scan, disable all phase-assisted termination and provide declared statistics/tuning. The implemented independent A1/A2 replay below is the immediately runnable engineering comparator, not a bit-exact libcamera reproduction. | Coarse/fine scan, local interpolation and fresh final verification can reduce redundant observations while preserving the chosen contrast maximum. Reject transfer if it needs phase inputs, imported dioptres/timing, or loses intended-plane quality at the matched failure rate. Compare sample count and travel separately. |
| S33: Zhu et al., *Stabilizing and Accelerating Autofocus with Expert Trajectory Regularized Deep Reinforcement Learning*, CVPR 2025, pp. 26440-26450, [paper and artifact page](https://openaccess.thecvf.com/content/CVPR2025/html/Zhu_Stabilizing_and_Accelerating_Autofocus_with_Expert_Trajectory_Regularized_Deep_Reinforcement_CVPR_2025_paper.html). | Paper, supplementary PDF and demonstration media fetched. Supplementary ZIP contains one PDF, three MP4s and 24 GIFs, not code, weights or expert trajectories. Paper uses two-channel dual-pixel patches plus RoI/lens positional encodings, yielding five input channels. Uses the Herrmann/Pixel 3 dataset. No implementation/weight reuse grant is established; CVF states copyright remains with authors/rightsholders. Dataset rights are separate. | Paper uses 49 focal indices and 128x128 patches; relative actions are clipped to bounds. Tables report a 2.35M-parameter relative-label MobileNetV2, about 9.4 MB for float32 weights alone by multiplication, excluding activations/runtime. A float32 five-channel input is 327,680 bytes by multiplication. Actor and critic are trained; the practical stop model is another classifier. Exact inference architecture closure, peak memory, operations and X-M1 runtime are unmeasured. Paper specifies PyTorch/TorchRL, 10k supervised iterations, PPO, four-step episodes and 6M expert trajectories, not a target deployment budget. | First reproduce contrast/ordinal-label bookkeeping on one licensed stack and a scene-held-out stack, then a separately identified ordinary-image ablation with A1/A2 as search baselines. The published dual-pixel network is not an ordinary-image reproduction. Full learned reproduction requires implementation, environment, weights/training and label-policy reconstruction; none is claimed here. | Relative-move labels and reversal penalties may improve sample/travel efficiency. Reject the published input path for X-M1 without independent dual-pixel evidence. Reject an ordinary-image adaptation if held-out final quality/failure worsens or target inference cost outweighs saved observations. Fewer reversals alone is insufficient. |
| S34: Fujifilm X-T3 Ver. 4.0, [official announcement dated 2020-10-15](https://www.fujifilm-x.com/en-us/news/fujifilm-releases-free-firmware-update-upgrading-the-af-performance-of-the-fujifilm-x-t3/), comparison footnote names Ver. 3.30. | Announcement fetched. It describes movement prediction, face/eye tracking, focus limiter and video AF-area controls. It supplies no algorithm source, weights, calibration or test dataset and no redistribution grant for a firmware implementation. No donor binary was fetched; an exact binary revision/digest is therefore not established. | Vendor claims 0.06 to 0.02 s fastest AF and improved tracking. Low-light -7 EV is explicitly phase-detection AF with XF50mmF1.0 R WR. Processor budget, memory, algorithm cost and dependence on contrast versus phase paths are undisclosed. No X-M1 addresses, timing, lens units or calibration follow from these claims. | Reproducible baseline here is A1 full-range search on the same synthetic curves. A genuine vendor reproduction needs exact rights-reviewed 3.30/4.0 inputs and a source-proven contrast path; the announcement alone cannot supply it. A bounded range-policy experiment can use the shared interface only after retained-state/scene-change inputs and a broader-search fallback exist. | Investigate explicit range limits and stopping/verification policy before transplanting a replacement controller. Reject any transfer needing another body's phase path, addresses or lens units, and any shortened search that silently drops valid close subjects or loses matched final quality. Tracking claims do not accept static AF-S improvements. |

### Source inspection details and limits

S11's [`estimator_discriminator.py`](https://github.com/ChengyuWang1007/Deep-Learning-for-Camera-Autofocus/blob/fbbf0c58b25bf0503e317cfad3fc466768d143b4/estimator_discriminator.py) references an undefined `image_size` in its discriminator definition, so it is not a turnkey script as shipped. The repository's [`requirements.txt`](https://github.com/ChengyuWang1007/Deep-Learning-for-Camera-Autofocus/blob/fbbf0c58b25bf0503e317cfad3fc466768d143b4/rl-agent/requirements.txt) describes Python 3.6.9, TensorFlow 1.14, Keras 2.2.4 and PyTorch 1.3.1-era dependencies. A future reproduction must pin compatible tools in an optional environment and distinguish an upstream compatibility repair from the published method. No project-authored Python or upstream runtime is added here.

S12's project advertises 510 stacks, while the dataset README describes 351 train and 47 test sweeps. The reason for that discrepancy is unresolved; do not assume an archive reproduces the paper split. The README's date is printed `11/02/2020`. The test object returned HTTP 200 for the size check, but its size differs from the page label; neither the labelled archive total nor individual training archives were downloaded or independently size-audited. Start with a small author-provided or selectively extracted licensed subset under a named experiment, not the complete dataset.

S13's `findPeak` fits three contrast samples; `doScan` records position, contrast, phase and confidence, changes coarse/fine states and can reverse direction. The contrast path also contains phase-assisted early termination, so selecting the contrast-related source lines is not sufficient to remove all phase dependence. `stepFrames`, `skipFrames`, `contrastRatio` and the final settling test are useful policy references. Their defaults are not X-M1 calibration. This report makes no estimate of milliseconds per libcamera decision.

S33's [supplement](https://openaccess.thecvf.com/content/CVPR2025/supplemental/Zhu_Stabilizing_and_Accelerating_CVPR_2025_supplemental.zip) was size-checked first: HTTP Content-Length 39,943,139 bytes. It was fetched to inspect whether reproduction assets existed, not to collect training data; SHA-256 `f912b577990d4e6c63b9c35e553753c18424ce34ef5e8c7a6b5ebae87df48f6b`. Supplement §6 labels focus using a weighted mix of three contrast and three dual-pixel metrics, not Herrmann's depth-map labels. An RGB adaptation must name its changed label policy and cannot claim to reproduce the reported table by dropping dual-pixel channels. Supplement §§7-8 specify ordinal pretraining and three expert-trajectory generators. The inspected CVF page links only the paper and this supplement. No claim of a comprehensive search for unpublished author assets is made.

Prioritize the source-available contrast-controller ideas and the independent A1/A2 baselines. Learned ordinary-image methods require licensed reproduction inputs and lens/domain calibration; S33 additionally requires an explicitly changed input/label experiment. Vendor change histories remain policy hypotheses until a target-bound contrast path is established. None of these leads establishes an X-M1 improvement.

## Shared decision implementation

[`af/candidates/controller.rs`](../af/candidates/controller.rs) is an original AGPL-3.0-only `no_std` Rust library. `Controller::new` accepts a validated `Config` and a closed `Candidate` enum. `Controller::step(Event)` returns `Observe { request, position }`, `Wait`, `Finish(position)` or a typed failure. State owns the phase, outstanding request, accepted sample history and finite budgets. The host adapter invokes this library, rather than maintaining another search implementation. A target adapter can link the same decision source; this change does not supply a firmware hook or target ABI.

The request means move to the bounded position and obtain a new, settled measurement. A caller must bind the request ID, actual observation position and verified freshness/completion at acquisition before submitting `Sample`. `fresh` and `settled` are explicit adapter obligations, not values inferred from host elapsed time. Stale, missing and unsettled observations produce `Wait`, consume the step budget and cannot finish focus. A wrong matching position/request fails. Cancellation terminates the controller. A terminal controller returns the same terminal action without issuing another observation.

- A0-structural instantiates the direction-search, peak-scan and three-position refinement structure decoded from X-M1 1.50. It uses hardware-observed Run-3 parameter-bank values for the mounted Viltrox lens; its unresolved lens increments and scalar statistic mapping remain synthetic. It does not reproduce stock convergence and is not usable as a controller reference until the increments and statistic units are bound.
- A1 scans the full configured range at coarse increments, including the upper endpoint, then measures unvisited positions at fine increments in the clamped one-coarse-step bracket around the strongest coarse sample. It revisits the best observed position for final verification. This is stock-like coarse/fine search, not a reconstruction of X-M1 stock states or tables.
- A2 uses the same full coarse scan. It fits an unequal-spacing three-point parabola around the strongest observation when curvature is negative, clamps the estimated peak to its sampled neighbours and requests that position if unvisited. Otherwise it halves the larger neighbouring interval. It stops when both gaps are within the fine step or no new integer position is available, then requests final verification. It never treats a local quadratic as a global scene model.
- All three reject insufficient contrast span and a final verification score differing by more than the configured span threshold. These thresholds are artificial score units, not calibrated probabilities. A1/A2 can miss multi-peak structure narrower than their coarse spacing. A flat-curve refusal does not establish a general low-light confidence policy.

### Arithmetic and resource contract

Positions use a `u16` newtype; scores are validated into 0 through 1,000,000. Configuration requires ordered limits, `1 <= fine <= coarse <= range`, a positive contrast span and a sample budget between 3 and 64. There are 64 fixed history slots and at most 256 nonterminal step calls. Accepted samples, including final verification, consume the sample budget; waits consume the step budget. No decision-path allocation, I/O, image crops, recursion, floating point or mutable globals are used.

Position additions/subtractions saturate before clamping. A2 converts differences to signed `i64`; its products remain below 2^54. A0-structural's absolute-position polynomial and tenths rounding remain below 2^58 under the shared score/position bounds. Its original integer logarithm uses Q31 `u64` squaring below 2^64 and 24 fractional iterations per transformed score. Its source-bound decimal rounding differs from A2's truncation. Fitted positions are clamped before conversion to `u16`. A1 searches at most 65 grid positions in a refinement call, checking at most 64 history slots per membership test. A2 and A0-structural scan bounded history. Exact target ABI, text size, stack high-water, division lowering and execution time remain unmeasured. No FPU, SIMD, cache size or available camera CPU budget is assumed.

The host-only stdio executable uses range 0 through the supplied curve maximum, 64 samples and contrast span 100. A1/A2 use coarse step 125 and fine step 10; A0-structural uses the mixed observed/synthetic parameter profile below. It parses `CANDIDATE2 A0-structural|A1|A2 maximum initial_position`, emits `OBSERVE request position` and consumes one bounded score per request. `FINISH position samples` and `FAIL reason samples` terminate. All candidates receive the fixture's initial position; A1/A2 deliberately begin their full-range scan at the low bound. The adapter supplies fresh, settled synthetic observations, not a camera movement/completion contract.

### A0-structural: decode-bound policy and parameter gap

The structural model is [original Rust decision code](../af/candidates/structural.rs), not copied firmware. Its anchors are [the static AF map](autofocus-static-map.md#3-search-decisions-and-constants), [the observation plan](af-observation-plan.md), and the private per-function decodes under `private-data/analysis/x-m1-1.50/af/functions/`. The decoded id5 image is SHA-256 `3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a`. Source addresses identify X-M1 1.50 code, not target hooks installed by this change.

The explicit phases are direction search, peak scan, fitted-target observation and final verification. Direction search probes from the initial position, reverses on a decline, counts rising evidence and forces inward motion at a limit. This scalar reduction uses `0x00f41c48`, the rise counters in `0x00f3c6c0`, threshold/direction decisions in `0x00f3c9f4` and the bound rule in `0x00f3d554`. It does not reproduce all the firmware's context, weak-signal, counter and failure branches. Peak scan follows one direction until consecutive declines or a relative-plus-absolute decline confirms a peak, using the scalar history predicate `0x00fa936c` called through `0x00f3de28` by `0x00f42abc`. Reaching a limit also ends this model's scan. It does not search for a globally stronger second depth.

The three-position record corresponds to preceding, maximum and following samples in `0x00f3e468`, `0x00f3e4bc` and `0x00f3e63c`; the latter duplicates its endpoint records before calling `0x00f8e9e4`. Ordered positions and a center maximum permit refinement; incomplete or unordered triples retain the measured center in this model. Hardware-observed key `0x927a = 1` selects `0x00f8e0c0`'s mode-3 logarithmic parabola. `0x00ee97f0` quantizes the score mantissa to integer hundredths, then uses a rounded base-2 logarithm. The original Q31 implementation was compared against all 100 reachable entries of the private firmware lookup table with zero mismatches; no firmware table is copied into the project. `0x00f8a390` normalizes gaps above 500 with scale `50000 / max_gap`, confirmed at `0x00f8a45c..0x00f8a46c`. A degenerate transformed parabola falls back to the untransformed fit, gated by hardware-observed `0x927b = 10`, then to `0x00f8dd3c`'s linear-side fit. Both fit routes round tenths remainders greater than 4 away from zero. The mode-4 five-position fit is not modeled.

| Model input or fixed arithmetic | Current profile | Provenance and boundary |
| --- | --- | --- |
| Resolved direction step before scaling | 50 artificial position units, synthetic | `0x00f41c48 -> 0x00faab3c`; the captured active step is zero, not a usable positive increment. |
| Resolved peak step | 50 artificial position units, synthetic | `0x00f3ec50 -> 0x00faa990`; the captured movement plan is zero. |
| Direction evidence threshold | 3, hardware-observed | Key `0x8811`, physical `0x01ad6011`; `0x00f3c9f4`. Real directional counters remain reduced to one rising-evidence count. |
| Consecutive decline threshold | 8, hardware-observed | Key `0x8831`, physical `0x01ad6031`; `0x00fa936c`, `param_5 != 1` branch. Observed alternate key `0x8833` is also 8. |
| Relative decline | 20 percent, hardware-observed | Key `0x8826`, physical `0x01ad6026`; fixed branch of `0x00f3de28`. Statistic-dependent blending remains outside the scalar reduction. |
| Absolute decline | Observed key 5, multiplied by decoded literal 10 | Key `0x8823`, physical `0x01ad6023`; `0x00fa936c`. The resulting model threshold is 50 synthetic score units. |
| Initial direction | Decreasing, from hardware-observed key 0 | Key `0x880e`, physical `0x01ad600e`; bit 1 in the selected `0x00f3c8d8` direction path. |
| Direction-band policy | Enabled; thresholds 15/80/1000/10000 and scales 2/6/30/40/50 divided by 10 | Observed `0x8778 = 1`, `0x877f..0x8782 = 15/8/10/10` with decoded multipliers 1/10/100/1000, and `0x8750..0x8754`; `0x00f41c48`. M/S scale-group selection remains a context assumption. Both observed scale groups have top scale 50, the band used by these five fixtures. |
| Direction range cap | Range divided by 30, hardware-observed | Key `0x8776`, physical `0x01ad5f76`; `0x00f41c48`. It caps the synthetic 50-unit base step after band scaling to 33 units on these 1000-unit curves. |
| Direction compensation | Disabled, hardware-observed | Key `0x878c = 0`, physical `0x01ad5f8c`; `0x00f41c48`. |
| Fit route / fallback curvature limit | Logarithmic parabola / 10, hardware-observed | Keys `0x927a = 1` and `0x927b = 10`, physical `0x01ad6a7a` and `0x01ad6a7b`; `0x00f8e9e4`, `0x00f8e0c0`, `0x00f8deb0`. |
| Fit arithmetic | Three positions; factors 2 and 10; rounding cutoff 4 | Fixed decoded arithmetic, not empirical tuning. |

The model still assumes stable context, aliases the firmware's separate focus statistics to one synthetic score, seeds peak history with the last two direction samples and omits frame/episode warm-up. The first-episode scale `0x8783` is now observed as 4, but its transient/context path is not implemented. Bound termination, fit clamping, flat-curve refusal, final verification and the 64-sample/256-call limits are host safeguards, not recovered firmware termination semantics. With the observed direction-evidence threshold, the reduced controller exhausts its budget near the single/noisy peak; the threshold is not retuned to hide that limitation.

#### Bounded convergence diagnosis

The bounded check revisited the direction and peak decodes and stopped a host-adapter diagnostic after its first 12 observation requests on the synthetic single-peak fixture. No threshold, increment or host budget was changed. This was a simulated diagnostic, not a stock AF trace.

| Apparent stock limit | Decoded behavior | What it establishes |
| --- | --- | --- |
| Direction-stall counter `0x016c11ae`, key `0x87d3` | `0x00f41c48` saturates the counter at `0xff`; the below-threshold branch clears the step and returns direction-search state `0x0d`. | A conditional stall-path count, not a whole-attempt sample or time limit. |
| Direction-search counter `0x016c11ab`, key `0x8816` | `0x00f41c48` saturates the counter at `0xfe`; one at-threshold branch clears the step and returns `0x0d`, while other branches can enter peak search. | A state-local branch bound, not a globally terminating budget. |
| Peak metadata guard `0x016c121c > 0xf7` | `0x00f42abc` calls `0x00f3e1cc` and returns peak-search state `0x0e`. | An early branch whose producer and units remain unbound, not a proven frame deadline. |
| Peak history index `0x016c1e38` | `0x00f42abc` clamps it to `0xff` before writing the three history arrays and incrementing it. | A storage-index bound, not termination of the attempt. |

No whole-attempt sample limit or elapsed-time limit is bound by this inspected routine pair. This agrees with the [static map's termination boundary](autofocus-static-map.md#3-search-decisions-and-constants); it does not establish that stock has no such limit elsewhere. None of these constants can replace the host's 64-sample cap as a decoded stock limit.

The threshold holding this host search open is **direction evidence `0x8811 = 3`**, applied by the reduced model to consecutive rising observations. Every score in the diagnostic prefix selects the highest observed direction band. The synthetic base step 50 becomes `50 * 50 / 10 = 250`, then observed key `0x8776 = 30` caps it to `1000 / 30 = 33` synthetic position units. The public adapter requested this prefix; the evidence column is derived from `structural_direction`'s rise/reset predicate:

| Request | Synthetic position | Synthetic score | Derived host rising evidence |
| ---: | ---: | ---: | ---: |
| 1 | 500 | 998631 | 0 |
| 2 | 467 | 995079 | 0 |
| 3 | 500 | 998631 | 1 |
| 4 | 533 | 999963 | 2 |
| 5 | 566 | 999135 | 0 |
| 6 | 533 | 999963 | 1 |
| 7 | 500 | 998631 | 0 |
| 8 | 533 | 999963 | 1 |
| 9 | 566 | 999135 | 0 |
| 10 | 533 | 999963 | 1 |
| 11 | 500 | 998631 | 0 |
| 12 | 533 | 999963 | 1 |

After the transient, the reduced transition rule repeats `533 -> 566 -> 533 -> 500 -> 533`: each decline reverses direction and clears the evidence. Evidence reaches only 2 initially and 1 in the repeating cycle, never 3. The peak phase is never entered, so neither its eight-decline threshold nor its 20-percent-plus-50-score-unit drop threshold keeps this case open. [INFERENCE] Raising the host sample budget alone would extend the cycle rather than produce convergence.

The native `0x00f3c9f4` compares key `0x8811` against two directional counters, `0x016c11a7` and `0x016c11a6`, and has context-dependent alternate exits; this model's single consecutive-rise count is not an exact reconstruction of those branches. The bounded diagnostic distinguishes a nonconvergent reduced policy from an insufficient host cap. It does not isolate synthetic increments, statistic mapping and omitted stock transitions as independent causes. **A0-structural with observed thresholds and synthetic increments does not reproduce stock convergence. It is not usable as a reference until the increments and statistic units are bound and the reduced transitions are validated.** The observed thresholds remain unchanged.

#### Bounded Run-3 parameter lead

The archived capture `private-data/sessions/20261025T050355Z-read-bulk/ram-00000000-05aa0000.bin` was inspected without camera or USB access. It is 95,027,200 bytes, address range `[0, 0x05aa0000)`, SHA-256 `fa933fcf723ff46d435161cfecf48edc4a29cc4dc4c0ff5232fd0c70e7a6fee2`. Values below are little-endian, hardware-observed in this archived capture for the mounted Viltrox lens. They do not establish an active AF episode or reusable step calibration.

| Captured address | Hardware-observed value for this lens | Interpretation boundary |
| --- | --- | --- |
| `0x0093c544` | Descriptor prefix `LY050A`, followed at `+8` by `FLSNW036AF 28/4.5 XF` | Identifies this capture's descriptor, not a universal lens profile. |
| `0x009504d4` / `0x00950508` | 32-bit 1 / 0 | Lens-present / lens-ready admission words from the existing lens-state decode. Presence is not readiness. |
| `0x00942ab0` | 32-bit 0 | State returned by `0x0076a224`, through `0x0076d9b8` / `0x0076d9c8`, in the direction-step lookup chain. |
| `0x016c0fc0` | 32-bit `0x0d` | Captured search state only. |
| `0x016c0fd8`, `0x016c0fda`, `0x016c0fe2`, `0x016c100c`, `0x016c100e` | 16-bit 0 at each address | Captured bound/step/plan fields are zero, not valid positive host step parameters. |
| `0x016c1040`, `0x016c1190`, `0x016c1f44`, `0x016c21f8` | 32-bit 0 at each address | Captured lookup context is insufficient to select an active focus-step table. |
| `0x0096ca50` | 32-bit `0x41acd800` | Uncached alias of captured physical `0x01acd800`; `0x0069f8fc` dereferences this base plus the key. |

The prior outside-capture conclusion was wrong: it omitted the `0x40000000` uncached DRAM alias documented by E-XM1-MPU-BOOT-RECORD-150. For this bank, translate `0x41acd800 + key` to file offset `0x01acd800 + key`. Thus key `0x8811` reads physical `0x01ad6011`, and halfword key `0x3e1445` reads `0x01eaec45`. `0x0069fec8` combines successive bytes little-endian. The console-store sanity read at alias `0x400a0800`, physical `0x000a0800`, returned `syslog Ver 2.3`.

All 65 inspected keyed lookups below resolve inside the capture after alias translation. Widths are bytes. The largest accessed physical end is `0x01eaff14`, below capture end `0x05aa0000`. **Lookups still outside after alias translation: none in this inspected set.** These are hardware-observed bytes for this body and mounted lens, not universal defaults or a coherent active AF trace.

| Key | Width | Physical file offset | Hardware-observed value |
| --- | ---: | --- | ---: |
| `0x8750` | 1 | `0x1ad5f50` | 2 |
| `0x8751` | 1 | `0x1ad5f51` | 6 |
| `0x8752` | 1 | `0x1ad5f52` | 30 |
| `0x8753` | 1 | `0x1ad5f53` | 40 |
| `0x8754` | 1 | `0x1ad5f54` | 50 |
| `0x8775` | 1 | `0x1ad5f75` | 15 |
| `0x8776` | 1 | `0x1ad5f76` | 30 |
| `0x8778` | 1 | `0x1ad5f78` | 1 |
| `0x8779` | 1 | `0x1ad5f79` | 10 |
| `0x877a` | 1 | `0x1ad5f7a` | 25 |
| `0x877b` | 1 | `0x1ad5f7b` | 30 |
| `0x877c` | 1 | `0x1ad5f7c` | 40 |
| `0x877d` | 1 | `0x1ad5f7d` | 50 |
| `0x877f` | 1 | `0x1ad5f7f` | 15 |
| `0x8780` | 1 | `0x1ad5f80` | 8 |
| `0x8781` | 1 | `0x1ad5f81` | 10 |
| `0x8782` | 1 | `0x1ad5f82` | 10 |
| `0x8783` | 1 | `0x1ad5f83` | 4 |
| `0x8787` | 1 | `0x1ad5f87` | 150 |
| `0x8788` | 1 | `0x1ad5f88` | 50 |
| `0x878c` | 1 | `0x1ad5f8c` | 0 |
| `0x878e` | 1 | `0x1ad5f8e` | 5 |
| `0x878f` | 1 | `0x1ad5f8f` | 1 |
| `0x87da` | 1 | `0x1ad5fda` | 1 |
| `0x87df` | 1 | `0x1ad5fdf` | 20 |
| `0x87e1` | 1 | `0x1ad5fe1` | 16 |
| `0x87f1` | 1 | `0x1ad5ff1` | 80 |
| `0x8807` | 1 | `0x1ad6007` | 1 |
| `0x880e` | 1 | `0x1ad600e` | 0 |
| `0x8810` | 1 | `0x1ad6010` | 2 |
| `0x8811` | 1 | `0x1ad6011` | 3 |
| `0x8812` | 1 | `0x1ad6012` | 4 |
| `0x8814` | 1 | `0x1ad6014` | 20 |
| `0x8818` | 1 | `0x1ad6018` | 1 |
| `0x8819` | 1 | `0x1ad6019` | 1 |
| `0x881a` | 2 | `0x1ad601a` | 65535 |
| `0x881e` | 1 | `0x1ad601e` | 6 |
| `0x8821` | 1 | `0x1ad6021` | 0 |
| `0x8822` | 1 | `0x1ad6022` | 20 |
| `0x8823` | 1 | `0x1ad6023` | 5 |
| `0x8826` | 1 | `0x1ad6026` | 20 |
| `0x8829` | 1 | `0x1ad6029` | 50 |
| `0x882a` | 1 | `0x1ad602a` | 50 |
| `0x882b` | 1 | `0x1ad602b` | 3 |
| `0x882c` | 1 | `0x1ad602c` | 6 |
| `0x882d` | 1 | `0x1ad602d` | 3 |
| `0x882e` | 1 | `0x1ad602e` | 6 |
| `0x882f` | 1 | `0x1ad602f` | 10 |
| `0x8831` | 1 | `0x1ad6031` | 8 |
| `0x8833` | 1 | `0x1ad6033` | 8 |
| `0x8835` | 1 | `0x1ad6035` | 4 |
| `0x8839` | 1 | `0x1ad6039` | 1 |
| `0x883a` | 1 | `0x1ad603a` | 10 |
| `0x927a` | 1 | `0x1ad6a7a` | 1 |
| `0x927b` | 1 | `0x1ad6a7b` | 10 |
| `0x9285` | 2 | `0x1ad6a85` | 526 |
| `0x3e0240` | 2 | `0x1eada40` | 0 |
| `0x3e0f80` | 1 | `0x1eae780` | 1 |
| `0x3e0f82` | 1 | `0x1eae782` | 1 |
| `0x3e1445` | 2 | `0x1eaec45` | 0 |
| `0x3e144d` | 1 | `0x1eaec4d` | 70 |
| `0x3e15a6` | 2 | `0x1eaeda6` | 100 |
| `0x3e162f` | 1 | `0x1eaee2f` | 0 |
| `0x3e2640` | 2 | `0x1eafe40` | 179 |
| `0x3e2713` | 1 | `0x1eaff13` | 0 |

The lens-step chain remains `0x00faab3c -> 0x00fab390 / 0x007492ac -> 0x00749254 -> 0x0074888c / 0x00748818`, with further transformations behind `0x0074690c` and `0x0074782c`. Its listed keyed inputs are now observed, not outside the capture. Recovering a coherent positive resolved step still requires the runtime selector/context and movement-plan semantics; the captured active fields are zero. Consequently only the two resolved increments and declared context/statistic reductions remain synthetic, while the modeled bank thresholds, scale factors, cap and fit selector use captured values. The raw capture and proprietary tables are not redistributed.


## Comparison and replay

[Supported commands](../af/README.md#shared-rust-candidate-controller) build the library, replay tests and Haskell comparison harness through Bazel and the locked Nix compiler. No Cargo graph is added. `//af:candidate-compare` runs A0-structural, A1 and A2 independently on each supplied curve, sharing bounds, sample budget, reference optimum and initial position. Their different search parameters are explicit above. Child observation loops are bounded to 30 seconds. Curve inputs are capped regular files with at most 4096 strictly ordered positions and bounded scores.

The capped reader follows fixture symlinks, including Bazel runfiles, and checks the opened target's type and size. `//af:candidate-compare-tests` exercises all three candidates through the Haskell CLI, checking focused positions, A0-structural's two near-peak budget refusals, flat-curve refusal and accounting bounds without fixing sample counts.

CSV fields are `origin,curve,candidate,sample_count,simulated_travel_units,final_position_error_units,outcome`. Every result row says `simulated`. Travel is the sum of absolute moves from the fixture's initial position through every requested observation, including returning for final verification. Final-position error is absolute distance from the declared analytic optimum; a refusal leaves error empty. These are position/sample comparisons, not latency, acquisition frequency or motor-energy measurements. The harness reports the actually accepted count and checks it against the adapter's count.

This is closed-loop synthetic curve simulation. Arbitrary requested scores use integer linear interpolation between the listed samples, with division towards zero. Movement is instantaneous and settled, repeated observations at one position have the same score, and there is no backlash, acquisition delay, exposure drift or time-varying noise. Interpolation is not recorded-path replay and does not calibrate counterfactual camera observations. Recorded camera inputs and target ranking remain dependent on #108. The existing delayed C simulator remains a separate legacy experiment, not a second implementation of these new candidates.

### Synthetic fixture provenance

The five [`.curve` fixtures](../af/candidates/fixtures/) are original project-authored mathematical tables, licensed AGPL-3.0-only. They contain no camera captures, donor data or upstream training samples. The first row is `synthetic name analytic_optimum initial_position`, followed by `position score`. Positions are 0, 10, ..., 1000; all formulas below are clipped to [0, 1,000,000]. The analytic optimum is not recomputed from the candidate's requested samples. Linear interpolation can shift the sampled optimum relative to the analytic value.

| Fixture | Score before clipping | Reference / initial position | Expected replay behaviour | SHA-256 |
| --- | --- | --- | --- | --- |
| `single-peak.curve` | `1000000 - (p - 537)^2` | 537 / 500 | A1/A2 finish within 10 units. A0-structural exposes its reduced direction model's budget limit with observed evidence threshold 3. | `e92306a1bf3f337fb9b7d7fa15dda904d008a5072c26a2231246605667519c82` |
| `two-depths.curve` | `max(900000 - 2*(p - 230)^2, 1000000 - 3*(p - 730)^2)` | 730 / 100 | All finish near the stronger depth. A0-structural's observed decline parameters continue across the first valley in this example. | `edfa4c83fce1db3d0ee786dc9d6603acf631a502a4d3794bda1c2a593e981a9f` |
| `edge-peak.curve` | `1000000 - p^2` | 0 / 900 | All may finish at the synthetic endpoint. A measured edge maximum invalidates a physical reference sweep under §11.3. | `728683fbc0be835cacd90303bbcbc6bc214cf86b1ffe631b93d1511840b1edb9` |
| `noisy-peak.curve` | `1000000 - (p - 537)^2 + ((17*p) mod 101) - 50` | 537 / 500 | A1/A2 finish within 10 units; A0-structural exhausts its direction budget. Noise is deterministic spatial perturbation. | `74b356edc28af8ec315782c2b93f8d0ecef504161a907507db0fa400c575f342` |
| `flat.curve` | `500000` | 0 / 500 | All refuse as flat; optimum 0 is a format sentinel, not a unique focus label. | `a45ee14cbcf76ccb14b21103ecfd54edbb06688cf2ff6d7f7c29f32051b9b7a3` |

The behavioural Rust replay suite consumes these public curves and checks the result positions/refusals, bounded samples, stale/missing/unsettled waits, wrong-position refusal, changed-final-score refusal, cancellation and eventual budget failure. The replay target passed in the checked-in environment. No synthetic result accepts a camera speed/accuracy gate.

An additional inline Rust replay fixture, tagged synthetic, samples `max(1, 2^(-3*p^2/2500 + 4*p/25 + 7))` at positions 0/50/100/150/1000. Its analytic optimum rounds to 67. It checks the logarithmic fit's final position independently of the private firmware table. This original mathematical fixture is not a sixth comparison-harness input.

### Exercised simulated comparison

The rerun of the five-fixture `//af:candidate-compare` command produced the table below after the alias correction and hardware-observed parameter update. Travel and errors are artificial position units. A0-structural now reaches the stronger depth in the two-depth example, but its reduced direction model exhausts the 64-sample budget on the single/noisy examples. The bounded diagnosis above identifies a repeating direction-search cycle, not evidence that stock needs a larger budget. Its mixed observed/synthetic profile differs from A1/A2; these figures cannot rank calibrated controllers or establish a camera improvement. A0-structural is not usable as §11.2's controller reference until its increments and statistic units are bound.

| Origin | Curve | Candidate | Sample count | Simulated travel | Final-position error | Outcome |
| --- | --- | --- | ---: | ---: | ---: | --- |
| simulated | single-peak | A0-structural | 64 | 2079 | | Budget |
| simulated | single-peak | A1 | 34 | 2425 | 2 | focused |
| simulated | single-peak | A2 | 17 | 2071 | 2 | focused |
| simulated | two-depths | A0-structural | 24 | 1242 | 6 | focused |
| simulated | two-depths | A1 | 34 | 1835 | 5 | focused |
| simulated | two-depths | A2 | 16 | 1507 | 1 | focused |
| simulated | edge-peak | A0-structural | 23 | 900 | 0 | focused |
| simulated | edge-peak | A1 | 22 | 3120 | 0 | focused |
| simulated | edge-peak | A2 | 14 | 2900 | 0 | focused |
| simulated | noisy-peak | A0-structural | 64 | 2079 | | Budget |
| simulated | noisy-peak | A1 | 34 | 2425 | 2 | focused |
| simulated | noisy-peak | A2 | 15 | 2056 | 3 | focused |
| simulated | flat | A0-structural | 48 | 1500 | | FlatCurve |
| simulated | flat | A1 | 9 | 1500 | | FlatCurve |
| simulated | flat | A2 | 9 | 1500 | | FlatCurve |
