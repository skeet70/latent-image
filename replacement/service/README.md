# Retained-service pilot: the stock row-availability probe

The packaged recipe-bank payload's field-row lane calls the X-M1 1.50 stock render probe `FUN_00d52684` and returns its state. Issue: [#123](https://github.com/skeet70/latent-image/issues/123).

## Target

| Field | Value |
| --- | --- |
| Body, firmware | X-M1, 1.50 |
| Retained symbol | `FUN_00d52684`, image 5, size `0x6038` (`reports/firmware-extraction.md` row 117) |
| Replacement identity | `replacement/bank/lib.rs` compiled for `armv7r-none-eabi` with `replacement/service/lib.rs` as a module; one staticlib linked into the existing bank payload |
| Mode | compatibility; the caller's result is unchanged for every state the stock function defines |
| Provenance | original implementation from the recovered contract; the stock function stays the oracle |

## Pieces

| Piece | Owner | Duty |
| --- | --- | --- |
| `menu/recipe_bank_edit.c` `rb_stage_edit_field_prop` | retained C | maps an edit-field host `0x267..0x26f` to the stock property it edits |
| `menu/recipe_bank_probe.c` `rb_probe_state` | retained C | answers every other property and forwards the field-row lane |
| `replacement/service/lib.rs` | owned Rust | closed `RowState` set, caller conversion for an out-of-domain word, the forward call |
| `replacement/service/rb_service_stock.S` | owned assembly | the AAPCS branch to `0x00d52684`; its 8-byte frame holds `r12` and `lr`, keeps `sp` aligned, and saves no callee-saved register |
| `replacement/bank/lib.rs` | owned Rust | the crate root: both modules, one panic handler |
| `replacement/service/Replacement/Service/Qualify.hs` | owned Haskell | the direct-case contract, write confinement and qualification report |
| `emulation/Emulation/NativeObservation.hs` | owned Haskell | the shared checked runner-output parser used by the service and UI consumers |
| `FUN_00d52684` and its callees/tables | retained stock | the row state |

## Owned capability and non-goals

Owned: the typed binding of the field-row lane. `rb_probe_state` maps the host and the Rust entry asks the retained probe for each row state. Non-goals: other properties' row states, the other consumers of the stock probe, the eight field mappings other than the exercised one, and any change to the states the menu receives.

## Boundary

- Call: `u32 rb_service_field_row_state(u32 field, u32 row)`, ARM AAPCS. `field` is the stock property from `RB_EDIT_FIELD`, `row` is the row token the menu walk passes.
- Retained call: `FUN_00d52684(field, row)`, result in `r0`. States are `0` offered, `2` disabled, `3` hidden (`menu/recipe_bank_ui.h`).
- Preserved: `r4-r11` and `sp`; the trampoline keeps `sp` 8-byte aligned across the call and touches only `r12` and `lr`. Its frame does not save `r4-r11`, so a stock clobber of those registers reaches the caller.
- Callers: `rb_probe_state`, reached from the bank's probe entry published over the stock probe's default branch. No callbacks or indirect references originate in this lane.
- Unknown results: a word outside the state set is returned to the caller unchanged, which is what the previous direct cast did. It is not a qualification success: the trace records the probe's arguments, and a word outside the set fails the declared domain rather than becoming a state.

## Lifetime

- Context: the menu task's synchronous row walk. One call returns before the walk continues; no completion, mailbox, allocation or retained state between calls.
- Repeated entry: once per rendered row, and the same field and row across page entries.
- Cancellation: nothing inside the call can be cancelled; leaving the page cancels the visit and the next visit calls again. No handback state exists.
- Teardown: `ram-unload` restores every publication word and leaves the 122,008-byte camera allocation retained and owned until power-off; unload does not free or reclaim it. Page visits reuse that allocation. Observed: 222 old words verified, rollback mailbox `operation 2`, `restored 222`, `status 0`, `peer_status 0`.
- Host bounds: one emulator slot, `MemoryMax=8G`, no swap, and the thermal guard stops the slot at `Tctl` 85 C (`build-support/README.md`).
- Host loss: the call has no host operation, pending completion, allocation transfer, retained host pointer or post-return callback. Host loss inside the synchronous interval therefore has nothing to detach; the whole-payload detached session is not claimed, and no detach API is added for this case.

