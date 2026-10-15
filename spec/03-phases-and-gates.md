# Execution phases, gates, and dependencies

## 8. Execution phases and gates

### Phase 0 - Reproducible host work without a camera

**Owners:** coordinator, A, F. **Class:** H0.

Tasks:

1. Maintain the repository, worktrees, private immutable inputs, GitHub Issues and evidence schema. Use [the camera-run procedure](07-governance-and-handoff.md#3-camera-runs) for device access.
2. Reproduce the environment and record exact source revisions. Inventory public code separately from README descriptions of private tools.
3. Read the minimal transport call chain before executing it: device selection, session open/close, identity queries, configuration reads, memory reads, exception handling, and imports.
4. Build a fake USB transport that captures outgoing bytes and supplies captured or synthetic responses. Add tests for missing device, wrong model, short reply, zero-progress read, address overflow, timeout, and restore-on-exception behavior where relevant.
5. Expose only reviewed closed operations with internally built bytes; do not expose raw experimental service dispatch.
6. Separate all hardware tests from the default test command. In the offline environment, device opening should be impossible through the normal project API.

These phases define research requirements, not a setup checklist or current completion status. [GitHub Issues](https://github.com/skeet70/latent-image/issues) owns open work and gate tracking. Completed work and earlier gate decisions remain in Git history; reports and the evidence ledger retain their supporting technical findings. [Current principles](../docs/principles.md#implementation-and-build) own implementation and build policy. Roles describe ownership, not camera authorization.

**Deliverables:** source/build inventory, command-side-effect matrix, pinned environment, offline broker tests and repository instructions.

**G0 acceptance:** A and F agree on what the first camera request actually sends. The environment reproduces on a clean worktree. No build or default test opens a device. A knows the exact terminal commands to run for first identification, with an expected response and a bounded stop path.

### Phase 1 — Identify and characterize the stock camera

**Owners:** A and E; F reviews access. **Class:** ordinary photography and H1; H2 only after its specific scope is accepted.

#### First physical session

1. Record model and installed body/lens firmware through normal camera controls; photograph the version screen if practical. Record normal power-up behavior and take reference images before service work.
2. Use a reliable data cable and an adequately charged battery. Remove accessories not needed for this session. Keep experiments that do not need a lens separate from lens-control work.
3. Ask the user to enter the documented service-mode button sequence after confirming it is the one being evaluated for this body. `fffw` documents powering on with USB connected while holding Shutter and Up. [S02](09-sources.md#s02)
4. On Linux, inspect USB enumeration. `lsusb -d 04cb:ff80` is the documented identification check. A USB product ID alone does not identify the camera model. [S02](09-sources.md#s02)
5. Execute the audited identity query through the broker. Save raw bytes and parsed fields. Confirm there is exactly one eligible device and the model is X-M1.
6. End the session cleanly, return to ordinary camera operation, and confirm the same baseline functions work. Repeat entry/exit enough to establish a reproducible sequence, not a large endurance test.

Select the device as in [the runtime contract](08-artifact-and-runtime-contracts.md#device-selection). Record service-mode restrictions on live view, AF, and lens communication as soon as they can be observed through audited operations; this feasibility finding should guide mapping priorities before a large emulator is built.

Do not start with an address-space sweep. Do not run an X-E2-specific example merely because it appears in the CLI.

#### Stock AF benchmark

E defines the first small test matrix while A works on access. Use a stationary target and manual camera controls if no programmatic control exists. Label manually gathered positions as approximate until actuator positions can be measured.

Collect at least:

- Bright, high-contrast target at a representative distance.
- Moderate indoor light and a low-contrast target.
- Near-to-far and far-to-near starts with the same target and lens settings.
- A scene that often hunts, plus a case already focused near the subject.
- Fixed AF area, aperture, exposure policy, focal length, and repeatable starting state.

Use external video or other documented timing observations for a coarse first measurement. Record timing uncertainty. Half-press to green confirmation and half-press to independently acceptable sharpness are distinct metrics; retain both where observable. Do not use JPEG save time as a focus-speed measurement.

**G1 acceptance:** exact body/lens identity, a reproducible service entry/exit procedure or a documented failure, stock camera baseline, and a small AF dataset. If service mode fails, host analysis and the photographic baseline continue; the result is a blocker for hardware access, not permission to probe arbitrary commands.

### Phase 2 — Preserve inputs and map firmware

**Owners:** B and A; C consumes boot artifacts; F reviews reads. **Class:** H0/H1, with explicitly classified H2 debug-setting changes.

1. Acquire an official firmware image matching the installed version when available. Preserve the exact file and SHA-256. If only another version is available, keep its findings separately scoped.
2. Inventory what public `fffw` and Fujihack can extract. Verify source files and dependencies exist before relying on a named command. Repair/reimplement only the needed parsing components, with test fixtures and documented format assumptions.
3. Parse the container's segments, sizes, encoding, compressed regions, raw trailers, load addresses, and checksums. Reject unsupported compression tokens; do not guess them. (Exception approved by the user 2026-10-03 for the X-M1 hardware-decompressed images only: a data-inferred grammar counts only if its output passes exact-length, header and cross-image call-target checks, and must be compared with a camera RAM read once such access is authorized (done for these images: E-RAM-PROBE-150, E-DUMP-RAM-150). See [the firmware report](../reports/firmware-extraction.md#boot-load-path-and-compression).) Extract without requiring a working encoder.
4. Generate an ELF or mapped binary for Ghidra, with the origin of every section recorded. Cross-check strings, vector/code patterns, data ranges, and known public structures.
5. Acquire bounded camera reads only for regions supported by the current model evidence. Preserve original debug flags, use structured exception cleanup, check restoration, and stop on anomalous replies. Handle host death or lost USB through the documented incident path; cleanup code alone cannot guarantee restoration. Avoid device registers: even reading some hardware registers can acknowledge or clear state.
6. Read configuration through reviewed dedicated calls and separately establish whether bytes came from a runtime RAM copy or persistent storage. Record missing regions. A runtime copy can help analysis but must not be labeled a verified EEPROM backup.
7. Identify candidate boot ROM from the source lead, using a bounded read whose side effects have been reviewed. Compare contents, fingerprint, instruction set, vectors, and strings. A match to another model is evidence of shared ROM code; it does not establish identical board wiring or storage parameters.
8. Compare stable decompressed firmware ranges against memory observations. Handle relocation and runtime-modified data explicitly. Do not demand byte identity for changing counters or task state.
9. Locate task/event tables and follow candidate focus, capture, image-statistics, lens, USB, and persistence paths. Export a first call graph with uncertain edges labeled.

**Deliverables:** input manifest, extraction report, mapped firmware, symbols, memory-region classifications, configuration coverage report, boot-ROM artifact if accessible, and first AF event graph.

**G2 acceptance:** exact correspondence between analyzed bytes and the chosen firmware; bounded known memory regions; repeatable stable reads; no unsupported claim of a full backup. If a parser and encoder share a bug, a round trip can still pass: use independent size/checksum expectations and observed decompressed bytes where available.

### Phase 3 — Recovery investigation and selected-function emulation

Run C's recovery work and D's emulation work concurrently. B supplies mapped code; A performs accepted observations. Both branches should produce useful milestones before any full camera emulator is attempted.

**Simulation decision:** build the smallest models that answer current questions. A full virtual X-M1 would require enough of the SoC, interrupts, RTOS interactions, sensor, image processor, storage, and lens interface to run the paths of interest. Its fidelity would itself need validation against hardware. Agent capability does not remove those missing observations. Treat full-system emulation as an optional, separately justified expansion.

| Tool | First useful outcome | What it cannot establish |
| --- | --- | --- |
| Protocol replay/fake USB | Packet construction, lengths, retries, and cleanup under known responses | Undocumented camera-side side effects. |
| Selected native-function emulation | Execute actual firmware instructions through mapped functions and callers | Unmodeled peripherals, complete startup, or real-time performance. |
| Calibrated AF/lens simulation | Compare search decisions against measured focus curves and delays | Generalization beyond measured scenes/lenses or final hardware reliability. |
| Full-system emulator, if later justified | Reproduce a specific blocked boot/task/peripheral interaction | Recovery or hardware equivalence beyond its validated scope. |

D should inspect Fujihack's existing `etc/unicorn` material before starting a new harness. [S01](09-sources.md#s01) Reuse supported components after auditing their assumptions. Every proposed emulator expansion must name the experiment it enables, the hardware traces needed to validate it, and a finite completion criterion. No simulator result alone authorizes a firmware flash.

#### 3A. Recovery research

Follow [section 10](04-recovery.md). Begin with boot control flow, entry conditions, storage dependencies, and independently executable SD code. Identify whether the proposed entry mechanism depends on intact application firmware, persistent boot flags, or particular NAND/eMMC data. Prefer a path that leaves those intact.

The first target is a small, independently booted diagnostic that does not write internal storage or configuration. Even an LED blink writes a device register, so its GPIO and electrical role must be established for X-M1. A successful X-E2 sample is reference material only.

#### 3B. Native-function harness

1. Select a small deterministic stock function from an understood region. Establish the evidenced CPU family and instruction mode (ARM/Thumb only if verified), calling convention, input/output memory, stack, global data, and all transitive calls. Use the emulator's documented APIs and explicitly supported CPU features. [S10](09-sources.md#s10)
2. Map only known regions. Make code/ROM read-only by default; writable state is explicitly listed. Trap accesses to unknown memory and every unstubbed device register.
3. Set instruction and wall-clock limits. Maintain instruction/access logs sufficient to reproduce a failure. Do not allow a stuck emulation to hang a worker indefinitely.
4. Preserve register state and ABI requirements in fixtures; include integer widths, overflow behavior, and float conventions if used. An emulated ARM core is not automatically the exact X-M1 CPU or its timing model.
5. Replace external operations with explicit stubs whose inputs, outputs, and side effects are documented. Unknown operations fail the case. Do not return generic success until a firmware path happens to complete.
6. Grow into connected AF caller/event paths using captured observations. Test both entry selection and completion/cancellation, including the mode conversion that an isolated inner function might miss.
7. Track unmodeled DMA, interrupts, caches, scheduler behavior, and hardware image processing. Passing arithmetic tests does not settle those integration questions.

Use Unicorn's C ABI through a narrow engine boundary under [the implementation policy](../docs/principles.md#implementation-and-build). Native cases follow [the runtime contract](08-artifact-and-runtime-contracts.md#native-case-and-payload-requirements) and [the native-case schema](../schemas/native-case.schema.json). `//emulation:unicorn-info` proves only host integration; stock-path acceptance requires independently known behavior.

**G3 acceptance:** one useful native path reproduces independently known behavior under bounded conditions; all stubs and coverage exclusions are listed. C separately reports the current recovery level, even if still low. Do not require a full-system emulator merely to complete this gate.

The accepted G3 result is the reviewed 1.50 area-3 record-validation callback `0x006a84d4` (E-AREA3-VALIDATOR-NATIVE-150). E-DUMP-RAM-150 establishes installed-byte correspondence for its seven executed-code windows at `0x006a8300`/`0x006a8400`/`0x006a8500`/`0x006a8600`/`0x00722100`/`0x00722200`/`0x00722300`. The owner accepted this scoped result on 2026-10-08; `probe-code` remains an optional reviewed fresh-boot re-confirmation, not a required unperformed acceptance step.

### Phase 4 — Establish a reversible execution and observation loop

**Owners:** A and B, with C's recovery assessment and F's independent review. **Class:** H2.

This phase depends on measured access, not only a source claim about writable executable RAM.

**G4 preflight:** before the first novel payload executes, A/B/F review identity, original bytes, allocated RAM, entry/return design, expected write set, offline results, stop procedure, and current recovery level. This applies to C's early-boot diagnostics too. R2 is a preferred prerequisite for later runtime experiments, not a circular prerequisite for the diagnostic that would establish R2. G4 is accepted only after the planned hardware observations below succeed.

Prerequisites are specific to the experiment: an early independent diagnostic cannot require proof of the later runtime hook it is intended to investigate. Static mapping and synthetic native fixtures can progress before a camera arrives; they do not satisfy hardware correspondence in G2 or stock-path behavior in G3. Record partial evidence and blockers without weakening the eventual gates.

1. Determine whether service mode can coexist with live view, lens communication, and AF. This is an early feasibility question. A dump taken while photography is disabled cannot establish an AF instrumentation path.
2. Establish a legitimate RAM allocation or reserved region and its lifetime. Zeros, apparent free space, and an unused-looking global buffer do not establish ownership.
3. Choose the smallest demonstrated call mechanism. Characterize how it enters/exits, stack and register handling, reentrancy, timing, cache maintenance, and interrupted transfers. Avoid patching a currently executing multi-instruction path without a justified synchronization mechanism.
4. Start with a payload that computes a constant or checksum and returns through the expected path. No lens, storage, GPIO, or register control. Verify exact binary/disassembly and expected write set in the emulator first.
5. Capture the bytes that will change. Match them immediately before the write. Load payload/data, verify readback, perform required instruction/data synchronization, then activate the reviewed entry mechanism.
6. Confirm clean return, inspect allowed memory effects, restore the entry mechanism, and return to normal camera operation. Repeat after an ordinary cold start. Preserve the pre-session and post-session configuration observations, comparing stable fields.
7. Add an observation-only probe that records an AF event/measurement into a bounded ring buffer. Avoid per-frame file writes and expensive logging in timing-critical code. Detect ring-buffer overflow explicitly.
8. Measure instrumentation overhead. Validate that stock AF with the probe behaves within baseline measurement variability before attributing later changes to an AF algorithm.

**G4 acceptance:** target identity and patch sites verified; owned RAM; demonstrated enter/return/cleanup; reviewed external-call set; recovery level recorded; no unexplained state changes; session scope covers this execution. Early independent SD boot at R2 is preferred before novel payloads. If R2 is unavailable, the coordinator must describe the remaining recovery limitation when seeking the initial H2 execution scope. Source review and a host broker reduce error risk but do not sandbox privileged code running on the camera.

**G5 acceptance:** a repeatable capture → replay → rebuild → load → observe → unload cycle, without a firmware flash, that can gather AF data in a photography-capable mode. If mode limitations prevent this, produce a feasibility report and continue offline while investigating an alternative transient entry path.

### Phase 5 — Characterize the real AF loop and evaluate candidates

**Owners:** E and B, supported by A and D. **Class:** H0 and scoped H2 experiments.

1. Correlate half-press, AF mode/area selection, lens commands, acquisition timestamps, delivered sharpness statistics, focus decisions, and completion/failure.
2. Determine what is actually slow: frame cadence, image-processing queue, too many focus samples, lens travel/settling, unnecessary reinitialization, focus confirmation, or some combination.
3. Establish actuator units, bounds, smallest useful step, backlash/direction effects, repeatability, completion signaling, and variation with aperture or lens firmware. Use existing bounded Fuji lens operations where possible; do not replace low-level motor drivers.
4. Gather additional focus sweeps and event traces for the primary lens. Record noise, exposure changes, stale samples, and dropped frames. Work with the existing sharpness statistic first if it is useful and accessible.
5. Implement the candidate sequence in section 11 against one contract. Compare to stock behavior on recorded paths and to a calibrated search baseline in the replay environment.
6. Use held-out scene-level splits and timed hardware verification. Reject a synthetic improvement that depends on unachievable lens steps, unrealistically fresh frames, or unavailable pixel data.

**Deliverables:** timing breakdown, camera/lens model with uncertainty, trace dataset, baseline controller, candidate comparisons, and target compute/memory estimates.

### Phase 6 — Integrate and verify improved autofocus

**Owners:** E and B implement; A operates; D and F validate. **Class:** H2.

1. Run the candidate first in shadow mode: it sees the same stock observations and proposes actions, but stock AF drives the lens. Use this to check units, bounds, latency, and state interpretation. Shadow mode cannot prove the candidate's full closed-loop performance because its proposed actions were not taken.
2. Integrate at the highest well-understood decision boundary that allows useful control. Preserve Fuji's validated lens communication and camera-state transitions where possible.
3. Test one bounded action, then a short complete acquisition, before opening the full scenario matrix. Avoid simultaneous changes to search policy, sharpness metric, lens transport, and UI.
4. Define fallback before enabling active control. Hand-back must occur at a known stock state or a validated restart/cancel boundary. Simply jumping back into stock AF with inconsistent lens/state history is not a safe fallback.
5. Test cancellation, half-press release/retry, timeout, stale measurement, dropped completion, lens disconnect, mode change, sleep transition, and low-confidence scenes in host simulation first. Only hardware-relevant, non-destructive cases proceed to the body.
6. Collect paired stock/candidate measurements with matched start conditions and randomized order. Evaluate section 12's accuracy and speed criteria together.
7. Repeat a small set after cold start and reload to verify portability of the process to the same body/version. Save a complete session bundle for every accepted build.

Also verify the declared untethered photography workflow: after the validated host-disconnect path, the camera retains control/inference and handles absent logging, unsupported modes, and cancellation. Record activation and cold-start loss for a RAM implementation. G6 requires this operational evidence alongside the benchmark.

**G6 acceptance:** real-camera evidence of a useful improvement, documented operating envelope, no silent regression in the required cases, target resource budgets met, and reliable removal/reload. A faster but less accurate controller fails the stated goal unless the user explicitly chooses that trade-off.

### Phase 7 — Release a reproducible result and decide installation separately

**Owners:** coordinator and F, with A/B/E. **Class:** H0/H2; H3 only if separately authorized.

Deliver a version-specific loader, source, configuration, environment locks, experiment fixtures, benchmark report, operation guide, uninstall/reset procedure, recovery status, and limitations. Another agent should be able to reproduce the research loop from those artifacts.

A release manifest identifies exactly the supported body firmware and lens combinations. Unknown versions are refused. Include provenance and applicable upstream licenses. Keep proprietary firmware inputs and camera-specific private data out of a public source release; distribute tooling/patch descriptions that use the user's own input files when practical.

Assess persistence only after the runtime result is useful. If a session loader is sufficient, retain that workflow. A permanent patch, persistent debug setting, or auto-start mechanism is a separate feature with separate recovery requirements. Do all offline packaging, diffing, and verification before presenting that concrete change to the user.

**G7 acceptance:** reproducible research and runtime deliverables, honest benchmark results, and an explicit installation status. An automatic or permanent installation is optional and cannot be implied by completion of G6.

## 9. Dependency map and parallel work

| Work | Needs | Can proceed alongside |
| --- | --- | --- |
| Environment and transport audit | This plan and upstream source | Firmware extraction, benchmark design, recovery-source reading. |
| First camera identity/session | G0, connected stock camera | Host analysis and test harness construction. |
| Firmware mapping | Exact firmware input; RAM observations improve confidence | Stock photography and algorithm baselines. |
| Boot/recovery investigation | Boot-ROM and firmware evidence | AF mapping and native-function emulation. |
| Meaningful native AF emulation | Mapped functions plus state/observation fixtures | Recovery work and dataset gathering. |
| First RAM execution | G2/G3-relevant evidence and G4 review | Offline algorithm work; complete recovery may still be in progress. |
| Controlled AF changes | G5, lens/measurement contracts, replay results | Recovery advancement and independent benchmark review. |
| Persistent installation | Proven useful prototype plus the required recovery evidence | Not on the critical path for the initial research loop. |

If blocked, keep unrelated H0 tasks moving. Do not turn an unknown X-M1 address into a guessed experiment merely to unblock a worker.
