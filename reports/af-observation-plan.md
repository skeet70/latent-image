# Autofocus observation plan for the 1.50 static map

Status: offline plan, `static-inference`. No camera authorization or gate acceptance. It specifies behavioural observations for the [1.50 static map](autofocus-static-map.md), what each instrument reaches and the ordering of questions. [The camera-run procedure](../spec/07-governance-and-handoff.md#31-procedure) owns authorization; [the instrument report](af-observe-instruments-150.md) owns in-body capabilities; [the first-session script](af-first-session-script.md) owns the proposed bench sequence.

The RAM-push and bulk-read results now supply a same-boot transient alternative to the historical media-carry route below. [The first transient ring design](#first-transient-g5-ring-loop) owns the new implementation and its narrower observation scope.

## Available instruments and feature scope

The console store is a usable instrument for AF. `read-log` returns the boot's whole message trace: every traced inter-task send with receiver, sender, message id and a tick, and the AF ids sit in the trace's allowlist (byte-read, E-AF-TRACE-MECH-150). In the owner's act-logged boot the trace carried 286 records from the six AF and lens tasks across the boot's first eleven seconds, and none of the three ids that mark a focus search (`0x3b0d` request, `0x3f2e` search start, `0x13a5` result), because a maintenance boot runs no search (hardware-observed, E-AF-LENS-TRAFFIC-150). One tick is 10.01 ms, the measured rate is 99.88 ticks per second, and the AF ids are in the allowlist, so a search that ran in a readable boot would appear as its message sequence at about 10 ms resolution: the §11.1 timeline at message level, without in-session hooks.

The film-simulation unlock is installed and its settings are reported retained after the v5 recovery (E-FILMSIM-INSTALL-150, E-RECOVERY-V5-RETENTION-150). New recipe and detailed AF acceptance remain separate; [GitHub Issues](https://github.com/skeet70/latent-image/issues) owns feature priority.

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

No ordinary-boot internal timeline is yet observed on this body. The host-only [carry closure](af-observe-instruments-150.md#9-same-boot-carry-closure-over-the-pinned-bytes) binds much of the file ABI; its [revision 2](af-observe-instruments-150.md#99-recorder-revision-2-bounded-scan-attempt-and-result-coverage-moved-placement) carries a retained-cursor bounded scan, attempt identity with no-result and deadline classes, a 36-byte trailer and a file ownership/lock-liveness contract, and it is placed in the plain window's tail padding, disjoint from the D3 prefix. What stays open: the capture is still live and non-atomic and carries messages, not statistic, frame or lens freshness; the stock semaphore `0x3c` waits are inherited and not bounded from the client side; the carrier region is loaded-but-unowned padding; and the emitted bytes, the regenerated fixture and any ordinary-boot observation are pending. No install is offered; [GitHub Issues](https://github.com/skeet70/latent-image/issues) tracks the remaining contracts.

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

## First transient G5 ring loop

Status: host-only implementation and proposed experiment, not G5 acceptance. [#72](https://github.com/skeet70/latent-image/issues/72) owns the loop; [#102](https://github.com/skeet70/latent-image/issues/102) owns actual emulator half-press/producer inputs; [#103](https://github.com/skeet70/latent-image/issues/103) owns independent review and live-run readiness.

### Content and publication contract

`menu/af_ring_payload.c` and `menu/af_ring_hooks.S` link with the unchanged shared installer and ABI. `BuildAfRingRam.hs` and `build-af-ring-ram.sh` use the existing pinned relink toolchain and manifest. The earlier flash recorder's failed boot, E-RECORDER-BOOT-FAILURE-150, and unstaged target, E-AF-RECORDER-RESIDENCY-EMU-150, remain failures of that recorder. This payload has no boot-path or media hook.

The `0x6800`-byte allocation contains content text in `[base,base+0x1000)`, the installer at `align32(base+0x1000)`, and a `0x2440`-byte ring at `base+0x4000`. The linker asserts each extent. Ring stores use the `0x40000000` alias, mapped as normal shareable non-cacheable DRAM in E-XM1-MPU-BOOT-RECORD-150. That is a static visibility basis, not a silicon observation.

| Kind | Entry | Replayed first word | Replayed second word | Continuation |
| --- | --- | --- | --- | --- |
| 1, direction decision | `0x00f41c48` | `0xe92d48f0` | `0xe28db014` | `0x00f41c50` |
| 2, peak decision | `0x00f42abc` | `0xe92d4870` | `0xe28db010` | `0x00f42ac4` |
| 3, accepted-statistic consumer | `0x00f44fec` | `0xe92d49f0` | `0xe28db018` | `0x00f44ff4` |

These X-M1 1.50 boundaries come from the [static map](autofocus-static-map.md). An entry is not a completed decision, fresh exposure or optical result. The builder pins image 5 to SHA-256 `3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a` and requires the exact prologues. Each wrapper saves `r0` through `r12`, `lr` and CPSR, calls the integer-only recorder, restores that state, replays the two PC-independent prologue words and resumes at the continuation. The content uses ARMv7-R ARM state and the soft-float ABI.

Six publications use installer ABI 5. Setup stores each wrapper address at entry+4, then activation stores `0xe51ff004` (`ldr pc,[pc,#-4]`) at each entry. Reverse rollback restores activation before setup. These redirects are new mechanism-adjacent bytes. Existing bank and Classic Chrome review pins do not approve them.

`menu/af_ring_allowlist.json` owns the three continuations and source-reviewed store declarations. `AfRingAllowlist.hs` checks allocation sections, admitted functions, branch closure and closed CPU-instruction forms. Indexed ring stores and synthetic callback output cells remain declarations rather than proved address provenance. A passing check does not replace their independent byte review.

### Ring and capture semantics

The little-endian 64-byte header is `{magic=0x31524641,version=1,header_bytes=64,event_bytes=36,capacity=256,committed,overwritten,active,core=0x4003,contention_loss,reserved[6]=0}`. Each 36-byte event is `{sequence,kind,r0,r1,r2,r3,search_state,statistic,core_identity}`. Every accepted event names core `0x4003`. The snapshots read `0x016c0fc0` and `0x016c1160`; their meaning and freshness remain bounded by the static map.

Only `mrc p14,0,Rt,c2,c0,0 == 0x4003` reaches a ring access. Other core identities return before the local try-lock. CPU0 attempts one LDREX/STREX on `active`, with no retry. Failure latches `contention_loss=1` and returns. The owning writer never clears that flag. The dependency is the CPU0 local exclusive monitor, not a global monitor or cross-core lock. The recorder makes no interrupt-mask or CPU-control-register write; wrappers restore condition flags without writing CPSR control bits.

The AF task creator `0x00ef8c90` registers saclac task `0x2b` with entry `0x00f01bc8` through `0x00725dd8`. That API reads kernel selector `*(0x007cad9c+(0x2b-1)*4)=1`, then dispatches through service-vector table `0x0096bd20`. In the existing run-3 capture, selector 1 names `0x4004dae0` and selector 2 names `0x400ddaa0`, the primary/secondary loader vectors; the primary task-create slot at vector+`0x80` is `0x0004d39c`. This binds the orchestration task to the primary-kernel service, not every possible caller of the three hooked functions. Runtime CPU0 filtering is the confinement boundary even if another caller exists.

The emulator receipt explicitly flags that the synthetic path exercises uncontended CPU0-local exclusive access. Contention/preemption behavior is not established by that run. Global exclusive-monitor behavior is neither established nor required. The reviewer must check that this distinction holds for the emitted instructions and emulator scheduling model.

Sequence numbers begin at 1 and choose slot `(sequence-1)&255`. The writer sets `active`, invalidates the slot sequence, fills the event, publishes its sequence and overwrite count, publishes `committed`, then clears `active`, with DMB ordering. At `committed=0xffffffff`, event mutation stops. The invariant is `overwritten=max(0,committed-256)`. Overflow therefore remains visible.

The separate host-emulation query entry `af_ring_synthetic` accepts 1 through 1024 events. Kind `0x80000001` carries arguments `{count,0x53594e54,index,0}` and cannot establish AF execution. Its output cells receive the ring alias and extent.

The Haskell decoder validates extent, header, reserved words, writer state, overwrite arithmetic, event kinds and retained slot sequences. It labels results best-effort and non-coherent. A DMA can overlap a writer after reading the header; slot validation does not make the copy atomic. There is no timestamp or cross-core completeness claim.

### Host and emulator loop

`FujiAF.AfRing.readAfRing` accepts a validated manifest/allocation/installed state and supplied `Clock`/`UsbApi`, derives the ring window, pulls exactly `0x2440` bytes through existing `runReadBulk`, then decodes the saved capture. It adds no live raw-command path. Offline decoding uses:

```sh
devenv shell -- bazel run //host/firmware:read-af-ring -- decode RING.bin [OUT.json]
```

`//emulation:xm1-af-ring-e2e` holds an emulator slot, uses a keyed ordinary-boot attach checkpoint, allocates/pushes with the shared installer, flags the unsupported half-press input, bulk-reads/decodes and unloads. The retained bridge lacks a half-press `MenuKey`; sending it terminated the bridge in `af-ring/e2e-fifth`, whose exact rejection remains in `bridge/emulator-session.json`. The explicit fallback calls the real synthetic recorder through the emulated query callback, emits 300 events and requires 256 retained synthetic events with overwrite accounting. [#102](https://github.com/skeet70/latent-image/issues/102) separates missing actual input/producer support from ring publication coverage.

```sh
XM1_EMULATOR_SLOTS=8 devenv shell -- bazel \
  --output_base="$HOME/.cache/bazel/latent-image-AfLoop" run \
  --jobs=8 --local_resources=memory=6144 //emulation:xm1-af-ring-e2e -- \
  --firmware PRIVATE_RC0_DAT --installed-nand PRIVATE_F7_NONZERO_NAND \
  --sd-card-image PRIVATE_CARD_IMAGE --receipt-output PRIVATE_NEW_OUTPUT
```

All inputs and receipts stay under `private-data/`. The completion receipt distinguishes `ordinary-af` from `synthetic-fallback`. Only the former could support an emulated AF observation claim; neither is hardware evidence.

### Independent review brief

Re-derive the prologues, six publications, activation/rollback order, continuations and complete register/flags/SP preservation. Explicit review question: does the CPU0 guard precede every ring and exclusive access, so the local monitor suffices without any cross-core exclusivity assumption? Inspect the primary-kernel task binding, local monitor behavior under preemption, sticky contention loss, alias ownership and every declared ring/output-cell store. Cover the actual-base relink, decoder rejection behavior and full-system push/read/decode/unload. The stock installer review does not close these items.

The AF payload has no approved review record or compiled AF pins. Live `ram-relink`, `ram-push` and `ram-unload` refuse it. [#103](https://github.com/skeet70/latent-image/issues/103) tracks the independent review and reviewed-live registration. Host-only unreviewed emulation remains available.

### Camera-run proposal, not authorization

After independent review and candidate checks, seek one owner H2 go naming the AF source identity and actual-base digest, fresh allocation/relink/install, one bounded manual half-press, ring read and power-removal cleanup. The asleep-owner assignment performs none of these operations. Stored nonzero `0xf7` and ordinary photography-capable ff80 must already exist.

1. Freeze the actual lens, focus mode, target, light, exposure and deadline, then collect the stock behavioral baseline. Idle RAM does not establish a statistic producer or usable lens.
2. At the idle shooting screen, perform the existing RAM-push scalar/worker preflight and compare all six old hook words. Require the stock query callback and unchanged installer preconditions.
3. Allocate `0x6800` once, relink for the returned base, pass the AF check and actual-base review, then install once. Retain manifest, runtime, outgoing journal and mailbox.
4. Read the initial ring, have the owner half-press/release once inside a frozen 10-second observation window, then read once. Retain empty/overflowed outcomes. Do not call the synthetic entry on the body.
5. Any unexpected reply, delay, display behavior or disconnect stops the sequence. An uncertain bulk chunk forbids another command until USB is unplugged, power removed and ordinary operation confirmed.
6. Remove USB and battery/dummy power for at least ten seconds. A switch cycle alone did not clear an earlier bank push. After owner-confirmed startup, separately authorized reads compare all six stock words and the callback slot. Never reuse the allocation state.

A first passing sample establishes observation of named CPU0 boundaries, not G5 overhead acceptance. Freeze a later stock-versus-instrumented overhead bound from stock variability, retaining failed and cancelled attempts and comparing the same external outcomes. This ring supplies ordered snapshots rather than a measured timeline. The freshness, optical behavior and cancellation questions in #2/#49 remain open.

### Count-specialized installer bytes and review question

The shared source and ABI remain unchanged. They define publication, setup, activation and ABI values as compile-time macros in the same translation unit, without LTO. The AF tuple is `(6,3,3,5)`; the converged bank tuple is `(253,167,86,2)`. AF requires installer-relative entries `0x600`, `0x834`, `0x860`, code extent `0xa80`, and complete installer extent `0xbe0`. Existing payload entries and reviewed pins remain unchanged. The AF builder validates these AF-specific extents and entries.

An isolated comparison compiled the unchanged installer with the bank's exact pinned compiler flags, linked it at the AF comparison address `0x00a82d00` with identical runtime bounds, and extracted `.ram_push.*`. It is a controlled build comparison, not a camera extraction. `cmp installer-253.bin installer-af.bin` returned 1, first difference at byte 57. Both files remain under `private-data/analysis/x-m1-1.50/af-ring/`.

| File | Bytes | SHA-256 |
|---|---:|---|
| `installer-253.bin` | 7008 | `64009309819378629a024642aac3429770176caa94c6f8d56f460f9421c2a898` |
| `installer-af.bin` | 3040 | `562bdb3dfbe1db02b801a4f394ee0e0e626983756efca0e2c597f0868472cc0f` |

The following exact instruction differences are section-relative in the two relocatable objects:

| Function and offset | 253 build | AF build |
|---|---|---|
| `synchronize_current_core+0x10` | `e35400fd`, `cmp r4,#253` | `e3540006`, `cmp r4,#6` |
| `execute_operation+0x44` | `e3a01002`, `mov r1,#2` | `e3a01005`, `mov r1,#5` |
| `execute_operation+0x84` | `e35000fd`, `cmp r0,#253` | `e3500006`, `cmp r0,#6` |
| `execute_operation+0x9c` | `e35000a6`, `cmp r0,#166` | `e3500002`, `cmp r0,#2` |
| Rendezvous count setup | `+0x114: e3a020fd`, `mov r2,#253`; `+0x134: e58d2014`, `str r2,[sp,#0x14]` | The existing `+0x138: e3a00006`, `mov r0,#6`, also supplies the count; `+0x150: e58d0014`, `str r0,[sp,#0x14]` |

Register allocation in that setup also changes zero from `r3` to `r2`, and the fixed 32-byte argument from `+0x140: e3a03020` to `+0x114: e3a03020`. The AF function is four bytes shorter. Branch displacements, literal-load displacements and relocated addresses consequently differ. Publication table length and following data addresses also differ. No byte-identity claim is made.

Reviewer question: after accounting for relocation, branch displacement and data-layout consequences, are the executable differences only count and ABI immediates plus the `r0` reuse and associated register allocation, with control flow equivalent? Review this alongside the CPU0 confinement and local-exclusive-monitor question above. The camera proposal remains blocked on this independent review.

### Exercised host and emulator result

E-AF-RAM-RING-E2E-150 records the successful `e2e-sixth` run on 2026-10-25. It restored the keyed ordinary-boot checkpoint, allocated at `0x00a81d00`, relinked runtime `fef60cf4565651f061a47577074679b85a3d38a2597b7e2464177528a7de80de`, installed six publications, and bulk-read a pristine ring. The synthetic callback then committed 300 events. The Haskell decoder returned the latest 256 CPU0 events, 44 overwritten events, overflow set and no observed contention loss. Mandatory unload completed before `e2e-complete.json` was written.

The run did not supply a half-press and did not observe genuine AF. `e2e-fifth/bridge/emulator-session.json` preserves `ValueError: Unknown menu key: half-press`; that rejected request terminates the retained bridge. The successful runner flags the missing binding without issuing it. The synthetic callback executes the real ring writer and does not seed ring bytes from the host. This proves a repeatable host/emulator publication and capture loop, not G5's physical AF or overhead gate.

Earlier failed attempts remain in the private `af-ring/e2e-first` through `e2e-fifth` directories. They exposed a missing private build directory, an unsafe mode on the agent-created artifact parent, the unthreaded host builder's blocked hash input pipe, the count-specialized installer extent, a symbol-list column mismatch, and the unsupported half-press request. The implementation fixes these host issues or explicitly flags the missing emulator input. `installer-{253,af}.bin` remain separate controlled-comparison evidence.

Final host verification passed `//transport:af-ring-tests`, `//transport:read-bulk-tests`, `//transport:ram-push-tests` and `//transport:cli-rejections`, and built `//host/firmware:build-af-ring-ram`, `//host/firmware:read-af-ring` and `//emulation:xm1-af-ring-e2e`. The integrated entry-contract test now covers AF's independently bound offsets alongside the unchanged existing payload offsets. The emulator command above completed with the supplied RC0 DAT, stored-F7 NAND and virtual card under `private-data/`. No camera or USB access occurred.
