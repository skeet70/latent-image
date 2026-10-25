# Autofocus observation plan for the 1.50 static map

Status: approved research direction, not camera authorization or gate acceptance. The [AF specification](../spec/05-autofocus.md#110-stock-first-sensor-backed-iteration) owns the process; this report owns observation order and current instrument limits. The [first-session procedure](af-first-session-script.md), [pilot protocol](stock-pilot-protocol.md) and [session kit](af-session-kit.md) separate feasibility, measurement and owner actions.

The RAM-push and bulk-read results now supply a same-boot transient alternative to the historical media-carry route below. [The first transient ring design](#first-transient-g5-ring-loop) owns the new implementation and its narrower observation scope.

## Available instruments and feature scope

Ordinary-boot RAM access is hardware-observed with stored `0xf7 = 1`, including bounded `read-bulk` transfers while cardless at the shooting screen (E-BULK-READ-RUN1-150, E-BULK-READ-RUN2-150, E-BULK-READ-RUN3-150). Their [report](ff80-bulk-read.md) owns exact bytes, rates and exit checks. This removes the maintenance-only premise of E-AF-TRACE-TENSION-150. It does not establish AF operation with USB attached, a coherent image buffer or a recorded ordinary AF search.

The existing console store admits AF request, lens and result messages (E-AF-TRACE-MECH-150). The characterized tick is about 10 ms; its source/rate must remain bound to the measured operating condition. It records message sends, not handler execution or sensor acquisition. The [console decoder](console-trace-instrument.md) owns layout, wrap and non-atomic-read limits. Maintenance captures show boot traffic, not a search (E-AF-LENS-TRAFFIC-150).

The retained emulator has a host image-source callback, while its X-M1 statistic producer generates declared synthetic data. An external picture alone does not make its AF statistics camera-derived. Actual X-M1 half-press and AF input coverage remain tracked in [#102](https://github.com/skeet70/latent-image/issues/102). Sibling AF fixtures and timing are not X-M1 evidence.

## The lane's questions, in order

1. **Can the ordinary-boot stock search run while its observations remain readable?** Prepare decoders from existing captures, then use a separately approved small ordinary-AF session with post-attempt trace reads in the same boot. If USB inhibits AF, establish a safe same-boot alternative before relying on attachment changes. Do not revive the persistent recorder merely to retrieve events already in RAM.
2. **Can one fresh image/statistic observation be exported coherently?** Bind the active producer, format, source extent, completion and ownership. Include an obvious scene change to distinguish fresh sensor output from stale or synthetic bytes. A large non-atomic dump or a plausible picture is insufficient.
3. **Which missing events require a hook?** Add only the observations needed to distinguish acquisition, delivery, accepted statistics, lens request, actual completion and optical settling. The first AF ring's ordered entries and state snapshots do not by themselves supply those times or freshness.
4. **Can bounded stock lens operations automate measurements?** Validate units, safe bounds, completion, cancellation and one-controller ownership before reference sweeps or a host-controlled algorithm. Use small records/crops in the feedback loop and larger images between attempts.
5. **Can replay and hardware comparisons distinguish candidates?** Gather reusable measured stacks, calibrate only ranking-relevant model behavior, and reserve scenes/sessions before tuning. Final speed is measured with local on-camera control, not inferred from host USB timing.

## The trace-versus-search tension

The store remains per boot and must be extracted before its initializer clears it. What changed is readability: the ordinary-boot transport now exists. The earlier conclusion that every search requires a RAM-to-card carry is superseded; the proposed same-boot trace experiment remains unperformed.

The old persistent recorder is host-only/non-installable under [#50](https://github.com/skeet70/latent-image/issues/50) and is off the AF critical path. Preserve its useful ABI/residency findings and the repaired incident's evidence. Do not reflash to remove inert residue or assume the installed plain image is pristine; [RAM calibration](ram-calibration.md) records the bounded installed-byte differences.

## Observation boundaries

| Question | Preferred observation | What remains separate |
| --- | --- | --- |
| Search request, lens traffic and result sequence | Existing stock trace read after the attempt | Sends are not execution timestamps; wrap, missing records and coarse timing remain explicit. |
| Frame/statistic freshness | Completed producer buffer or reviewed owned snapshot with frame/configuration identity | A checksum, stable pointer or repeated header does not prove DMA coherence. |
| Lens completion | Bind the lensdc completion callback and associate acquisition-time state | The early predicted callback is not motion completion; completion is not automatically optical settling. |
| Independent focus quality | Intended-plane image ROI against a repeated reference sweep in the same processing domain | AF's own scalar score cannot validate itself; preview does not establish full-resolution photo quality. |
| Actual acquisition latency | Independently validated continuous quality with synchronized request/acquisition clocks | Otherwise report confirmation latency plus final quality or an interval. |
| Cancellation and capture compatibility | Bounded stock/candidate behavioral checks with all attempts retained | Cardless preview cannot accept ordinary capture/save/playback. |

The [snapshot/session contract](../spec/08-artifact-and-runtime-contracts.md#camera-snapshots-and-live-experiment-sessions) owns buffer lifetime, overflow, cache visibility, bounded actions, host loss and checkpoint invalidation. The [benchmark specification](../spec/06-benchmarks.md) owns quality, clock uncertainty and failure accounting.

## Cardless research and optional full-resolution capture

Use cardless preview and statistics first. SD-slot repair is not a prerequisite for those observations or offline algorithm work; the unavailable recovery route is a run-specific risk and ordinary storage acceptance remains deferred under [#85](https://github.com/skeet70/latent-image/issues/85).

If preview cannot support independent quality, investigate a bounded fresh capture-to-RAM path with classified pixels, finite buffer ownership, complete cleanup and normal preview restoration. A no-card-guard bypass or global successful-write stub is not that path. The service imaging jobs in [#39](https://github.com/skeet70/latent-image/issues/39) remain alternatives with unproved exposure and adjacent storage hazards, not the default preview experiment.

## G4/G5-gated internal bindings (question three)

Existing closed reads need their operation-specific scope, not a new hook merely because an event is internal. A new snapshot hook, AF ring or lens-control mechanism needs G4-relevant ownership/ABI evidence, independent review and a scoped owner go. The [static map](autofocus-static-map.md) supplies candidates, not permission to call them.

| Binding | Candidate source sites | Missing evidence |
| --- | --- | --- |
| Request/state producer | `0x00d0b318`, `0x00df48dc` | Actual S1-to-request path and cancellation state |
| Statistic intake/freshness | `0x00f40944`, `0x00f44fec`, `0x00ff6b4c` | Source frame, exposure, delivery and acquisition-time lens state |
| Predicted versus completed movement | `0x00ff9178`, `0x00ff5d80` | Physical timing and optical settling |
| Lens coordinates and bounded operations | `0x0076a270`, `0x006bc8d8` | Units, safe operating range and observed movement contract |
| Result versus presentation | `0x00defa30`, recorder and UI consumers | Presentation delay, not inferred from one UI iteration |

## Evidence and gates

Record new durable observations with their body/firmware/lens, source domain, clocks, uncertainty and limitations. Existing transport evidence does not accept a new frame capture, actual emulator AF, G5 or G6. A proposed bridge is not implemented by this design. No camera access is authorized here; the [camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs) remains authoritative.

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
