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
- Teardown: `ram-unload` restores every publication word and frees the allocation at power-off. Observed: 222 old words verified, rollback mailbox `operation 2`, `restored 222`, `status 0`, `peer_status 0`.
- Host loss: the call has no host operation, pending completion, allocation transfer, retained host pointer or post-return callback. Host loss inside the synchronous interval therefore has nothing to detach; the whole-payload detached session is not claimed, and no detach API is added for this case.

## Effects

The adapter returns one word and writes nothing: the Rust entry holds no state, the trampoline writes only its own 8-byte frame, and the bank ELF allowlist proves the replacement's stores reach only bank data and the entry stack and that its only external branch is the declared `0x00d52684`.

The direct qualification cases split every row, so each execution's write provenance is complete (the native log retains 4096 events; the combined 21-row diagnostic exceeded that at 17711). The checked consumer declares `casebuf[0x1e00,0x2000)` as stack scratch and the stock table mirror as transient scratch. Every write must stay inside those spans and every other byte in every declared region must equal its initial value. The stack is filled with a non-zero pattern so an unwritten frame byte is not mistaken for zero; the stock table mirror's final flag is transient scratch and is not a service output. This bounds the adapter and the executed stock closure together; it is not a claim that the probe has no other effects on the body.

## Verification

- Direct native rows:
  ```sh
  devenv shell -- bazel build //emulation:recipe-bank-cases //emulation:recipe-bank-interop //replacement/bank:bank
  CASE='svc-*' JOBS=1 devenv shell -- bash emulation/build-recipe-bank-cases.sh
  ```
  The generator also accepts `--service-qualification`, which selects the same 26 rows when it is invoked directly with its build/runner arguments.
  26 bounded cases enter the built `rb_service_field_row_state`, the assembly trampoline and `FUN_00d52684`. Property `0x110` rows `0..4` each return `0` (offered); the 21 stock ISO-list rows of property `0x141` return `3` for row `0` and `0` for the rest in the M fixture. Each case checks `r4-r11` against non-zero distinct seeds, `sp` against its initial value, `r0` against the independent expectation, complete write confinement and immutable non-scratch bytes; the stock table mirror is transient scratch whose final flag is not asserted. The same consumer refuses an injected `r7` clobber and an injected immutable write; the report is `service-qualification.json`. `//replacement/service:qualification-tests` also runs a test-only ARM effect mutant whose real code writes `casebuf+0x100` and restores it: the final bytes are unchanged and the native consumer still refuses the write.
- Packaged receipt consumer:
  ```sh
  devenv shell -- bazel run //replacement/service:packaged-qualification -- \
    --receipt E2E_DIR --package PACKAGE_DIR [--report FILE]
  ```
  The consumer requires a candidate-qualified receipt, the relinked manifest and `kernel.syms`, and the package archive/facts identities. It resolves `svc-rust-entry`, `svc-stock-trampoline` and `svc-stock-return` from the relinked symbol table and `stock-row-probe` from the fixed stock address, then requires each of the five selected `0x110` rows twice with the ordered entry, trampoline, outer stock call, one or two grounded `FUN_00d52684(5,0x1a)` nested calls and trampoline return. Stock calls outside those intervals are reported separately. It also requires matched `service-page-cancel`/`service-field-reopen` phases, the post-unload stock observation and the rollback mailbox. The Rust entry tail-branches; `rb_call_stock_probe+0x10` is the return. `service-field-reopen` reuses the already-exercised `ui-trajectories.json` transition to Edit2 (`menu/recipe_bank_ui.h:165-168`: Save Current, Edit Name, Edit, Reset), with no reset/apply step.
- Diagnostic native case `e-iso-edit-list-rows`: the 21-row ISO edit list through host `0x26e` still checks the exact public 168 bytes. Its stack residue is not an expectation; the direct rows above own the stack/ABI contract.
- Packaged route: `//emulation:xm1-viewer -- --ram-push-e2e --cardless --fujimod --unreviewed-relink` with the cardless inputs and `emulation/ram-push-service-trajectories.fujimod.json`. Receipt 7 observed allowlist pass with zero violations; manifest carries the crate sources, the archive digest and the toolchain-facts digest; `ram-push` publishes; the edit value list for host `0x268` asks stock property `0x110` for rows `0..4` 35 times across re-renders (`r0=0x110`, `r1=0..4` at `0x00d52684`); the list's cursor moves through those rows and the trajectory's menu states match; `ram-unload` verifies the 222 old words. Receipt 7 binds the pre-frame-change archive. The final package at `final-service-package-20261027/recipe-complete.DAT` has sha256 `393bd420562ed0afbeefa6cabd696e3ad544f483ee3abacc562851505e6b0262`, archive `55628c4af69db2031562d9cda71e28a0db88f614aa0fad394be42d7ad90f3185`, toolchain facts `5b765637e7d5bbc170569b0b0e91860b3f334f5a45572e0e41075a3a51b3bd3f` and kernel `f786647bb1aeaeeafc0f1396272db21b7ca7b9cf646b81dae78dcbb15e044329`. `final-service-package-20261027-relocated/` preserves those bytes and the same firmware/executable identities, changing only the two relink input paths to `inputs/bank.a` and `inputs/build-facts.txt`; `relocation.json` records the path relocation and `run-plan.json` carries the final RAM-push and packaged-qualification commands. The run plan uses a checkpointed boot without `--fresh-boot`; an existing checkpoint is restored only when the final DAT, stored-f7 NAND, guest state, UI conversion profile and host procedure match. `--fresh-boot` is a fallback only after a matching keyed checkpoint fails identity validation, with the rejected DAT/runtime/procedure identities recorded. Receipt 7's DAT `acb0fbbb9507421f48a6254920cafb5ce20e1fd6bf821bc80c912c1f99c29ca8` does not match the final DAT and its checkpoint is not used. A fresh RAM-push/unload run against the preserved package is pending compute.
- Negative controls: a bank object that stores to an undeclared address fails the allowlist before USB; a manifest that differs from the relinked runtime or its contract fails authentication before USB; and the allowlist refuses `rb_media_request_post`'s open-completion store when its declaration is removed or narrowed.

## Retained dependencies

Stock image 5 code, jump table and live cells behind `FUN_00d52684` and its callees; the stock probe epilogue; the C installer and publication mechanism; the bank's other C modules; `core` and `compiler_builtins` from the pinned cross toolchain.

## Hardware-only unknowns

The probe's interrupt state and stack depth under the menu task on the body; stock callers entering it with properties outside the edited-field set; transient writes the model does not show; timing under the real render task. No camera run is claimed.

## Open prerequisites

- A reviewed publication: the reviewed RAM-push contract is fail-closed against the new bank sources and identities until they are re-pinned.
- A fresh packaged RAM-push/unload receipt for the final archive after the trampoline frame change.
- The eight unexercised edit-field mappings and the stock probe's other mode columns.
