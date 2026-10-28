# Volatile-bank RAM candidate: owner-run bounded trial

Owner procedure for the current combined recipe-bank, D3 and Classic Chrome RAM candidate: **222 publications, ABI 5, allocation `0x1dc98` (122008 bytes), 131 setup and 91 activation**. The operation is H2 under [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs): a RAM write with no firmware, configuration or boot write. The commands and their mechanics are in [transport/README.md](../transport/README.md). Nothing here authorizes a run. Accounts imported from the private research history at `cbbd8f4` describe their dated earlier candidates, not current qualification.

**Current source review.** `transport/ram-push-review.json` records `RamPayloadArtifactReview` approval of the bounded RAM source at `c6c760ad29364f8017c178d3ef12f9a13852349f`, independently verified identical across the five changed workspace subjects and complete eleven-file builder closure to the retained two-reference-base emissions. This is not a build at that integrated commit, current guest qualification, owner visual acceptance, actual-allocation approval or live authority. Before a run, require the current [pre-go gate](../transport/README.md#pre-go-gate), candidate-specific emulator evidence and owner visual acceptance, followed by fresh scope/go and R2 actual-base review. The Classic Chrome standalone payloads retain their separate gates.

**Historical physical state, 253-publication candidate.** In the private history that candidate passed one bounded cardless trial: installation, first SELECT, C1 Save Current, a working HIGHLIGHT TONE edit, C1 apply, NONE restoration, normal restart after full power removal and the post-reboot stock-word read (E-RAM-PUSH253-VOLATILE-RECOVERED-150). The NONE confirmation blink flashed the prior C1 row (RECIPE-BANK-POLISH-01). The owner chose a volatile RAM bank because slot pin2 is broken and no card is available (E-XM1-CARD-SLOT-UNAVAILABLE-150). The current relinker defines `RB_VOLATILE_BANK=1`; bank operations use the resident version-3 image without SD files. The ordinary SD-backed firmware path is unchanged. **The physical SD recovery route remains unavailable; flash `installable=false`.**

**Historical converged 253-publication trial, 2026-10-25.** The converged installer passed the then-current bounded cardless trial (E-RAM-PUSH253-CONVERGED-RUN-150). `ram-alloc` returned `0x00a83608`; ActualBaseReview accepted that relink. `ram-push` reported `installed`: INSTALL status 0, 253 validated, 167 setup and 86 activation, peer status 0, CPU0 wait 2 and CPU1 wait `0x13de` below `0x80000`. The owner completed the then-current seven-step sequence, and the NONE blink of the prior C1 row recurred (#83). After at least ten seconds without power, the body booted normally with no recipe rows and unchanged M/C settings. The 63-window post-reboot read found all 253 publication words at their stock values and callback `0x00dc7874`. Those states and that manifest are spent. This does not qualify the current 222-publication candidate or the seven-slot sequence below.

## 1. Evidence and candidate identity

### 1.1 Current candidate

The current combined manifest has 222 distinct publication addresses in transaction order: 131 setup, then 91 activation. The publication contract remains the one in `transport/ram-push-review.json`; the accepted actual-base manifest supplies addresses, old words, entries, source identity and runtime digest. Do not copy an earlier base, entry address, digest or allocation state. The installer extent is `0x1980`; generated data begins at `base+0xa800`, writable data ends at `names+0x400`, and the installer starts at `ALIGN(names+0x800,32)` ([current layout](../transport/README.md#operations-and-their-bytes)).

The volatile build excludes the zero-tag media completion arm. Ordinary published forwarding remains. The carried 60-byte flash-linked store is non-entry data in this RAM closure, not approved for execution after copying ([named-recipe reach section 6.8](named-recipe-reach.md#68-file-worker-failure-boundary)).

### 1.2 Dated earlier evidence

The following reference identities, allocation calculations, gate results and physical observations concern the named earlier 250/253-publication candidates. They do not establish current emulator behavior, all-slot visual acceptance or a fresh live go.

The 250-publication bank of 2026-10-18 left a persistent aspect-0 save hang (E-RAM-PUSH-FULL-SESSION-150, E-SAVE-HANG-ASPECT-150). The current recall and selection admission refuses unoffered IMAGE SIZE pairs, aspect 0 included, before any write and restores atomically ([recipe design section 17.12](recipe-design.md#1712-recall-admission-composite-image-size-and-image-quality-commits-live-seeding-and-atomic-refusal-2026-10-18)). Legacy v1 banks whose populated payloads carry unoffered IMAGE SIZE size/aspect `(0,0)` refuse as `RB_UNSAFE_BANK`, the newer generation winning, and SELECT returns to page 1 with the MANUAL header highlighted. Exact preserved-buffer replay reproduces that refusal; a fresh bank creates valid v2 `(0,1)` payloads and completes Save Current, C1/NONE, AUTO IMAGE SIZE editing, resumed C2 and durable NONE/base restoration (E-RAM-PUSH253-CAPTURED-REPLAY-150, E-RAM-PUSH253-FRESH-REFRESH-150). Do not weaken admission or guess aspect values. Card-backed visual and byte acceptances do not transfer to this variant.

The private history's exercised 253-publication host reference was linked at emulator-returned base `0x00a81be0`: runtime `41de7940…6f34`, manifest `1e588c2e…2dce`. At the historical convergence step, only the installer region changed relative to that exercised reference. This is not a byte-equivalence claim for the current combined candidate. An emulated allocation is never a physical address prescription or reusable camera state.

That historical allocation was 121336 bytes, `0x1d9f8`. Generated data began at `base+0xa800`, with a `0x1260` reservation; names began no earlier than `base+0xbb00`, retaining low half `0xb784`. Worst aligned base low half `0xfc88` gave bank-end offset `0x1be98`; installer extent `0x1b60` gave `0x1d9f8`. The historical contract was `ee44381db2e006e3b5461a07811ec505030861ad62cd7b33419364804ced7fe2`: 253 publications, 167 setup then 86 activation. Its ABI, allocation command and read plans are obsolete for the current candidate.

| Historical host gate | Evidence for the named earlier candidate |
| --- | --- |
| Focused contracts/build | Six transport/format contract targets pass; camera, viewer, embedded bridge and native-case executables build. The ten volatile native cases pass, covering empty/readback, save/reload, staged re-entry, C1/NONE, C/unset refusal, stale session and invalid ISO. |
| Default SD regression | All 2,738 unique case IDs pass in one complete fresh run, with unchanged coverage and limits and no receipt replay. The macro-off guest source is unchanged. Core SD admission tries contained ELF `STT_FUNC` full and model-only domains under the existing conflict, overlap and 256-function guards, and assertions decode only claimed memory regions (E-RECIPE-NATIVE-COMPLETE-150). |
| Authenticated relinker | `//transport:ram-push-reviewed-build-check` accepts `0x00c40000` and worst-case `0x00c4fc88`, refuses overlap, substituted tools/bytes and altered installed identity before its deliberately failing USB provider. No native camera access. |
| Full-system cardless workflow | Pinned relinking installs all 253 words with status 0, initially in S (`0x22`). All seven strict observations match: empty SELECT, Save Current, HIGHLIGHT TONE 0 to -1, C1 apply and NONE restoration. Independent host decoding accepts resident generations 1/2/4/5 as v2 and confirms saved/working separation and unchanged C bytes. C1/NONE restoration is demonstrated by read windows, not their identical menu screenshots. The controller snapshot observes absent media, no image and zero dirty sectors. UI conversion uses the declared CPU-fallback profile, not measured hardware timing (E-RAM-PUSH253-VOLATILE-HOST-150). On the converged mechanism at `cbd9eb4` the route passes again (2026-10-25): allocation `0x00a81be0`, runtime `622ee56d…3008`, INSTALL status 0 with 253 validated, 167 setup, 86 activation and peer status 0, all seven segments, rollback status 0 with 253 restored, and unload verifying 253 old words. |
| Primary technical review | Opus accepts the frozen source and exact exercised-reference runtime/manifest at virtual base `0x00a81be0`; Sol accepts the final bridge and bounded operator handoff. Both scoped reviews find no blocking defect (E-RAM-PUSH253-VOLATILE-FROZEN-150, E-RAM-PUSH253-VOLATILE-HANDOFF-150). No review accepts unknown physical returned-base bytes. |
| Embedded bridge boundary | The final bridge passes actual cold boot, attach restore and checkpoint-save refusal. Pipelined start/finish retains the finish request; attach metadata omits an installed checkpoint, and restore duration includes the restore. Selected-profile and executable/library bytes participate in identity. This short smoke performs no RAM upload; the full workflow and fresh-upload receipts exercise the same guest bytes with earlier bridge binaries. |
| Fresh-upload boundary | An independent cold boot and fresh upload leave loaded/generation/resident/activation zero, start in S and retain the same stock C baseline. The controller again observes absent media and no dirty sectors. This is a fixed-DAT/NAND model reset, not physical battery-cut or stock-settings-persistence evidence. |
| Owner visual check | The owner accepted the retained first-SELECT and HIGHLIGHT TONE edit emulator surfaces (E-RAM-PUSH253-VOLATILE-VISUAL-150). Acceptance covers those surfaces only, not physical behavior or a live go. |
| Mechanism review in this tree | `MechReview` approved the converged installer, ABI header, layout module, relinker and builder pins at `87a081c` with no must-fix (E-RAM-PUSH-MECHANISM-REVIEW-150). The installer region of the runtime differs from the exercised reference; every byte before it is identical. The converged bank installer has run only in the emulator; the same installer object code, apart from addresses and count and ABI immediates, ran on the body as the Classic Chrome payloads. |

All earlier physical allocation/push states are invalid after full power removal. Do not reuse them, their manifests or their DSC1/SEL1 read plans. This runtime provides neither diagnostic record nor photographic overlay. Bank data, selection and the stored ordinary base last only within the powered RAM session; a new upload starts empty. There is no SD fallback or bank migration in this RAM build.

## 2. Fresh trial prerequisites and scope

### 2.1 Owner scope and baseline

1. Require current availability and an ordinary supported carried known-good baseline with no recipe rows/setup prompt. Keep this body's backups and known-good artifacts; never substitute another body's data. Renew the bounded no-card exit exception for this trial. It must acknowledge the unavailable SD recovery route and possible settings persistence; the historical exception is not a new installation go.
2. Keep the damaged slot empty. No card insertion, format, repair, migration, bank-file or persistence operation. An absent-card software model does not establish the electrical condition of the broken pin.
3. Record the actual ordinary M and stock C settings. Do not change them to historical ISO, IMAGE SIZE, film simulation or menu defaults. The watch list must cover the fields used to judge restoration.
4. Require the current verdict, byte-pin and reviewed-build gates, independent source/artifact review, candidate-specific guest qualification and the owner’s candidate-specific emulator visual check. Record the inspected surfaces and the owner’s decision; earlier visual acceptance does not transfer automatically. A source review alone does not satisfy these gates. This prerequisite does not close the SELECT/NONE transient-flash or all-slot/action-page acceptance criteria in section 4 and #83.
5. The ordinary `ff80` route requires stored config `0xf7` nonzero. A zero value stops this procedure; a separate jig/config-write operation is not an automatic prerequisite action.
6. Install from ordinary shooting mode S, idle at the shooting screen with menus closed and no card activity. Require this before R1 and R3. Switch to M only after offline install acceptance. From R1 through R3 and bank use, pause all other USB clients and model requests: no intervening read, query, link reset or second client. R2 review and plan preparation are host-only.

A fresh go names the accepted current candidate, fresh allocation/relink/install and section 4's RAM-only sequence. It acknowledges loss of the bank/base on full power removal, possible persistence of applied stock settings, the unavailable SD recovery route and the limited no-card exit check. It does not authorize firmware/configuration/calibration writes, End Setting, recovery experiments, automatic retry or a second attempt.

### 2.2 Readiness observations before R1

After fresh attachment and read-grant confirmation, read config with `private-data/read-plans/gate-byte.json`. Create the current readiness plan directly from the twelve fixed route, worker and M/C baseline windows below. No historical private publication plan or recorder-residue observation is an input:

```sh
umask 077
READINESS_PLAN="$PWD/private-data/read-plans/volatile-bank-readiness.json"
test ! -e "$READINESS_PLAN" &&
jq -n '{windows: ([
  ["0x4089e9f8",100,"worker sample 1"],
  ["0x00829a00",16,"heap break"],
  ["0x01469860",32,"query output cells"],
  ["0x01469b20",32,"request-owned bootstrap window"],
  ["0x0146a0c0",16,"service mode and callback table pointer"],
  ["0x015c22a0",32,"callback table and stock slot"],
  ["0x40a0fbac",4,"channel58 queue message size"],
  ["0x4089ea94",4,"stock post prehook"],
  ["0x40a5b5e0",16,"message-trace controls"],
  ["0x4089e9f4",104,"worker sample 2 with preceding word"],
  ["0x0096cc64",248,"ordinary shooting settings baseline"],
  ["0x0096cda4",40,"stock C settings baseline"]
] | to_entries | map({
  index:.key,address:.value[0],length:.value[1],purpose:.value[2]
}))}' > "$READINESS_PLAN"
```

Require exactly these domains and lengths, in this order:

| Index | Address | Bytes |
| --- | --- | --- |
| 0 | `0x4089e9f8` | 100 |
| 1 | `0x00829a00` | 16 |
| 2 | `0x01469860` | 32 |
| 3 | `0x01469b20` | 32 |
| 4 | `0x0146a0c0` | 16 |
| 5 | `0x015c22a0` | 32 |
| 6 | `0x40a0fbac` | 4 |
| 7 | `0x4089ea94` | 4 |
| 8 | `0x40a5b5e0` | 16 |
| 9 | `0x4089e9f4` | 104 |
| 10 | `0x0096cc64` | 248 |
| 11 | `0x0096cda4` | 40 |

Independently check the resulting twelve windows, 644 bytes total, against the reviewed readiness scope before use. Keep their source order: the two worker observations are separated. Require X-M1, nonzero config `0xf7`, service mode `0x0146a0c0=3`, table `0x0146a0c4=0x015c22a0`, callback `0x015c22ac=0x00dc7874`, channel58 size `0x40a0fbac=0x24` and prehook `0x4089ea94=0x00726478`. At `0x40a5b5e0`, bytes 0/1/2 must be 0/1/1 and word +0xc `0x41ac9800`. Both worker samples must retain `(0x58,1),(0x59,1),(0x5a,1),(0x5b,0),(0x5c,0)`; worker58 also requires zero kind and waiting selector. Compare M/C observations to the recorded ordinary baseline. Sampling does not establish future quiescence or silicon coherence. These controls do not prove a natural error record or recorder inertness.

Run only the already-approved read operations, with new session output directories. A mismatch stops before allocation.

### 2.3 Actual-manifest publication and recovery read plans

After R2 has produced and independently accepted the actual-base manifest, derive a **post-battery read plan**, host-only: one four-byte window per publication in manifest array order, then the callback slot. Do not execute it while the fresh allocation is retained. R1, host-only R2 and R3 remain an uninterrupted allocation-to-push sequence with no intervening camera query. This is an existing `jq` derivation, not a bank-builder `--read-plans` option. The current bank builder has no such option. Do not sort by logical publication index, group historical addresses or reuse a 253-publication DSC1/SEL1 plan.

```sh
umask 077
WORD_PLAN="$LINK_OUT/stock-words-plan.json"
EXPECTED_WORDS="$LINK_OUT/stock-words-expected.txt"
test ! -e "$WORD_PLAN" && test ! -e "$EXPECTED_WORDS" &&
jq '{windows: (
  [.publications[] | {
    address:.address, length:4,
    purpose:("original publication " + (.index|tostring))
  }] + [{address:"0x015c22ac",length:4,purpose:"stock callback slot"}]
  | to_entries | map(.value + {index:.key})
)}' "$LINK_OUT/ram-push-manifest.json" > "$WORD_PLAN" &&
jq -r --slurpfile allocation "$ALLOC_OUT/ram-alloc-state.json" \
  '(.publications[].old, $allocation[0].slot_old) | ltrimstr("0x")' \
  "$LINK_OUT/ram-push-manifest.json" > "$EXPECTED_WORDS"
```

The independently checked plan must have 223 distinct four-byte windows, 892 bytes total: all 222 actual-manifest addresses, followed by `0x015c22ac`. The expected file must have 223 eight-digit lowercase words in that same order, ending with the accepted allocation's stock callback word `00dc7874`.

Only after section 4's successful battery-ending path, the separately directed ordinary reboot and renewed read permission, run `read-ram "$WORD_PLAN" "$READ_OUT"` for section 5. `READ_OUT` must be new. Require a successful complete one-pass session and all 223 files of exactly four bytes. `read-ram` writes them as `ram/000-AAAAAAAA.bin`, with lowercase address digits and no `0x`. Decode only those exact files in plan order:

```sh
set -o pipefail
jq -r '.windows[].length' "$WORD_PLAN" > "$READ_OUT/stock-word-sizes-expected.txt" &&
jq -j --arg out "$READ_OUT" \
  '.windows[] | $out + "/ram/000-" + (.address|ltrimstr("0x")) + ".bin\u0000"' \
  "$WORD_PLAN" |
  xargs -0 stat --format='%s' -- > "$READ_OUT/stock-word-sizes-observed.txt" &&
diff -- "$READ_OUT/stock-word-sizes-expected.txt" "$READ_OUT/stock-word-sizes-observed.txt" &&
jq -j --arg out "$READ_OUT" \
  '.windows[] | $out + "/ram/000-" + (.address|ltrimstr("0x")) + ".bin\u0000"' \
  "$WORD_PLAN" |
  xargs -0 od --endian=little -An -v -tx4 -w4 -- |
  tr -d ' ' > "$READ_OUT/stock-words-observed.txt" &&
diff -- "$EXPECTED_WORDS" "$READ_OUT/stock-words-observed.txt"
```

A missing, short, extra-length or mismatching window fails the check. Do not use a filename glob, directory listing order or a partial/uncertain session as evidence. Preserve the accepted manifest, plan, expected words and journals together. Before upload, R3's built-in preflight checks the original words without an extra `read-ram` session.

## 3. One-command-at-a-time handoff

These commands are conditional procedure, not an instruction to run before readiness and a fresh go. Run from the repository root. Every output directory must be new; never remove or reuse an old session to make a command proceed.

```sh
ALLOC_OUT="$PWD/private-data/sessions/xm1-ram-alloc-volatile-bank"
LINK_OUT="$PWD/private-data/analysis/x-m1-1.50/ram-push/operator-linked-volatile-bank"
PUSH_OUT="$PWD/private-data/sessions/xm1-ram-push-volatile-bank"
```

**R1, owner, after the fresh go and the idle-screen/no-menu/no-card-activity check:**

```sh
devenv shell -- bazel run //transport:camera -- ram-alloc 0x1dc98 "$ALLOC_OUT"
```

Require `ram-alloc: retained`. Inspect the saved state offline: nonzero eight-byte-aligned base, size 122008, X-M1, nonzero config `0xf7`, CPU0 `0x4003` and stock table/slot. Do not request a historical base or allocate again to obtain one.

**R2, host only:**

```sh
devenv shell -- bazel run //transport:camera -- ram-relink "$ALLOC_OUT/ram-alloc-state.json" "$LINK_OUT"
```

Authenticate the actual-base runtime/manifest against the pinned builder. An independent reviewer reviews the actual-base bytes, allowlist and bounds before R3. A reference-base review is not that acceptance. Derive and independently check section 2.3's post-battery plan offline; do not execute a camera read before R3. No manual relocation, pin change or historical diagnostic-plan reuse.

**R3, owner, only after R2 acceptance and the idle-screen/no-menu/no-card-activity check:**

```sh
devenv shell -- bazel run //transport:camera -- ram-push "$ALLOC_OUT/ram-alloc-state.json" "$LINK_OUT/ram-push-manifest.json" "$PUSH_OUT"
```

Before the first upload write, `ram-push` rechecks product identity, config gate, callback table/slot, all 222 old publication words and the full retained-allocation content guard. A refusal stops; do not bypass it. Leave the camera untouched until `ram-push: installed`. Require ABI 5, INSTALL status 0, validated 222, setup 131, activation 91, peer status 0, bound `0x80000` and both CPU0 waits below it. Check the raw matching mailbox reply in the saved journal offline before menu use. Separately compare the fields actually present in `ram-push-state.json` with the accepted allocation/manifest: `base`, `size`, `entry` (`install`, `rollback`, `status`), `manifest_sha256`, `runtime_sha256`, all 222 `publications`, `slot_old`, `bootstrap` and `nonce`. Mailbox status fields are not fields of that file. An uncertain result stops; do not retry, reallocate, unload or free.

## 4. Bounded bank-use sequence

1. Start external phone video of the camera LCD, not camera movie recording. Switch S to M and require the recorded ordinary M settings. Require one EDIT/SAVE CUSTOM SETTING and one SELECT CUSTOM SETTING on shooting menu page 5. Enter SELECT and require the ordinary child page with NONE, without a diagnostic banner or MANUAL-header parent return. Leave with BACK without selecting a slot. A mismatch stops; do not create a bank to repair a failed entry.
2. Populate C1 through C7 with **RAM-only SAVE CURRENT**, once per still-empty slot and without changing the ordinary M baseline between saves. Require completion and return for each. This is the minimum population needed for seven selectable slots, not an edit, rename, reset, policy or file test.
3. Confirm C1, C2, C3, C4, C5, C6 and C7 once each, in that order, with no intervening NONE confirmation. Enter SELECT afresh for each, require its initial cursor on NONE, then navigate to the chosen slot. The chosen row alone must flash. Require the ordinary shooting settings to match that slot's unchanged saved baseline.
4. Immediately after C7, reopen SELECT and confirm NONE once without moving its cursor away and back. This is the eighth and final selection confirmation. NONE must flash, not the prior C7 or another slot, and watched ordinary M settings must return to baseline. Do not scroll to inspect C7 before or after this confirmation. A failure stops; do not repeat a selection or perform NONE cleanup.
5. Inspect all seven action pages at all four navigation positions: SAVE CURRENT, EDIT NAME, EDIT and RESET, 28 positions total. Navigate only and leave with BACK. Do not confirm an action in this navigation pass. Require readable, correct rows, no stale or prior-selection highlight bar at any position, and ordinary return behavior.
6. Finish with successful NONE restoration, menus closed and all camera/host I/O finished. No edit, rename, reset, auto-update/policy change, shutter, mode excursion, card operation, persistence test or additional bank action is covered.
7. On that successful path, disconnect USB and remove the battery or cut the dummy supply for at least ten seconds. An ordinary OFF transition is **not required**; under [session ending](../spec/07-governance-and-handoff.md#34-standing-confirmations), it commits ordinary settings in area 2 and is optional only if separately chosen and acknowledged. A switch cycle alone does not prove removal: added rows survived one historically (E-RAM-PUSH253-SWITCH-RECOVERY-FAIL-150). Allocation/push state and that actual-base manifest are invalid after power removal. Neither state records a power-cycle identity; only allocation state has a content guard. Never reuse them for another trial. Do not use `ram-unload` or `ram-free` as cleanup.

Any unexpected response, screen, delay, disconnect, failed check or uncertain operation stops the sequence immediately. Do not add a menu operation, automatic NONE, unload, switch-off or battery-cut sequence. Report the state and wait for a separately directed response. Interrupting stock settings activity remains a residual risk, and SD recovery is unavailable.

## 5. Exit and acceptance boundary

Stage 5 shutdown/base restoration is absent. The successful path restores NONE before USB disconnection and full power removal. A failed path stops without further menus or automatic power actions. Removing power while a custom selection is active loses its stored ordinary base while applied stock settings may persist. Volatile bank storage does not prevent ordinary settings persistence, and no boot-time base restoration exists.

Preserve the external phone video and host journals. Under the current no-card exception, a separately directed ordinary reboot with USB disconnected checks normal startup without recipe rows/setup prompt and unchanged watched M/C settings. After renewed attachment and read permission, use section 2.3's accepted actual-manifest plan to compare **all 222 original publication words and the callback slot**, with a new read output directory. A screen report alone does not establish original-byte recovery. Capture/save/playback and SD-backed bank persistence remain deferred while the physical slot is unavailable. A later trial starts with a fresh go, allocation and authenticated relink.

This is a bounded volatile-bank RAM procedure, not physical feature acceptance or permanent-install readiness. A live attempt requires the passing pre-go gate, fresh availability/preflight/go and independent actual-base review. Flash remains non-installable; persistent boot/shutdown and later management/auto-update stages remain outside scope.
