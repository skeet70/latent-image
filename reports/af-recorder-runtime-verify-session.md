# AF recorder revision-2: installed bytes and unresolved residency (X-M1 1.50)

The revision-2 install completed, but the first ordinary boot stayed black with no LEDs (E-RECORDER-INSTALL-150, E-RECORDER-BOOT-FAILURE-150). The session stopped before any capture act; no recorder validation input exists. The body is now recovered through the exact v5 rollback, with ordinary startup, photography, normal-card operation/playback and retained film simulations/settings reported working ([recovery result](sd-boot-card-entry.md#260-physical-rollback-readback-status-20), E-RECOVERY-V5-NORMAL-CARD-PLAYBACK-150, E-RECOVERY-V5-RETENTION-150).

This report retains the byte identity and failed safety criterion, not an installation procedure. Do not reinstall or repeat the retired session. Future camera work follows [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs), with separately reviewed bytes and a named owner go for persistent operations. Superseded operator instructions remain in Git history.

## 2. The install bytes

These identify the failed candidate, not a supported release:

| Field | Recorded value | Evidence |
| --- | --- | --- |
| Owner-supplied input | Official X-M1 1.50 DAT; emitted candidate retained privately | E-RECORDER-INSTALL-150; extraction and identity checks in [the firmware map](firmware-extraction.md) |
| Card lookup | Root `FWUP0002.DAT` | Byte-read body lookup derived from the body's `0002` configuration bytes |
| Length | 38,405,148 bytes | Byte-read emitted container |
| SHA-256 | `72a78c5595f40f88af26c4ed429529a756f759aa9b8e5c5704a745613f9ed0a8` | Byte-read emitted container |
| Body/version | X-M1, 1.50; version words `1` / `0x50` | Byte-read identity/version fields, unchanged from stock |
| Site patch | Image-4 word `0x0071feb4` becomes `0xeb34a9c9`, `bl 0x0144a5e0` | Byte-read emitted container |
| Recorder placement | 1,896-byte blob at RAM `0x0144a5e0`, flash media offset `0x008ea5e0`, in the id-6 plain window | Byte-read emitted container; fixture and manifest agree on the tap word |

## The originally proposed residency observation

The patched receive call enters the hook before the original receive helper. A plain-image load completed before the first media message does not prove that the first hook invocation waited for that load. Stock media task creation at `0x0071fd40`, its initial wait on semaphore `0x3b` and the first patched call need independent ordering against id-6 loading (E-RECORDER-FIRST-RECEIVE-150).

The proposed whole-store carry at `0x400a0800`, length `0x1fff4`, with zero trailer wrap count would have supplied:

- The media task's first message after boot, task id `0x1c`, and its tick.
- The id-6 flashloader request `0x4c03`, built by `0x0073b0f4` from dispatcher `0x0073cd00`, and completion records with ticks.
- The recorder trigger tick from its trailer.

Those records measure ordering relative to messages, not first hook entry. A capture requiring the hook to execute cannot independently establish that entering it was safe. Even strictly earlier completion ticks or decisive equal-tick record ordering leave the residency precondition unresolved; missing or wrapped records also leave message ordering unresolved. Bind semaphore release and first call to completed loading, or activate the tap only on a proven post-load path, before proposing new bytes.

The black boot does not by itself prove residency was the failure cause. The [instrument report](af-observe-instruments-150.md#9-same-boot-carry-closure-over-the-pinned-bytes) retains the recovered file ABI, recorder design, ownership and coherence limits. The [install procedure](install-procedure.md) owns update power and interruption constraints. None of these host findings establishes arbitrary interrupted-update recovery or accepts an AF gate.
