# Autofocus observation plan for the 1.50 static map

Status: offline plan, `static-inference`. No camera authorization or gate acceptance. It specifies behavioural observations for the [1.50 static map](autofocus-static-map.md), what each instrument reaches and the ordering of questions. [The camera-run procedure](../spec/07-governance-and-handoff.md#31-procedure) owns authorization; [the instrument report](af-observe-instruments-150.md) owns in-body capabilities; [the first-session script](af-first-session-script.md) owns the proposed bench sequence.

## Available instruments and feature scope

The console store is a usable instrument for AF. `read-log` returns the boot's whole message trace: every traced inter-task send with receiver, sender, message id and a tick, and the AF ids sit in the trace's allowlist (byte-read, E-AF-TRACE-MECH-150). In the owner's act-logged boot the trace carried 286 records from the six AF and lens tasks across the boot's first eleven seconds, and none of the three ids that mark a focus search (`0x3b0d` request, `0x3f2e` search start, `0x13a5` result), because a maintenance boot runs no search (hardware-observed, E-AF-LENS-TRAFFIC-150). One tick is 10.01 ms, the measured rate is 99.88 ticks per second, and the AF ids are in the allowlist, so a search that ran in a readable boot would appear as its message sequence at about 10 ms resolution: the §11.1 timeline at message level, without in-session hooks.

The film-simulation unlock is installed and its settings are reported retained after the v5 recovery (E-FILMSIM-INSTALL-150, E-RECOVERY-V5-RETENTION-150). New recipe and detailed AF acceptance remain separate; [the task board](../docs/task-board.md) owns feature priority.

An emulator reference exists ([reference assessment](x70-emulator-reference.md)) and its AF interest overlaps this lane. It is a lead only, labelled and scoped in the X70 section below.

## The lane's questions, in order

**First: what is readable in an idle maintenance boot?** Read the AF state cells and traced sends, and record whether physical shutter input changes them under [the first-session script](af-first-session-script.md). This establishes an idle baseline and the read channel's scope. An unchanged or zero state does not exclude lazy initialization or host-driven imaging jobs, and does not close maintenance-boot AF feasibility.

**Second: what does a focus search do when it runs?** Observe ordinary-boot searches externally: confirmation repeatability, image usability at confirmation, cancellation, full press and request-proxy-to-visible-indication timing. The stock pilot and [session kit](af-session-kit.md) need no new camera bytes. These observations address AF-STATIC-01 and establish the baseline G6 measures against; they do not require an internal trace.

**Third: what carries the search's internal timeline off the body?** The store dates events to about 10 ms and cannot carry a search, for the three reasons in the next section. The question splits into whether any mapped route can drive a search inside a boot whose trace is readable, and, if none can, what instrument replaces the store. The expected answer is a carry path to media, and the tension section names it.

Work order follows the owner's recipe priority. When AF bench work resumes, collect the idle/read-channel baseline and ordinary-boot behavioral block. Separately map each service-job candidate's initialization and terminal outcomes before selecting a discriminating drive experiment. A negative result is scoped to its exact handler, configuration and observation interval. New drive or recorder bytes need the camera-run procedure; one boot remains one sample.

## The trace-versus-search tension

The console store is the only instrument that dates AF events, and it cannot carry a focus search. Three facts hold together.

The store is per boot. Loader A's initializer clears it at every boot (byte-read, E-LOG-STORE-150), and the four captures each begin at slot 0 with the two jig captures sharing no non-zero slot (hardware-observed, E-LOG-PERSIST-150). Whether the per-boot emptiness is the initializer's explicit clear or plain RAM volatility is undetermined (E-LOG-PERSIST-150); both readings agree that nothing crosses a power cycle.

The store is readable only in a maintenance boot. `read-log` is a service-session operation, and the vendor service session exists only in maintenance mode `0xb` (byte-read).

Physical-key-driven focus searches are established only in ordinary boots. In maintenance mode `0xb` the key modal covering S1, S2 and menu codes is not dispatched and ordinary preview does not start (E-SERVICE-SHOOT-150). This closes that key route, not the distinct service imaging-job candidates.

A search performed in an ordinary boot therefore writes its trace into RAM in a boot where no read operation exists, and the next boot overwrites it before any read. The three direct answers follow.

**Does anything survive a boot?** No search content does. What survives is stored configuration and media: the adjustment area's stored FOCUS MODE and AF-area properties, the three AF debug gate bytes at `0x887d`, `0x887e` and `0xe8`, the persisted autofocus evaluation-window parameter store at `0x400000`-`0x40fcff` (E-ADJ-BLOCK-400000-150), and the images and card files the camera wrote. Those are parameters and products, and none of them records what a search did. The firmware's own AF debug text is no escape: the printer is inert on this body, its gates read zero in stored configuration and the text ring is not installed in the captured boots (E-AF-DEBUG-GATE-150), and its output would land in the same per-boot RAM.

**Can a search be driven in a maintenance boot by any mapped route?** No route is established. The imaging-job handlers have distinct prerequisites and must be evaluated separately.

Keys cannot. The key modal is never dispatched in maintenance outer state 10 (byte-read, E-SERVICE-SHOOT-150), so neither the shutter nor a scripted key reaches the AF request path. The AUTO_ACT script's `key` statement posts an application event through `0x72b680` whose mode-`0xb` consumer is unmapped (unproved), and the script engine has not fired on this body at all: its gate test falls about 1 s after task start and the host's arm arrives after it (E-AUTOACT-UNIT-150, E-AUTOACT-NOSEND-150). The facility would in any case be a drive-and-mark instrument here: its `WT_LOG` statement writes a card file and its vocabulary cannot read RAM.

RAM writes cannot. Service opcode `0x200001` function 3 writes RAM byte by byte and is not a closed operation, and a written request field is not a search: the search consumes statistic frames, and a maintenance boot starts no preview and produces none (static-inference, the static map's statistic intake and E-SERVICE-SHOOT-150).

The imaging-job family remains a set of candidates. Host configuration values `0xa0`-`0xac` reach `0x450006` ids `0x12`/`0x263` (E-SERVICE-CAPTURE-PATH-150); `0xac` includes direct saclac requests and `0xa2` uses a distinct job slot. Map initialization, preview/statistic dependencies, focus fields and completion/refusal/cancellation for each before choosing a drive experiment. That experiment needs a closed operation, byte review and the owner's go. A null result excludes only the tested handler under its demonstrated prerequisites, not every readable-boot AF route.

**What instrument does the lane need?** A carry path: firmware-side code that runs inside the ordinary boot and writes the store's records, or the named AF state cells, to media before the boot ends. Recording already happens: the kernel's send hook writes every traced send to the store at about 10 ms (E-AF-TRACE-MECH-150), so the missing piece is only the hop from RAM to media. Two build routes exist, and both are persistent-change work with a byte review, the owner's go naming the operation, and the recorded recovery position.

An installed container patch on the proven RC0 route (patch set, encoder, `//host/firmware:rebuild-update`, stock updater flash; E-UPDATE-REBUILD-150, E-FILMSIM-INSTALL-150) carrying a store-to-card flush at a key event or on a timer.

An owned G4/G5 hook whose retrieval step lands the record on media. A hook that leaves its record in RAM loses it with the boot like everything else.

No ordinary-boot internal timeline is yet observed on this body. The host-only [carry closure](af-observe-instruments-150.md#9-same-boot-carry-closure-over-the-pinned-bytes) binds much of the file ABI; its [revision 2](af-observe-instruments-150.md#99-recorder-revision-2-bounded-scan-attempt-and-result-coverage-moved-placement) carries a retained-cursor bounded scan, attempt identity with no-result and deadline classes, a 36-byte trailer and a file ownership/lock-liveness contract, and it is placed in the plain window's tail padding, disjoint from the D3 prefix. What stays open: the capture is still live and non-atomic and carries messages, not statistic, frame or lens freshness; the stock semaphore `0x3c` waits are inherited and not bounded from the client side; the carrier region is loaded-but-unowned padding; and the emitted bytes, the regenerated fixture and any ordinary-boot observation are pending. No install is offered; [the task board](../docs/task-board.md) owns the remaining contracts.

## The X70 overlap (reported, about the X70)

The owner reports explicit permission from jitl (justjake) to redistribute the X70 emulator with credit (E-X70-REDISTRIBUTION-PERMISSION-150), following the earlier personal permission. [Reference terms](x70-emulator-reference.md#12-the-authors-grant) and [licensing](../LICENSING.md) record the scope without inventing AGPL relicensing or downstream modification terms. Fujifilm's supplied X70 1.10 firmware and per-unit RAM captures are not inputs. Vendored components keep their own licenses. Every device-behaviour statement below concerns X70 1.10, not an X-M1 binding.

Reported about that project: it models snap focus and investigates recording AF traces for emulator replay (`research/af-scenes/` and the project's AF notes). The research overlap offers:

- The record/replay method for pairing a captured search with an emulated device, with implementation available as a local research reference. Its checkpoint-and-suffix-replay loop matches this project's iteration pattern.
- A sibling-derived lead for what an AF-capable model must supply before a search can run in emulation: the scene and lens fixtures under `research/af-scenes/` and the reported snap-focus implementation. Third-party fixture terms still apply where the author is not their rights holder.
- The author's experience with which AF records are worth carrying off a camera, if it is shared. The supplied per-unit RAM captures remain excluded.

X70-keyed device bindings do not become X-M1 facts; an X-M1 AF-layer model needs target decoding or capture. Adapted implementation proposed for public distribution must first resolve the licensing requirements. That review does not supply recorder residency, physical AF timing or camera authorization.

## What this bounds

The static map binds the AF-area input path (§1), the completion graph (§4) and the confirmation path (§5) in code. Code reading cannot supply external behaviour: whether the visible indication is repeatable, how long it takes, whether the image quality at the indication is usable, and what physical cancellation and full press look like from outside. Ordinary photography plus external video supply exactly those and nothing more.

Every camera-visible quantity here has a declared proxy. The externally recorded onset of the half-press is a proxy for the operator's physical action, not for AF-request acceptance inside the body; the first visible confirmation indication is a proxy for the camera's report, not for the internal result. A measured interval therefore reads as request-proxy-to-visible-indication latency, and it contains the unknown internal request delay, the AF decision and the presentation delay together. The static map places at least two message hops and one UI-loop iteration between the AF result and presentation, with no measured display-refresh or OSD-present bound ([static map](autofocus-static-map.md) §5), so no frame-based correction exists and no tight internal-result latency bound is derivable. Internal acceptance, the instant `0x00defa30` runs and the instant `0x13a5` is sent stay unresolved here: without a closed static producer trace for the physical `S1`-to-request step, or synchronized instrumentation, they are not observable this way. The stock pilot protocol and the [machine manifest](../manifests/benchmark-protocol.json) already use these proxies (`start_event`: visible half-press onset proxy, AF acceptance unobserved; `confirmation_event`: first visible confirmation indication; `default_report`: confirmation latency plus independently verified final quality).

## Camera-access observations (question two)

Ordinary photography plus external video reach these. [The stock pilot protocol](stock-pilot-protocol.md) supplies the timing and quality harness; three items ride it and one adds a focused correlation.

| Item | Observable counterpart of the static candidate | Method | Harness |
| --- | --- | --- | --- |
| Confirmation indication | the first visible indication (LCD focus-box change, beep as a secondary marker) is repeatable and bracketable; the map's `0x00defa30` -> `0x13a5` -> beep/green-box chain is not timed and is not identified in the recording | bracket the first visible confirmation indication in external video | stock-pilot confirmation latency |
| Focus quality at confirmation | the search reaches a usable plane at the indicated moment | independent intended-ROI sharpness at the confirmation/deadline capture | stock-pilot quality sweep |
| Cancellation and S2 hand-off | physical cancellation and full-press behaviour: continued hunting, stale confirmation, restart, capture reliability (bounded sequence below) | staged cancel, re-press and full-press procedures with declared windows | stock-pilot recordkeeping; spec/06 §12.1 deadlines and §12.4 coverage |
| Request-proxy-to-indication latency | how long after the visible half-press onset the visible indication appears; not internal acceptance and not `0x00defa30` | external trigger onset to confirmation bracket, reported as an interval | stock-pilot timing |

Row note. The latency is the protocol's interval `[max(0,c0-s1), c1-s0]`, widened outward by the declared synchronization and visibility uncertainty; its temporal resolution is no finer than the measured frame spacing and its ceiling is what the recording can show. Because the presentation delay is unmeasured, a stock value cannot be split into request, decision and presentation parts, and a 20% latency comparison compares the whole external interval. A negative or empty interval means only that the brackets overlap at the recording's frame rate.

### Cancellation and S2: bounded behavioural sequence

A finite supplemental sequence, under the pilot's frozen settings, fixtures and recordkeeping ([the session kit](af-session-kit.md) §3 staging recipe and §8 block carry the executable form), on the P02 fixture after its timed attempts, with its own attempt IDs, trial records and retained artifacts. Every initiated attempt is retained (spec/06 §12.1 finite deadlines and retained attempts; [the protocol](stock-pilot-protocol.md) outcome vocabulary and all-attempt accounting, lines 49-53). These are development data, never held-out data.

Protocol validity and behavioural outcome are separate, and validity is recorded for every attempt:

| Validity | Condition |
| --- | --- |
| `complete` | every bracket and window the procedure needs is readable in one recording, and no frozen rule was broken |
| `indeterminate` | a needed bracket, window or observation cannot be read, or the frame spacing cannot resolve the event |
| `protocol-error` | staging not restaged, recording stopped or restarted inside the scene, camera or stand touched inside a declared window, or another frozen-rule violation; `cancelled` covers an operator interruption. Both are retained as non-success |

A readable attempt is not thereby a pass. The procedures, their timing, and the behavioural classification predeclared for each:

| Procedure | Steps and timing | Declared window | Repeats |
| --- | --- | --- | --- |
| `cancel-search` | stage `far`; half-press and hold; release S1 during visible search, before any confirmation | 2.0 s from the release, camera untouched; then re-press and hold to visible confirmation or the 10 s deadline | 3 |
| `cancel-confirmed` | stage `far`; half-press to visible confirmation; release S1 | 2.0 s from the release, camera untouched; then re-press and hold to visible confirmation or the 10 s deadline | 3 |
| `full-press-confirmed` | stage `far`; half-press to visible confirmation; full press immediately | shutter within 2.0 s of the full press | 3 |
| `full-press-held` | stage `far`; half-press to visible confirmation; hold the confirmed state 1.0 s; full press | shutter within 2.0 s of the full press | 3 |
| `full-press-late` | stage `far`; half-press to visible confirmation; hold 1.0 s; release S1 fully; wait 1.0 s; half-press again and hold to visible confirmation or the 10 s deadline; hold the confirmed state 1.0 s; full press | shutter within 2.0 s of the full press | 3 |

| Behavioural classification, predeclared | Pass | Fail |
| --- | --- | --- |
| `full-press-confirmed`, `full-press-held`, `full-press-late` | the shutter fires within 2.0 s of the full press (the kit's `s2_press` to `capture` bracket), the whole sequence stays inside the pilot's 10 s acquisition deadline from the first trigger, and the saved image is valid (no visible refocus after the full press) | timeout, no image, an invalid capture, or a visible refocus after the full press |
| `cancel-search`, `cancel-confirmed` | no aggregate pass: the map leaves the expected cancellation behaviour open, so an aggregate would encode an assumption. Each observation is classified and reported on its own: `continued_hunting` (visible focusing motion continues inside the release window), `stale_confirmation` (a confirmation indication appears inside the release window after the release), `repress_outcome` = `confirmed` (the re-press reaches visible confirmation), `deadline` (no confirmation before the 10 s deadline) or `none` (no re-press) | no aggregate fail; the three observations above stand alone |

Recorded per attempt: the `s1_release`, `s2_press` and `capture` brackets, the validity, the three cancellation observations where the procedure has them, whether the next half-press restarts or resumes the search (`focus_restart_observed`), whether the shutter fired on time, the saved image, and the trial `outcome`. Every attempt's record keeps the brackets and the validity so an adverse but readable outcome stays discoverable. These are physical cancellation and full-press behaviours. The map leaves the binding of `0x00df1d8c`/`0x00df2200` to physical S1 release, and the complete cleanup propagation, open; the two sites were re-checked as request wrappers only (`0x00df1d8c` -> `0x00ef9370` with `r0` = 0, `0x00df2200` -> `0x00ef9320`), so no attempt is read as invoking them and no internal cleanup claim is made in either direction.

### Settling-versus-confirmation correlation

The map separates the predicted callback `0x00ff9178` from the lensdc completion `0x00ff5d80` -> `0x3f0d`, but not whether completion means a stopped, optically settled lens, and presentation is deferred (§5). A plain-camera experiment can correlate two visible things; it cannot infer internal confirmation or optical settling from their order, and a displayed indication trailing visible motion is compatible with a result produced while the lens was still moving.

Reference acquisition and staging follow [the session kit](af-session-kit.md) §3 and §5, which are authoritative for this lens: it has no focus ring, so the reference is acquired by stepping the target plane while the camera focuses with its own AF and S1 is held across the geometry change, and no manual or ring-position recipe applies here. Step 0, before any settling attempt: establish that a visible feature tracks the focusing motion. Record a short high-frame-rate pass over the near and far marks with that staging recipe and check whether the chosen feature (lens front edge, barrel seam, any visible scale) moves with the plane changes and is resolvable at that frame rate; this lens's barrel may move not at all. If no feature tracks, or the motion is not resolvable, record focusing motion as unobservable, report this correlation as unresolved and stop the block.

Then, per direction (near-to-far, far-to-near), stage and trigger exactly like a timed attempt and keep the barrel and the LCD in one recording clock. Annotate the last frame with visible motion and the first without as `[m0,m1]`, and the confirmation bracket as `[c0,c1]`, and use the kit's authoritative relation definition ([session kit](af-session-kit.md) §11 barrel relation and the annotation tool): the signed confirmation-minus-motion-end interval `[c0-m1, c1-m0]` widened by the declared uncertainty, with `trails` when strictly positive, `leads` when strictly negative, `overlaps` when the brackets interleave, and `unresolved` when uncertainty leaves the sign spanning zero. Report the widened interval and the relation per direction. Overlap, `unresolved` and a trailing indication are indeterminate order or a correlation only: never evidence that the lens was unsettled or that the internal result preceded the motion, and never evidence of settling. Never state a settling relation across a bracket too coarse for the interval of interest; below that resolution the result is unresolved.

If optical settling is to remain an objective, it needs a validated continuous measure rather than frame brackets: an intended-ROI/plane stability metric (the frozen acutance of the reference acquisition, extended to a continuous series), its quality domain, acceptance threshold, observation window, clock and synchronization uncertainty, and the frame-rate resolution it requires, all validated against known staged defocus before use. Until such a measure is validated, the relation of the confirmation indication to `0x3f0d`, to the internal result and to optical settling stays unmeasured; dating any of those requires synchronized internal events and therefore G4/G5, or the carry path of the tension section.

## G4/G5-gated internal bindings (question three)

Camera access alone does not reach these. They need owned hooks (reversible execution, exact target bytes, recovery scope), so they are gated on G4/G5 and are not part of a plain camera session. The store trace and the RAM read make a maintenance boot readable at the message and state level, as [the instrument report](af-observe-instruments-150.md) surveys, but that boot runs no search; the carry path the tension section names would date an ordinary boot's search, and until it exists these bindings stay gated. The map names hook candidates in §11.1; the last row below adds the confirmation-chain sites the static map reads, as observation candidates only. No instrumentation design is supplied.

| Item | Static claim to settle | Hook candidate |
| --- | --- | --- |
| S1/state-cell producer | the physical `S1` writes the request field | `0x00d0b318`, `0x00df48dc` |
| Statistic cadence and post-move freshness | when a statistic frame becomes valid for a new lens state | `0x00f40944`, `0x00f44fec`, `0x00ff6b4c` |
| Direct completion timing | when `0x3f0d` fires relative to motion | `0x00ff5d80`, `0x00ff9178` |
| Physical lens units | the units of `0x00942b84` and the step arithmetic | `0x0076a270`, `0x006bc8d8` |
| Internal result to presentation | the queue hops and UI-loop iteration the static map reads | `0x00defa30`, `0x00dca604`/`0x00e00a80`, `0x00c22f60`, `0x00d17370` |

### Resolved statically, not gated

The completion-to-callback linkage is closed by static trace: `0x3f0d` reaches `0x00defa30` by relay through `0x00f3c14c` and message `0x3b46`, not by pointer identity ([static map](autofocus-static-map.md) §5). Only the linkage is closed. When `0x3f0d` fires relative to motion, when the callback runs relative to presentation, and whether completion means a settled lens remain unmeasured, so this item needs no camera session and adds no timing claim.

## Evidence and gates

Each camera observation records a `hardware-observed` entry bound to the behavioural counterpart of the static claim it bears on, under the camera-run procedure. Ordinary photography and external video need no service bytes; any service command needs a reviewed byte plan first. This plan advances no gate on its own. The camera-access observations feed AF-STATIC-01's behavioural closure. Their contribution to the §11.1 timeline requirement is limited to the external endpoints: the visible half-press onset proxy, the first visible confirmation indication, the capture, and the bracketed intervals between them. Nothing here dates an internal event, and the internal bindings stay behind G4/G5. The completion-to-callback linkage is resolved statically and needs no session. This revision adds no observation and advances no gate; its durable claim is recorded as E-AF-TRACE-TENSION-150.
