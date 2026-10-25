# Issue #83: SELECT confirmation feedback review brief

## Decision requested

Review the new X-M1 1.50 stock call and bank presentation changes before any H2 RAM-push run. This is a mechanism revision because `menu/recipe_bank_allowlist.json` admits `0x0061ceac`. The author has not approved the change or any review pin. Host execution is H0. The owner is asleep, camera access is forbidden, and the owner's visual check remains pending.

## Source changes

- `menu/recipe_bank_actions.c`: after a successful SELECT child transition, refresh its stock snapshot after the bank resets the live entry cursor to NONE and refreshes slot names. Other child classes and refused transitions do not make the new call.
- `menu/recipe_bank_arm.S` and `menu/recipe_bank_ui.h`: an A32 trampoline and typed `RbMenuRoot` argument for `FUN_0061ceac`.
- `menu/recipe_bank_allowlist.json`: explicitly admit that call with its source evidence. No store declaration, unproven declaration or other stock call is broadened.
- The verb drawer sets its current-value row to the hidden dialog row, index 4, before calling the existing stock pane renderer. It does not change the cursor, animation preparation or selection data.
- The UI native harness maps SELECT's stock snapshot and asserts the saved cursor token consumed by the blink; its verb-pane case reads the hidden current token through the stock accessor. The dedicated cardless trajectory selects C1, reopens SELECT on NONE and confirms NONE without moving the cursor.

## Cause and X-M1 evidence

The 1.50 stock transition at `0x00bfab98` completes its page walk at `0x00bfacf0` and saves the resulting frame snapshot with `FUN_0061ceac(root)` at `0x00bfacfc`. `rb_ui_child_enter` then resets the live cursor to row 0. Before the fix, immediately confirming NONE after reopening SELECT leaves the snapshot carrying the prior C1 cursor. The kind-7 preparation in `FUN_01165aa0` reads the depth-2 saved snapshot through `FUN_0061ce1c(root, 2, snapshot)`. A cursor move happens to refresh it through stock navigation.

Private source inputs, retained locally rather than copied into this brief:

- `private-data/analysis/x-m1-1.50/filmsim/id5.dis`, `0x00bfab98..0x00bfad00`, the transition and snapshot-save order.
- `private-data/analysis/x-m1-1.50/filmsim/id4.dis`, `0x0061ceac..0x0061cf10` and `0x0061cd14..0x0061ce18`, the refresh and snapshot stores.
- `private-data/analysis/x-m1-1.50/filmsim/ghidra/all.c`, `FUN_0061ca2c`, `FUN_0061cd14`, `FUN_0061ce1c`, `FUN_0061ceac`, the decoded depth, saved-snapshot and fresh-snapshot interfaces.
- `private-data/analysis/x-m1-1.50/decode/ghidra/img6.c`, `FUN_01165aa0` and `FUN_011662a0`, the kind-7 saved cursor and current-value marker consumer.
- `host/firmware/BuildRecipeBank.hs`, SELECT's root 1 and childless row values, and the four verb rows followed by hidden dialog token `0x82`.

## New call contract and stores

`rb_call_snapshot_store((RbMenuRoot){child->id})` passes the SELECT root, 1, in r0. It executes only after `rb_call_transition` returns `0xffffffff` or 2 and the child is class `RB_UI_CLASS_SELECT` (`0x285`). The bank's entry cursor is then 0, which indexes the valid NONE row. SELECT's current-value row is a valid effective-selection row or its in-array terminator.

`FUN_0061ceac` calls `FUN_0061ca2c` to determine frame depth. Depth 1 uses `0x0093eb74 + root * 0x2c`; depth 2 uses `0x009412d4 + root * 0x2c`; depth 0 uses `0x00944414`; another depth returns without copying. SELECT's parent and child both have root 1, so its admitted route has depth 2 and writes `[0x00941300,0x0094132c)`.

`FUN_0061cd14` initializes and fills eleven words: depth, root, node type, class, cursor token, current-value token, raw cursor, visible ordinal, scroll state, the stock page calculation and the remaining navigation word. Its reads come from the current frame, its values array and the stock navigation cells. The call closure writes the destination snapshot and ordinary call-stack frames. It does not apply a setting, post a message, allocate memory, touch a device register or write persistent storage.

The call remains in the stock menu action task after the successful push. It refreshes exactly the presentation state that stock navigation already saves, before a later SET pop can request the confirmation animation. No cross-core atomicity claim is added: stock saves the same multiword record using sequential stores. The failed-transition path restores the child's parent and returns before this refresh.

## Review gates and limits

Every relink must pass the bank ELF allowlist without an override. Re-pin the four changed source identities as unreviewed: actions, arm, header and allowlist. The publication contract and installer do not intentionally change. Byte and reviewed-build checks establish the pins; verdict checks must continue refusing a camera go until an independent reviewer approves the mechanism. The cardless volatile e2e, dedicated screenshot trajectory and verifier bank routes other than native-sd are the current host gate. Native-sd is deferred under the owner's explicit instruction until EventsCap's fix reaches main, then main is merged and that route runs once.

Screenshots and receipts live under `private-data/analysis/select-blink-83/`. They establish emulated presentation only. Independent review, the owner's visual check, and any separately authorized physical acceptance remain open.

## Host gate results

The cardless volatile e2e and both verifier bank routes execute successfully. All 362 differences from the shared volatile baseline are classified in `private-data/analysis/select-blink-83/baseline-difference-classification.json`: 11 nonce fields, 326 symbol-bound relocation fields and 25 manifest/source-identity or static-audit fields. Menu states, steps, observation outcomes and all seven screenshots are unchanged. Both manifests contain 253 publications with unchanged sites, old words, cores and phases. The owner authorized only `private-data/verify-candidate/baselines-select-blink/`; recording and comparison there pass for `ram-push-volatile` and `native-volatile`. Shared baselines are unchanged.

At JOBS=4 with the same `package-20261024/image` fixture, unchanged main `3525e2e` and this branch both pass 52/55 UI cases. The same three cases stop at `stub_contract` with identical aggregate and native mismatch lists: `n-rename-select`, `n-reset-select-other` and `n-select-nothing-restores-base`. Detailed results are in `private-data/analysis/select-blink-83/ui-main-comparison.json`.

The UI script now explicitly binds getter/setter callbacks and SELECT's linked value/label arrays to the current kernel symbols before running. The generator validates every consumed declared bank pointer and rejects unbound bank-code pointers before overlaying the plan. `//emulation:recipe-bank-ui-plan-tests` passes stale-callback, stale-array, binding and unbound-pointer checks.

The all-slot trajectory captures all four verbs in all seven slots under `private-data/analysis/select-blink-83/fixed-slots/segment-slot-feedback/screenshots/`. Raw C1-to-NONE samples and snapshot/animation records are in `transient-before-v3/` and `transient-after-v3/`. Saved SELECT cursor tokens change from C1 (`0x61`) to NONE (`0x60`). Image preview returned inconsistent pictures for byte-identical PNGs, so visual confirmation of the actual blink frame remains pending.
