# Autofocus observation plan for the 1.50 static map

Status: approved research direction, not camera authorization or gate acceptance. The [AF specification](../spec/05-autofocus.md#110-stock-first-sensor-backed-iteration) owns the process; this report owns observation order and current instrument limits. The [first-session procedure](af-first-session-script.md), [pilot protocol](stock-pilot-protocol.md) and [session kit](af-session-kit.md) separate feasibility, measurement and owner actions.

RAM push and bulk read supply a same-boot transient route. The [cardless admission stage](#stage-1-publication-and-review-brief) is the selected implementation work; the [earlier event ring](#first-transient-g5-ring-loop) remains rejected and supplies no camera-ready observation mechanism.

## Available instruments and feature scope

Ordinary-boot RAM access is hardware-observed with stored `0xf7 = 1`, including bounded `read-bulk` transfers while cardless at the shooting screen (E-BULK-READ-RUN1-150, E-BULK-READ-RUN2-150, E-BULK-READ-RUN3-150). Their [report](ff80-bulk-read.md) owns exact bytes, rates and exit checks. This removes the maintenance-only premise of E-AF-TRACE-TENSION-150. It does not establish AF operation with USB attached, a coherent image buffer or a recorded ordinary AF search. The first stock-first attempt instead encountered the card-error indication on S1 (E-AF-CARDLESS-S1-BLOCKED-150); [the cardless result and options](#cardless-s1-result-and-decision-options) now gate an ordinary AF trial.

The existing console store admits AF request, lens and result messages (E-AF-TRACE-MECH-150). The characterized tick is about 10 ms; its source/rate must remain bound to the measured operating condition. It records message sends, not handler execution or sensor acquisition. The [console decoder](console-trace-instrument.md) owns layout, wrap and non-atomic-read limits. Maintenance captures show boot traffic, not a search (E-AF-LENS-TRAFFIC-150).

The retained emulator has a host image-source callback, while its X-M1 statistic producer generates declared synthetic data. An external picture alone does not make its AF statistics camera-derived. Actual X-M1 half-press and AF input coverage remain tracked in [#102](https://github.com/skeet70/latent-image/issues/102). Sibling AF fixtures and timing are not X-M1 evidence.

## The lane's questions, in order

1. **Can the ordinary-boot stock search run while its observations remain readable?** Resolve the observed cardless S1/card-error gate before another ordinary-AF attempt, then use a separately approved small session with post-attempt trace reads in the same boot. USB-attached AF remains unestablished independently of the storage gate. Do not revive the persistent recorder merely to retrieve events already in RAM.
2. **Can one fresh image/statistic observation be exported coherently?** Bind the active producer, format, source extent, completion and ownership. Include an obvious scene change to distinguish fresh sensor output from stale or synthetic bytes. A large non-atomic dump or a plausible picture is insufficient.
3. **Which missing events require a hook?** Add only the observations needed to distinguish acquisition, delivery, accepted statistics, lens request, actual completion and optical settling. The first AF ring's ordered entries and state snapshots do not by themselves supply those times or freshness.
4. **Can bounded stock lens operations automate measurements?** Validate units, safe bounds, completion, cancellation and one-controller ownership before reference sweeps or a host-controlled algorithm. Use small records/crops in the feedback loop and larger images between attempts.
5. **Can replay and hardware comparisons distinguish candidates?** Gather reusable measured stacks, calibrate only ranking-relevant model behavior, and reserve scenes/sessions before tuning. Final speed is measured with local on-camera control, not inferred from host USB timing.

## The trace-versus-search tension

The store remains per boot and must be extracted before its initializer clears it. What changed is readability: the ordinary-boot transport now exists. The earlier conclusion that every search requires a RAM-to-card carry is superseded. The first same-boot trace attempt reached a valid baseline but stopped at the cardless S1 error without post-attempt reads; it did not capture a search.

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

The [X-M1 snapshot-producer findings](xm1-snapshot-producers.md) distinguish captured preview/statistic source bytes from coherent exports and bind a completed AF-result copy point. The existing idle Run-3 capture retains no completed AF worker result. A new hook remains conditional on stock-buffer lifetime, acquisition provenance and independent mechanism review.

## Cardless research and optional full-resolution capture

The owner has selected a transient cardless shooting mode while broken pin 2 awaits port replacement. Stage 1 admits ordinary still S1 past CARD ERROR without changing unrelated inhibitions; stage 2 separately completes no-save captures, initially by discarding the frame. The physical slot, SD recovery and ordinary save/playback remain unavailable under [#85](https://github.com/skeet70/latent-image/issues/85). The owner approved the spec §11.0 amendment as written on 2026-10-26; [§3.4 records that policy approval](../spec/07-governance-and-handoff.md#34-standing-confirmations), not camera authorization.

The implementation is tracked in [#113](https://github.com/skeet70/latent-image/issues/113). AF admission does not supply a fresh exposure, classified pixels, finite ownership, save completion, cleanup or preview restoration. The [stage-2 design](cardless-capture-feasibility.md) incorporates [#105](https://github.com/skeet70/latent-image/issues/105); the service imaging jobs in [#39](https://github.com/skeet70/latent-image/issues/39) remain separate diagnostic paths with adjacent storage hazards. A global successful-write stub or media-ready fiction is not the selected design.

## Cardless S1 result and decision options

### Hardware observation

E-AF-CARDLESS-S1-BLOCKED-150 records the first stock-first block on 2026-10-25. The baseline met every required control value and contained 1245 contiguous console records, below the 7164-slot admission bound. The control snapshots' next indices were 1241 and 1249, so the extraction is not an atomic snapshot. At the requested ordinary half-press, the owner reported: “I can't, half press blinks the RED led, because of the card error.” Stock AF did not run according to that report. The block stopped without post-attempt reads or further camera commands.

This is hardware-observed card-error behavior, not a captured instruction trace or proof that no internal AF message was emitted. There is no post store to associate new messages with the attempt. It does not establish USB as the cause. The body remained at its ordinary shooting screen. Do not repeat the cardless script below as if the feasibility premise had passed.

### Firmware state and presentation

The X-M1's resolved English layout ids distinguish `0x7e1` (“NO CARD”) from `0x7e2` (“CARD ERROR”). `0x0112ede0` maps reason `0x11`/`0x2d` to the former and `0x13`/`0x2e` to the latter. `0x01127e04` and `0x0112a550` read the cached warning at `[DAT_00940de4 + 0xbb94]` and select those messages for `0x11` and `0x13`.

The older Run-3 capture binds `DAT_00940de4 = 0x00944580`, making that cached word `0x00950114`; its captured value is `0x13`, consistent with the reported CARD ERROR rather than a generic no-card label. This is another boot's non-atomic RAM observation, not the state at the attempted half-press. The cache is not a raw card-detect bit and its message consumer is not itself the S1 admission predicate. Clearing it globally would also affect unrelated error and mode consumers.

#### S1 refusal predicate, not a raw presence check

The bound check is a **media-error warning predicate**, not a direct SD-detect read. `0x00d0c87c` maps the cached media-status word `0x00918af8`: value 2 activates warning `0x11` (NO CARD), 3 activates `0x12` (CARD NOT INITIALIZED), and 4 activates `0x13` (CARD ERROR), through `0x00d3bc34`. `0x00632478`/`0x006322f0` maintain the active-warning bitset at `0x00950118`; `0x0063249c(id)` tests it. Run 3 contains media status 4 and active mask `0x00080000`, with warning `0x13` also present in the presentation-history bitset `0x00950128`. Those agree with its cached CARD ERROR word. Raw `KEY_CARD_DET` is a different input, code `0x11`, from port `0xfff89314` bit 7 ([input-map binding](input-map-150.md#4-code-to-control-map-and-firmware-comparison)); the identical number does not make that input the warning predicate.

The ordinary S1 route is bound through the UI tables:

| Boundary | X-M1 bytes and decoded effect |
| --- | --- |
| Press dispatch | `0x00c09d24` sees event `0x0007` in `0x00944580` and qualifier 0 in `0x00944588`, sets modal `0x4a` event 0 and calls `0x00bfb028` |
| Shooting-state action | Modal `0x4a` has 38 states, matrix `0x00cea6e8`, action table `0x00d0150c`; state 4, observed at `0x0146761c` in Run 3, selects cell `0x00cea6f8 = 0x00010003`, action 3 at `0x00d01518 = 0x00c58c38` |
| Common inhibition | At `0x00c58cc8`, word `0xeb033059` calls `0x00d24e34`; results above 1 branch at `0x00c58cdc` to the refusal arm `0x00c58d60` |
| Card-error predicate | `0x00d24e34` calls `0x00d19324` at `0x00d25180`. That routine's first query, `0x00d1932c`-`0x00d1933c`, is `MOV r0,#0x13; BL 0x0063249c; CMP r0,#1; MOVEQ r0,#1; POPEQ {r11,pc}`. Its true result becomes inhibition result 3 at `0x00d25188` |
| Refusal rather than normal request | Result 3 makes `0x00c58d68`-`0x00c58d70` call `0x00d14480`, `0x00d1513c` and `0x00d1d464`, then return. The normal input/request dispatch in this action is not reached |
| Warning selection | `0x00d1d464 -> 0x00d1d440(0x13) -> 0x00d3bb18` selects the active warning in bitset `0x00950138` and marks the warning state changed. `0x00d3b64c` chooses the highest active warning, updates `0x00950114`, and requests its display, tone and indicator policy |

For warning `0x13`, Run 3's five-byte policy at `0x009501af` is `03 01 01 00 00`. The indicator arm calls `0x00d39410(8)`, whose stock profile at `0x00d0a1c4` is `{0xc9,250,250}`; `0x00db50b0` forwards that as `0x006a4d84(1,3,250,250)`. These are stock request values, not a measured blink period or an independent pin-to-visible-red binding. The red blink itself is the owner's hardware observation.

This establishes an S1-to-card-error refusal route with byte-backed tables and instructions. The attempted block did not read the current UI modal state or the media-warning words, so it cannot identify the executed branch on that attempt. The older state-4/media-error capture and the reported indication support this route without replacing that missing observation.

#### Stock no-card alternatives

The distinction between NO CARD and CARD ERROR matters. `0x00d19324` does **not** reject warning `0x11`; it rejects `0x13`, `0x14`, `0x12`, `0x15`, `0x18`, `0x17`, `0x36`, `0x1d`, `0x1b` and conditional warning 8. In still mode, a stock NO CARD-only state can therefore pass this S1 media-error predicate if the remaining inhibition checks pass. This is a source-bound candidate, not demonstrated cardless AF or proof that the damaged body's current error can be cleared by an ordinary setting.

S2 uses distinct actions. `0x00c09d24` maps input 8/qualifier 0 to modal `0x4a` event 2. State 4 selects cell `0x00cea828 = 0x0001001c`, action 28 at `0x00d0157c = 0x00c59578`; its ordinary input-8 argument can post the S1/S2 warm-up inputs with tag `0x100`. State 6 selects action 29 at `0x00d01580 = 0x00c595f4`. That action calls stock `0x00d24b58` before its drive/capture continuation; the still-mode predicate independently calls `0x00d19324` and retains its CARD ERROR refusal. The movie predicate `0x00d1944c` also retains both that error and its explicit NO CARD rejection at `0x00d19480`-`0x00d19490`. A local S1-only interception leaves these paths unchanged.

The X-M1 menu inventory offers SHOOT WITHOUT LENS, node `0x0077eae4`, property `0xa3`, not a bound SHOOT WITHOUT CARD setting. `0x00d24e34` reads `0xa3` for separate lens-state checks before the media-error predicate; changing it does not remove warning `0x13`. No supplied menu label establishes a no-card override.

Switching to video does not avoid this S1 error predicate. Mode `0x2c` additionally rejects NO CARD through `0x0063249c(0x11)` at `0x00d25190`-`0x00d251b0`. Downstream movie/continuous AF actions exist, including `0x00e24ad0 -> 0x00df48b4(0x19,...)`, but their outer scheduling/admission path without recording is not closed. Likewise, `0x00e33c58`'s initial `0x00df5590(3,0)` call dispatches machine `0x48`; its return predicate cannot be inferred from the event number or from the absence of a literal card flag in the wrapper. These are research candidates, not an authorized or verified stock workaround. Playback has no bound fresh-AF route in the inspected paths.

#### Stock detect and re-detect conclusion

Host inspection now closes the producer chain for Run 3 (E-AF-CARD-DETECT-RUN3-150, E-AF-CARD-MEDIA-PRODUCER-150; [full report](cardless-capture-feasibility.md#1-what-makes-card-error-instead-of-no-card)). Input row `0x00828200` specifies active-low GPIO12.7 and retains current/previous asserted bytes `1/1`, despite the reported empty slot. The SD driver selects its present-card initialization path, whose error flag is captured as `0x10` at `0x009738c4`. Filesystem flags at resolved physical `0x021808f8` and `0x0217a3d8` are both 2; `0x00ec6934` maps that to media status 4, and warning refresh reasserts CARD ERROR `0x13`. This is the older, non-atomic Run-3 observation, not a post-S1 trace or a diagnosis of the damaged conductor.

The stock absent path exists: GPIO12.7 high returns absence through `0x006eddc0 -> 0x006f4d24`, yielding media status 2 / NO CARD `0x11`. Card-edge handling in `0x006a1c50` invalidates SD initialization through `0x006fae5c`, so a real detect transition has a stock re-detect path. No inspected mode or power path makes an unchanged present indication mean absent. Mode changes may clear warning presentation; they do not repair the media producer, which can reassert `0x13`.

The subsequent idle read E-CARD-DETECT-LIVE-150 confirms that the empty slot still reads present/error. A stock absent-state route would require the detect input to become electrically absent, not a warning clear or demonstrated menu workaround. The owner has selected a software admission mechanism rather than contact manipulation, a mode sweep or repair before port replacement.


#### Minimal override review boundary

The stage-1 implementation uses one function-entry A32 `B` at `0x00d19324`, replacing only its first `PUSH {r11,lr}`. Its owned veneer requires incoming LR `0x00d25184`, the saved caller LR at `[r11]` equal to `0x00c58ccc`, and mode `0x00956838` to be neither `0x1f` nor `0x2c`. The first guard binds `0x00d24e34`; its pinned frame prologue makes `[r11]` the saved return to the exact S1 action. Nonmatching contexts replay the displaced PUSH and resume untouched `0x00d19328`. The matching still-S1 context replays PUSH/ADD-frame-pointer and resumes `0x00d19340`, skipping only the first CARD ERROR query and its early return. Mode `0x1f` has a distinct `app 0x78 -> 0x00c2f830 -> 0x00d13e88(0x30) -> 0x00db4254` configuration route producing reccont `0x209/0x23c0`; that route is not eligible for this bounded AF-only bypass.

Four read-only UI-cache guards also require `u32[0x00956b6c] != 1`, `u32[0x00956900]` to be neither 9 nor 10, and `(u32[0x00956938] != 0 || u32[0x00956934] != 1)`. They confine the bypass to the normal AF branch selected by `0x00d0f6f4`. The alternate `0xad` application request changes mode/configuration and queues reccont `0x20c`, not ordinary AF `0x204`; it retains the stock CARD ERROR refusal. The four canonical setters and their explicit call-owner families are listed in the [reccont closure](cardless-capture-feasibility.md#21-selected-stage-1-scope-and-post-admission-graph). This is a bounded UI-actor contract with no concurrent mode/property/service changes, not a global IRQ-ownership claim.

The pending-input guard requires `(u8[0x00951fc4] & 6) == 0`. Bit 2 represents pending S2 input 8; bit 4 replays movie input `0x34` through `0x00c5a858 -> app 0xb3/0x7f -> 0x00c2de74 -> 0x00d20098`, which changes mode/configuration and sends reccont `0x20c`. Bit 1 represents ordinary S1 input 7 and remains allowed. `0x00d36310` with the `0x2000` family copies the current input cache into its backup before publishing the first S1 job. The selected S1 notifications cannot recreate an old input 8 or `0x34` from that backup. No held, external or previously queued S2, movie, mode-change or lifecycle command is permitted during the bounded attempt.

The notification-route guard additionally requires `u32[0x009134f8] != 1`. Stock `0x0073abec` reads that word; value 1 redirects relevant AF notifications to service queue 7 instead of the audited ordinary UI queue. Run 3 observed zero and the stock callback chain `u32[0x0095b074] = 0x0072b680 -> u32[0x00829a0c] = 0x0072b210`. Independent review must accept the source-bound ownership and no-service-transition contract before body use.

The initial lens-readiness guard requires `u32[0x0095e1f4] == 1`. It is not an asynchronous lifetime proof: a selected lens-transport error can reset queue-`0x63` state `0x0096aff4` and later invoke `0x006c31f0`, clear the latch and produce `0xb8` during owned recovery. The post-admission closure must include that recovery and the `0xb8` receiver. Any new S1 entry reevaluates the latch.

The photographic flash-charge guard requires `u16[0x0096b56c]` to be idle 0 or paused 7. The selected pause/resume messages `0x1a08/0x1a09` and timer messages stay within `{0,7}` and do not invoke a prior capture's saved flash callback. Allowing state 7 preserves S1 reentry while a stock internal-profile fetch has paused charging. Run 3 observed lens latch 1 and flash state 0. The payload neither changes these values nor commands the photographic flash.

The profile guards pin `u32[0x01529fb4] = 0x0095a6b4` and the secondary context `u32[0x01529fb8] = 0x009432a0`, require cached mode `u32[0x0095a6b4]` to equal current mode `u32[0x00956838]`, require cached/current profile field `+0x320` at `0x0095a9d4` and `0x00956b58` to match, and refuse requested capacity-refresh bit 3 at `0x0152c8d0`. Stock `0x00dd1254` can request that bit only for a transition into movie mode or a changed `+0x320` field. Its capacity-refresh path `0x00de9b7c -> 0x0070f718 -> FileTask 0x2c26 -> 0x00715a58 -> 0x007012a0` can read FAT storage and is forbidden here. The source proof must preserve these equalities through selected profile copies and configuration callbacks, not just observe a clear bit at entry.

The read-only actor guards require the following initial context. A mismatch retains the stock CARD ERROR refusal.

| Actor field | RAM address and width | Required initial value |
| --- | --- | --- |
| Recorder `0x04` | `u16[0x01545858]` | `1` |
| Recorder `0x05` | `u16[0x0154585a]` | `3` |
| Recorder `0x06` | `u16[0x0154585c]` | `0` |
| Recorder `0x07` | `u16[0x0154585e]` | `0` |
| Recorder `0x0f` | `u16[0x0154586e]` | `0` |
| Recorder `0x14` | `u16[0x01545878]` | `0` |
| Recorder `0x15` | `u16[0x0154587a]` | `0` |
| Recorder `0x4a` | `u16[0x015458e4]` | `0` |
| Recorder `0x4b` | `u16[0x015458e6]` | `0` |
| Recorder `0x4d` | `u16[0x015458ea]` | `0` |
| Recorder `0x4e` | `u16[0x015458ec]` | `0` |
| Recorder `0x59` | `u16[0x01545902]` | `0` |
| Recorder `0x5a` | `u16[0x01545904]` | `0` |
| System root | `u16[0x015b8078]` | `1` |
| System module 2 | `u16[0x015b807c]` | `2` |
| Queue-`0x3b` startup root | `u16[0x00972e88]` | `1` |
| Packed frame 0/1 states | `u32[0x0157bd20]` | `1`, meaning frame 0 state 1 and frame 1 state 0 |
| Frame 6 | `u16[0x0157bd2c]` | `0` |

These are initial-domain checks, not a general claim that their values stay constant after admission. Owned `0x201` completion can set recorder `0x04` to zero and start initialization through `0x0f`, `0x59` and `0x5a`. Initial `0x0f = 0` excludes an inherited callback at `0x0152c988`; initial `0x59/0x5a = 0` excludes inherited profile and mask transactions. Initial `0x4a/0x4b = 0` excludes an inherited alternate-initialization profile, callback or already-ready child. Initial `0x4e = 0` excludes capture-promotion continuations in its foreign states 1, 2 and 5; initial `0x07 = 0` excludes foreign configuration buffers. Initial `0x15 = 0` selects the fresh AF entry rather than its busy diagnostic branch. The selected callbacks require their own state fixed point. The all-state configuration union contains capture-control keys `0x1334/0x1335/0x1337/0x1339/0x138b/0x138c`; the selected state-cell and callback projection must exclude them.

Initial system module 2 state 2 excludes inherited startup and nested lifecycle transactions. Its owned `0x13e7` transaction can move it to 3 and back to 2; this field is not constant after admission. The selected module-2 prefix for the `0x1200` completion must be composed with that cycle, rather than inferred from system-root state 1.

Normal startup completes the bank initializer before creating its initcont owner: `0x0069d1e4 BL 0x0069f204` returns before `0x0069d27c BL 0x0069ca44`, then `0x0069cbc0 BL 0x0073afe0 → 0x0073afe8 BL 0x0073c0d4`, which creates task entry `0x0073e048`. That task later invokes module-5 startup `0x00d985a4`, which creates reccont. This is call-return-before-task-creation ordering, not a snapshot or a cross-core monitor assumption. Root store `0x0073e1bc` still publishes 1 before `0x0073e1c0 BL 0x0073cdc0` invokes module-5 startup, so root 1 does not prove that later startup has returned; it does not demonstrate a pending bank initializer on this normal creation path. `0x0069f000` creates the `flashext` status worker, whose timer callback is `0x0069e03c`, not the bank initializer. Alternate actor creation, computed/global reentry and later bank reload or guard mutation remain unclosed.

Run 3 sampled these initial fields, matching modes `0x24`, matching profile `+0x320` values 1, zero requested/issued/completed masks and sequencer phase 3 (`E-CARDLESS-AF-CONTEXT-RUN3-150`). These historical, separately sampled values are not a coherent snapshot or a future preflight. The payload does not force readiness or clear a prior transaction.

The sensor-sequencer guards require `u32[0x0096bd70] = 0` and phase `u32[0x0096bd74]` in `{1,3,5,7,9,0xb,0xd}`. The selected kind-2 queue-`0x22` request can target only the initialized odd-phase family. This excludes the uninitialized/even or busy contexts whose descriptor pairs can contain negative counts. The payload does not stop the sequencer or assume that a busy flag proves the absence of unrelated queued work.

All remaining warning, lens, mode, busy and readiness checks execute stock instructions. General registers and CPSR flags are restored before either continuation; the veneer writes only its current stack frame. It has no mutable shared state, lock, exclusive access, core-ID assumption, storage/lens/register operation or global media-warning override. Movie and S2 reach the original predicate. Entry+4 remains executable, including for a task already suspended after the old PUSH. Power removal, not hot unload, ends a body session.

The current UI-event argument `u32[0x00944584]` must not be `0x100`. S2 action `0x00c59578` can call `0x00d3b198(7,0x100,0)` and then `0x00d3b198(8,0x100,0)` without setting pending-input bit 2. Its first queued event can therefore reach the ordinary S1 action while that bit is clear. `0x00d3b0dc` copies the queued record at `0x0094fa58` into `0x00944580`; queued second word `0x0094fa5c` becomes the guarded current argument. The argument guard preserves stock CARD ERROR refusal for this S2-origin warm-up. Pending-mask checks alone do not distinguish it. The native trajectory includes this argument-`0x100`/pending-zero refusal and argument-`0x110` S1 replay as separate cases.

The ordinary GPIO publisher `0x006a1c50 -> 0x006a1b40(key,0)` supplies argument zero. S1 replay `0x00d1973c -> 0x006334dc(7,0x110)` carries argument `0x110` through the ordinary queue-1 message into the same current field. After the UI actor copies or receives a record, its preprocessing, the `0x00c58c38` prefix and the `0x00d24e34` prefix before `0x00d19324` neither pump another event nor rewrite that argument. The synchronous guard therefore observes the dispatched argument, not the producer's next queued record.

This is not a claim to recognize every physical S2 origin. `0x006a1c50` can synthesize an ordinary `7,0` record when it sees key 8 while its cached key-7 slot is clear. That record is indistinguishable from ordinary S1 at this boundary. The actual S2 capture-admission predicate remains untouched, and the bounded stage-1 source contract still excludes held, queued or new external S2/movie input. The `0x100` guard additionally preserves refusal for the distinct UI-generated S2 warm-up; it does not replace that contract.

The assembly ABI veneer uses 32 additional bytes of the current task's stack before calling the separately compiled C guard in `menu/cardless_af.c`. It saves `r0..r3`, `r12`, LR and CPSR flags, restores that space and the incoming registers/flags before either continuation, then reproduces the stock 8-byte prologue. Review must account for the veneer frame plus the emitted C guard's own frame. The elided warning query is a read-only bit test; the media status, warning bitmap and error presentation are not cleared.

#### Diagnostic-file guards and observed configuration

The owner-confirmed storage boundary, recorded on 2026-10-26 in [§3.4](../spec/07-governance-and-handoff.md#34-standing-confirmations), permits existing stock read-only internal NAND fetches of firmware or calibration profiles, including the `0xfff52400` profile load. SD controller access, filesystem calls, file numbering, and NAND program or erase are forbidden after admission. Every reached forbidden branch must be refused or neutralized; the permission for internal profile reads is not a permission for NAND mutation.

The bound S1 AF producer union permits a conditional internal profile fetch: operation `0x0d` through `0x00df48dc/0x00ef9264`, selector `0x0c` at `0x00f08be0`, changed-mask path `0x00f31548`, imgdc `0x3f34`, `0x00ff412c -> 0x00fe977c`, and queue-`0x37` request `0x4703` can reach `0x006f1440` and the `0xfff52400` NAND FIFO. The bound NAND commands are read `0x00/0x30`; this path is not a filesystem or SD request. It is source-permitted under the profile/cache conditions, not an observed fresh read from the later S1 session. The [AF source map](autofocus-static-map.md#sd-error-s1-admission-boundary) records the full chain and the separate program/erase exclusions.

The owned lens-error recovery also reaches stock read-only calibration-cache fetches: registered lens data through `0x006ac16c -> 0x006a8e84 -> 0x006faf00 -> 0x006fb49c -> 0x006fce40 -> 0x006f1a74 -> 0x006f1440`, or defaults through `0x006aa1fc -> 0x006a8ff0 -> 0x006fb058`. These are additional allowed internal NAND reads, not evidence that the whole recovery is storage-free.

Admission also requires the stock byte-property bank pointer at `0x0096ca50` to equal `0x41acd800`. It refuses the bypass if property `0x24c & 7` or `0x22c & 7` is nonzero, `0xa8 & 0x80` is nonzero, `0xdc & 0x80` is nonzero, `0xb6 == 1`, `0x98c0 != 0`, or `0x8cfc == 1`. Each failed guard resumes the untouched stock predicate, so CARD ERROR continues to refuse admission. These are read-only guards, not changes to diagnostic settings.

The property getter `0x0069f8fc(key)` reads one byte at `[0x0096ca50] + key`; startup `0x0069f204` supplies bank `0x41acd800`, whose physical alias in the bulk snapshot is `0x01acd800`. Property `0x24c` therefore lives at virtual `0x41acda4c`, physical `0x01acda4c`. Its bits 1, 2 and 4 enable the `0x00f48f54 -> 0x00ee92e4` statistic dumps. That facade calls `0x0070fbfc -> 0x0070fb68 -> 0x00716324` for DCIM/file-number allocation before `0x0071c648` opens a file; trapping file open alone would be too late.

Owned mode-`0x200` initialization adds a separate diagnostic-file producer: recorder `0x59` row `0x17 -> 0x00e388ec -> 0x00dd2c68 -> 0x00eb0c24 -> 0x00ed3de4`. The latter opens a TS log, writes records and closes it through `0x0071c648/0x0071c75c/0x0071c7bc`, publishing FileTask operations `0x2c17/0x2c19/0x2c1a`. Property `0x22c` controls its three event classes; the guard requires mask `7` to be clear without relying on the secondary enable at `0x4089ebec` or the initial `0x00dca960` query result. Its byte is at virtual `0x41acda2c`, physical `0x01acda2c`. This is distinct from the `0x24c` statistic dump.

Run 3's idle capture observed the expected bank pointer and the following low bytes. They are historical observations from a different boot, not preflight authorization for a future session.

| Property | Physical byte address | Observed byte | Admission requirement |
| --- | --- | --- | --- |
| `0x24c` | `0x01acda4c` | `0x00` | `(value & 7) == 0` |
| `0x22c` | `0x01acda2c` | `0x00` | `(value & 7) == 0` |
| `0xa8` | `0x01acd8a8` | `0x00` | `(value & 0x80) == 0` |
| `0xdc` | `0x01acd8dc` | `0x00` | `(value & 0x80) == 0` |
| `0xb6` | `0x01acd8b6` | `0x00` | `value != 1` |
| `0x98c0` | `0x01ad70c0` | `0x00` | `value == 0` |
| `0x8cfc` | `0x01ad64fc` | `0x00` | `value != 1` |

Known mutation routes include startup/default loading, bank reload, generic service parameter writer `0x00dc7564`, the parameter editor and the `E_` record writer. The editor's event-1/argument-3-zero confirm arm does not check mode `0x0096b480==3`; Run-3 mode 0 therefore does not exclude a stale cached confirm. The service writer can mutate bytes before returning refusal for argument 1 zero. The retained AF/control/queue-30 projection's setter `0x00742098` restricts keys to `0x81..0xa1`, but that restriction does not cover exported raw-bank aliases or generic writers. The [bank-lifetime cut](cardless-capture-feasibility.md#diagnostic-guards-reload-attempts-and-completion-epochs) retains those exact transfer facts and open ownership joins. The guards remain mandatory.

Review must explicitly confirm stability across the asynchronous continuation, including the reccont and image-control message receivers. The proposed session excludes external service/editor activity and parameter-write USB operations. Operator restrictions cannot exclude a reload or writer reached by the firmware's own S1 continuation. If the final source closure cannot exclude those effects, admission-time checks are insufficient and the design must instead use a scoped no-dump boundary. The live CLI remains refused.

#### Storage-closure status, 2026-10-26

The composed post-admission graph is not closed. The owner-confirmed boundary remains unchanged: existing stock read-only internal-NAND firmware/calibration profile fetches are allowed; SD controller access, filesystem calls, file numbering, and NAND program or erase are forbidden.

- Queue `0x28`/`0x3800` scripts `0x007cf138/0x007cf15c` and their selected IRQ callbacks are bound through delivery to `0x00ec558c`. This does not close the receiving system continuation. Queue `0x29` scripts/completions and the selected queue-`0x3b` requests are bound under the initial root-1 guard.
- Phase-0 action `0x00c6bc78` can advance to phase 4 before queued reconciliation keys drain. The phase-4 route `0x00c6898c → 0x00d427ec → APP7D → 0x00d1a7d8 → 0x006337d8/0x006434f8` remains open. The lifetimes of `0x009514a4`, `0x00954cf8`, `0x00952000`, `0x00952018`, the physical mode-key bitmap, current/cached mode, and field `+0x320` must be composed before pruning it. Alternate initialization states 1, 2, `0x1e` and `0x21` are not yet excluded.
- Whole-profile copy `0x0063219c` copies `0x1000` bytes, including mode and field `+0x320`. Its selected callers `0x00637e5c`, `0x00d20098`, `0x00d203e4`, `0x00d21590` and `0x00d2163c` remain part of the mode-lifetime frontier.
- The [conditional E7 suffix](cardless-capture-feasibility.md#21-selected-stage-1-scope-and-post-admission-graph) reaches FileTask `0x2c00` and the later update-file open/close substep. Its mask join tolerates either selected callback order under one owner, but `AC00/AC17` carry no token, close is nonwaiting, and final UI cleanup can enter reconciliation again. Pending-control-5 production/preservation from admitted S1, matching callback ownership, actual release and preview restoration remain unproved. Historical Run-3 pending control 0 and initialization flag 0 establish no lifetime invariant.
- Bank candidates have different effects: `0x00ecc654` uses bank byte `0x81` to select a page reload, full-bank reload or RAM-status-only result; the selector domain is not bound to admitted S1. `0x00ecc66c` exports through NAND program/erase. System candidate `0x00ec88c8` reaches NAND program and conditional erase before its optional bank export. System `0x603` requires root state 4 and module-9 state 0; root 1 instead selects rejection. Task-7 `0x1707` requires its root state 6/module-0 state 3 and payload-selected rows `0x19/0x1a`. These are conditional forward dispatch cuts, not admitted-S1 origins (E-CARDLESS-BANK-FORWARD-CUT-150).
- The [bank-lifetime cut](cardless-capture-feasibility.md#diagnostic-guards-reload-attempts-and-completion-epochs) binds a conditional completion-array reset at `0x0069cdf8(0)` and local reload-attempt/store/semaphore/callback order, but not validated bank data or a no-reset visibility epoch. Property `0x244` enables reload over diagnostic controls `0x8cfc/0x98c0`; full/page reloads through `0x0069fe20` bypass that completion latch and can replace other guards. The four kind-6 producers, raw aliases, stale editor confirm and record writer still require selected-S1 lifetime composition (E-CARDLESS-BANK-LIFETIME-CUT-150).
- Queue `0x49`/`0x5902` still needs the final byte-backed effect contracts for its new format/GPU paths, including leaf `0x00890fac`. Operation 7 through `0x008935b8` returns `-1`; it is not evidence of a successful format operation.

The earlier APP6-only, invariant-lens-latch and phase-0-null arguments are withdrawn. Stage 1 remains technically NON-ADMISSIBLE because the owned prefix, forbidden-path exclusions, guard lifetimes and completion/release proof remain open. This is not a user-permission freeze. If a forbidden path cannot be excluded, the next design must refuse or neutralize that path at a source-bound boundary; another admission-time snapshot does not close it. The earlier guard changes have not been rebuilt or exercised; that unrun status is not a current host-work or resource ban.

#### Revised boundary design for review, not implementation

The existing one-publication candidate is non-admissible. If the complete prefix establishes the E7 filesystem substep as reachable from admitted still-S1, one conditional design option is to intercept at or before entry `0x00ec6790`, before either `0x0071f6ac` query. A hook at `0x0071f550` would be too late. This is not yet a proven required fix: the prefix, ownership and completion contracts remain open. No revised hook, additional guard or CLI admission is implemented here.

- Bind the complete admitted-S1 prefix and its recovery predicates before selecting or implementing this option. The boundary must distinguish the owned still-AF E7 transaction from boot, movie, capture and foreign lifecycle work using stock ownership or request provenance. The `0x00ec3740` caller and `0x00ec591c` callback identify a system completion path, not by themselves its S1 origin. A historical mode, pending-control value or root snapshot is not sufficient.
- Suppress the selected E7 filesystem substep as a whole, including its queries, parameter construction and queue-`0x65` publication. Do not globally disable FileTask, change media status or pretend that a volume was initialized. The exact entry instruction, argument preservation and original continuation require byte binding before any future one-word A32 branch publication.
- Preserve work before this boundary: the preceding image-fill completion and `0x00ec5c0c(2)` sensor/configuration request remain stock. Their ownership, callback delivery and failure paths must still run.
- Account for completion through the existing `0x00ec591c` contract, which posts `AC00` to queue 2. The [conditional completion cut](cardless-capture-feasibility.md#21-selected-stage-1-scope-and-post-admission-graph) establishes local order independence of the mask-4/bit-1/bit-2 join under one matching serialized owner, and one final notification attempt before the state-0 callback rows become null. It does not establish delivery: the final handler clears state before publishing and returns `0xff` on notification failure without retrying. `AC00` and `AC17` carry zero payload words, not transaction tokens. A future replacement must bind provenance and loss behavior, deliver the owned completion exactly once, and preserve the original `0x00ec6790` return value.
- Delivering `AC00` alone does not close storage effects. Handler `0x00ec3778` reads cached status through `0x00ec607c → 0x007101e4(0)`. When that status is not 7, `0x00ec6800 → 0x00db971c → 0x0071c648 → 0x0071c2b0` publishes FileTask `0x2c17/0x2c2d` for the update-file probe. On successful open, callback `0x00ec6360` publishes callback-free, nonwaiting close `0x2c1a` before posting `AC17`; the latter is not close-completion evidence. A conditional no-filesystem design must account for this substep and its failure/completion ownership too. It must not force cached status 7 to simulate an initialization that never occurred.
- Bind every buffer release and preview-restoration effect after the final UI notification. The matching 16-bit family-`0x1380` token routes through UI `0x22` row 4/state 1 to `0x00c31df0`, then conditional UI `0x65` cleanup `0x00ca58d4 → 0x00d37a30`, which can enter mode/property reconciliation again. That is a continuation frontier, not preview restoration. `0x00ec62e4` performs its cleanup calls only for E5. Skipping filesystem work is acceptable only if completion does not depend on a filesystem-owned resource or on an unperformed initialization. Otherwise the design needs a source-bound abort/release path, not a fabricated success.
- Reuse the established normal-startup return-before-task-creation ordering for the original bank initialization. Separately bind later startup work, alternate actor creation, reentry and any pre-existing E7 transaction that could still own resources. A future entry hook protects future calls, not a task already suspended inside `0x00ec6790`. Root 1 is not an overall-startup completion witness.
- Any added publication changes the count-specialized installer and the payload allowlist/review scope. It requires a new complete byte inventory and independent review while leaving the shared installer, ABI and existing payload pins unchanged.

This is a host-only design review. The mechanism remains technically NON-ADMISSIBLE pending the owned-prefix, lifetime and completion/release proof. No camera authorization follows from this proposal.

#### Review evidence classification

| Class | Current claim |
| --- | --- |
| Established source and byte bindings | E7's FileTask suffix, conditional local completion-order/one-attempt contract, zero-payload callbacks, nonwaiting close and final UI token/actions (E-CARDLESS-E7-COMPLETION-CUT-150); normal UI pre-loop construction and concrete configured-flag reset routes (E-CARDLESS-INITIAL-OWNER-CUT-150); conditional completion-array reset and local reload-attempt order (E-CARDLESS-BANK-LIFETIME-CUT-150); corrected reload/export effects, System-root/Task-7 forward selectors and earlier NAND mutation (E-CARDLESS-BANK-FORWARD-CUT-150). None supplies the missing selected-history or lifetime premise. |
| Conditional path | Reaching the E7 suffix from admitted still-S1 requires pending control 5 at the selected callback. No complete owned producer path or source-bound infeasibility proof has been established. The `0x00ec6790` boundary is a conditional option, not an implemented or approved fix. |
| Invalidated reasoning | Phase-4 notification `0x939 → 0x00c68744` calls `0x00d350c4(1)` and sets pending control 3, not case 3/pending control 5. In phase 6, notification `0x939` selects `0x00c59500 → APP5A → 0x205` stop. Neither establishes the earlier claimed pending-control-5 prefix. Root 1 and `0x0095709c = 1` do not establish completed startup. |
| Unresolved facts | Complete admitted-S1 predicates and pending-control-5 history; matching E7 ownership/release/preview; task-1 reset/alias and computed reentry ownership; inherited family-`0x1380` work and the opcode-2/module-2 restoration join; selected bank-reload epoch, data validity and cache/semaphore visibility; owned origins or exclusion of System `0x603`, Task-7 `0x1705/0x1707` and the VM write command. |

#### Finite initial-domain source cut

One inherited-state candidate is incompatible with a specific stock entry path. Physical key 7 selects UI-root default row `0x30`; in root state 3, cell `0x00ce0290 = 0x00010061` selects `0x00c24994`. That function calls `0x00d3a678` before `0x00c09d24`. With latch byte `0x00950110 == 1`, the gate refuses key 7: `0x00d35f3c` is exactly `currentKey < 0x600`, and the permissive latch-1 exceptions require key 9 or `0xb`. Thus an initial pending-5/latch-1 state does not reach the hook through this root-3 action.

The configured transition does not add a callback after that refusal: `u32[0x00d0013c] = 0x00cfdf34` is the root transition vector and its transition-1 slot `0x00cfdf38` is null.

The normal owner is task 1, created by `0x00d0a238` with entry `0x00c22f60`. Its loop consults two internal event rings before receiving queue 1 and invokes `0x00bfbd20 → 0x00bfb20c`. This binds a stock consumer and its action boundary, not an empty-queue observation. The framework invokes an action before its transition. The current guard does not explicitly pin root state 3, and alternate root handlers, writer/reentry ownership and transient pending-5/latch-0 windows remain open.

The pending-control publisher `0x00d350c4` publishes A05 through `0x006334dc` before setting `0x00950110 = 1`, then copies the current event. Any persistent pending/latch invariant needs the stock UI-owner serialization. The enqueue alone does not establish that invariant across cores.

The non-latching initial writer has a narrower configured source path. `0x006330d8` can change pending 3 to 5 inside `0x00635280`; `0x00639b1c` then calls `0x00637478` and `0x0063975c`, which consumes and clears pending before returning. The configured coordinator's row-0/state-0 cell `0x00817724 = 0x0102` selects `0x0073e0b0`, whose `0x0073ccb8 → 0x00639b1c` call returns before its root-1 publication. A scan of both pinned images finds aligned immediate callers only along this constructor chain and no aligned pointer to its four inner entries. This narrows that transient writer; it is not computed-reentry, foreign-writer or complete initial-work closure.

The separate aligned-reference census binds all 17 immediate caller entries of `0x00c09d24` to literal slots in the UI-root function vector, and the sole aligned `0x00c58c38` pointer to `0x00d01518`. It does not prove that computed pointers or an alternate actor cannot enter these paths.

The extra literal-9 producer `0x00c327a0` is not a direct physical-S1 action. Its sole aligned pointer `0x00cfe244` selects UI module `0x31`, row 1, states 0 through 4. `0x00c029d0:12–20` selects that row for key `0x10`, qualifier `0x80`. This binds a key-`0x10` release action, not key 7 or its release. Internal production of key `0x10`, dynamic arguments to `0x00d350c4`, and the earlier literal-3 producers remain separate joins.

The selected physical-root-3 prefix also has an owned timer path. Before `0x00c09d24`, `0x00c24994 → 0x00d0c188` can reach `0x00d0bf78`: `u32[0x009500ec]` must differ from `0xb`, the argument must differ from `u32[0x0094f6e4]` and `u32[0x0094f6f0]`, and `u32[0x0094f664] != 0x20e`. Unless byte `0x0096b465 = 1`, a nonzero property-`0x36`-selected delay arms callback `0x00d0a8f4` through `0x00d0b97c → 0x00db51dc → 0x0069b328`, with handle destination `0x0094f934`. That callback publishes queue-1 event `0xa02`. Its root-state-3 action `0x00c23f0c` calls `0x00c09d24` and, on `0xffffffff`, invokes notification `0x00d38f34(0x11)` and pending writer `0x00d350c4(0xe)`. UI `0x4a`/row `0x90`/state 9 selects the exact-constant-`0xffffffff` action `0x00c61aec`; other states have their own predicates. The timer-cancel chain `0x00d0bf3c → 0x00d0ba20 → 0x00db5214 → 0x0069b7a4` has not been shown to remove an already queued `0xa02`. Callback scheduling, normal initial handle state and its composition with selected S1 remain open. This is an internal-work candidate, not a proved forbidden continuation or a proved cancellation race.

Notification identifiers are not UI-input identifiers. The pre-dispatch `0x00d3d658 → 0x00d38d28(0x935)` branch maps through `0x00db68dc` to notification `0x5054` and `0x00db6804`, subject to inhibition and duplicate suppression. It is not a bound queue-1 key-`0x935` producer. These source cuts are recorded separately in `private-data/analysis/x-m1-1.50/af/s1-ui-owned-events-cut.txt`.

This is a partial cut for proof order A, not complete initial-domain closure or an E7 exclusion. A later A05 replay is not physical key 7 and can reach a C-query without this physical-input gate. Subsequent selected events, all 15 query responses and stop/201 completion still need composition. The separate author artifacts are `private-data/analysis/x-m1-1.50/af/s1-initial-domain-cut.txt`, `af/s1-initial-owner-addendum.txt` and `af/s1-ui-entry-candidates.json`; the original design packet and independent review remain unchanged.

#### Conditional fifteen-child query composition

The ordered `0x00ca4bb8:23–43` query has a source-bound sufficient response vector, not yet a selected-S1 history. With APP word `u32[0x0094f64c] = 0xc`, each listed UI actor in state 0, and the two additional query-time predicates below, all 15 children return `0xffffffff`:

| Order | UI module / row | State-0 action | Additional query-time predicate |
| --- | --- | --- | --- |
| 1 | `0xa6 / 0` | `0x00ccf3e8` | None |
| 2 | `0x1a / 4` | `0x00c2d03c` | None |
| 3 | `0x1e / 0x44` | `0x00c2d8d4` | None |
| 4 | `0x8f / 3` | `0x00cc3470` | None |
| 5 | `0x18 / 6` | `0x00c2caa8` | None |
| 6 | `0x16 / 6` | `0x00c2b910` | None |
| 7 | `0x1c / 3` | `0x00c2d650` | None |
| 8 | `0x1d / 3` | `0x00c2d754` | `u32[0x00951560] != 1` |
| 9 | `0x1f / 0x16` | `0x00c2f984` | Mode `u32[0x00956838] != 0x1f` |
| 10 | `0x17 / 0xc` | `0x00c2bc5c` | None |
| 11 | `0x1b / 4` | `0x00c2d3f4` | None |
| 12 | `0x20 / 6` | `0x00c31754` | None |
| 13 | `0x8e / 8` | `0x00cc32bc` | None |
| 14 | `0x6b / 6` | `0x00caacb4` | None |
| 15 | `0x76 / 4` | `0x00cb41e4` | None |

The 13 unconditional actions have the same six-word constant-return body. The selected branches of the other two actions have no shared-state stores or calls. Every listed cell has null transition 1. The common `0x00bfb028 → 0x00d0cc68` pre-call writes only the dispatch-context halfwords before an empty `0x00d3cde8`; wrappers 9 and 10 can additionally call `0x00635ec0`, whose sole store is `u32[0x00951508] = argument`. None of those bound effects changes APP, the remaining queried actors, `0x00951560` or mode. Thus the synchronous response vector composes within one UI-owner invocation if no foreign writer changes those fields. Ownership and absence of foreign writes are conditions still to prove, not inferred from a snapshot.

An all-`0xffffffff` vector invokes `0x00bff63c`. APP `0xc` then selects module `0x19`, row 0; in state 0 its action is `0x00c2cd30`. That action calls `0x00d3cf28` before testing `u32[0x00950438]`: only a value other than 1 takes `0x00d191f0` and its opcode-`0x201` request. The other branch invokes notification `0x00d38d28(0x50d)`, not by itself a bound queue-1 event. These module-`0x19` and flag conditions are additional to the 15-child lemma.

The selected S1's APP-`0xc` producer, query-time actor states, pending-5 production/preservation, request-stop ordering and successful `0x201` completion are not yet composed. Admission-time mode is not a lifetime invariant. The lemma neither establishes initial-work closure nor makes the E7/FileTask suffix feasible. Byte-backed cell/action inventories and the conditional proof are separate author artifacts `af/s1-query-cells-first.txt`, `af/s1-query-cells-second.txt` and `af/s1-query-idle-lemma.txt`; the frozen review packet is unchanged. Stage 1 remains **NON-ADMISSIBLE**.



#### Selected bank-writer predicates and review sources

These selections bind conditional forward dispatcher edges, not an admitted-S1 origin. The [selected graph](cardless-capture-feasibility.md#21-selected-stage-1-scope-and-post-admission-graph) owns the full effect and VM-envelope details.

| Candidate | Required dispatch selection and effect |
| --- | --- |
| `0x00ec88c8` | System `0x603`, root state 4, then module-9 row 3/state 0 cell `0x00ec5152=0x0106`, and cached status `u32[u32[0x015c20a8]+0x224]==0`. Root 1 rejects. Actor-8/application-`0x2c3` UI producer uses token `0x1540`. Reached code performs NAND program/conditional erase before optional bank export. |
| `0x00ecc654` | Task-7 `0x1707`, root state 6/module-0 state 3, payload byte `0x21`, module-2 decimal row 25 (`0x19`), state `{0,1,2,3,5,6}`, action `0x1c`. Reloads selector `bank[0x81]` through `0x0069fe20`; it is not bank export. |
| `0x00ecc66c` | Same outer owners, payload byte `0x22`, decimal row 26 (`0x1a`), action `0x1d`; module-2 state 4 excludes both candidates. Exports through `0x0069fea8` to NAND program/erase. |

Task-7 `0x1705` additionally requires root state 3 before module-0 state 0 or 3 can register `0x00dc7564` through queue-`0x24` operation `0x3403`. VM command `0x1a0002` has a concrete setter envelope, but its command-buffer producer, pending request ownership and inherited-transaction exclusion remain open.

The default initializer `0x0069f144` clears `0x420000` bytes, then applies 125 ROM entries from `0x007c8a44` through `0x007c8c34`, ending at sentinel `0x007c8c38`. None names property `0x244` or any of the seven diagnostic guards, so those bytes are zero after that initializer returns. The valid-bank branch of `0x0069f204` does not reset `0x244`. Later full/paged reloads and generic byte stores remain separate writer routes.

Kind-6 completion has a source-bound store: after `0x0069f920` returns, common tail `0x0073b69c` reads its completion word and, if zero, `0x0073b6ac` (`E7863105`, `STR r3,[r6,r5,LSL #2]`) writes 1 to `u32[0x00944458]` before releasing its semaphore and calling back. A conditional reset is bound: `0x0069cdf8(0)` with `u32[0x0061c008]==1` clears `[0x0089ec00,0x00bf1a00)`, including that array. Its selected-epoch invocation, later reset/alias exclusion and cache/semaphore visibility against admission remain open. Worker nonzero-slot suppression and wrappers' exact-1 shortcuts do not validate loaded bank data. Startup child readiness is published before asynchronous kind 6; neither that state nor assumed raw/logical-read idempotence establishes completion.

This latch records a returned attempt, not validated bank data. The shared tail derives callback error status from `r8` at `0x0073b6e0-0x0073b6e8`, but kind-6 callee `0x0069f920` ignores its underlying reload return values and returns 0. A nonzero return from that callee has not been established.

Normal startup does not supply a kind-6 callback witness through `0x0073cd4c`: its `(6,0,0)` request has a null callback, and the subsequent `0x0073cce8(3)` is another asynchronous null-callback request, not a barrier. The retained develop-job lists narrow one existing synchronous-wait lead to jobs `0x03/0x0c/0x0d/0x0e/0x0f`; its `0x00e9bd88 → 0x00e54c84` call requires frame field `+0x27cc == 0`, then `0x00e51858() != 3`. Those job identities have not been mapped to the initial preview or admitted S1. They are not a completion witness without that mapping.

The retained design packet is `private-data/analysis/x-m1-1.50/af/s1-design-review-packet.txt`. Its source windows are relative to `private-data/analysis/x-m1-1.50`:

| Evidence | Source window |
| --- | --- |
| Normal startup and actor creation | `lz/id4.dis:131933-132017`, `:131573`, `:293629-293632`, `:294717-294742`; startup descriptor `:131734-131752` |
| Root publication order | `lz/id4.dis:296817-296823` |
| E7 publication and callback | `af/functions/00ec3740.c:1-12`, `00ec6790.c:1-34`, `0071f550.c:1-33`, `0071fde4.c:1-19`, `00ec591c.c:1-19` |
| Bank read/default/store contracts | `af/functions/0069f920.c:1-71`, `0069f144.c:1-23`, `0069f204.c:1-31`, `0069fe20.c:1-21`, `0069fc94.c:1-8`, `00dc7564.c:1-61` |
| Export request selectors | `af/functions/00ec1b5c.c:1-54`, `00ec34a0.c:1-31`, `00ec8eac.c:1-19`, `00ec9314.c:5-9`, `00eca4c0.c:13-36,162-170`, `00ec9b50.c:1-37` |
| Mechanically extracted ROM cells and pointer vectors | `af/s1-ecc654-ecc66c-registration-extract.txt`, `af/s1-registration-followups.txt`, `af/s1-bank-default-key-extract.txt` |



### Selected staged implementation

| Option | What it could unblock | Risks and advancement boundary |
| --- | --- | --- |
| Deferred slot replacement | Restore ordinary storage, playback and the missing SD recovery route later | The damaged slot remains outside this software mechanism. |
| Stage 1: still-S1 admission | Stock AF initiation despite CARD ERROR, preserving every other stock inhibition | One entry-word publication through the shared installer; source-bound post-admission storage analysis, emulator admission proof and independent mechanism review are required. Admission is not proof of AF completion or USB-attached optical behavior. |
| Stage 2: no-save S2 capture | Complete a stock exposure and processing transaction, then discard the frame or provide a separately bounded export lease | Save launch and completion must be replaced as a transaction, while release and preview restoration still run. Metadata/file-number preparation before save, cancellation and all storage side effects need their own closure. |

Only stage 1 is implemented here. Stage 2 is a design, not an S2 permission or a capture implementation. The rejected recorder remains unchanged; admission, no-save completion and any future indispensable observation hooks have distinct justification and independent-review scopes.

### Stage-1 publication and review brief

`menu/cardless_af.c`, `menu/cardless_af_hooks.S`, `menu/cardless_af_ram.ld` and `menu/cardless_af_allowlist.json` define a stateless target C guard with an assembly ABI veneer in an owned `0x2000` allocation. Content occupies at most the first `0x1000`; the unchanged shared installer source follows at the next 32-byte boundary. ABI 6 has zero setup entries and one activation entry: address `0x00d19324`, old word `0xe92d4800`, new word a range-checked A32 `B` to `cardless_af_admission`. Both continuation branches are range-checked by the linker and mechanically restricted to `0x00d19328` and `0x00d19340`.

The Haskell builder `//host/firmware:build-cardless-af-ram` checks the private image-5 identity, source-bound guard/prologue words, pinned relink tools, allocation/publication nonoverlap, ELF confinement, instruction/call/store allowlist and specialized installer entries. Internal branches must remain in their owning function and target A32-mapped code; only the veneer may call the guard or branch to the two stock continuations. It emits the normal runtime and publication manifest through `build-cardless-af-ram.sh`. Existing installer source/ABI, old payload offsets and review pins are unchanged. New count specialization is a separate reviewed-byte question, not a byte-identity claim.

The observed ABI-6 installer entries, relative to its own start, are install `0x5ac`, rollback `0x7c0` and status `0x7ec`. These are additive registrations for the new payload; the 253-publication and rejected-ring offsets remain unchanged. The one-publication compiler specialization is materially larger than the earlier six-publication diff: it removes loops and changes conditional-return lowering as well as immediates and register allocation.

The complete `llvm-objdump -dr --print-imm-hex` listings of the retained 253 object and the new `ram_push_installer.o`, their unified diff and a 22-section census are retained under `private-data/analysis/x-m1-1.50/cardless-af/installer-*`. Nine text sections differ; the other thirteen have identical instruction and relocation listings. This inventory includes literal-pool/relocation movement inside changed sections, not just source-level count substitutions.

| Changed installer section | Instruction inventory against the 253 object |
| --- | --- |
| `validate_old_words` | Replaces the 253-entry loop with one `LDRD r0,r1,[r0]` address/old-word load, one live-word comparison, conditional error return and one validation-counter increment. 21 instructions become 14. |
| `validate_rollback_words` | Replaces the loop with one `LDRD r2,r3,[r0]`, old/new comparisons, conditional loads/return and one counter increment. 25 instructions become 16. |
| `peer_park_action` | At `+0x120`, `e3a010fd MOV r1,#253` becomes `e3a01001 MOV r1,#1`. |
| `synchronize_current_core` | Removes the iteration/back-edge and fixes the publication index at zero; preserves the cache-line operation, DSB, instruction-cache invalidation and ISB. Saved-register selection changes from `r4,r5` to `r4,r10`. 23 instructions become 18. |
| `read_peer_report` | At `+0x54`, `e25100fd SUBS r0,r1,#253` becomes `e2510001 SUBS r0,r1,#1`. |
| `execute_operation` | ABI immediate 2 becomes 6; the count/alignment/phase loop becomes one alignment test and phase-equals-one check; publication-count immediate 253 becomes 1. Operation/context registers change from `r8,r10` to `r5,r8`; the early-return block moves, branch destinations and literal loads move, and call relocations follow their new instruction offsets. 105 instructions become 96. |
| `store_new_words` | Replaces the loop and setup/activation selection with one new-word store and one activation-counter increment. 20 instructions become 11. |
| `restore_old_words` | Replaces reverse iteration with `LDRD r0,r1,[r0]`, one old-word store and one rollback-counter increment. 16 instructions become 10. |
| `peer_cache_action` | At `+0x40`, `935700fd CMPLS r7,#253` becomes `93570001 CMPLS r7,#1`. |

The three newly introduced publication-table `LDRD` sites load the first address/old-word pair at entry offset zero. The linker aligns the sole publication to eight bytes and the builder checks its absolute alignment. The other retained doubleword loads are not new count-one lowerings.

The review question is whether these single-publication lowerings preserve validation, all-or-rollback publication, cache synchronization and failure semantics for counts `(setup=0, activation=1)`, including the new paired loads and their alignment. They are not only count/ABI immediate changes, and byte identity with the 253 installer is not claimed.

The live camera CLI explicitly refuses this new unreviewed payload kind. The H0 harness `//emulation:cardless-af-e2e` may allocate, upload and execute the installer in the emulator; its admission cases execute real stock predicate bodies with isolated, explicitly synthetic RAM media status 4 and active warning `0x13`. The optional device-driven `present-init-fails` fixture is a distinct provenance class. Predicate success does not establish the asynchronous AF request's storage safety or a completed optical search.

The native executor admits only the additional non-PC `MRS CPSR` read and register-source flags-only `MSR` needed to preserve entry flags. SPSR, control-field writes, PC operands, immediate MSR, unsupported R5-only features and exclusive dependencies remain refused. The payload itself contains no exclusive access or multiprocessing instruction.

The independent reviewer must answer:

- Do the LR/frame/mode/argument guards bind the intended still-S1 call, and do both tails reproduce the displaced prologue and stock stack/register/flags contract?
- Does resuming at `0x00d19340` omit only warning `0x13`, with every other warning/lens/battery/mode/readiness inhibition still intact?
- Does argument `0x100` retain the UI-generated S2 warm-up refusal while argument-zero S1 and `0x110` replay retain their stock remaining checks, with the indistinguishable GPIO-generated `7,0` record and no-external-S2 contract stated explicitly?
- Does the single-word publication remain safe for suspended PCs at entry, entry+4 and later body locations, without relying on a global exclusive monitor?
- Does the source-bound post-admission graph exclude every SD-controller, filesystem, file-numbering and NAND-program/erase path, including asynchronous callbacks, while permitting only the stated stock read-only internal firmware/calibration fetches?
- Do the initial actor/input guards select the proved domain without assuming those fields remain constant, and do the property-bank, diagnostic-flag and capacity-refresh exclusions remain valid where their asynchronous source proofs require them?
- Does the one-publication ABI-6 installer preserve the shared rendezvous, publication/cache ordering and failure behavior despite its compiler specialization?
- Do the synthetic/device-driven fixture provenance and bounded admission results support only their stated scope, and does the proposed body session end through power removal plus post-reboot stock-word reads?

Stage-1 body operation is H2 with new mechanism bytes. It requires independent review, the owner's scoped go and a bounded proposal that excludes S2. A new session must verify the hook/context words, retain stock trace before and after its one ordinary S1 attempt, and stop on any unexpected behavior. End by removing battery power, reboot stock and read back `0x00d19324` plus the unchanged continuation/context words; do not invoke unload/free while a hook context may remain suspended.


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

Status: frozen host-only prototype `d3b8c1e`, **rejected for live use** by AfRingReview. Its two-word entry redirects have a blocking suspended-context hazard; the installer inventory below corrects the other executable differences identified in review. Hook rework waits for the stock-first observation and the per-hook necessity gate below. Payload source and build files remain unchanged. [#72](https://github.com/skeet70/latent-image/issues/72) owns the loop; [#102](https://github.com/skeet70/latent-image/issues/102) owns actual emulator half-press/producer inputs; [#103](https://github.com/skeet70/latent-image/issues/103) owns re-review and live-run readiness.

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
XM1_EMULATOR_SLOTS=1 devenv shell -- bazel \
  --output_base="$HOME/.cache/bazel/latent-image-AfLoop" run \
  --jobs=2 --local_resources=memory=6144 //emulation:xm1-af-ring-e2e -- \
  --firmware PRIVATE_RC0_DAT --installed-nand PRIVATE_F7_NONZERO_NAND \
  --sd-card-image PRIVATE_CARD_IMAGE --receipt-output PRIVATE_NEW_OUTPUT
```

All inputs and receipts stay under `private-data/`. The completion receipt distinguishes `ordinary-af` from `synthetic-fallback`. Only the former could support an emulated AF observation claim; neither is hardware evidence.

### Independent review brief

The completed independent review rejects the current six-publication mechanism. Its required corrections are: remove the suspended-entry+4 hazard if hooks are still needed; account for every installer specialization, including the different reverse-restore lowering; and establish the indispensable observation for each retained hook. CPU0 confinement, bounded ring stores and synthetic extraction were inspected, but those findings do not close the install hazard. Emulator unload/checkpoint behavior supplies no body-side hot-unload guarantee.

The AF payload has no approved review record or compiled AF pins. Live `ram-relink`, `ram-push` and `ram-unload` continue to refuse it. [#103](https://github.com/skeet70/latent-image/issues/103) records rejection and the stock-first dependency. Any reduced hook mechanism returns to the same independent reviewer after implementation and deliberate suspended-context cases.

#### Per-hook necessity gate before rework

Do not implement the one-word design merely to retain the existing three hooks. First inspect the before/after stock trace and existing bounded state/history observations. For each proposed hook, state the unresolved research question, the indispensable ordered observation, its producer meaning and why the existing observations cannot answer it. Remove a hook justified only by reading a current scalar or an already logged event. No hook currently has that necessity established.

| Candidate entry | Distinct signal it could add | Required stock-first decision |
| --- | --- | --- |
| Direction `0x00f41c48` | Ordered occurrence of this entry and its raw entry-time register/state sample, not a completed direction choice | Identify a question requiring invocation history that admitted request/lens sends and retained state/history cannot answer; bind the needed arguments before treating raw registers as decisions |
| Peak `0x00f42abc` | Ordered peak-routine entry history between stock sends, not a completed peak result | Establish why entry count/order is indispensable beyond the trace and retained peak arrays; otherwise omit it |
| Statistic consumer `0x00f44fec` | Entry occurrence and the **prior** `0x016c1160` value | Name an entry-order question that needs this history. A fresh accepted statistic is not supplied: the stock write is later at `0x00f4507c`; use the completed-producer/coherence work in #107 for that question |

If the stock observations suffice, retain no hooks. If they do not, reduce the publication contract to the justified subset, then implement and re-review it. Preserve the no-hot-unload, power-removal ending for any later hook trial.

### Camera-run proposal, not authorization

The stock-first block reached a valid baseline but not ordinary AF: cardless S1 produced the card-error indication. Resolve [the card-admission decision](#cardless-s1-result-and-decision-options) before proposing another attempt. After that decision, the feasibility question remains [specification §11.0](../spec/05-autofocus.md#110-stock-first-sensor-backed-iteration), not installation of this prototype: freeze the actual body/firmware, Viltrox AF 28mm F4.5 firmware 1.05, intended AF mode/region, household target, observation window and authorized read operations, then compare a completed ordinary AF attempt with its same-boot stock trace.

Any later ring trial first requires the stock observation to establish which hooks, if any, provide indispensable information. A justified subset then needs a safe one-word replacement, revised checks, deliberate suspended-context cases, re-review by the same reviewer and a separate owner H2 go naming the revised contract and actual-base digest. The rejected `d3b8c1e` bytes must not be installed on the body. Stored nonzero `0xf7` and ordinary photography-capable ff80 must already exist. The broken SD slot removes a recovery route and prevents ordinary save/playback acceptance; neither the stock-first block nor the ring trial repairs that limitation.

1. Freeze the unresolved event question, source/build identities, observed settings, intended region, clocks/unknowns, finite observation/session budget and exit procedure. Do not demand a manual-focus ring, per-attempt target movement or external high-speed video.
2. At the idle shooting screen, perform the existing scalar/worker preflight and compare all three original entry words and their three untouched `ADD` words. Require the stock query callback and the installer's preconditions.
3. Allocate once, relink at the returned base, reject any out-of-range branch, pass the revised AF check and actual-base review, then install once. Retain manifest, runtime, outgoing journal and mailbox. This is one approved load, not an automatically repeated batch.
4. Read the initial ring, have the owner half-press/release once within the frozen observation window, then read the ring and relevant stock trace once after the attempt. The draft 10-second window is administrative, not a proved AF execution bound. Retain empty/overflowed outcomes and unknown completion. Never invoke the synthetic entry or submit a lens command on the body in this trial.
5. Any unexpected reply, delay, display behavior or disconnect stops new commands without retry. An uncertain read chunk or state-changing reply does not authorize a software cleanup attempt. Tell the owner and end the session through the agreed physical exit.
6. Do not call `ram-unload` or `ram-free`. Emulator unload is not proof of physical hot-unload safety or of the absence of saved PCs in payload code. Remove USB and battery/dummy power for at least ten seconds, as in E-RAM-PUSH253-CONVERGED-RUN-150 run B. A switch cycle alone did not clear an earlier bank push.
7. After owner-confirmed ordinary startup, separately authorized reads compare all three entry words, the three untouched `ADD` words and the callback slot with the pinned stock values. Record the normal shooting-screen/cardless exit check and storage limitation. Never reuse the old allocation, epoch or pending action state after power removal; no camera checkpoint is restored.

A passing ring sample would establish observation of named CPU0 entry boundaries, not coherent snapshots, a bounded lens controller, physical motor completion or G5 overhead acceptance. Freeze any later stock-versus-instrumented overhead bound from stock variability. This ring has no acquisition clock and supplies best-effort ordered records rather than a measured AF timeline.

### Count-specialized installer bytes and review question

The shared source and ABI remain unchanged. They define publication, setup, activation and ABI values as compile-time macros in the same translation unit, without LTO. The AF tuple is `(6,3,3,5)`; the converged bank tuple is `(253,167,86,2)`. AF requires installer-relative entries `0x600`, `0x834`, `0x860`, code extent `0xa80`, and complete installer extent `0xbe0`. Existing payload entries and reviewed pins remain unchanged. The AF builder validates these AF-specific extents and entries.

An isolated comparison compiled the unchanged installer with the bank's exact pinned compiler flags, linked it at the AF comparison address `0x00a82d00` with identical runtime bounds, and extracted `.ram_push.*`. It is a controlled build comparison, not a camera extraction. `cmp installer-253.bin installer-af.bin` returned 1, first difference at byte 57. Both files remain under `private-data/analysis/x-m1-1.50/af-ring/`.

| File | Bytes | SHA-256 |
|---|---:|---|
| `installer-253.bin` | 7008 | `64009309819378629a024642aac3429770176caa94c6f8d56f460f9421c2a898` |
| `installer-af.bin` | 3040 | `562bdb3dfbe1db02b801a4f394ee0e0e626983756efca0e2c597f0868472cc0f` |

The earlier inventory omitted seven count-immediate instructions in six functions and the different lowering of `restore_old_words`. The claim that the differences are only count/ABI immediates plus `r0` reuse is withdrawn. A fresh LLVM 21.1.8 disassembly of **both retained relocatable objects**, with a full text diff and separate relocation inspection, gives the complete changed-code-section inventory below. Offsets are relative to each function, not the linked installer base. All other disassembled code sections are identical before relocation.

| Function and offset | 253 instruction word and instruction | AF instruction word and instruction |
| --- | --- | --- |
| `validate_old_words+0x18` | `e37300fd`, `cmn r3,#253` | `e3730006`, `cmn r3,#6` |
| `validate_rollback_words+0x18` | `e37300fd`, `cmn r3,#253` | `e3730006`, `cmn r3,#6` |
| `peer_park_action+0x120` | `e3a010fd`, `mov r1,#253` | `e3a01006`, `mov r1,#6` |
| `synchronize_current_core+0x10` | `e35400fd`, `cmp r4,#253` | `e3540006`, `cmp r4,#6` |
| `read_peer_report+0x54` | `e25100fd`, `subs r0,r1,#253` | `e2510006`, `subs r0,r1,#6` |
| `execute_operation+0x44` | `e3a01002`, `mov r1,#2` | `e3a01005`, `mov r1,#5` |
| `execute_operation+0x84` | `e35000fd`, `cmp r0,#253` | `e3500006`, `cmp r0,#6` |
| `execute_operation+0x9c` | `e35000a6`, `cmp r0,#166` | `e3500002`, `cmp r0,#2` |
| `store_new_words+0x14` | `e35000fd`, `cmp r0,#253` | `e3500006`, `cmp r0,#6` |
| `store_new_words+0x20` | `e35000a6`, `cmp r0,#166` | `e3500002`, `cmp r0,#2` |
| `peer_cache_action+0x40` | `935700fd`, `cmpls r7,#253` | `93570006`, `cmpls r7,#6` |

In `execute_operation`, every changed same-offset PC-relative load/early branch is listed here. The exit moves from `+0x198` to `+0x194`; the literal pool moves back four bytes. The target roles remain unchanged.

| Offset | 253 word | AF word |
| --- | --- | --- |
| `0x0c` | `e59f7190` | `e59f718c` |
| `0x38` | `e59f1168` | `e59f1164` |
| `0x74` | `1a000047` | `1a000046` |
| `0x78` | `e59f412c` | `e59f4128` |
| `0x94` | `1a00003f` | `1a00003e` |
| `0xb8` | `ea000036` | `ea000035` |
| `0xbc` | `e59f10ec` | `e59f10e8` |
| `0xc0` | `e59f00ec` | `e59f00e8` |
| `0xc8` | `e59f00e8` | `e59f00e4` |
| `0xe4` | `1a00002b` | `1a00002a` |
| `0xe8` | `e59f10cc` | `e59f10c8` |
| `0xf0` | `e59f50c0` | `e59f50bc` |
| `0x104` | `1a000023` | `1a000022` |
| `0x108` | `e59f10b0` | `e59f10ac` |
| `0x130` | `e59f0078` | `e59f0074` |

The rendezvous argument setup has these instruction-level changes or moves. Instructions at `+0x110`, `+0x118`, `+0x11c`, `+0x124` and `+0x12c` are unchanged. The fixed payload length remains 32 bytes, zero begin index remains zero and every stack argument retains its role.

| 253 offset, word and instruction | AF offset, word and instruction |
| --- | --- |
| `+0x10c e3a03000 mov r3,#0` | `+0x10c e3a02000 mov r2,#0` |
| `+0x114 e3a020fd mov r2,#253` | Removed; AF reuses the syscall argument `r0=6` at `+0x138` |
| `+0x120 e5813004 str r3,[r1,#4]` | `+0x120 e5812004 str r2,[r1,#4]` |
| `+0x128 e581300c str r3,[r1,#12]` | `+0x128 e581200c str r2,[r1,#12]` |
| `+0x134 e58d2014 str r2,[sp,#20]` | `+0x150 e58d0014 str r0,[sp,#20]` |
| `+0x138 e28d2004 add r2,sp,#4` | `+0x140 e28d2004 add r2,sp,#4` |
| `+0x13c e58d3010 str r3,[sp,#16]` | `+0x13c e58d2010 str r2,[sp,#16]` |
| `+0x140 e3a03020 mov r3,#32` | `+0x114 e3a03020 mov r3,#32` |
| `+0x144 e58d0018 str r0,[sp,#24]` | `+0x134 e58d0018 str r0,[sp,#24]` |
| `+0x148 e3a00006 mov r0,#6` | `+0x138 e3a00006 mov r0,#6` |
| `+0x14c e58d400c str r4,[sp,#12]` | `+0x144 e58d400c str r4,[sp,#12]` |
| `+0x150 e58d1020 str r1,[sp,#32]` | `+0x148 e58d1020 str r1,[sp,#32]` |
| `+0x154 e58d901c str r9,[sp,#28]` | `+0x14c e58d901c str r9,[sp,#28]` |

Every instruction from 253 `+0x158..+0x1a0` moves to AF `+0x154..+0x19c` with its instruction word unchanged. This includes cache maintenance, DSB, the indirect peer-cache call, `read_peer_report`, the two conditional exits, `run_rendezvous` and the epilogue. The call relocations move `+0x170 -> +0x16c` and `+0x190 -> +0x18c`, retaining their target symbols. The nine literal slots at `+0x1a4..+0x1c4` move to `+0x1a0..+0x1c0`; their values/symbol roles are unchanged. The relocation table has no other target/offset change. Linked branch words and absolute addresses also follow the changed function/table/mailbox layout; they are not new calls or new stock addresses.

`restore_old_words` has a different loop, frame and epilogue, not merely a shifted immediate. Its full executable sequence is:

| Offset | 253 word and instruction | AF word and instruction |
| --- | --- | --- |
| `0x00` | `e92d4800 push {r11,lr}` | `e92d4830 push {r4,r5,r11,lr}` |
| `0x04` | `e1a0b00d mov r11,sp` | `e28db008 add r11,sp,#8` |
| `0x08` | `e59fc030 ldr r12,[pc,#48]` | `e59f1030 ldr r1,[pc,#48]` |
| `0x0c` | `e3a00000 mov r0,#0` | `e3a00000 mov r0,#0` |
| `0x10` | `e59f202c ldr r2,[pc,#44]` | `e59f202c ldr r2,[pc,#44]` |
| `0x14` | `e3700efd cmn r0,#4048` | `e3700006 cmn r0,#6` |
| `0x18` | `08bd8800 popeq {r11,pc}` | `0a000007 beq +0x3c` |
| `0x1c` | `e08c3000 add r3,r12,r0` | `e0813200 add r3,r1,r0,lsl#4` |
| `0x20` | `e2400010 sub r0,r0,#16` | `e2400001 sub r0,r0,#1` |
| `0x24` | `e5931fc0 ldr r1,[r3,#4032]` | `e1c345d0 ldrd r4,r5,[r3,#80]` |
| `0x28` | `e5933fc4 ldr r3,[r3,#4036]` | `e5845000 str r5,[r4]` |
| `0x2c` | `e5813000 str r3,[r1]` | `e5923020 ldr r3,[r2,#32]` |
| `0x30` | `e5921020 ldr r1,[r2,#32]` | `e2833001 add r3,r3,#1` |
| `0x34` | `e2811001 add r1,r1,#1` | `e5823020 str r3,[r2,#32]` |
| `0x38` | `e5821020 str r1,[r2,#32]` | `eafffff5 b +0x14` |
| `0x3c` | `eafffff4 b +0x14` | `e8bd8830 pop {r4,r5,r11,pc}` |

Both forms start at the last publication and restore descending rows, updating the restored count once per store. Bank `r0=0,-16,..,-4032` addresses rows 252 through 0 using `table+r0+0xfc0`; `r0=-4048` exits before a read. AF `r0=0,-1,..,-5` addresses rows 5 through 0 using `table+16*r0+0x50`; `r0=-6` branches to its separate epilogue. AF's LDRD loads the adjacent address/old-word pair. At the exercised link the table starts at `0x00a83780`, so every load address `0x00a837d0..0x00a83780` in descending sixteen-byte steps is sixteen-byte aligned and its eight-byte pair stays within a row. The new access does not rely on unaligned LDRD support.

AF saves/restores the newly used callee-saved `r4,r5` along with `r11,lr`; its stack frame is sixteen bytes rather than the bank form's eight, and its frame pointer is entry-SP minus eight. Each matching POP restores that frame and returns through the saved LR. Both have balanced SP, but their instruction paths, frame sizes and register allocation are not identical. The reviewer found equivalent reverse-restore effects for these aligned records, not an unchanged control-flow graph or approval of the AF hooks.

Reproduction uses the two objects recorded in E-AF-RING-REVIEW-CORRECTION-150 and the pinned development shell. Keep the full diff and the independent semantic delta privately; the tables above enumerate all executable changes rather than selecting only the ABI setup:

```sh
# BANK_OBJECT and AF_OBJECT name the retained controlled-253 and AF installer objects.
devenv shell -- llvm-objdump -d --print-imm-hex --triple=armv7-none-eabi "$BANK_OBJECT" > "$PRIVATE_OUTPUT/installer-253-object.txt"
devenv shell -- llvm-objdump -d --print-imm-hex --triple=armv7-none-eabi "$AF_OBJECT" > "$PRIVATE_OUTPUT/installer-af-object.txt"
diff -u "$PRIVATE_OUTPUT/installer-253-object.txt" "$PRIVATE_OUTPUT/installer-af-object.txt"
devenv shell -- llvm-objdump -r "$BANK_OBJECT" "$AF_OBJECT"
```

The private `author-installer-{253,af}-object.txt`, `author-installer-object-delta.diff` and independently produced `reviewer-installer-semantic-delta.json` retain the reproduced comparison. `installer-{253,af}.bin` remain unchanged. Corrected reviewer question: do all listed count/ABI changes, argument rescheduling and the distinct reverse-restore lowering preserve their bounded publication/rendezvous effects, with the LDRD alignment and callee-saved invariants stated above? This is broader than the withdrawn immediate-only question. The current hook mechanism remains rejected independently of the installer comparison.

### Exercised host and emulator result

E-AF-RAM-RING-E2E-150 records the successful `e2e-sixth` run on 2026-10-25. It restored the keyed ordinary-boot checkpoint, allocated at `0x00a81d00`, relinked runtime `fef60cf4565651f061a47577074679b85a3d38a2597b7e2464177528a7de80de`, installed six publications, and bulk-read a pristine ring. The synthetic callback then committed 300 events. The Haskell decoder returned the latest 256 CPU0 events, 44 overwritten events, overflow set and no observed contention loss. Mandatory unload completed before `e2e-complete.json` was written.

The run did not supply a half-press and did not observe genuine AF. `e2e-fifth/bridge/emulator-session.json` preserves `ValueError: Unknown menu key: half-press`; that rejected request terminates the retained bridge. The successful runner flags the missing binding without issuing it. The synthetic callback executes the real ring writer and does not seed ring bytes from the host. This proves a repeatable host/emulator publication and capture loop, not G5's physical AF or overhead gate.

Earlier failed attempts remain in the private `af-ring/e2e-first` through `e2e-fifth` directories. They exposed a missing private build directory, an unsafe mode on the agent-created artifact parent, the unthreaded host builder's blocked hash input pipe, the count-specialized installer extent, a symbol-list column mismatch, and the unsupported half-press request. The implementation fixes these host issues or explicitly flags the missing emulator input. `installer-{253,af}.bin` remain separate controlled-comparison evidence.

On the frozen `d3b8c1e` tree, host verification passed `//transport:af-ring-tests`, `//transport:read-bulk-tests`, `//transport:ram-push-tests` and `//transport:cli-rejections`, and built `//host/firmware:build-af-ring-ram`, `//host/firmware:read-af-ring` and `//emulation:xm1-af-ring-e2e`. The integrated entry-contract test covers AF's independently bound offsets alongside the unchanged existing payload offsets. The emulator command above completed with the supplied RC0 DAT, stored-F7 NAND and virtual card under `private-data/`. Those checks do not cover the suspended-context hazard or a one-word replacement. No camera or USB access occurred.

## Alignment with the stock-first, sensor-backed design

This note applies the design landed in `b5c5560` and main `7176d74`, [§11.0](../spec/05-autofocus.md#110-stock-first-sensor-backed-iteration), [snapshot/session contracts](../spec/08-artifact-and-runtime-contracts.md#camera-snapshots-and-live-experiment-sessions), the [pilot protocol](stock-pilot-protocol.md) and issues [#105](https://github.com/skeet70/latent-image/issues/105) through [#109](https://github.com/skeet70/latent-image/issues/109). It changes observation order and the proposed session exit, not the frozen payload. The successful synthetic emulator run remains evidence of transport/ring mechanics only.

### Stock signal to try before adding a hook

Yes: X-M1 1.50 already installs a filtered message-send trace whose loader-owned console store is readable through the ordinary-boot RAM route. E-AF-ORDINARY-TRACE-ROUTE-150 reconciles that route with the previous maintenance-only premise. It is not yet a demonstrated useful ordinary AF trace with USB attached.

The decoded startup calls `0x007262cc` and registers `0x00726478` through `0x00724cbc`; the setter writes the send-hook cell `0x4089ea94`. The filter at `0x00726478` consults `0x00726320` and the 1716 four-byte `{id,code}` rows at `0x0081485c`. Initialization sets mode `0x40a5b5e0=0`, filter enable `0x40a5b5e1=1`, threshold `0x40a5b5e2=1`, write offset `0x40a5b5e4=8` and compact-trace buffer pointer `0x40a5b5ec=0x41ac9800`. These are initializer values, not a substitute for observing the current boot's controls. No proposal here enables a gate or changes a parameter.

For admitted nonzero messages in mode 0, `0x00726478` calls loader writer `0x00041c8c` with the receiver/sender/id signature and zero extra payload words. That writer appends a 16-byte type-`0x0f` record to the store at `0x400a0800`, extent `0x1fff4`: task byte, timer delta, packed signature, code/parameter pair and tick from `0x400c0800`. It advances the next-index halfword and clears the new next slot. Capacity is 8188 with at most 8187 retained records. The measured tick is about 10 ms in its characterized operating condition. This is a send timestamp, not handler, sensor or motor timing. The compact image-4 buffer at `0x41ac9800` and the service text ring at `0x41acb800` are different facilities; neither is silently substituted for the console store.

Direct reads of the pinned image-4 table establish code 1 for the following ids. The listed table addresses contain little-endian `{id u16,code u16}`; the [console instrument](console-trace-instrument.md#2-the-instrument) owns the decoder and lifetime limits.

| Stock observation | Producer binding | Id and table row |
| --- | --- | --- |
| AF request send | `0x00df48dc -> 0x00ef9264 -> 0x00ef9a94`, operation `0x0d` to task `0x2b` | `0x3b0d` at `0x008155fc` |
| Search/lens bridge request send | `0x00f3e9cc -> 0x00fea634 -> 0x00fe9580` | `0x3f2e` at `0x00815968` |
| Lens movement request attempt | `0x00ff97a0 -> 0x0076e2d0`, destination lensdc task `0x32` | `0x420c` at `0x00815bc4` |
| Lens serializer traffic | `0x006bc8d8`, destination lenscom task `0x62` | `0x721d` at `0x008161d0` |
| Lensdc completion callback's send | `0x00ff5d80 -> 0x00fe9580` | `0x3f0d` at `0x008158e4` |
| Evaluation callback delivery | `0x00f3c104 -> 0x00ef9a94`, operation `0x89` | `0x3b89` at `0x008157ec` |
| Successful higher-layer result send | `0x00defa30 -> 0x00dcb950` only when status is zero | `0x13a5` at `0x00814ddc` |

The ordinary extraction candidate is physical `0x000a0800`, length `0x1fff4`, through the existing bounded `read-bulk` operation, decoded with `//console-trace:ring-check` and `//console-trace:trace-census`. The [stock-first block](#stock-first-console-observation-block) below supplies two admitted bulk windows whose union is the store; `0x1fff4` itself is not an admitted bulk length. Check the current header/filter/writer gates and preserve raw bytes and uncertainty. Read in the same boot before clearing or overwrite. A moving next index, wrap ambiguity or non-atomic read prevents a completeness claim. Message `0x3f0d` dates a callback send after the stock completion path, not optical settling; `0x420c` does not prove successful movement, and `0x13a5` does not carry the entire 16-byte result in the default mode-0 console record.

| Frozen ring event | Stock coverage first | Increment supplied only by the proposed entry hook |
| --- | --- | --- |
| Direction entry `0x00f41c48` | `0x3f2e` and lens traffic expose requests; readable stock search state and peak history can expose retained state, not every direction invocation | Exact invocation of this entry, raw entry `r0-r3` and state/statistic values sampled there |
| Peak entry `0x00f42abc` | Same stock send sequence; conditional peak history at `0x016c1238`, `0x016c1638`, `0x016c1a38` is already a candidate for offline extraction | Exact peak-function invocation and its entry-time register/state sample |
| Statistic-consumer entry `0x00f44fec` | Admitted `0x3b89` identifies an evaluation-completion send; current statistic/state words are already readable | Entry of the acceptance routine, distinct from the earlier queued send. The frozen hook reads `0x016c1160` before this routine writes it, so it does not record the newly accepted statistic or prove acceptance completed |

No ring event exactly duplicates a stock send record. That does not justify installing all three hooks: a question about request, completion-send or result ordering uses the stock trace first, and a question about retained values first uses existing bounded state/history reads. Raw `r0-r3` are not established semantic arguments. The ring records neither movement requests nor actual completion, result status or sensor acquisition; it cannot replace those stock observations.

There is also a conditional stock diagnostic candidate, not an established ordinary AF log: `0x00f44fec` calls `0x00f47e00` only when parameter `0x8791==1` and state word `0x016c0f54` is 4 through 6. The callee applies decimation parameter `0x8792`, selects fields with `0x8793`, and formats ten 32-byte rows at `0x016c222c`, including search-state labels such as `DIRTION` and `PEK_SRH`. Existing activation, selected fields, source-record lifetime and meaningful ordinary-boot contents are unobserved. Do not enable these parameters, treat text as a per-decision trace, or infer that diagnostic availability supplies the ring's exact entry samples.

### What the ring contributes to #107 and #106

| Contract | Delivered by the frozen prototype | Not delivered; advancement remains open |
| --- | --- | --- |
| [#107 coherent snapshots](https://github.com/skeet70/latent-image/issues/107) | Versioned bounded owned event storage, exact extent, sequence checks, overwrite/saturation accounting, CPU identity, sticky contention indication, cache-alias rationale and Haskell bulk extraction/decoding | No coherent immutable export, retention/release/expiry, session epoch, frame/sample identity, geometry or pixel domain, exposure/configuration, acquisition-time lens command, acquisition/delivery clocks or source freshness. The writer can overwrite during extraction. Slot checks are rejection aids, not DMA-coherence proof; sticky contention is not an exact drop count. No measured hook overhead or fresh scene-change observation exists |
| [#106 bounded batches](https://github.com/skeet70/latent-image/issues/106) | Host/emulator push/read/decode mechanics, explicitly synthetic provenance and finite event/transfer storage; it controls no lens, sensor, GPIO, storage or registers | No stock lens action/completion/cancel contract, controller ownership arbitration, supported-mode or position/rate/travel enforcement, attempt/pair IDs, epoch invalidation, camera-side action/session bounds, reference sweeps, randomized paired order, host-loss safe completion, or finite real optical-feedback batch. A host timeout and one manual half-press are not these guarantees |

Keep callbacks independent of USB and add only bounded copies after the active producer and its ownership are bound. Prefer a completed stock observation with an evidenced lifetime; otherwise the #107 producer must publish into separately owned retained export storage. Existing ring bytes do not become a validated sensor-statistic snapshot by adding an outer JSON header. #108 needs measured, source-bound observations and independent quality; #109 needs reproducible policy comparisons, not synthetic ring throughput. Conditional full-resolution work in #105 is not required to answer the stock-trace question or justify every ring hook.

### One-word-hook replacement design, not implemented

The critical independent review finding on `d3b8c1e` is confirmed: setup overwrites each executable `ADD` at entry+4 with pointer data. The installer rendezvous stops the running cores but does not prove that no suspended task has a saved PC at entry+4 after executing the original `PUSH`. Resuming such a task can execute the pointer word as an instruction. Idle-screen preflight and a passing synthetic push/unload do not exclude that context. Do not install those bytes on the body.

Prefer one aligned A32 unconditional `B` at each entry, directly to its wrapper in the owned allocation. A `B`, unlike `BL`, leaves the incoming LR unchanged. At relink, compute the signed displacement `wrapper-(entry+8)` without unsigned wrap, require four-byte alignment and the exact A32 range `[-0x02000000,+0x01fffffc]`, then encode `0xea000000 | ((displacement >> 2) & 0x00ffffff)`. Require every selected entry to fit for the actual allocation. The earlier emulator allocation is evidence of one address only, not a guarantee about the next allocation. Reject an out-of-range allocation; do not search for an unowned code cave, write an adjacent literal, widen the read bound or truncate the branch.

The revised wrapper preserves the original registers/flags, records the event, replays only the displaced first `PUSH`, and returns to untouched entry+4. It must no longer replay the second `ADD`; the stock instruction executes it once. A suspended task at entry+4 has already pushed and resumes its unchanged `ADD`; a task at entry enters through either the synchronized old `PUSH` path or the new branch/wrapper path. Tasks already beyond the entry continue stock code and may contribute no event. Activation therefore does not retroactively give complete coverage of an in-flight attempt.

This is a design option, not an instruction to implement three hooks now. Stock-first observations decide whether any hook is necessary. For a justified subset of N entries, use N activation publications and zero setup publications, a new publication/ABI identity and newly bound installer extents/entries for that count tuple. Existing reviewed installer source, other payload offsets and their pins stay unchanged. The checker must bind each selected original first word, branch destination/range, untouched second word, replayed `PUSH`, continuation and register/flags/SP/LR preservation. Rework then returns to the same independent reviewer. A range-limited owned veneer could be considered separately if direct branches cannot be placed; no such placement/lifetime evidence or adjacent literal slot is assumed.

The next emulator cases must resume a context saved at entry+4 with the first `PUSH` already reflected in its stack, and another already inside the stock body, across install. They must preserve the stock outcome and stack/register behavior while allowing the installation-boundary event gap. The one-word redirect removes this particular data-resumption hazard; it does not justify freeing wrapper memory while a saved PC can still reference it, prove safe hot unload, fix snapshot coherence, or establish the remaining AfRingReview findings. Physical session exit remains battery removal and post-reboot stock-word reads.

## Stock-first console observation block

**Blocked after the first attempt:** baseline extraction passed, but cardless S1 produced the red card-error indication and the block stopped without post reads (E-AF-CARDLESS-S1-BLOCKED-150). The script below records the attempted operation; it is not a proposal to retry until the card-admission decision is resolved.

### Observed Run-3 controls and retained traffic

**Hardware-observed bytes, host-only decoding, E-AF-STOCK-CONSOLE-RUN3-150:** the existing Run-3 capture contains a valid console store with capacity 8188, next index **2419 (`0x0973`)** and 2419 occupied records. `ring-check --quiet` counts 1189 type-`0x0f` messages; `trace-census` agrees on the index and occupied count. The 2419 records are the retained history of that boot, not a newly performed or isolated AF attempt. Unknown type `0x81` has no decoded tick.

| Control | Physical capture address | Observed bytes/value |
| --- | --- | --- |
| Trace mode, filter enable, threshold | `0x00a5b5e0..e2` | `00 01 01`, mode 0, enabled, threshold 1 |
| Compact-buffer write offset | `0x00a5b5e4` | `30 1d 00 00`, `0x1d30`, not the initializer's 8 |
| Compact-buffer pointer | `0x00a5b5ec` | `00 98 ac 41`, `0x41ac9800` |
| Loader console-writer gate | `0x0004f25c` | `01 00 00 00`, enabled |
| Installed send-hook pointer | `0x0089ea94` | `78 64 72 00`, `0x00726478` |
| Header capacity / next index | `0x000a081c / 0x000a081e` | `fc 1f / 73 09`, 8188 / 2419 |

All 6864 bytes of the 1716-row filter table in Run 3 match the pinned image-4 table. Thus the seven previously bound AF ids have observed code 1 under the observed threshold 1, not merely initializer-based admission.

| Bound id | Retained sends | Slots | Decoded send ticks |
| --- | --- | --- | --- |
| `0x3b0d`, signature `0x3b0d032b` | 8 | 456, 541, 630, 790, 1960, 2045, 2172, 2257 | 602, 963, 1155, 1410, 71250, 71318, 127303, 127373 |
| `0x13a5`, signature `0x13a52b03` | 8 | 457, 542, 631, 791, 1961, 2046, 2173, 2258 | 603, 963, 1155, 1410, 71250, 71318, 127303, 127373 |
| `0x721d`, signature `0x721d3262` | 3 | 134, 1090, 1620 | 63, 1710, 14006 |
| `0x3f2e`, `0x420c`, `0x3f0d`, `0x3b89` | 0 each | None in this retained store | Unavailable |

The eight request/result pairs establish retained AF-layer sends, not an observed lens search, physical focus, evaluation completion or green-box success. Their times span the boot; boot initialization and earlier ordinary interactions are not separated by an attempt marker. A new before/after block must establish that association. Do not infer USB caused an AF failure from these bytes alone.

Reproduce the retained decode from the repository root. Absolute decoder paths avoid dependence on Bazel's run directory. The commands operate on private files only:

```sh
ROOT=$PWD
BAZEL_BASE="$HOME/.cache/bazel/latent-image-AfLoop"
S="$ROOT/private-data/sessions/20261025T050355Z-read-bulk/ram-00000000-05aa0000.bin"
STORE="$ROOT/private-data/analysis/x-m1-1.50/af-ring/run3-console-store.bin"
export S STORE
sha256sum "$S"
devenv shell -- ghc -e 'import qualified Data.ByteString as B' -e 'import System.Environment (getEnv)' -e 'do { s <- getEnv "S"; out <- getEnv "STORE"; bytes <- B.readFile s; B.writeFile out (B.take 0x1fff4 (B.drop 0xa0800 bytes)) }'
devenv shell -- bazel --output_base="$BAZEL_BASE" run --jobs=2 --local_resources=memory=6144 //console-trace:ring-check -- --quiet store "$STORE"
devenv shell -- bazel --output_base="$BAZEL_BASE" run --jobs=2 --local_resources=memory=6144 //console-trace:trace-census -- store "$STORE"
xxd -g1 -s0x00a5b5e0 -l16 "$S"
xxd -g1 -s0x0004f25c -l4 "$S"
xxd -g4 -s0x0089ea94 -l4 "$S"
cmp --ignore-initial=8472668:2065500 --bytes=6864 "$S" "$ROOT/private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin"
```

Private provenance: Run-3 source SHA-256 `fa933fcf723ff46d435161cfecf48edc4a29cc4dc4c0ff5232fd0c70e7a6fee2`; extracted `0x1fff4`-byte store SHA-256 `e9ec7b74c11fe3d868bccc58c4d706d420795b3767bc67964a1bce6ef2f19d75`. The decoders and the three operation shapes below were exercised offline. These observations do not establish a coherent DMA snapshot.

### Closed operation, scope and preparation

Under [spec/07 §3.2](../spec/07-governance-and-handoff.md#32-operation-classes), preparation and retained decoding are **H0**; the five-window `read-ram` operation is **H1**; this combined camera block is **H2**, using the already reviewed `read-bulk` mechanism. It installs no payload or hook and sends no config, gate, lens, storage or register write. Nevertheless the stock bulk query/E1 path rewrites its published source pair and transfer bookkeeping, so “read” does not make it H1. The manual half-press is an ordinary owner action, not a new USB focus-control mechanism. The standing H1 grant alone does not cover this H2 block; request the operation-specific confirmation in chat. No new mechanism review is required for these existing closed operations and bounded RAM windows.

Scope: ordinary X-M1 1.50 shooting screen, current baseline, no card, unchanged target and mode, one half-press and release, same boot. The owner places the supported camera on a table, points it at a clear book/package edge and confirms an adequate battery and normal preview. Do not enter a menu, jig mode or playback; insert no card. There is no preview/statistic snapshot, automated lens action, recorder, gate enable, full shutter press or persistent change.

The console extent is not a multiple of 64. Two single-chunk operations avoid reading the adjacent twelve bytes or the tick cell through DMA:

| Read | Address | Length | End, exclusive |
| --- | --- | --- | --- |
| Main | `0x000a0800` | `0x0001ffc0`, 131008 bytes | `0x000c07c0` |
| Tail | `0x000c07b4` | `0x00000040`, 64 bytes | `0x000c07f4` |

The windows overlap by twelve bytes and their union is `[0x000a0800,0x000c07f4)`. Reconstruction checks the overlap and lengths before dropping the duplicate twelve bytes. Gate/header/tick reads use the fixed `read-ram` plan, not a widened bulk window.

Run the following setup and rehearsals **before attachment**. Keep the shell open. The plan must be an owned regular file with mode 600. The shell functions are command abbreviations, not additional camera operations.

```sh
ROOT=$PWD
BAZEL_BASE="$HOME/.cache/bazel/latent-image-AfLoop"
PLAN="$ROOT/private-data/read-plans/stock-af-console-controls.json"
SESSION="$ROOT/private-data/sessions/$(date -u +%Y%m%dT%H%M%SZ)-stock-af-console"
export ROOT PLAN SESSION
umask 077
mkdir -p "$ROOT/private-data/read-plans" "$SESSION/pre" "$SESSION/post"
printf '%s\n' '{"windows":[{"index":0,"address":"0x40a5b5e0","length":16,"purpose":"stock trace mode, filter enable, threshold, compact offset and pointer"},{"index":1,"address":"0x4004f25c","length":4,"purpose":"loader console writer gate"},{"index":2,"address":"0x4089ea94","length":4,"purpose":"stock send-hook pointer"},{"index":3,"address":"0x400a0800","length":52,"purpose":"console header, capacity and next index"},{"index":4,"address":"0x400c0800","length":4,"purpose":"console tick cell"}]}' > "$PLAN"
chmod 600 "$PLAN"
run_target() { devenv shell -- bazel --output_base="$BAZEL_BASE" run --jobs=2 --local_resources=memory=6144 "$@"; }
camera() { run_target //transport:camera -- "$@"; }
assemble_decode() {
  export AF_PARTS="$1"
  devenv shell -- ghc -e 'import qualified Data.ByteString as B' -e 'import System.Environment (getEnv)' -e 'getEnv "AF_PARTS" >>= \d -> do { a <- B.readFile (d ++ "/main/ram-000a0800-0001ffc0.bin"); b <- B.readFile (d ++ "/tail/ram-000c07b4-00000040.bin"); if B.length a == 0x1ffc0 && B.length b == 64 && B.drop 0x1ffb4 a == B.take 12 b then B.writeFile (d ++ "/store.bin") (a <> B.drop 12 b) else fail "console length or overlap mismatch" }' &&
    run_target //console-trace:ring-check -- --quiet store "$1/store.bin" &&
    run_target //console-trace:trace-census -- store "$1/store.bin"
}
camera rehearse read-ram "$PLAN"
camera rehearse read-bulk 0x000a0800 0x0001ffc0
camera rehearse read-bulk 0x000c07b4 0x00000040
```

### Baseline, one attempt and same-boot extraction

Execute each live line only after the scoped confirmation. A failure stops the sequence; do not paste the block as an unattended batch. Every output directory below is new.

```sh
camera read-ram "$PLAN" "$SESSION/pre/controls-before"
camera read-bulk 0x000a0800 0x0001ffc0 "$SESSION/pre/main"
camera read-bulk 0x000c07b4 0x00000040 "$SESSION/pre/tail"
camera read-ram "$PLAN" "$SESSION/pre/controls-after"
assemble_decode "$SESSION/pre"
```

Inspect both control snapshots before touching the shutter. Each `read-ram` capture contains `config-000000.bin` and `ram/000-ADDRESS.bin`. `0xf7` at config offset 247 must be nonzero; the operation itself enforces this. Require mode/enable/threshold `00 01 01`, writer gate `01 00 00 00`, hook pointer `78 64 72 00`, capacity 8188 and compact pointer `00 98 ac 41`. The compact write offset is dynamic, not required to equal Run 3. Record the tick and both next indices. A different gate/route requires a different proposal, not a write to repair it.

```sh
for phase in controls-before controls-after; do
  D="$SESSION/pre/$phase"
  od -An -tx1 -j247 -N1 "$D/config-000000.bin"
  xxd -g1 "$D/ram/000-40a5b5e0.bin"
  xxd -g1 "$D/ram/000-4004f25c.bin"
  xxd -g1 "$D/ram/000-4089ea94.bin"
  od --endian=little -An -tu2 -j28 -N4 "$D/ram/000-400a0800.bin"
  od --endian=little -An -tu4 "$D/ram/000-400c0800.bin"
done
```

The preceding `assemble_decode` call checks the overlap and runs both public decoders on the baseline. Require a contiguous occupied prefix with `entries-decoded == next-index < 7164`, leaving at least 1024 slots, and zero unoccupied slots as represented by the census. Existing wrap, a hole, overlap mismatch or a changing stable prefix makes this block unsuitable for a bounded before/after comparison. A next index advancing during extraction is recorded, not silently normalized into an atomic boundary.

The owner then half-presses **once**, holds only until a normal AF indication or at most five seconds, and releases without fully pressing. Record whether the box turned green and visible focus motion occurred, plus the approximate host time of press/release. If the owner reports **no green box or no focus motion**, stop the block without further camera commands. Record “USB-attached AF feasibility not established”; do not retry, disconnect/reconnect to force AF, modify gates, or infer that USB caused the outcome.

Only if normal AF behavior was reported, leave the body/target untouched and run:

```sh
camera read-ram "$PLAN" "$SESSION/post/controls-before"
camera read-bulk 0x000a0800 0x0001ffc0 "$SESSION/post/main"
camera read-bulk 0x000c07b4 0x00000040 "$SESSION/post/tail"
camera read-ram "$PLAN" "$SESSION/post/controls-after"
assemble_decode "$SESSION/post"
```

Repeat the displayed control inspection loop with `D="$SESSION/post/$phase"` after a completed post read. Reject wrap/regressing index, changed gates/hook/capacity, torn/partial records, overlap mismatch or mutation of the pre-attempt occupied prefix. For the prefix comparison, use the decoded pre index, not a count inferred from a message family:

```sh
PRE_INDEX=$(od --endian=little -An -tu2 -j30 -N2 "$SESSION/pre/store.bin")
POST_INDEX=$(od --endian=little -An -tu2 -j30 -N2 "$SESSION/post/store.bin")
test "$POST_INDEX" -ge "$PRE_INDEX" &&
  cmp --ignore-initial=52:52 --bytes="$((16 * PRE_INDEX))" "$SESSION/pre/store.bin" "$SESSION/post/store.bin"
```

Compare only newly retained slots and their full signatures against the baseline. The census `arg` high halfword is the message id; its separately printed `code` is not the id. Retain failed/no-new-message outcomes. Do not call unchanged bytes or a structural pass proof of freshness: dirty lines on the other core can remain stale under bulk DMA, and the store has no immutable export or acquisition identity. This block answers whether one ordinary observed AF attempt leaves a useful stock send sequence, not #107 coherence, calibrated lens position, timing or #106 bounded automatic control.

### Duration, stops and ending

At Run 3's measured 5,933,421 bytes/s, 131072 transferred bytes per main/tail pair imply about **22 ms of payload transfer**. Session setup, filesystem sync, tool startup and the five debug reads dominate this small block. **Planning estimate, not a body measurement:** allow one to five seconds per command, about 8–40 seconds of extraction commands plus the owner's at-most-five-second attempt and inspection. Build/download time belongs before attachment.

Each bulk command has one chunk, a 5000 ms transfer limit and a 130 s aggregate operation budget; each five-window `read-ram` has a 180 s aggregate budget. These are operation/provider budgets, not process watchdogs. Do not add an external kill timer or retry. Eight live commands, four of each shape, are the finite maximum; baseline failure or missing AF indication ends the sequence earlier.

Stop on an unexpected reply/status, wrong identity or route, nonzero camera error, short read, disconnect, uncertain cleanup, unexpected camera behavior, gate/filter mismatch, unrecognized format, near-capacity/wrapped store or missing owner-reported AF behavior. Preserve completed captures and journals. After a failed bulk chunk, send no camera command until USB is unplugged, the body is power-cycled and ordinary operation is confirmed, following the reviewed bulk operation's incident procedure.

After success, end this read-only-content block through the reviewed bulk operation's normal cardless exit: ask whether the body remains at its ordinary shooting screen and works normally, then detach USB when the owner wishes. No `ram-unload`, `ram-free`, payload rollback, gate restore or mandatory battery pull is introduced for this block. This is distinct from any later RAM-hook experiment, whose power-removal ending remains required. Card/save/playback behavior is outside this cardless exception.
