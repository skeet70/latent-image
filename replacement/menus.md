# Menus and RAM-package workflows

[Replacement index](README.md). The [menu project #128](https://github.com/skeet70/latent-image/issues/128) owns open work. The [owned engine design](../reports/owned-menu-engine-design.md) owns its boundary, shims and current qualification; [controller ownership](../reports/recipe-bank-ui-ownership.md) owns recovered menu behavior. Renderer packaging has its own [render-adapter guide](render_adapter/README.md).

## Bank publication selection

`//replacement/bank:bank` is the standard archive. `:bank-modal`, `:bank-lifecycle` and `:bank-modal-lifecycle` compile the same root with independent scoped selections. The matching `//host/firmware:ram-push-{modal,lifecycle,modal-lifecycle}-relinker` filegroups select `RB_PUBLICATION_GROUPS` at the shared [RAM builder](../host/firmware/build-recipe-bank-ram.sh). The [layout module](../host/firmware/RamPushLayout.hs) owns variant counts and allocation geometry; do not infer a variant from a payload display name.

- Ordinary `--ram-push` selects the standard package.
- `--ram-push-variant modal75`, `lifecycle` or `lifecycle,modal75` selects explicit optional groups in canonical order.
- `modal75,menu-engine` adds the engine; `modal75,menu-engine,render` adds the renderer; `modal75,menu-engine,menu-value` adds the value transaction. Renderer and value publication are separate opt-ins.

The builder composes the base allowlist with selected fragments in lifecycle-then-modal order, refuses missing/reordered/unselected inputs and duplicate capabilities, and emits `bank-allowlist-input.json`. `bank-allowlist.json` is the checker's report, not the input. It also emits `kernel-text.bin` separately from `kernel.elf`; native consumers bind text, ELF and symbols independently.

Scoped modal packaging emits `modal-scope.json` with controller 75 and `global_admission=false`. Source/build admission does not qualify all controllers or the selected lifecycle. [Lifecycle project #129](https://github.com/skeet70/latent-image/issues/129) owns its completion/device prerequisites; the [transport guide](../transport/README.md#bank-elf-allowlist) owns the allowlist contract. Historical package-selection refusals are not current build instructions.

## Engine inputs and host oracle

`menu_engine/` contains the address-independent engine and stock-state mirror; `menu_engine/adapter/` is the guest seam. The [engine boundary](../reports/owned-menu-engine-design.md#engine-boundary) and [guest placement](../reports/owned-menu-engine-design.md#placement-in-the-guest) explain ownership/fallback and retained services. Event-class coverage belongs in the engine and [increment reports](../reports/owned-menu-engine-design.md#increments), not a second evolving class list here.

```sh
devenv shell -- host-build build //replacement/modal_runtime:menu-engine-tree \
  //replacement/modal_runtime:menu-engine-cases
devenv shell -- host-build test //replacement/menu_engine:menu-engine-tests \
  //replacement/modal_runtime:menu-engine-generator-tests
```

Both generators refuse existing output paths:

```text
menu-engine-tree --tables TABLES_JSON --tree-out NEW_TREE_RS --records-out NEW_RECORDS_RS --labels-out NEW_LABELS_RS
menu-engine-cases --out NEW_CASES_RS NAME=RECEIPT_DIR:EVENTS ...
```

`EVENTS` is a semicolon-separated sequence of `KIND,NODE,ID`; a `-` window generates no case. Quote or escape semicolons in shell arguments. Stage entries must match the declared events in order. A receipt with several cases yields `NAME-1`, `NAME-2`, etc. Receipts must bind the correct firmware, config, trace sites and watch ranges. Cases retain stock service order, the replayed block at each call, the after block, last-move pair, ordered snapshot writes and result. The [verification contract](../reports/owned-menu-engine-design.md#verification) owns the oracle expectations and controls.

The host owner harness consumes generated files through `RB_MENU_ENGINE_TREE`, `RB_MENU_ENGINE_RECORDS` and `RB_MENU_ENGINE_CASES`, compiling `menu_engine/src/lib.rs` with `--test --cfg rb_menu_engine_cases` using the [pinned toolchain](toolchain/README.md). Invoke the resulting executable as `owner --nocapture --test-threads=1`. Generated test inputs are not a new build system or permission to use a host-selected compiler.

## Value transaction

`menu_value/` owns the row-token to settings value commit the stage consumer posts after the engine accepts an event. The default rule resolves the open node's token through the property's conversion table; the IMAGE SIZE property is a composite that resolves the token's size/aspect pair into the two ordinary properties. The module names no stock address and writes only through the supplied settings service. Working cells, the current-property marker, the display update (including its `0x161`/`0x165` arm) and the size-family change marker `0x1e5` are returned as named retained effects for the caller that still owns them; they are not part of the owned write set.

The tables are generated from the pinned image-4 bytes and the menu-tables document. The generator checks every conversion row against the existing decoder, decodes the pair table at `0x7a00cc`, and refuses a document or image with another identity or a pair that disagrees with the intended token 1/2 values:

```sh
devenv shell -- host-build build //replacement/modal_runtime:menu-value-table
bazel-bin/replacement/modal_runtime/menu-value-table --image4 IMAGE4 --tables MENU_TABLES_JSON --out NEW_TABLE_RS
```

```sh
devenv shell -- host-build test //replacement/menu_value:menu-value-tests //replacement/modal_runtime:menu-value-table-tests
```

The host oracle drives entry capture, DOWN/UP commits, unconfirmed BACK restore and confirm without a write, and refuses an unoffered token or a missing table without writing. Conversion selects the first matching row and then rejects the `0x7ffffffe` sentinel; a later duplicate key cannot turn a failed conversion into success. The generator test checks the decoded tables, pairs and refusals through `generateTables`, not emitted source text.

Generated conversion rows, conversion registry, image-size pairs and the `#[used]` table root have typed static storage in `.menu_engine.rodata.value_tables`; identical conversion rows share one array. Mutable engine/value state and the C bank BSS stay in the writable bank-data region. The relinker renames only owned engine/value text and named read-only sections, not anonymous constants or archive-wide `.data`/`.bss`. `kernel.map` and `kernel-why-extract.tsv` record the linked demand and archive extraction chain. Garbage collection retains the declared publication entries through their linker roots and references.

The first guest seam is property-scoped at the stock dispatcher's jump-table word for `0xf6` (`0x00d50084`, stock `0x00d51b58`). With `--cfg rb_menu_value_owned` and `RB_MENU_VALUE_TABLE` naming the generated file, the word points at `rb_menu_value_size_arm`, which runs the owned transaction for every `0xf6` caller and returns to the stock arm only when the target refuses, before any effect. The continuations are linker-resolved symbols. The seam applies the stock size/aspect/third-cell comparison before the retained `0x1e5` marker call and skips the current-property marker and display update when the live mode is `0x2c`. Every other property keeps its stock word. Default bank publications are unchanged until the seam is admitted and qualified.

The integrated picker lifecycle intercepts only IMAGE SIZE. It begins after Push has landed on the picker, refuses a second begin, leaves the session open through a picker action, and closes on Pop: `0x1a`/`0x1c` cancel and other Pop ids confirm, including `0x1d`. Notification of the old parent falls through to stock. IMAGE QUALITY and other composites retain stock lifecycle until their complete affected-property sets are owned; the address-independent ordinary-value API is not package ownership of those properties.

`//emulation:menu-value-cases` runs stock and owned roles through the bounded native runner. The owned role patches the jump-table word to the compiled adapter image; a small carrier at the controller return site calls the dispatcher with the real return address, and the carrier returns through the exit link. Positive owned cases replace the stock arm's first instruction with a trap, so a declining owned path fails instead of passing on the stock arm. The driver compares status, cells, settings, registers and the retained call arguments across all twelve tokens, modes `0x2c`/`0x1f`/`0x20`/`0x21`/`0x24`/`0x25`, equal and different comparison cells including a third-cell-only change, the bank caller, the second controller return site and an unoffered-token fallback. An always-decline mutant image fails the positive cohort. The display and reconciliation routines are recording stubs, so their internal effects are not executed; the retained call arguments and their mode gate are observed.

The owned image is built with the property service's Cortex-R flags `-C target-feature=+strict-align -C jump-tables=no`. Without `+strict-align` the generated `Commit` copy emitted `ldrh r0,[r11,#-31]` at an odd frame offset, which the native runner refuses; the strict-align build emits byte loads instead.

With a sixth argument naming the image's symbol list, the same driver runs the picker session cohort after the stateless pairs. One carrier per role runs the whole sequence on one resident image without a reset: the compiled `rb_menu_value_begin` at open, repeated row selections through the stock dispatcher, `rb_menu_value_confirm` on LEFT and OK, `rb_menu_value_cancel` on BACK, and a reopen that captures the committed values. The stock role runs the same sequence through `0xd5b350`, so its handback is the stock save/restore rather than the owned scoped restore. The cohort covers the required counterexample (entry `(0,1)`, DOWN `(0,2)`, LEFT accept, reopen, DOWN `(0,3)`, BACK ends at `(0,2)`), OK accept, repeated DOWN/UP moves, a BACK with an unrelated `0x13` edit that persists under the owned restore, the derived `0x161`/`0x165`/`0x1e5` values, a bank commit followed by a cancel with no session, and the display mode `0x21`. The driver compares the working cells, the live settings block, the final property readouts and the display-call arguments between the roles; the two void retained helpers `0x63220c`/`0x63647c` are recording stubs whose arguments are not asserted, because their callers pass scratch registers and their effects are not consumer-visible in this carrier.

```sh
devenv shell -- host-build build //emulation:menu-value-cases
bazel-bin/emulation/menu-value-cases RUNNER IMAGE4 IMAGE5 OWNED_IMAGE OUT [OWNED_SYMBOLS]
```

The binary is built with the threaded RTS: `FujiAF.Host.runBoundedProcess` runs its stdin writer and output readers concurrently with `waitForProcess`.

## Guest package and comparison

For the guest tree, add `menu-engine-tree --package-plan BANK_PLAN_JSON` using the builder's own plan. This composes decoded rows with the installed package's rows. Package children are not decoded nodes; using a stock-only tree for the guest would misclassify them. The host oracle keeps the stock tree.

The engine archive is built with `--cfg rb_modal_scoped --cfg rb_modal75_owned --cfg rb_menu_engine_owned` and `-C llvm-args=-min-jump-table-entries=4096`, in addition to the normal bank flags. The jump-table policy applies to the entire bank crate. The viewer's `--menu-engine-owned` requires both `--ram-push-modal-scoped` and `--modal75-owned-archive`; see [scoped modal observation](../emulation/README.md) for the complete runtime invocation.

The shared relinker uses [the engine linker script](../menu/recipe_bank_ram_engine.ld) and [engine allowlist fragment](../menu/recipe_bank_menu_engine_allowlist_fragment.json). `EngineStack` bounds owned engine plus modal-dispatch depth while retaining a separate stock-callee margin. The [design's stack boundary](../reports/owned-menu-engine-design.md#placement-in-the-guest) records its evidence and exclusions; the estimate is not a whole-task stack proof.

[`emulation/menu-engine-pair-trajectories/`](../emulation/menu-engine-pair-trajectories/) generates pair inputs. `picker.jq` adds reads/sites to picker trajectories; `oracle.jq` maps menu-oracle paths onto packaged landings. `//replacement/modal_runtime:menu-engine-pair-check --package-plan` independently classifies events from the composed tables and observations, checks owned/fallback residency, guard traps, engine reads, posts and stack constraints. `modal-settings-compare` and `render-compare` complete a pair. `RuntimeRelocationCompare` does not admit this package.

`value.jq`, applied to `modal75-picker-trajectories/picker-246.json` with `--arg role stock` or `owned`, records separate open, move, LEFT/OK acceptance, reopen, BACK cancellation and handback captures, followed by unload and stock reopen. The value package's engine checker requires executed begin/apply/confirm/cancel/handback seams and refuses resident execution of the replaced IMAGE SIZE arm. Whole-settings readback and render comparisons remain required; trace coverage alone is not lifecycle correctness.

The combined value package links 226 publications in the unchanged `0x33d00` allocation at `0xa81be0`. The current `private-data/scratch/MenuFirmwarePackage/relink9/` map, extraction chain and enforced report record `.text` `0x506c`, writable `.data` `0xfc8`, engine region `0x142c0`, and zero violations. Earlier source/artifact admission is E-MENU-VALUE-PACKAGE-ADMISSION-150; actual controls and the remaining continuation dependency are recorded separately below.

The installed value package carries an explicit typed variant through manifest, transport state and lifetime-guard requests; untagged 226-publication requests retain the legacy lifecycle interpretation and cannot admit the value package. The canonical value source-identity bundle binds all three source files within the unchanged 68-entry bound. Controller-75 admission resolves the bank's published LEFT/RIGHT interposers to their stock identities while preserving the original fallback target. The exported confirmation entry remains callable and observable.

The actual stock and current owned changed-value guests complete the same four-phase prefix, with all 450 settings bytes and complete PNG/raw-plane captures equal. LEFT then stays at picker `(1,246,1)` instead of parent `(1,5,3)`. Installed-code probes prove the owned select sees busy query `0x53` return 1 through the retained `0x2000` pending-mask condition; the global busy word and `0x1240` query are clear, and AF result word `0x017030d0` remains 1. `private-data/scratch/MenuFirmwarePackage/actual-busy-dependency.json` binds these observations to the actual installed code bytes. Settings and family/semaphore ownership do not cover that AF/result continuation. Neither the historical pool-phase explanation nor a cleared busy flag establishes a current completion.

The supplementary unchanged-value control `private-data/verify-candidate/runs/menu-firmware-value-nochange-1/` passes installed open, RIGHT-stays-open, OK confirm, reopen, BACK cancel, handback, unload and stock reopen. It executes begin/apply/confirm/cancel/handback, restores all 226 publications, and ends the lifetime guard without a violation. All 450 settings bytes after opening equal the completed stock-open snapshot; nine full PNG/raw-plane captures equal their explicitly named stock states. The actual swapped-table package control changes settings byte 125 from stock `0x29` to `0x49`, changes the full render, and is rejected on DOWN. These are executed controls, not qualification of changed-value acceptance, cancellation or AF processing. Evidence and exact commands are under `private-data/scratch/MenuFirmwarePackage/installed-nochange-control.json`, `executed-swapped-control.json`, and `lifecycle-closure.json`.

Trace sites may include `reads`, `lr` and `sp` options. Reads run at each entry; `lr` is retained for sites with reads or an explicit LR request, and SP only when requested. The render-enabled checker resolves OSD identities across all phases before individual captures; conflicting identities refuse. Both streaming and inline readers retain the bottom-of-stack guard check.

Use current [engine-only cutover qualification](../reports/owned-menu-engine-design.md#engine-only-cutover-qualification) rather than private increment scripts or historical pair counts. Guest observations require an emulator slot and a frozen input/candidate cohort. Engine-state/settings agreement, renderer ownership and camera acceptance remain separate.
