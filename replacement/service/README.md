# Retained-service pilot: the stock row-availability probe

The packaged recipe-bank payload's field-row lane calls the X-M1 1.50 stock render probe `FUN_00d52684` and returns its state. The linked Rust entry is `rb_service_field_row_state` (`lib.rs`); the branch to the stock address is `rb_service_stock.S`. Issue: [#123](https://github.com/skeet70/latent-image/issues/123).

## Target

| Field | Value |
| --- | --- |
| Body, firmware | X-M1, 1.50 |
| Retained symbol | `FUN_00d52684`, image 5, size `0x6038` (`reports/firmware-extraction.md` row 117) |
| Replacement identity | `replacement/service/lib.rs` compiled for `armv7r-none-eabi`, linked into the existing bank payload |
| Mode | compatibility; the caller's result is unchanged for every state the stock function defines |
| Provenance | original implementation from the recovered contract; the stock function stays the oracle |

## Owned capability and non-goals

Owned: the field-row availability lane of the bank's probe. `rb_probe_state` maps an edit-field host (`0x267..0x26f`, `menu/recipe_bank_edit.c`) to the stock property it edits, and the lane asks the retained probe for each row state. The Rust entry is the only code in that lane.

Non-goals: every other property's row state (the C lane answers those), the stock probe's other consumers, the eight field mappings other than the exercised one, and any change to the states the menu receives.

## Boundary

- Call: `u32 rb_service_field_row_state(u32 field, u32 row)`, ARM AAPCS. `field` is the stock property from `RB_EDIT_FIELD`, `row` is the row token the menu walk passes.
- Retained call: `FUN_00d52684(field, row)`, result in `r0`. States are `0` offered, `2` disabled, `3` hidden (`menu/recipe_bank_ui.h`).
- Preserved: `r4-r11` and `sp`; the trampoline keeps `sp` 8-byte aligned across the call and touches only `r12` and `lr`.
- Callers: `rb_probe_state` (`menu/recipe_bank_probe.c`), reached from the bank's probe entry published over the stock probe's default branch. No callbacks or indirect references from the retained call are observed.
- Unknown results: a word outside the state set is returned to the caller unchanged, which is what the previous direct cast did. It is not a qualification success: the run's trace records the probe's arguments and the native case's expectations define the accepted states, so an out-of-domain word fails qualification rather than becoming a state.

## Lifetime

- Context: the menu task's synchronous row walk. One call returns before the walk continues; there is no completion, mailbox, retained allocation or state between calls.
- Repeated entry: the probe runs once per rendered row and revisits the same field and row across page entries.
- Cancellation: nothing inside the call can be cancelled; the enclosing page visit can be left, and the next visit calls again. No handback state exists.
- Teardown: the payload's teardown is `ram-unload`/`ram-free`, which restores the stock probe's branch. Nothing in this lane needs its own teardown.
- Host loss: the call has no host dependency. A pushed payload keeps answering the camera's own menu walk after the host session ends; the host-loss case is about the payload and installer, not this call.

## Effects

The call returns one word and writes nothing outside its own stack. Evidence: the `e-iso-edit-list-rows` native case runs the stock probe under the bounded runner's declared-span rule, which refuses any undeclared byte change; the bank ELF allowlist proves the replacement's stores reach only bank data and the entry stack and that its only external branch is the declared `0x00d52684`.

## Verification

- Native case `e-iso-edit-list-rows` (`emulation/RecipeBankCases.hs`): the ISO edit list, host `0x26e`, stock property `0x141`: row `0` hidden, the other twenty rows offered, AUTO row `0x1b` among them.
- Packaged route: `//emulation:xm1-viewer -- --ram-push-e2e --unreviewed-relink` on the FujiMod RAM-push bridge with the cardless inputs, `emulation/ram-push-service-trajectories.fujimod.json`, segments `service-bank-navigation,service-save-current,service-field-list`. The receipt records every entry of `0x00d52684` with `r0`/`r1` (the field and row the packaged build asked for) and the row-state window the list received.
- Negative controls: a bank object that stores to an undeclared address fails the allowlist before USB; a manifest that differs from the relinked runtime or its contract fails authentication before USB.

## Retained dependencies

Stock image 5 code and tables behind `FUN_00d52684`, including the live settings and mode cells it reads; the C installer and publication mechanism; the bank's other C modules; `core` and `compiler_builtins` for the Rust object; the stock probe's epilogue.

## Hardware-only unknowns

The probe's interrupt state and stack depth under the menu task on the body; whether any stock caller enters it with a property outside the edited-field closure; timing under the real render task. No camera run is claimed.

## Open prerequisites

- The shared armv7r-none-eabi Rust build (#119) supplies the object, its compiler identity and the panic contract.
- The manifest binds `replacement/service/lib.rs`, `rb_service_stock.S`, the header and the compiled object digest; a reviewed publication still needs the corresponding review-record pins.
- The other eight edit-field mappings have no full-system expectation yet; the pilot claims only the exercised one.
