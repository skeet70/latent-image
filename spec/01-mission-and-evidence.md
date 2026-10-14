# Mission, evidence, and source baseline

## 1. Mission and definition of success

Improve the X-M1's contrast-based autofocus through firmware research, using Fujihack, `fffw`, selected-function emulation, and reproducible measurements. Establish a development loop that minimizes persistent camera changes, investigates recovery early, and lets agents do most iteration on the computer.

### 1.1 Three deliverables

1. **A reproducible research platform.** Version-pinned tooling; audited camera access; preserved original firmware and camera-specific data; an evidence ledger; firmware analysis; selected-function emulation; a camera protocol and timing trace format.
2. **An iterable on-camera prototype, if access permits it.** A small, version-specific implementation loaded into RAM, observable and removable, with stock firmware as the baseline. Normal development should not require reflashing the camera.
3. **A demonstrated AF improvement.** A controller that runs on the camera and improves measured focus acquisition while retaining focus accuracy and acceptable failure behavior. Begin with one explicitly identified lens and single-shot, single-area autofocus. Generalize only after that works.

The final photography workflow should run inference and focus control on the camera. A tethered computer is acceptable for research, logging, loading a prototype, and early experiments; a laptop making every focus decision is not the completed camera feature. If persistent installation would require disproportionate risk, an explicitly activated RAM implementation can be a useful intermediate deliverable, with that limitation clearly stated.

### 1.2 What counts as progress

- A working service connection is not proof of recovery after firmware corruption.
- A repeatable memory dump is not necessarily a complete persistent backup.
- A simulated speedup is not measured wall-clock speedup on the camera.
- Code executing in RAM can still invoke flash writes, corrupt calibration, or command hardware incorrectly.
- A successful boot or a passing checksum does not establish that a modified firmware image is safe.
- A negative feasibility result is acceptable if accompanied by the specific blocking evidence and useful completed artifacts. Do not describe an unfinished research platform as improved autofocus.

### 1.3 Initial scope

The primary target is the owner's Viltrox AF 28mm F4.5 on lens firmware 1.05, identified in G1 (E-S02-IDENTITY-QUERY). Confirm body/lens identity before establishing acceptance thresholds or collecting a new baseline. Keep the initial AF target narrow: stationary subjects, one selected AF area, acquisition from known near/far starting positions, and ordinary exposure conditions. Moving subjects, multi-area selection, continuous AF, video AF, subject recognition, and multiple lens models are later work.

## 2. Source baseline and evidence scope

The table records the scope of the source leads, not current gate status. Bind X-M1 hardware claims to the owner's exact body and firmware. Current selected-path audits and source/format findings are in [the evidence ledger](../docs/evidence.jsonl), [transport audit](../reports/source-and-transport-audit.md) and [firmware findings](../reports/firmware-extraction.md); [the task board](../docs/task-board.md) records accepted gates. Reproduce exact source pins and attach new observations to their scopes.

| Evidence | What it supports | What remains unproved |
| --- | --- | --- |
| Fujihack provides firmware patches, RTOS knowledge, and debugging examples for related cameras. [S01](09-sources.md#s01) | Useful naming, architecture, and comparison material. | A ready-made X-M1 AF port. |
| `fffw` provides `ff80`, a native service-protocol client. [S02](09-sources.md#s02) | A route worth auditing before considering firmware flashing. | Complete behavior and safety on the user's body. |
| At the inspected commit, `cmd_bootrom` explicitly accepts the model string `X-M1`. [S03](09-sources.md#s03) | Stronger evidence than family membership alone for investigating this command. | That its address assumptions match this camera, or that recovery works. |
| The same source temporarily changes a RAM debug setting during several memory-reading operations. [S03, S04] | Read-command names alone are insufficient for classifying side effects. | Whether firmware might persist a changed setting through another path. |
| `fffw` has public `ffun` code, but its own subdirectory README calls the translation incomplete. [S05](09-sources.md#s05) | Some extraction/build work may be reusable. | That the full command set builds or that an X-M1 manifest is available. |
| Public `boot/` files contain X-E2-specific startup and LED examples. [S06](09-sources.md#s06) | Concrete early-boot research material. | X-M1 register compatibility or a complete rescue image. |
| The maintainer describes early SD boot on X-E2; `ffre` and `ffem` are described as unpublished, with recovery on hold. [S02, S07] | Recovery and selective emulation are grounded research directions. | A published X-M1 recovery procedure or a usable dependency on those private tools. |
| Fuji lists X-M1 firmware 1.50. [S08](09-sources.md#s08) | An official analysis input and version reference. | The installed version, or that upgrading immediately would help this research. |
| Post-2013 image-based AF papers and open camera-control code exist. [S11–S13](09-sources.md#s11) | Candidate algorithms, datasets, and control-loop references. | A speedup over Fuji's implementation on this processor and lens. |
| The user supplied an X-T3 firmware-modification account involving selected ARM-function emulation and integration failures. [S14](09-sources.md#s14) | A useful test-design example. | Reproducible code, X-M1 compatibility, or a verified recovery method. |

### 2.1 Pinned source baseline

The inspected `fffw` baseline is:

```text
bedc091e0b54a1a34aaf6929dd08e1db36d13b08
```

Fetch this commit into an isolated source checkout. Record any newer commit as a separate input, inspect its diff, and deliberately choose a research baseline. Do not let a branch update silently change the implementation used on the camera.

Specific source-audit leads, not commands to execute blindly:

- `ff80/ff80.py`: parser, device selection, session lifetime, exception handling.
- `ff80/fftlib.py`: USB framing, endpoints, transfer lengths, timeouts.
- `ff80/ffjlib.py`: memory/configuration operations and debug-setting changes.
- `ff80/ff80_cmd/cmd.py`: `info`, boot-ROM probing, reads, and persistence paths.
- `ff80/ff80_cmd/cmd_exp.py`: model-specific experimental operations; exclude from initial access.
- `ffun/README.md`, `go.mod`, `Makefile`, and imports: determine what is actually buildable.
- `boot/xe2-start.S`, `boot/xe2-led0-main.c`: reference-only until independently mapped to X-M1.

The inspection identified a model allowlist, a hard-coded candidate boot-ROM range, a known-ROM fingerprint check, temporary debug writes, and incomplete cleanup paths around some operations. These are concrete items to verify in a source review; they are not a completed command-safety audit. Prefer the implementation over stale examples: for example, the inspected parser gives `ram read` a bounded default and separately exposes an unbounded dump mode.

### 2.2 Stock firmware and lens compatibility

After observing the user's body/lens versions, review official release notes for that combination. Fujifilm's [X-M1 release history](https://www.fujifilm-x.com/en-us/support/download/firmware/cameras/x-m1/) includes lens-specific AF changes. A known stock improvement can inform the baseline and attainable goal. An improvement already supplied by a stock update must not be attributed to a custom controller. The body moved from 1.00 to 1.50 after the 1.00 baseline and calibration capture; 1.00 baseline images remain the comparison until a 1.50 baseline is needed.
