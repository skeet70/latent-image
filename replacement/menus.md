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

The host tests commit every offered IMAGE SIZE token's size/aspect pair and the ordinary rule's stored code through a fake settings service, and refuse an unoffered token or a property without a table without writing. Conversion selects the first matching row and then rejects the `0x7ffffffe` sentinel; a later duplicate key cannot turn a failed conversion into success. The generator test checks the decoded tables, pairs and refusals through `generateTables`, not emitted source text.

Generated conversion rows, conversion registry, image-size pairs and the `#[used]` table root have typed static storage in `.menu_engine.rodata.value_tables`; identical conversion rows share one array. Mutable engine/value state and the C bank BSS stay in the writable bank-data region. The relinker renames only owned engine/value text and named read-only sections, not anonymous constants or archive-wide `.data`/`.bss`. `kernel.map` and `kernel-why-extract.tsv` record the linked demand and archive extraction chain. Garbage collection retains the declared publication entries through their linker roots and references.

The first guest seam is property-scoped at the stock dispatcher's jump-table word for `0xf6` (`0x00d50084`, stock `0x00d51b58`). With `--cfg rb_menu_value_owned` and `RB_MENU_VALUE_TABLE` naming the generated file, the word points at `rb_menu_value_size_arm`, which runs the owned transaction for every `0xf6` caller and returns to the stock arm only when the target refuses, before any effect. Its continuations `__rb_menu_value_stock_arm` and `__rb_menu_value_shared_tail` are bound in `menu/recipe_bank_ram_engine.ld`. The seam applies the stock size/aspect/third-cell comparison before the retained `0x1e5` marker call and skips the current-property marker and display update when the live mode is `0x2c`. Every other property keeps its stock word. Only the value package publishes the word; the standard and engine-only packages keep it stock.

The IMAGE SIZE seam is stateless: a selection resolves its row token and commits the pair, and no owned state survives the event. Pop and Close keep stock's `0xd5b350` notification, including the `0x637ed0` snapshot and the `0x61c90c` settings-bank copy. IMAGE QUALITY and other composites retain stock lifecycle until their complete affected-property sets are owned; the address-independent ordinary-value API is not package ownership of those properties.

## Guest package and comparison

For the guest tree, add `menu-engine-tree --package-plan BANK_PLAN_JSON` using the builder's own plan. This composes decoded rows with the installed package's rows. Package children are not decoded nodes; using a stock-only tree for the guest would misclassify them. The host oracle keeps the stock tree.

The engine archive is built with `--cfg rb_modal_scoped --cfg rb_modal75_owned --cfg rb_menu_engine_owned` and `-C llvm-args=-min-jump-table-entries=4096`, in addition to the normal bank flags. The jump-table policy applies to the entire bank crate. The viewer's `--menu-engine-owned` requires both `--ram-push-modal-scoped` and `--modal75-owned-archive`; see [scoped modal observation](../emulation/README.md) for the complete runtime invocation.

The shared relinker uses [the engine linker script](../menu/recipe_bank_ram_engine.ld) and [engine allowlist fragment](../menu/recipe_bank_menu_engine_allowlist_fragment.json). `EngineStack` bounds owned engine plus modal-dispatch depth while retaining a separate stock-callee margin. The [design's stack boundary](../reports/owned-menu-engine-design.md#placement-in-the-guest) records its evidence and exclusions; the estimate is not a whole-task stack proof.

[`emulation/menu-engine-pair-trajectories/`](../emulation/menu-engine-pair-trajectories/) generates pair inputs. `picker.jq` adds reads/sites to picker trajectories; `oracle.jq` maps menu-oracle paths onto packaged landings. `//replacement/modal_runtime:menu-engine-pair-check --package-plan` independently classifies events from the composed tables and observations, checks owned/fallback residency, guard traps, engine reads, posts and stack constraints. `modal-settings-compare` and `render-compare` complete a pair. `RuntimeRelocationCompare` does not admit this package.

The bounded menu qualifier runs the value package as its owned image for every route ([cutover qualification](../reports/owned-menu-engine-design.md#cutover-qualification)). `value.jq` adds stock's IMAGE SIZE arm `0x00d51b58` and stock's menu notification `0x00d5b350` to every route's trace sites and, in the owned role, `runtime:rb_menu_value_size_apply`; with `--arg route close-pop` it renders the route `value-close-pop` from the picker-260 trajectory: OK opens the IMAGE SIZE picker, a half-press closes the menu from inside it (stage Close `(2,1,0x1d)`, notified with class `0xf6`), OK reopens SHOOTING MENU, and picker 260 is moved DOWN and left with BACK, whose Pop runs stock's restore, before a reopen reads the restored row. The value package's pair check requires, in each resident segment, one owned apply for each entry of stock's arm in the same stock segment and no entry of the arm itself, and, in every segment, the same `0xd5b350` calls (kind, landed node, event id, class) in call order in both roles and equal `live-settings` reads at the segment's end, or none in either role. The qualifier passes `--value-arm-required` for `value-close-pop`, which also requires a resident arm entry; the passing line prints the route's resident arm entries, notifications and live-settings segments.

Opening IMAGE SIZE applies its landing token and changes live settings in both roles (E-MENU-VALUE-PACKAGE-150). A changed selection is gated by `run-value-change.sh` and `//replacement/modal_runtime:menu-value-change-check` ([IMAGE SIZE changed selection](../reports/owned-menu-engine-design.md#image-size-changed-selection), E-MENU-VALUE-CHANGED-SELECTION-150): under the qualifier's run document both roles submit the same `0x4010` and stay pending; under the labelled schedule `output-clock-200000` completion before Close, before OK confirmation and before a restore elsewhere is handled identically; and with the completion held until a route step releases it, a half-press, OK or BACK pressed while pending and then the completion are handled identically. Stock completes only with status 0, so there is no failure input; cancellation by a later `0x4010` or pipeline init has no natural input on this route.

The value package links 226 publications in the `0x3dd00` allocation at `0xa81be0`, the engine package's size, with the engine, value and bank C code in the `0x20000` region from `0x1dd00`. It carries an explicit typed variant through manifest, transport state and lifetime-guard requests; untagged 226-publication requests retain the legacy lifecycle interpretation and cannot admit it. The canonical value source-identity bundle binds all three source files within the unchanged 68-entry bound. Controller-75 admission resolves the bank's published LEFT/RIGHT interposers to their stock identities while preserving the original fallback target. The value retired document adds the arm's first word `[0xd51b58, 0xd51b5c)` to the lifetime guard while the package is resident.

Trace sites may include `reads`, `lr` and `sp` options. Reads run at each entry; `lr` is retained for sites with reads or an explicit LR request, and SP only when requested. The render-enabled checker resolves OSD identities across all phases before individual captures; conflicting identities refuse. Both streaming and inline readers retain the bottom-of-stack guard check.

Use current [cutover qualification](../reports/owned-menu-engine-design.md#cutover-qualification) rather than private increment scripts or historical pair counts. Guest observations require an emulator slot and a frozen input/candidate cohort. Engine-state/settings agreement, renderer ownership and camera acceptance remain separate.
