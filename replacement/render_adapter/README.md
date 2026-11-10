# Owned render adapter, R1

`rb_render_owned` composes the picker, flat tree and node-1 pane widgets with the modal-75 and menu-engine bank. It requires `rb_menu_engine_owned` and the composed `RB_MENU_ENGINE_TREE` / `RB_MENU_ENGINE_RECORDS` inputs.

## Entries and ownership

`entry.S` saves r0-r12 and LR in a 56-byte frame, passes the entry registers, original stack arguments and LR to Rust, then restores the registers. Tree/picker entries also pass the caller's report pointer; the pane has no output argument. An owned draw returns to the original LR. A refused draw recreates `push {r4-r11,lr}` and branches to the corresponding stock resume at entry + 4. All Rust entries share a busy flag; nested entries refuse without clearing the outer invocation's flag.

`class.rs` owns the call-site, snapshot-source and effective-list-kind decision. Picker LRs `0x01168ce4` (`ValuePicker`) and `0x0116c588` (`FnPicker`) with a live snapshot, and tree LR `0x01167a60` with a live, effective-flat snapshot are owned. The menu engine checks the current record pointer against the composed record table as well as r0's record id. Package rows are identified by `RowChild::Package`; an unrecognized record is also refused. Record ids alone are insufficient: package row tokens `0x7d` and `0x7e` overlap stock record ids.

Tree classification calls the retained read-only kind getter `0x0061ca2c`. Its only store in `id4.dis[0x61ca2c,0x61cd10)` is the saved r11 stack word. The C reading aid is `app/ui/menus/native_menu_report_destination.c`. The node-0x25 override requires parent `0x27`, selection zero; node 0x38 requires parent `0x228`, selection 11 decimal. The widget still makes its qualified live-snapshot service call at its original point.

`//emulation:render-adapter-class-tests` generates schema-2 JSON with separate `native_model_inputs`, `installation_entries` and `pane_entries` arrays. The 522 native inputs (141 tree and 381 picker) preserve their actual caller return addresses and carry no installation expectation. Explicit installation vectors cover the admitted stock/live caller classes, saved/skipped and high-mode boundaries, effective-flat overrides and refused callers/origins. Native algorithm coverage does not admit installation; secondary-tree and additional picker callers remain uninstalled. Pane contract vectors are unchanged.

`picker.rs` and `tree.rs` implement the widget service traits through A32 `blx` trampolines. The trampolines preserve stacked arguments for the six- and seven-argument stock services. The remainder service adapts the loader's r1 remainder result into an ordinary r0 result. The widgets write the caller's 0x44-byte report, preserve their contract's panel-font save/restore sequence and, for the tree, publish side-state words at `0x015e64d8` and `0x015e64e0`. Picker labels use the installed `0x849` bound.

`FnPicker` passes x=14, y=3 and its report through r6 from caller `fp-0x58`, picker-entry SP+76. The native picker matrix exercises this in-frame report with node `0x63`; `render-picker-fn-return` reads 68 bytes at SP+76. The independent pair classifier admits the same live stock-node class and refuses saved/skipped modes and package/unknown records.

The pane owns node 1 from LR `0x01158184` with modes `{0,1,3,4,6,8,9,10,12,14,255}`, live snapshot depths `{0,1,2}` and node types `{0,1,2,3,9}`. Other modes, types, callers, nodes and package/unknown records fall back before preparation or drawing. `pane.rs` retains `0x01165aa0(node,mode)` and reads its published D/N/R state. Its root-value service returns resource zero for navigation children `0x281` and `0x285`; nested lookups remain unchanged. The independent Haskell classifier reads mode from r1 and derives live depth from the composed tables. Its return capture reads the global report at `0x015e650c` and all seven D words plus three summary words at `0x015e64d8`.

## Reproducing the package

Run from the repository root. Use a fresh output directory under a named `private-data/scratch/` subdirectory. The existing host-only RAM-push output root remains accepted; output admission checks the canonical path. Generated package artifacts are owner-only.

```sh
devenv shell -- replacement/render_adapter/build.sh \
  0x00c40000 private-data/scratch/RenderAdapter/FRESH \
  private-data/scratch/ControllerStrategy/menu-tables.json \
  private-data/scratch/ControllerStrategy/menu-engine/bank-plan.json \
  private-data/scratch/ControllerStrategy/picker-fold/modal75-table-picker.rs
```

The script builds the host tools through `host-build` and the pinned toolchain, generates the composed tree, builds one bank staticlib with `--cfg rb_render_owned`, relinks the explicit `modal75,menu-engine,render` variant and checks its composed allowlist. It then runs `//transport:render-manifest-admission-tests` on that manifest through the same `readRamPushManifest`/`parseRamPushManifest` admission used before installation. `RB_BAZEL_OUTPUT_BASE` selects the Bazel output base; its default is `$HOME/.cache/bazel/latent-image-RenderAdapter`. Stock input paths and hashes are enforced by the existing RAM-push builder. Nothing is installed or run in a guest.

