# Governance, camera runs, and handoff

## 3. Camera runs

### 3.1 Procedure

This is the whole process for any camera operation.

1. **Model:** implement or adjust the operation in `//transport:camera` with its bytes built internally, pass the host tests and the offline rehearsal, and commit. Clear the gate its class sets in section 3.2.
2. **Owner:** confirms in chat, with a go for this run or a standing confirmation in section 3.4 that covers it. For H3 the go is per run and names the operation and the image or payload digest.
3. **Owner:** puts the camera in the mode the operation needs, for example jig mode, and leaves it attached. **Model:** runs the single command, e.g. `devenv shell -- bazel run //transport:camera -- end-setting`. The command picks the one connected `04cb:ff80` device, journals every outgoing byte before sending it, writes its output under `private-data/sessions/` and prints the result, which the model reports in chat.
4. **Owner:** exits jig mode, power-cycles, takes a shot and plays it back, and tells the model the result in chat, with photos or logs only if something looked wrong.
5. **Model:** keeps the session record under `private-data/sessions/`. At a milestone or decision it records the result in the relevant report and GitHub issue, adds an evidence-ledger entry if it is a durable research claim, and commits.

A failed or stopped run needs no cleanup record. Fix the code, rehearse, commit, and go back to step 2.

### 3.2 Operation classes

| Class | Examples | Before the run |
| --- | --- | --- |
| H0 host only | Firmware parsing, emulation, synthetic tests, source inspection | Nothing. Work on an emulated camera is H0. |
| H1 read or query | USB enumeration, identity query, bounded config reads | Owner's go or a standing confirmation. |
| H2 temporary state change | Debug flag, RAM payload, a read whose request is staged by writing camera RAM, ordinary focus/capture control over USB | Owner's go or a standing confirmation naming the operation. New mechanism bytes: one independent review. A content-only revision of a reviewed RAM payload: the mechanical payload check, the emulator run and the owner's visual check instead. |
| H3 persistent state | End Setting, firmware update, configuration, calibration or boot writes | Owner's per-run go naming the operation and digest; one independent review of new bytes; a flash image boots to the stock menus under `//emulation:flash-boot-check`; no boot-path prerequisite is open. |
| Invasive | Physical debug pins, opening the body, intentional recovery testing | Discuss a plan with the owner first. |

Classify by camera effect, not USB direction. A routine named `read` can modify a debug flag; an observation query still sends requests.

Mechanism is the installer, rendezvous, hooks, host operations and any code that runs outside the payload's allocation or redirects stock control flow. Content is payload code and data that the mechanical payload check confines to the allocation and allowlisted calls; a revision that changes anything else is a mechanism revision. Independent review runs alongside the emulator run and the owner's visual check, and the live run waits for all of them. Host-only, read-only and research work gets no independent review unless it becomes the basis of an H2 or H3 decision.

### 3.3 Standing rules

- Camera code exposes only closed operations with internally built bytes. It has no raw command path, never retries, and stops on any unexpected reply, status or disconnect.
- Do not write guessed values into configuration offsets, GPIOs, regulator registers, lens command mailboxes or boot controls.
- Never restore another camera's dump onto this body. Keep this body's calibration backup in two verified locations; it exists for the 1.00 adjustment area (E-S03-DURABLE-PRESERVATION).
- Do not deliberately corrupt the owner's only camera to prove recovery.
- After unexpected camera behavior, stop camera runs and describe it to the owner. Host work continues.

### 3.4 Standing confirmations

The owner may grant in chat a standing confirmation for a named H1 operation or a reviewed H2 operation. It is recorded here with its date, scope, conditions and lapse, and while it holds the model runs the covered operation without a per-run go. H3 always needs a per-run go.

## 4. Collaboration

Agents may be spawned with the models listed in [AGENTS.md](../AGENTS.md). Split work by workstream (transport, firmware analysis, boot/recovery, emulation, AF experiments) when that helps; one agent can hold several. Independent review follows section 3.2, and an agent must not review its own change.

Gate acceptance is a recorded decision linked to evidence and its limitations; it is distinct from task completion. Use [GitHub Issues](https://github.com/skeet70/latent-image/issues) for open gate tracking. Completed decisions remain in Git history; retain their technical findings and limitations in the relevant reports and evidence ledger.

A task that establishes a research result states what was established and on which firmware/input, how to reproduce it, its evidence label, and what remains open. Avoid arguments from sibling-camera similarity alone; caller tracing, address mapping and behavior establish the X-M1 contract.

## 14. Development loop after G5

1. Propose one change and its experiment.
2. Run the relevant stock and candidate fixtures in emulation; clear the section 3.2 gate for the candidate's bytes.
3. Load the reviewed artifact, run the bounded experiment and collect evidence through the [procedure](#31-procedure).
4. Remove the temporary change through the known path and confirm normal operation.
5. Analyze speed and accuracy; record accept/reject and why.

Keep a known-good runtime build alongside every candidate. A known-good payload is a rollback artifact, not a guarantee it can execute after arbitrary corruption.

## 15. Stopping rules and alternative paths

| Finding | Next action |
| --- | --- |
| Public tool is incomplete | Inventory missing pieces; implement the smallest needed component or continue from accessible firmware/RAM evidence. Do not wait indefinitely for private code. |
| Service mode lacks usable preview/AF | Map the mode transition and possible transient instrumentation route. Continue host work; do not claim a working AF loop. |
| No owned RAM or justified entry mechanism | Stop live injection. Continue static analysis and emulation. |
| Recovery requires unknown persistent settings | Keep recovery at its current evidence level; do not set guessed flags. |
| AF is executed mainly in an inaccessible DSP or fixed-function block | Determine whether host CPU controls tunable parameters or scheduling. If not, report the specific limitation. |
| The lens or sensor dominates latency | Reassess achievable gains from reduced travel/samples; stop pursuing arithmetic-only optimizations that cannot matter. |
| A learned predictor cannot meet target latency | Reduce inputs/model, distill into a small calibrated predictor, or retain a conventional controller. |
| Performance improves only by losing accuracy | Reject or explicitly scope an optional trade-off; do not call the original goal met. |
| Emulator passes but hardware diverges | Capture and model the earliest divergent input/state. Avoid adding unconditional-success stubs. |
| Unexpected persistent or hardware effect | Stop camera runs and tell the owner. Host work continues. |
| Persistent installation is too risky | Deliver the proven temporary implementation and its operational limits. |

## 16. Final handoff requirements

The project is ready to hand off when a fresh agent can answer these questions from the repository:

1. Which exact body, firmware, lens, and tool versions are supported?
2. What actually improved, under which measured conditions, and with what uncertainty?
3. Which functions, callers, memory regions, and device operations were verified?
4. Which pieces were simulated or stubbed, and what has never been observed on the camera?
5. How does the camera return to stock behavior after a normal session, a host failure, or a payload crash?
6. What recovery failure classes are demonstrated, and what data is missing from backups?
7. What commands reproduce the offline tests and the accepted on-camera experiment?
8. Which persistent changes, if any, were authorized and performed?
9. Can the resulting feature be used for ordinary untethered photography, and how is it activated?

Deliver source, lockfiles, manifests, tests, private-data access instructions, benchmark results, known limitations, and a concise operator guide. Do not rely on an earlier chat turn to communicate a critical address assumption, activation procedure, or recovery limitation.