## Effects

The adapter returns one word and writes nothing: the Rust entry holds no state, the trampoline writes only its own 8-byte frame, and the bank ELF allowlist proves the replacement's stores reach only bank data and the entry stack and that its only external branch is the declared `0x00d52684`.

The direct qualification cases split every row, so each execution's write provenance is complete (the native log retains 4096 events; the combined 21-row diagnostic exceeded that at 17711). The checked consumer declares `casebuf[0x1e00,0x2000)` as stack scratch and the stock table mirror as transient scratch. Every write must stay inside those spans and every other byte in every declared region must equal its initial value. The stack is filled with a non-zero pattern so an unwritten frame byte is not mistaken for zero; the stock table mirror's final flag is transient scratch and is not a service output. This bounds the adapter and the executed stock closure together; it is not a claim that the probe has no other effects on the body.

## Verification

- Direct native rows:
  ```sh
  devenv shell -- bazel build //emulation:recipe-bank-cases //emulation:recipe-bank-interop //replacement/bank:bank
  CASE='svc-*' JOBS=1 devenv shell -- env XM1_EMULATOR_SLOTS=1 XM1_EMULATOR_MEMORY_MAX=8G \
    bazel --batch run //emulation:emulator-slot -- \
    taskset -c 0,1 cpulimit --include-children --limit=75 -- \
    bash emulation/build-recipe-bank-cases.sh
  ```
  The generator also accepts `--service-qualification`, which selects the same 26 rows when it is invoked directly with its build/runner arguments. `//replacement/service:qualification-tests` executes the real ARM effect mutant and must use the same emulator-slot wrapper, not a bare host run.
  26 bounded cases enter the built `rb_service_field_row_state`, the assembly trampoline and `FUN_00d52684`. Property `0x110` rows `0..4` each return `0` (offered); the 21 stock ISO-list rows of property `0x141` return `3` for row `0` and `0` for the rest in the M fixture. Each case checks `r4-r11` against non-zero distinct seeds, `sp` against its initial value, `r0` against the independent expectation, complete write confinement and immutable non-scratch bytes; the stock table mirror is transient scratch whose final flag is not asserted. The same consumer refuses an injected `r7` clobber and an injected immutable write; the report is `service-qualification.json`. `//replacement/service:qualification-tests` also runs a test-only ARM effect mutant whose real code writes `casebuf+0x100` and restores it: the returned ABI, final bytes and complete provenance are asserted, and the native consumer still refuses the write. Its NOP-store control writes nothing, satisfies the same assertions and qualifies.
- Packaged receipt consumer:
  ```sh
  devenv shell -- bazel run //replacement/service:packaged-qualification -- \
    --receipt E2E_DIR --package PACKAGE_DIR --reference-dat REFERENCE_DAT [--report FILE]
  ```
  The consumer requires a candidate-qualified receipt, the relinked manifest and `kernel.syms`, the package archive/facts identities, and the typed boot/install reference. The CLI accepts `--reference-dat FILE`; `loadReferenceDat` hashes the file once at that boundary and returns an opaque `ReferenceDat`. `PackagedInputs` carries that value, and the consumer requires the receipt's `installed_dat` to equal its digest; `stock_sha256` remains the package's build provenance. It resolves `svc-rust-entry`, `svc-stock-trampoline` and `svc-stock-return` from the relinked symbol table and `stock-row-probe` from the fixed stock address, then requires each of the five selected `0x110` rows twice with the ordered entry, trampoline, outer stock call, one or two grounded `FUN_00d52684(5,0x1a)` nested calls and trampoline return. Stock calls outside those intervals are reported separately. It also requires matched `service-page-cancel`/`service-field-reopen` phases, the post-unload stock observation and the rollback mailbox. The Rust entry tail-branches; `rb_call_stock_probe+0x10` is the return. `service-field-reopen` reuses the already-exercised `ui-trajectories.json` transition to Edit2 (`menu/recipe_bank_ui.h:165-168`: Save Current, Edit Name, Edit, Reset), with no reset/apply step.