`xm1-viewer` accepts opt-in GHC RTS heap limits. `RENDER_VIEWER_GHCRTS` applies them only to the viewer launched by `run-render-pairs.sh`, leaving admission and comparison tools unchanged. These bound the host viewer, not the native emulator or their combined memory scope.

The producer keeps strict render/menu effect summaries while streaming trace pages. It preserves the recorded rows without retaining discarded hit payloads in the summary.

The frozen recipe admits `candidate-qualified` for the stock role and `qualified-scoped-lifetime-diagnostic` for the owned role. Both require a completed trajectory and installed-lifetime coverage; the owned role also requires verified task identity and its complete publication vector. This is renderer-subsystem evidence, not whole-route stack freedom or hardware qualification.

The render region begins at allocation base + `0x1dd00` and has extent `0xffc0`. `BuildRecipeBank` checks both task-1 engine and task-33 render stack bounds while packaging. Task 33's descriptor declares a `0x1000`-byte stack. The held FujiMod calibration dump has node `0x4006060c`, creation record `0x4005c3ac`, base `0x00a4ce00`, size `0x1000` and top `0x00a4de00`. A cutover observer must follow the descriptor rather than fixing this base.

The manifest keeps the transport parser's 68-entry source-identity bound. The fourteen render-specific source hashes live in canonical `render-source-identity.json`; its SHA-256 is the manifest's `render_sources_sha256` entry. Shared source, archive, toolchain and allowlist identities remain explicit. The builder recomputes those hashes and refuses an existing side file that differs. The admission regression checks the side file's canonical bytes, digest and all fourteen source files, as well as admitting the actual 228-publication manifest. Empty and over-bound identities have distinct parser errors.

The 68-entry emission guard is render-only. Legacy lifecycle 226 and modal-lifecycle 227 manifests retain their complete ungrouped source identities; `//host/firmware:legacy-bank-manifest-tests` checks both emitted files, including every lifecycle source key. This restores the legacy emission that the unconditional guard in `33ffe81d` had refused.

The viewer's `recipe-bank-modal75-menu-engine` payload name is shared with render. The relinker script, allocation, publication count and `BankVariant` are separate payload fields; guard counts and runtime regions derive from the manifest's variant, not that display name.

The pane increment expands only the render variant to 228 publications and 97 activations. Its final logical and activation hooks are modal `0x00bfb028`, engine `0x00bf95c8`, tree `0x01164e1c`, picker `0x01163f2c`, pane `0x011662a0`. Rollback reverses that order. The bank's existing value-call publications `0x0116657c` and `0x011675c4` remain inside the pane body `[0x011662a0,0x011675ec)`: they precede the entry hook on installation and follow its removal on rollback. Stock fallback still executes those patched calls; the owned pane applies their node-1 navigation-child zero-resource rule in its value service.


## Historical two-hook host qualification

The package in `private-data/analysis/x-m1-1.50/ram-push/RenderAdapter/r6/` passed `check-bank-allowlist`: zero violations, 363 declared sites, 1,084 proven stores, 1,491 proven branches and 13,420 instructions in 384 functions. Its static owned stack depths are tree 540 and picker 620 bytes, including their entry frames and owned service trampolines. With the retained-stock margin of 1,024 bytes, these fit task-33 headrooms of 3,416 and 3,720 bytes respectively. The task-1 engine remains 904 + 164 bytes against limit 1,656.

| Artifact | SHA-256 |
| --- | --- |
| `runtime.bin` | `ca967bfcfea3b2b3a1903956a83cddfdec2c8aff14066b1d83f25d966f719d0e` |
| `kernel.elf` | `8bd101040a5c1c4c606d4086472bfc3c0a24bc9cc94d5261242c4abd8ec348fa` |
| `ram-push-manifest.json` | `27e68e9139c3acc85db18b9a6f98327181d272c39bd157ed9e593aef67c52bbf` |

The manifest has 227 publications. Render activation 225 at `0x01164e1c` changes `0xe92d4ff0` to `0xeaebe4ac`; activation 226 at `0x01163f2c` changes the same old word to `0xeaebe7a9`.

These host tests passed on the composed sources:

```sh
devenv shell -- bazel --output_base=$HOME/.cache/bazel/latent-image-RenderAdapter test --jobs=2 \
  //emulation:render-adapter-class-tests \
  //emulation:tree-list-cases-tests //emulation:value-picker-cases-tests \
  //replacement/menu_widgets:tree-list-tests //replacement/menu_widgets:value-picker-tests \
  //host/firmware:render-stack-tests //host/firmware:engine-stack-tests \
  //host/firmware:bank-allowlist-composition-tests //host/firmware:build-recipe-bank-ram-tests
```

