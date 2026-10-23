# Volatile-bank RAM candidate: owner-run bounded trial

Imported from the private research history (fuji-af) at `cbbd8f4`; commit ids here name that history. Owner procedure for loading the 253-publication recipe bank (editable bank, D3 compiled film rows and Q exposure) into RAM on XM1-body-01 over USB in an ordinary boot, using it once and ending with full power removal. The operation is H2 under [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs): a RAM write with no firmware, configuration or boot write. The commands and their mechanics are in [transport/README.md](../transport/README.md) and are quoted, not redefined. Nothing here authorizes a run.

**Not go-ready in this tree.** `transport/ram-push-review.json` pins this tree's relink of the bank. Pins whose bytes equal the private history's reviewed `cbbd8f4` pins carry that review; the others are recorded `unreviewed`, so the manual pre-go gate `//transport:ram-push-review-verdicts` fails until an independent import review approves them, as RamPushImportReview did for the previous import. No bank push runs from this script until that review approves the pins, the [pre-go gate](../transport/README.md#pre-go-gate) passes and the owner gives a fresh go. The Classic Chrome payloads keep their own review records and gates.

**Physical state.** In the private history this candidate passed one bounded cardless trial on the body: installation, first SELECT, C1 Save Current, a working HIGHLIGHT TONE edit, C1 apply, NONE restoration, normal restart after full power removal and the post-reboot stock-word read (E-RAM-PUSH253-VOLATILE-RECOVERED-150). The NONE confirmation blink flashes the prior C1 row (RECIPE-BANK-POLISH-01). The owner chose a volatile RAM bank because slot pin2 is broken and no card is available (E-XM1-CARD-SLOT-UNAVAILABLE-150). The RAM relinker defines `RB_VOLATILE_BANK=1`: bank operations use the resident v2 image, not SD files. The ordinary SD-backed firmware path is unchanged. The SD-card recovery route is unavailable. **Flash `installable=false`.**

## 1. Evidence and candidate identity

The 250-publication bank of 2026-10-18 left a persistent aspect-0 save hang (E-RAM-PUSH-FULL-SESSION-150, E-SAVE-HANG-ASPECT-150). The current recall and selection admission refuses unoffered IMAGE SIZE pairs, aspect 0 included, before any write and restores atomically ([recipe design section 17.12](recipe-design.md#1712-recall-admission-composite-image-size-and-image-quality-commits-live-seeding-and-atomic-refusal-2026-10-18)). Legacy v1 banks whose populated payloads carry unoffered IMAGE SIZE size/aspect `(0,0)` refuse as `RB_UNSAFE_BANK`, the newer generation winning, and SELECT returns to page 1 with the MANUAL header highlighted. Exact preserved-buffer replay reproduces that refusal; a fresh bank creates valid v2 `(0,1)` payloads and completes Save Current, C1/NONE, AUTO IMAGE SIZE editing, resumed C2 and durable NONE/base restoration (E-RAM-PUSH253-CAPTURED-REPLAY-150, E-RAM-PUSH253-FRESH-REFRESH-150). Do not weaken admission or guess aspect values. Card-backed visual and byte acceptances do not transfer to this variant.

The private history's exercised host reference is `private-data/analysis/x-m1-1.50/ram-push/VolatileBank/exercised-reference/`, linked at the emulator-returned base `0x00a81be0`: runtime `41de7940…6f34`, manifest `1e588c2e…2dce`. This tree adds the bank ELF allowlist to the builder identity, so its manifests differ from those; the import review compares `runtime.bin` and the publication records with the private history's reviewed relink at the same base. An emulated allocation is never a physical address prescription or reusable camera state. The manifest's `source_identity` map is authoritative.

The allocation is 121304 bytes (`host/firmware/RecipeBankRamPlacement.hs`). Generated data begins at `base+0xa800`, with a `0x1260` reservation. Names begin no earlier than `base+0xbb00`, retaining low half `0xb784`. The worst aligned base low half `0xfc88` gives bank-end offset `0x1be98`; installer extent `0x1b40` gives `0x1d9d8`. The publication contract digest is `ee44381db2e006e3b5461a07811ec505030861ad62cd7b33419364804ced7fe2`: 253 publications, 167 setup then 86 activation. The bank's installer and ABI are `menu/recipe_bank_installer/{ram_push_installer.c,ram_push_abi.h}`, byte-identical to the private history's `cbbd8f4`; the generic `menu/ram_push_installer.c` and `menu/ram_push_abi.h` stay pinned by the Classic Chrome review records.

| Host gate | Evidence |
| --- | --- |
| Focused contracts/build | Six transport/format contract targets pass; camera, viewer, embedded bridge and native-case executables build. The ten volatile native cases pass, covering empty/readback, save/reload, staged re-entry, C1/NONE, C/unset refusal, stale session and invalid ISO. |
| Default SD regression | All 2,738 unique case IDs pass in one complete fresh run, with unchanged coverage and limits and no receipt replay. The macro-off guest source is unchanged. Core SD admission tries contained ELF `STT_FUNC` full and model-only domains under the existing conflict, overlap and 256-function guards, and assertions decode only claimed memory regions (E-RECIPE-NATIVE-COMPLETE-150). |
| Authenticated relinker | `//transport:ram-push-reviewed-build-check` accepts `0x00c40000` and worst-case `0x00c4fc88`, refuses overlap, substituted tools/bytes and altered installed identity before its deliberately failing USB provider. No native camera access. |
| Full-system cardless workflow | Pinned relinking installs all 253 words with status 0, initially in S (`0x22`). All seven strict observations match: empty SELECT, Save Current, HIGHLIGHT TONE 0 to -1, C1 apply and NONE restoration. Independent host decoding accepts resident generations 1/2/4/5 as v2 and confirms saved/working separation and unchanged C bytes. C1/NONE restoration is demonstrated by read windows, not their identical menu screenshots. The controller snapshot observes absent media, no image and zero dirty sectors. UI conversion uses the declared CPU-fallback profile, not measured hardware timing (E-RAM-PUSH253-VOLATILE-HOST-150). |
| Primary technical review | Opus accepts the frozen source and exact exercised-reference runtime/manifest at virtual base `0x00a81be0`; Sol accepts the final bridge and bounded operator handoff. Both scoped reviews find no blocking defect (E-RAM-PUSH253-VOLATILE-FROZEN-150, E-RAM-PUSH253-VOLATILE-HANDOFF-150). No review accepts unknown physical returned-base bytes. |
| Embedded bridge boundary | The final bridge passes actual cold boot, attach restore and checkpoint-save refusal. Pipelined start/finish retains the finish request; attach metadata omits an installed checkpoint, and restore duration includes the restore. Selected-profile and executable/library bytes participate in identity. This short smoke performs no RAM upload; the full workflow and fresh-upload receipts exercise the same guest bytes with earlier bridge binaries. |
| Fresh-upload boundary | An independent cold boot and fresh upload leave loaded/generation/resident/activation zero, start in S and retain the same stock C baseline. The controller again observes absent media and no dirty sectors. This is a fixed-DAT/NAND model reset, not physical battery-cut or stock-settings-persistence evidence. |
| Owner visual check | The owner accepted the retained first-SELECT and HIGHLIGHT TONE edit emulator surfaces (E-RAM-PUSH253-VOLATILE-VISUAL-150). Acceptance covers those surfaces only, not physical behavior or a live go. |
| Import review in this tree | Pending: the pins this import changed are `unreviewed` (above). |

All earlier physical allocation/push states are invalid after full power removal. Do not reuse them, their manifests or their DSC1/SEL1 read plans. This runtime provides neither diagnostic record nor photographic overlay. Bank data, selection and the stored ordinary base last only within the powered RAM session; a new upload starts empty. There is no SD fallback or bank migration in this RAM build.

## 2. Fresh trial prerequisites and scope

1. Coordinate current camera availability and require the supported ordinary RC0 baseline with no recipe rows/setup prompt. Keep this body's backups and known-good artifacts; never restore another body's data. The owner approved a bounded no-card exception to the capture/save/playback exit check of [the camera-run procedure](../spec/07-governance-and-handoff.md#31-procedure) step 4 (E-RAM-PUSH253-NO-CARD-EXIT-150). It covers this RAM-only menu trial and acknowledges the unavailable SD recovery route and possible settings persistence. It is not an installation go.
2. Keep the slot empty. Do not insert a card into the damaged slot or issue format, repair, migration or bank-file operations. The software model's absent-card result does not establish the electrical condition of the broken pin or a usable SD recovery route.
3. Record the current ordinary M and stock C settings. The baseline recorded before the trial was, for both, ISO 200, IMAGE SIZE L 3:2, quality F, DR AUTO, film simulation STANDARD and menu 2 defaults. Do not change settings to conceal a mismatch.
4. An ordinary boot offers the `ff80` route only with stored `0xf7` nonzero; if it is zero, `commit-f7-enable` in a jig session under its own go comes first ([transport README](../transport/README.md#current-state)).
5. After fresh attachment/read-grant confirmation, the model runs `read-config` with `private-data/read-plans/gate-byte.json` (four windows) and one-pass `read-ram` with `private-data/read-plans/ram-push-prepush-private253.json` (72 windows), under the [standing read grant](../spec/07-governance-and-handoff.md#34-standing-confirmations) and with new private session output directories. Compare all 253 publication words with the new manifest's old values. Require X-M1, nonzero config `0xf7`, service mode `0x0146a0c0=3`, table `0x0146a0c4=0x015c22a0`, callback `0x015c22ac=0x00dc7874`, channel58 size `0x40a0fbac=0x24`, and prehook `0x4089ea94=0x00726478`. At `0x40a5b5e0`, bytes 0/1/2 are 0/1/1 and word +0xc is `0x41ac9800`. Both separated worker samples retain `(0x58,1),(0x59,1),(0x5a,1),(0x5b,0),(0x5c,0)`; worker58 also requires zero kind and waiting selector. Sampling does not prove future quiescence or silicon coherence.
6. Pause all model camera requests and other USB clients before allocation. Install from ordinary shooting mode S, matching the full-system installation route, idle at the shooting screen with no menu open and no SD-card activity. Require that idle condition before both R1 and R3. Switch to M only after offline install acceptance. No model reads, link reset or second client during allocation/install/use.

A fresh go follows the passing pre-go gate and must cover the source identity, fresh allocation/relink/install and the bounded volatile-bank menu sequence below. It must acknowledge loss of the bank/base on full power removal, possible persistence of applied stock settings, the unavailable SD recovery route and the limited no-card exit check. It does not authorize firmware/configuration/calibration writes, End Setting, shutdown/boot stage 5, recovery experiments, automatic retry or a second attempt.

## 3. One-command-at-a-time handoff

These commands are conditional procedure, not an instruction to run before readiness and a fresh go. Run from the repository root. Every output directory must be new; never remove or reuse an old session to make a command proceed.

```sh
ALLOC_OUT="$PWD/private-data/sessions/xm1-ram-alloc-volatile-bank"
LINK_OUT="$PWD/private-data/analysis/x-m1-1.50/ram-push/operator-linked-volatile-bank"
PUSH_OUT="$PWD/private-data/sessions/xm1-ram-push-volatile-bank"
```

**R1, owner, after the fresh go and the idle-screen/no-menu/no-card-activity check:**

```sh
devenv shell -- bazel run //transport:camera -- ram-alloc 0x1d9d8 "$ALLOC_OUT"
```

Require `ram-alloc: retained`. The model inspects the saved state offline: nonzero eight-byte-aligned base, size 121304, X-M1, nonzero config `0xf7`, CPU0 `0x4003` and stock table/slot. Do not request a historical base or allocate again to obtain one.

**R2, host only:**

```sh
devenv shell -- bazel run //transport:camera -- ram-relink "$ALLOC_OUT/ram-alloc-state.json" "$LINK_OUT"
```

Authenticate the actual-base runtime/manifest against the pinned builder. An independent reviewer on `openai-codex/gpt-6.1-sol:high` or `anthropic/claude-opus-5-5:high` reviews the actual-base bytes and bounds before R3. The emulated reference is not that acceptance. No manual relocation, pin change or diagnostic capture-plan reuse.

**R3, owner, only after R2 acceptance and the idle-screen/no-menu/no-card-activity check:**

```sh
devenv shell -- bazel run //transport:camera -- ram-push "$ALLOC_OUT/ram-alloc-state.json" "$LINK_OUT/ram-push-manifest.json" "$PUSH_OUT"
```

Leave the camera untouched until `ram-push: installed`. The command enforces ABI 2, INSTALL status 0, validated 253, setup 167, activation 86, peer status 0, bound `0x80000` and both CPU0 waits below it before saving installed state. The model checks the raw matching mailbox reply in the saved journal offline before menu use. Separately inspect the fields actually present in `ram-push-state.json`: base, size, install/rollback/status entries, manifest/runtime SHA-256, all 253 publications, stock slot word `0x00dc7874`, bootstrap and nonce. Require equality with the accepted actual-base manifest and allocation; the mailbox fields are not fields of that file. An uncertain result stops; do not retry, reallocate, unload or free.

## 4. Bounded bank-use sequence

1. Start external phone video of the camera LCD, not camera movie recording. Switch S to M and require the recorded ordinary M settings. Require one EDIT/SAVE CUSTOM SETTING and one SELECT CUSTOM SETTING on shooting menu page 5. A mismatch stops the trial.
2. Enter SELECT once. Require NONE and an ordinary child page, with no diagnostic banner or MANUAL-header parent return. Leave with BACK without selecting a slot. With no card, CARD ERROR on the shooting screen after BACK is the empty-slot baseline. Any refusal or other abnormality stops; do not create a bank to repair a failed entry.
3. Enter EDIT/SAVE, choose C1 and SAVE CURRENT. Require completion and return, not a hang or error. This creates the initial v2 generation in RAM. Return to page 5.
4. Require the recorded ordinary HIGHLIGHT TONE value 0. Edit C1's working HIGHLIGHT TONE to -1, matching the exercised path. Confirm the field with OK, then leave the editor with BACK. Item OK stores the working field; there is no separate whole-edit confirmation. Do not rename, reset or change another field. A different ordinary baseline stops rather than being changed to fit the test.
5. SELECT C1. Require a return to page 5 and the chosen highlight value in the ordinary shooting settings. Keep the dial at M. No shutter, mode excursion, other bank or additional edit in this sequence.
6. SELECT NONE. Require a return to page 5 and restoration of the recorded ordinary highlight value and watched M settings. Then stop menu use. Emulated AUTO excursion coverage neither accepts that path physically nor expands this sequence.
7. Switch off, disconnect USB and remove battery/dummy supply for at least ten seconds. A switch cycle alone does not clear the push: added rows stayed after one (E-RAM-PUSH253-SWITCH-RECOVERY-FAIL-150). The R1 allocation and R3 push state are invalid after power removal. Neither state records a power-cycle identity; only the allocation state has a stored content guard. Never reuse those states or that actual-base manifest for a later trial. Do not use `ram-unload` or `ram-free` as cleanup.

Any unexpected response, screen, delay, disconnect, failed check or uncertain operation stops the sequence immediately. Do not attempt NONE cleanup after a failure. Use the power switch; if shutdown does not complete within 60 seconds, disconnect USB and remove the battery or cut the dummy supply, then wait ten seconds. Interrupting stock settings activity remains a residual risk, and SD recovery is unavailable.

## 5. Exit and acceptance boundary

Stage 5 shutdown/base restoration is absent. A successful path selects NONE before power removal; a failed path exits without further menus. Power removal while a custom selection is active loses its stored ordinary base while the applied stock settings may persist. Volatile bank storage does not prevent stock settings persistence, and no boot-time base restoration exists.

Preserve the external phone video and host journals. Under the no-card exception, a separately directed ordinary reboot with USB disconnected checks normal startup without recipe rows/setup prompt and unchanged watched M/C settings. After renewed attachment and read grant, the existing 63-window `ram-push-post-reboot-private253.json` plan must confirm every publication and the callback slot restored. A screen report alone does not establish stock-word recovery. Color capture/save/playback cannot be accepted with the damaged, empty slot and remains deferred until repair. No automatic firmware restoration or repeated RAM trial; a later trial starts with a fresh go, allocation and authenticated relink.

This is a bounded volatile-bank RAM procedure, not physical feature acceptance or permanent-install readiness. A live attempt from this tree requires the import review of its pins, the passing pre-go gate, fresh availability/preflight/go and independent actual-base review. Flash remains non-installable; persistent boot/shutdown and later management/auto-update stages remain outside scope.
