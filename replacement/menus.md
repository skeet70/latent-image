# Menus and RAM-package workflows

[Replacement index](README.md). The [menu project #128](https://github.com/skeet70/latent-image/issues/128) owns open work. The [owned engine design](../reports/owned-menu-engine-design.md) owns its boundary, shims and current qualification; [controller ownership](../reports/recipe-bank-ui-ownership.md) owns recovered menu behavior. Renderer packaging has its own [render-adapter guide](render_adapter/README.md).

## Bank publication selection

`//replacement/bank:bank` is the standard archive. `:bank-modal`, `:bank-lifecycle` and `:bank-modal-lifecycle` compile the same root with independent scoped selections. The matching `//host/firmware:ram-push-{modal,lifecycle,modal-lifecycle}-relinker` filegroups select `RB_PUBLICATION_GROUPS` at the shared [RAM builder](../host/firmware/build-recipe-bank-ram.sh). The [layout module](../host/firmware/RamPushLayout.hs) owns variant counts and allocation geometry; do not infer a variant from a payload display name.

- Ordinary `--ram-push` selects the standard package.
- `--ram-push-variant modal75`, `lifecycle` or `lifecycle,modal75` selects explicit optional groups in canonical order.
- `modal75,menu-engine` adds the engine; `modal75,menu-engine,render` adds the renderer. The renderer is not engine-only qualification.

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

## Guest package and comparison

For the guest tree, add `menu-engine-tree --package-plan BANK_PLAN_JSON` using the builder's own plan. This composes decoded rows with the installed package's rows. Package children are not decoded nodes; using a stock-only tree for the guest would misclassify them. The host oracle keeps the stock tree.

The engine archive is built with `--cfg rb_modal_scoped --cfg rb_modal75_owned --cfg rb_menu_engine_owned` and `-C llvm-args=-min-jump-table-entries=4096`, in addition to the normal bank flags. The jump-table policy applies to the entire bank crate. The viewer's `--menu-engine-owned` requires both `--ram-push-modal-scoped` and `--modal75-owned-archive`; see [scoped modal observation](../emulation/README.md) for the complete runtime invocation.

The shared relinker uses [the engine linker script](../menu/recipe_bank_ram_engine.ld) and [engine allowlist fragment](../menu/recipe_bank_menu_engine_allowlist_fragment.json). `EngineStack` bounds owned engine plus modal-dispatch depth while retaining a separate stock-callee margin. The [design's stack boundary](../reports/owned-menu-engine-design.md#placement-in-the-guest) records its evidence and exclusions; the estimate is not a whole-task stack proof.

[`emulation/menu-engine-pair-trajectories/`](../emulation/menu-engine-pair-trajectories/) generates pair inputs. `picker.jq` adds reads/sites to picker trajectories; `oracle.jq` maps menu-oracle paths onto packaged landings. `//replacement/modal_runtime:menu-engine-pair-check --package-plan` independently classifies events from the composed tables and observations, checks owned/fallback residency, guard traps, engine reads, posts and stack constraints. `modal-settings-compare` and `render-compare` complete a pair. `RuntimeRelocationCompare` does not admit this package.

Trace sites may include `reads`, `lr` and `sp` options. Reads run at each entry; `lr` is retained for sites with reads or an explicit LR request, and SP only when requested. The render-enabled checker resolves OSD identities across all phases before individual captures; conflicting identities refuse. Both streaming and inline readers retain the bottom-of-stack guard check.

Use current [engine-only cutover qualification](../reports/owned-menu-engine-design.md#engine-only-cutover-qualification) rather than private increment scripts or historical pair counts. Guest observations require an emulator slot and a frozen input/candidate cohort. Engine-state/settings agreement, renderer ownership and camera acceptance remain separate.