- Diagnostic native case `e-iso-edit-list-rows`: the 21-row ISO edit list through host `0x26e` still checks the exact public 168 bytes. Its stack residue is not an expectation; the direct rows above own the stack/ABI contract.
- Packaged route: `//emulation:xm1-viewer -- --ram-push-e2e --cardless --fujimod --unreviewed-relink` with the cardless inputs, `emulation/ram-push-service-trajectories.fujimod.json`, and the preserved `final-service-package-20261027-relocated/` package. `run-plan.json` owns the checkpoint choice and preserved input paths; the recipe manifest and relocation record own artifact identities. The paced final launch used one outer verified thermal guard, `taskset`/soft-75 `cpulimit`, `bazel --batch`, then `//emulation:xm1-viewer`; the inherited-slot fix was applied. The cardless command correctly omits `--sd-card-image`; the existing synthetic FAT32 fixture and provenance remain named in the run plan for identity, not passed under `--cardless`. Receipt7 is not candidate evidence. The final receipt must cover publication and rollback, symbol-bound Rust entry/trampoline/stock/return, all five selected `0x110` rows in both `service-field-list` and `service-field-reopen`, page-visit cancellation/reopen, unload, post-unload stock activity and the actual service captures. The qualified packaged run booted the RC0 prefix `acb0fbbb...ca8` as the installed reference and separately pushed the candidate package `393bd4...` with archive `55628c...` and facts `5b7656...`; vendor stock `43d67a...` remains build provenance only. The 222 publication old words come from that reference image set (93 RC0 image-4 prefix, 44 plain, 36 image 5, 10 image 2, 39 zero below the RC0 image-4 load); install published all 222 and rollback restored all 222 old words. The 122,008-byte allocation remains owned until power-off. The selected five `0x110` rows return `0` in both visits; the report records 75 owned sequences, 15 entries and 30 nested `FUN_00d52684(5,0x1a)` calls per row, and 2,949 retained stock hits. Page cancellation, reopen, unload and post-unload stock activity all passed. The consumer report is `private-data/scratch/issue117-service-final/receipt-complete/service-packaged-qualification.json`; the actual captures are `private-data/scratch/issue117-service-final/receipt-complete/screenshots/service-slot-list.png` and `service-field-list.png`. Pass absolute input and output paths with `bazel run`; it does not resolve workspace-relative `private-data` paths.
- Negative controls: a bank object that stores to an undeclared address fails the allowlist before USB; and a manifest that differs from the relinked runtime or its contract fails authentication before USB. From `e014759` through `9ee0398`, the allowlist also refused `rb_media_request_post`'s open-completion store when its declaration was removed or narrowed. `c6c760a` removes that store from the `RB_VOLATILE_BANK=1` build, the only build the allowlist checks, so the declaration and that control are gone.
- Negative acceptance evidence: `//replacement/service:qualification-tests` executes the real ARM case `service-effect-mutant` in `replacement/service/ServiceQualifyTests.hs`; its code writes `casebuf+0x100` and restores it, and `runServiceCase` reports `status: failed`. The test asserts that the real effect mutant is refused. The 52 `mutants` entries in the 26-row direct `service-qualification.json` are injected observation checks (`abi-r7-clobber`, `effect-immutable-write`), not separate candidate binaries; they are parsed-fixture gating. The direct 26-row ABI/effect proof is actual ARM final-bank execution; no packaged mutation mechanism is added and no full-system register/effect coverage is claimed from the E2E trace.

## Retained dependencies

Stock image 5 code, jump table and live cells behind `FUN_00d52684` and its callees; the stock probe epilogue; the C installer and publication mechanism; the bank's other C modules; `core` and `compiler_builtins` from the pinned cross toolchain.

## Hardware-only unknowns

The probe's interrupt state and stack depth under the menu task on the body; stock callers entering it with properties outside the edited-field set; transient writes the model does not show; timing under the real render task. No camera run or human visual/UI acceptance is claimed; the two PNGs are emulator captures.

## Open prerequisites

- A reviewed publication: the reviewed RAM-push contract is fail-closed against the new bank sources and identities until they are re-pinned.
- The eight unexercised edit-field mappings and the stock probe's other mode columns.
