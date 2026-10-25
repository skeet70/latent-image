# First AF observation session: ordinary-boot feasibility (1.50)

Status: general procedure design, not camera authorization. The [stock-first console block](af-observation-plan.md#stock-first-console-observation-block) now supplies the closed operations, source-bound windows, one-attempt script, finite budgets and operation-specific ending for the first stock trace observation. Other preview/statistic/session work below remains design. The [observation plan](af-observation-plan.md) owns the questions and the [camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs) owns authority.

## 1. Host preparation

- Decode the stock console store from existing captures and prepare bounded same-boot extraction using the public [console tools](console-trace-instrument.md). Record wrap and incomplete-read rules.
- Bind any proposed preview/statistic window to its active producer, format, completion and ownership. Do not sweep RAM looking for a recognizable picture or expand physical bounds from a firmware buffer catalogue.
- Distinguish existing `read-ram` from `read-bulk` and new snapshot mechanisms by their actual effects and approved scope. A function named read is not necessarily H1.
- Pin the installed baseline and exact hook preimages. The [RAM-calibration report](ram-calibration.md) records an inert recorder remainder; no clean-image reflash is part of this session.
- Prepare one short question-driven block, its stopping conditions, camera-state checks and private output paths. Rehearse the exact supported operation offline. A new mechanism must pass independent review before its first live use.

## 2. Owner setup and scope

The owner places the mounted camera safely on a table or ordinary support, points it at a book or package with a clear intended region, confirms the ordinary shooting screen and adequate battery, and gives the scoped go. No tripod, card insertion, printed chart, measured distance, light meter or external high-speed recording is required.

Current stored `0xf7 = 1` permits ordinary attachment; no gate commit or maintenance-mode entry is part of this plan. Verify the current identity/route rather than assuming a historical state still holds. The broken SD slot remains untouched. A cardless exit check needs explicit scope and cannot establish save/playback.

## 3. Observation block

1. Establish whether ordinary preview and a few stock AF attempts work with the approved USB attachment. A normal menu alone is not AF evidence.
2. Read the existing message trace after each small attempt group, in the same boot and outside timed acquisition. Keep all attempts, including no-result outcomes; do not infer fine timing or freshness from a message send.
3. If the approved operation supports a coherent image/statistic snapshot, make one obvious scene change and verify fresh source content, then record source/configuration/frame identity and acquisition-time lens evidence. Do not promote an arbitrary live-buffer dump to a coherent frame.
4. Stop the block when the predeclared question is answered or its finite budget is exhausted. If USB inhibits AF or no coherent source exists, retain the exact boundary and continue host work. Do not try unreviewed disconnect, service capture or hook variants in the same run.

Before programmatic focus control is validated, these are feasibility observations, not calibrated lens sweeps or a speed/accuracy benchmark. The owner need not stage a target for dozens of attempts.

## 4. Stops and ending

Stop on unexpected replies, disconnect, camera behavior or uncertain completion. Send no retries or improvised cleanup commands. Use the operation-specific incident procedure; a read-only session, a maintenance session and a RAM payload do not automatically share an ending.

For a RAM-payload session, current procedures require shutdown followed by complete USB and battery/dummy-supply disconnection for at least ten seconds, then separately scoped stock-return checks. Switch-off alone has retained patches. Do not cut power during a possible persistent write, and do not claim arbitrary payload corruption is recoverable merely because the payload was in RAM.

## 5. Decision and next experiment

Report whether stock AF ran, what the trace actually contains, whether source pixels/statistics were fresh and coherent, observation overhead, missing clocks/units, and the stock-return outcome. Preserve unavailable fields as unavailable. The next experiment adds only a missing observation or validates one bounded stock action; it does not automatically advance to a full host controller or full-resolution cardless capture.
