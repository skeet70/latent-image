# Autofocus observation plan for the 1.50 static map

Status: approved research direction, not camera authorization or gate acceptance. The [AF specification](../spec/05-autofocus.md#110-stock-first-sensor-backed-iteration) owns the process; this report owns observation order and current instrument limits. The [first-session procedure](af-first-session-script.md), [pilot protocol](stock-pilot-protocol.md) and [session kit](af-session-kit.md) separate feasibility, measurement and owner actions.

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
