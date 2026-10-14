# Synthetic fixture provenance

All three JSON fixtures were authored for this repository on 2026-09-28. They contain constructed numbers and no camera observations, photographs, firmware, private identity, measured lens calibration, or external dataset. Their checked-in JSON is the reproducible input; the historical E-02 handoff records the original SHA-256s. Fixture IDs identify artificial cases and do not name physical scenes. Original synthetic fixtures follow [the repository licensing terms](../../LICENSING.md).

| Fixture | Constructed expected behavior |
| --- | --- |
| `peak-delayed.json` | Range 0–100, contrast `max(0, 10000 - (position - 60)^2)`, zero noise, coarse step 25/fine step 5; finish at artificial position 60. Frames arrive 15 arbitrary ms after acquisition while motion takes 5 + distance arbitrary ms. |
| `flat.json` | Same range/delivery/motion, constant contrast 10000; fail with low confidence after the coarse sweep. |
| `unknown-telemetry.json` | Same parabolic curve, position/completion unavailable and explicitly null; optional telemetry policy, instantaneous model motion; finish at artificial position 60. |

The noise case in the test mutates the peak fixture to amplitude 10 with seed 1. Its unsigned 32-bit LCG is `x = (1664525*x + 1013904223) mod 2^32`; noise is `x mod (2*amplitude+1) - amplitude`, with resulting scores clamped to uint32. This is repeatable test noise, not a measured sensor distribution. The test also derives explicitly synthetic faults/configurations from the peak fixture without storing extra datasets.

Expected peaks follow directly from the analytic parabola and include the peak in the fine grid. They establish controller behavior on these artificial curves. They do not infer real signed defocus, global best focus in multiple-depth scenes, latency gains, generalization, or calibration. Recorded replay and measured-model simulation remain separate future artifacts.