With the Fn extension, the class test checks 164 recorded native model entries and 50 refusal/boundary inputs against the production decision. Stock and owned picker native runs each pass 101 cases, including 46 Fn cases; all five mutations reject Fn cases. The independent Haskell render-pair classifier tests pass. The widget native qualifications are recorded in `reports/display-render-contracts/tree-list.md` and `value-picker.md`. The held calibration descriptor is a source-inspected stack extent, not a measured render high-water mark. The R1 cutover pair, residency and guest high-water checks remain outside this host-only composition.

## Three-hook package qualification

`private-data/analysis/x-m1-1.50/ram-push/PaneSeam/r4/` contains the pane plus Fn-picker archive. The real 228-publication manifest passes transport admission with 67 top-level identities and fourteen canonical render-source hashes. The composed allowlist passes with zero violations and zero unused declarations, 364 declared sites, 1,257 proven stores, 1,626 proven branches and 14,582 instructions in 402 functions.

| Artifact | SHA-256 |
| --- | --- |
| `bank-modal75-engine-render.a` | `30c7f3723c7b5018dc0d9a9b72d05864fbe12fe63a495cb76d447e29cffa6dd5` |
| `runtime.bin` | `a0f46667bc07dc55f3f357d72d0d1f85c4908960a157c4e355ff395f9ed55547` |
| `kernel.elf` | `2c046cffa47d74cf715afcac979e37e0384647b4dad99e4868d510aeaa6b334c` |
| `ram-push-manifest.json` | `aa6fdea81fb84578930b0df91e433dc682f722d183bace32577f257632368e1f` |
| `render-source-identity.json` | `f8a3019e0789b2b7f634c8591b2fadc1d5975aafb5ae8246903da6e2cf58fa6f` |

Owned stack depths are tree 556, picker 636 and pane 452 bytes. Static admission retains the 1,024-byte stock estimate and headrooms of 3,416, 3,720 and 3,904 bytes respectively. The pane headroom subtracts 96 stock caller bytes and the ELF-derived 96-byte bank-root prefix from task 33's 4,096 bytes. `EngineStack` traverses the bank root's bounded, data-mapped seven-entry branch table; unmapped, unbounded and out-of-function table fixtures refuse. The imported estimate is not a bound on retained draw-service demand.

Finite guest qualification uses the [revised R1 criterion](../../reports/display-render-increment-r1.md#prerequisite-2-uilib-stack-admission): complete introduced-invocation architectural minima against the validated owner allocation and existing bottom-64-byte guard, plus separate whole-lifetime descriptor/backlink and attempted-write evidence. The original `owned ELF + 1024` estimate failures remain reported, including the classifier's 15 historical overruns. Changed-word marks remain lower bounds. This does not establish unobserved native paths or whole-task overflow freedom.

The class agreement tests pass the 164 tree/picker model inputs, 50 tree/picker boundaries and 185 pane inputs (165 covered mode/depth/type combinations and 20 refusal boundaries). The independent Haskell pane classifier, entry/stock/return declarations, adapter residency, global D/N/R capture and stack checks pass `//replacement/modal_runtime:render-pair-tests`. The publication-order, stack, manifest transport, Haskell lifetime and opt-in trajectory tests pass. Both in-body bank value sites precede pane activation and follow its reverse removal. No guest run was performed.

`//emulation:fujimod-xm1-tests` and `//emulation:xm1-viewer-tests` also pass on the combined sources. The Rust bridge suite exercises all three render hook words, forward/reverse writes and rollback readback, including corruption at both in-body pane value sites.

The emitted-manifest regression oracle also passes on the rebuilt lifecycle 226 and modal-lifecycle 227 packages, retaining 70 and 74 ungrouped source identities respectively.

## Pane integration legacy proof

The five legacy variants were rebuilt at `0x00c40000` against pre-regression baseline `920b9b52` (`33ffe81d^`), using the same retained archives from `private-data/scratch/ControllerStrategy/menu-engine/i8-types/after/` in each role. All 20 `cmp` comparisons passed: `runtime.bin`, `kernel.elf`, canonical `.publications`, and canonical manifest fields after deleting only `.source_identity`, for standard 223, modal 224, engine 225, lifecycle 226 and modal-lifecycle 227. The before/after outputs are in `private-data/analysis/x-m1-1.50/ram-push/PaneSeam/legacy/`.

| Variant | Identical runtime SHA-256 |
| --- | --- |
| standard 223 | `fc86437e819ae1d0061f904e91234c0e0b5b1ccc94b35d847b4f486c86fd379d` |
| modal 224 | `4fa42b765f2f9375b51b36f0f227e19ad2371e9f6d3b6df359a1f6d8974d21d9` |
| engine 225 | `a2ef17e99f041ed82edb4edbb1657e6c085f0f67a03aeb472f7824357c6cef5a` |
| lifecycle 226 | `5bf54e1e31319573df6c4bfbcbf79b41c96119465dc56f63fe8a83ce9c988033` |
| modal-lifecycle 227 | `af3c76566f06434f4ce1a6d23deee24fcec956d636015860ed6709ed695e3a45` |
