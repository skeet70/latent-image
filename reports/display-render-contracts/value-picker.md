# R1 value picker contract

## Source and ownership

Pinned source: x70view `a72c95a3572562fc05f359b4f35915f3a0243b19`, `overlay_draw_choice_list` at `0x01163f2c`, extent 3,824 bytes. The C is NONMATCHING and is a reading aid only. Addresses below refer to the listing of `private-data/analysis/x-m1-1.50/decode/img6.dis`, confirmed against instructions, not inferred from symbol names.

R1 owns the live-snapshot, non-package-node classes called at `0x01168ce0` (`ValuePicker`) and `0x0116c584` (`FnPicker`), as defined in [R1 section 3](../display-render-increment-r1.md#3-draw-class-and-the-owned-set). Saved snapshots and package nodes remain stock.

## ABI and entry sequence

Entry is A32, `push {r4-r11,lr}; add r11,sp,#32; sub sp,sp,#308` (`1163f2c-1163f34`). Denote entry stack words by `a4..a10`, register arguments by `node,x,y,width`. After the push, `fp = entry_sp-4`, so `fp+4` is `a4`. The owned call site supplies `y=4,width=0,a4=0,a5=2,a6=0,a7=1,a8=1`; `x`, node and mode vary (`1168ca8-1168ce0`). `a8` is truncated to a byte (`1163f48`). `a9` is mode and `a10` the report pointer.

### Fn picker caller

The NONMATCHING `FUN_0116c4f0` reading aid at the pinned revision names the call with `(node,14,3,0,0,2,0,1,1,mode,report)`. The instruction contract is `img6.dis[0x116c4f0,0x116c594)`: the caller pushes 24 bytes, sets `fp=entry_sp-4`, reserves 144 bytes, saves node in r7 and mode in r5, and sets r6 to `fp-0x58` (`116c504`). It clears 68 bytes there (`116c508-116c514`). At `116c550-116c580` it supplies r0..r3 = r7,14,3,0 and stack words 0,2,0,1,1,r5,r6. The call at `116c584` returns to `116c588`. Thus the report is at picker-entry SP+76, not `fp-0x68`.

The caller reads report byte `+0x3e` with `ldrb r6,[fp,#-0x1a]` at `116c588`, then computes `3+2*byte`. At `116caf8` it reads report byte `+0x3c` with `ldrb r4,[fp,#-0x1c]` and adds twice that ordinal. At `116cb3c` and `116cb48` it reads report byte `+0x3d` from `[fp,#-0x1b]`; modes 7 and 13 reuse the selected-row position, other modes use the current-row ordinal, and `0xff` suppresses the current marker. The stock probe observed node `0x63` at this site. These caller facts are confirmed in bytes, not taken from the draft's frame-variable names.

Native Fn cases use x=14, y=3, node `0x63`, scroll layout, and a report at entry SP+76 inside the mapped caller frame. The existing row, cursor, state, hook, mode, detail and label-bound cases are reused with contract-derived coordinates and report bytes. Only the callee's private frame is ignored; the caller report and neighboring bytes remain checked.

Zero local records, including the 64-byte row-hook record via `0x43550(record,0,64)`, then read and retain the panel-font word at `0x015e46c0` (`1163f54-1163fe4`). Modes 2 and 3 return without snapshot, report initialization or font restoration service (`1163fec-1163ff4`, `1164e14-1164e18`). Other modes at most 13 with `(1 << mode) & 0x2280 != 0` call `0x61ce1c(node,2,&snapshot)`; all others call `0x61cd14(&snapshot)` (`1163ff8-1164028`). Unlike the tree, mask `0x1860` does not select saved slot 1.

## Service records and report

Snapshot is eleven words at `fp-0x50`: `s0..s10`. The summary is six words at `fp-0x68`: `t0..t5`. The child descriptor is three words at `fp-0x74`: id, state, auxiliary. These records are zeroed before the services fill them (`1163f58-1163fa4`). Call `0xbf8f4c(node,s3,&summary)` (`1164080-116408c`). Normalize `t2=0` to 1 (`1164090-116409c`).

For nonnull report, call `0x7221c8(report,0,0x44)` and write the following fields (`1164034-1164074`, `11640a0-11640b4`):

| Offset | Value |
| --- | --- |
| `+0x00` | `s0`, without a kind-based branch |
| `+0x04` | node |
| `+0x20` | `s3` |
| `+0x28` | `s8` |
| `+0x2c` | normalized `t2`, except node `0x4c` writes 3 |
| `+0x30` | `s10` |
| `+0x38` | `s5` |
| byte `+0x3d` | `0xff` initially |

When `t2 > s8` and node is not `0x4c`, report byte `+0x3e = ((t2-s8)>>1) & 0xff`; increment y by `a5 * byte(+0x3e)` and truncate to u16 (`1164174-11641a8`). This path dereferences the report without a null check.

A drawn child whose id equals `s5` writes byte `+0x3d`; the selected child whose id equals `s4` writes `+0x34=id,+0x40=child.auxiliary` and byte `+0x3c` (`1164768-11647f8`). In scroll mode (`a7!=0`), both bytes are `(raw_ordinal - s10 - hidden_count) & 0xff`. In paged mode they are `(raw_ordinal-hidden_count-t2*(s9-1)) & 0xff`. All unassigned report bytes remain zero.

## Width prepass and label bounds

Truncate x and y to u16. For snapshot kind `s2==4`, enumerate every raw child with `0xbf90f0(node,s3,index,&child)`; skip state 3, stop if `t2==0`; ask `0xbf93e0(node,s3,id,1)` for a label, and accumulate the width service return (`11640c0-116415c`). Labels at most the installed gate use `0x1112a6c(label)`, others `0x11391a0(label)`. The prepass gate at `11640f0` is stock `0x83d`; the recipe bank changes this one instruction to `0x849`. Cases must distinguish both installations and labels at bound, bound+1, `0x83e` and `0x849`. The draw-loop gate at `1164308` is independently stock `0x83d`; do not assume the prepass patch changes it. Center x as `u16(17-((total_width+2)>>1))` (`1164160-1164170`). Other snapshot kinds keep the supplied x.

## Main loop and ordering

Loop raw indices from 0 to `t0-1`, with visible count initially 0, hidden count 0, page count 0, byte ordinal `u8(-scroll_top)`, and `scroll_top = s10` only when `a7!=0` (`11641ac-1164260`). For each raw row call `0xbf90f0(node,s3,index,&child)` first. State 3 increments hidden count and skips the row. Check visible count against `t2` before decrementing nonzero scroll_top. In paged mode call `0x4d978(page_count+t2,t2)` and compare the return against `s9`: skip while below the requested page (increment page_count), stop when above (`1164270-11642ec`). The divisor is inherited in r1 from `116429c`; it is not a one-argument division contract.

For a drawn row call `0xbf93e0(node,s3,id,1)` and then the independent draw-loop width gate (`11642f0-1164324`). Construct the hook record with node, `s3`, child id, a 20-byte copy of `0x015e64d8`, signed x/y, selected flag `(id==s4)`, and label (`1164328-116438c`). Construct the resource record with label, u16 `(x-1)*20`, u16 `(y-1)*24`, u16 `(width-1)*20`, u16 `(a5-1)*24`, `a6` and zero flags (`1164390-11643ec`). Call font service `0x1112b2c(12)` (`11643f0-11643f4`).

For `s2!=4`, choose palette `0x69` if selected, otherwise `0x6b` for state 2 and `0x6a` for other states. Call `0x1137cf8(palette,255,0,0)`, then `0x116f948(&hook_record)`. A zero hook return calls `0x1112e78(&resource_record)`, any nonzero return skips that leaf (`1164600-116465c`). This is a nonzero test, not equality with 1.

For `s2==4`, unselected rows repeat font 12, use palette `0x6b` for state 2 or palette 0 otherwise, and draw the resource without invoking the hook (`1164584-11645fc`). Selected rows perform the bracket/background sequence in `1164414-1164580`: font 12, `0x1112e38()`, palette lookup `0x1138a00(2)`, palette `(7,255,u8(return),255)`, resource leaf, grid resources `0xff00035f` and `0xff000360` around its width, `0x1112e58()`, `0x1130e68(0)`, font 15, then table-indexed left/middle/right resources from `0x0110ba2c + result*12`. This adds 2 to the row width used by `a4==255` layout.

Selected rows in modes 7 and 13 perform a second highlight sequence before report selection updates: `0x1177e10(0x01113bf4,1,0)`, font 13, palette `(0x6a,255,0,0)`, hook, and only for zero hook another font/palette/resource leaf; fill rectangle with `(sx-20,sy,extent,sy+48,15,124,255)` then `0x1177e10(0x01113bf4,0,0)` (`1164694-1164764`). Extent starts at 640; nodes `0x21..0x23`, `0x28`, `0x4c`, `0x83` change it to 600 (`1164660-1164690`).

After report selection updates, unselected rows skip selected-value details. Selected kind 4 calls the hook at `116480c`, and nonzero skips all remaining details (`11647ac-1164814`). This is its first hook call except when the mode-7/13 highlight has called it already. Otherwise labels 0,2,3 are requested in that order through `0xbf93e0`; details and their layout are the inline sequence `1164818-1164b98`, not deferred drawing. This sequence uses `0x111258c` output byte 1 for width, `0x111299c` output bytes for metrics, `0xbf96b0()` to choose palette 99 for return 5 or 0 otherwise, fills and grid markers `0xff000361..0xff000363`.

Selected kind 1 requests label 2, draws it when nonzero, requests label 3 and reads setting `0x44` through `0x61c93c(0x44,0)` only when that label is nonzero. Zero setting permits its leaf; node 6 overrides resource x to 140 (`1164b9c-1164c74`). Other selected kinds request label 3, optionally draw it with font 15/palette 19 and format enable/disable; then request label 2 and, when nonzero, read the same setting. Zero setting permits the resource, metrics and `0x1139a68(5,2,u8(metric0-1),25,20,18,1)`. Kind 9 subsequently calls `0x1159168(node,s3,id)` whether or not label 2 was drawn (`1164c78-1164da8`).

After each drawn row increment page_count and visible count; advance y by `a5`, x by `a4` (or measured label width when `a4==255`), both truncated to u16. After every raw row, including hidden and skipped rows, increment raw index and byte ordinal (`1164dac-1164e08`). Restore the saved panel font through `0x1112b2c(saved)` on normal completion, including zero children (`1164e0c-1164e10`).

### Selected kind-4 detail arguments

Let `L0,L2,L3` be the three returned labels, `start=u16(y+2)`, `end=start`, and `help_start` initially zero for the draw. Every detail resource has zero width/height, style and flags. Its trailing words stay zero. `draw_detail(label,x,y)` means font 15 (`0x1112b2c`), grid palette 10 (`0x1137d2c`), resource leaf (`0x1112e78`), in that order.

1. When `L0!=0`, `draw_detail(L0,L2!=0 ? 120 : 280,u16((signed_y+1)*24))`; call `0x111258c(L0,&size)`; set `end=u16(start+size[1])` (`116487c-1164908`).
2. When `L2!=0`, `draw_detail(L2,0xffec,0xffec)`; only when `L0==0`, call `0x111258c(L2,&size)` and increment end by size byte 1, truncating to u16 (`1164914-1164980`).
3. When `a8==0 && L3!=0`, `draw_detail(L3,0xffec,0xffec)`; call `0x111299c(L3,0,&extent)`, then `0x111258c(L3,&size)`; set help_start to old end and end to `u16(end+size[1])` (`1164984-1164a08`).
4. Call `0xbf96b0()` and choose palette `p=99` for return 5, otherwise 0. Left/right grid bounds `(l,r)` are `(5,28)` for nodes `0x10` and `0x30`, otherwise `(6,29)` (`11641fc-116423c`, `1164a0c-1164a18`).
5. Call palette `(p,204,0,0)`. When `a8==0 && L3!=0`, call `0x1113514(l,s8(start),r,s8(help_start-1),17,p,204)`, palette `(p,153,0,0)`, then `0x1113514(l,s8(help_start),r,s8(end-1),17,p,153)`. Otherwise call only `0x1113514(l,s8(start),r,s8(end-1),17,p,204)` (`1164a1c-1164b30`).
6. Font 17, then `0x1138060(l+1,r-1,s8(end),1,1,0xff000361)`, `0x111236c(l,s8(end),0xff000362)`, `0x111236c(r,s8(end),0xff000363)` (`1164b34-1164b94`).

Here `s8(v)` is signed low-byte extension. The size out-record is two bytes at `fp-0x9c`; the extent scratch is two bytes at `fp-0x76`. Size starts zero. The extent is read after its service fills byte 0 (`1163fc8`, `11648ec-11648f4`, `11649dc-11649f4`, `1164d54-1164d5c`). Service `0x111258c` writes size bytes 0 and 1 (`11125c0`, `11125f8`); `0x111299c` with argument 1 null writes only byte 0 through argument 2 (`11129ec-1112a0c`). Stub outputs must supply every byte the following instructions read.

### Record layouts at service calls

The 64-byte hook record is `{node,parent,child,side[5],signed_x,signed_y,word10,selected,label,reserved[3]}` in 32-bit words. It is cleared once on entry; row setup assigns the listed fields except word10 and reserved, whose values can persist if a hook writes them (`1163fcc-1163fd4`, `1164328-116438c`). The 32-byte resource record is `{label:u32,x:u16,y:u16,width:u16,height:u16,word3:u32,word4:u32,style:u32,flags:u32,word7:u32}` (`1164390-11643ec`). Main-row setup writes label/geometry/style/flags; the other words start zero. Service events that observe these pointers compare bytes, not stack-address identity.


## Boundary inventory and side effects

The 30 distinct direct call targets are `0x43550`, `0x4d978`, `0x61c93c`, `0x61cd14`, `0x61ce1c`, `0x7221c8`, `0xbf8f4c`, `0xbf90f0`, `0xbf93e0`, `0xbf96b0`, `0x111236c`, `0x111258c`, `0x111299c`, `0x1112a6c`, `0x1112b2c`, `0x1112e38`, `0x1112e58`, `0x1112e78`, `0x11130e0`, `0x1113514`, `0x1130e68`, `0x1137cf8`, `0x1137d2c`, `0x1138060`, `0x1138a00`, `0x11391a0`, `0x1139a68`, `0x1159168`, `0x116f948`, `0x1177e10`. Call-site addresses above delimit their argument preparation. There are no indirect calls in this extent.

The picker copies the 20-byte side-state block into the hook record; it does not directly write `0x015e64d8`. Stock services may change global palette/font/resource state. Cases must distinguish loop writes from stub service writes and check report bytes, side-state preservation and final panel-font restoration independently of the event sequence.

## Qualification status

Host-qualified on 2026-11-05: stock 55/55 and owned 55/55 cases passed. The expectations were generated from this contract before execution, with the same report, ordered service events and exact non-scratch memory for both roles. `Emulation.ValuePickerCases` defines the 55 cases, `Emulation.RenderCases` the typed service scripts, and `ValuePickerCases.hs` maps either stock bytes or the owned A32 entry. The matrix includes live owned-caller classes plus broader ABI controls for saved modes, skipped modes and paged layout. Both stock `0x83d` and bank `0x849` prepass gates are covered independently of the unpatched main-loop gate.

`value-picker-tests` passed all five behavioral tests after implementation commit `5c6456b5`. Native runner dependency `9be24994` adds scripted out-writes, pointed record contracts and the stock UBFX form at `1164190`; no picker-specific service execution occurs outside declared stubs. Candidate execution needed no additional ISA admission.

The five mutation batches each contained the same 55 cases, using the mutated stock-bound candidate for StockGate cases and the passing normal bank-bound candidate for BankGate cases:

| Mutation | Rejected cases | Representative rejecting case |
| --- | ---: | --- |
| selected row unstyled | 34 | `picker-rows-1-cursor-1-candidate` |
| scroll top ignored | 3 | `picker-scroll-1-candidate` |
| hidden row drawn | 2 | `picker-state-hidden-hook-drawleaf-candidate` |
| disabled row styled as offered | 8 | `picker-state-disabled-hook-drawleaf-candidate` |
| report byte `+0x3e` incremented | 3 | `picker-centered-short-list-candidate` |

The first four representative cases stop with `stub_contract` at the differing ordered service boundary. The report control returns normally and fails exact report memory. Each mutation batch exits 1; the unmodified candidate batch exits 0. No guest pair, device render, camera operation, package-node takeover, RAM-push adapter or entry seam was run or added.

Receipts are under `private-data/analysis/x-m1-1.50/native-case/`. The following SHA-256 values bind the retained batch summaries:

| Path relative to that directory | SHA-256 |
| --- | --- |
| `value-picker/results/batch-summary.json` | `9b836ba37bd9545e8b935eb7b1287c80971ab27b39388e5a3dd2c949ab4664a6` |
| `value-picker-owned/results/batch-summary.json` | `6ca75a3f13166f19a6a96bad15e5a1fbbfc2148abd48845c9507d33093d16fd7` |
| `value-picker-control-selected-unstyled/results/batch-summary.json` | `25cfb6723f1933ba82ba8b2b607238fb2811eb8ac7d9d2820c5567852efe583b` |
| `value-picker-control-scroll-ignored/results/batch-summary.json` | `ec2d1a5f6fe8c79ee02b94147a47151e4b0ce25ce32f3d6bb4df49b2bda4b7b8` |
| `value-picker-control-hidden-drawn/results/batch-summary.json` | `f130317b3f64d1998c962ecac3ad2ebed4dfcd29680eab4af546c9864f0e57f9` |
| `value-picker-control-disabled-offered/results/batch-summary.json` | `61ec986054e5b33a66cf4922fcc5da2132d3361d78919a8adeec73988b12ffc3` |
| `value-picker-control-report-offset-wrong/results/batch-summary.json` | `954e2d4e0f5626b5c86fc4ea7d0465869036c8ed27d0ec81d3b67f39b9188f5f` |

The owned flat images have SHA-256 `ca5467129c9fd04e85435540afa83a4227f7e44d9ec73f663f82624490e05b50` (stock bound) and `bcdf70326b307fc8cac73a6b8f9afe01ff5a194e96c0ad964cb49bf21171b028` (bank bound). `replacement/menu_widgets/cases/value_picker.rs` is the qualification-only A32 service implementation; `src/value_picker.rs` is the owned loop. Linker garbage collection removes unreachable compiler builtins.

Reproduction, from a worktree with the qualified generator and runner built:

```sh
devenv shell -- bazel --output_base=$HOME/.cache/bazel/latent-image-render-picker-r1 test --jobs=2 //replacement/menu_widgets:value-picker-tests
devenv shell -- bazel --output_base=$HOME/.cache/bazel/latent-image-render-picker-r1 build --jobs=2 //replacement/menu_widgets:value-picker-case-image //replacement/menu_widgets:value-picker-bank-case-image //emulation:value-picker-cases //emulation:native-case //emulation:native-runner
devenv shell -- bazel-bin/emulation/value-picker-cases --candidate "$PWD/bazel-bin/replacement/menu_widgets/value-picker-case-image.bin" --candidate-bank "$PWD/bazel-bin/replacement/menu_widgets/value-picker-bank-case-image.bin" --out "$PWD/private-data/analysis/x-m1-1.50/native-case/value-picker-owned"
devenv shell -- bazel-bin/emulation/native-case --runner "$PWD/bazel-bin/emulation/native-runner" --batch "$PWD/private-data/analysis/x-m1-1.50/native-case/value-picker-owned/batch.json" --output "$PWD/private-data/analysis/x-m1-1.50/native-case/value-picker-owned/results"
```

The recorded native runs used the qualified `value-picker-cases`, `native-case` and `native-runner` executables in `/var/home/murph/Code/latent-image-worktrees/PickerCases/bazel-bin/emulation/`, invoked through `devenv shell` from the picker implementation worktree. The implementation worktree's combined build reached and passed the widget tests and built all seven images, then exceeded its initial 300-second command deadline while compiling native-runner Nix dependencies. The standalone native prerequisite build was resumed with a longer deadline, then cancelled after all native runs completed through the qualified PickerCases tools. A completed native-runner build in the implementation worktree is not claimed.

### Fn call-site extension qualification

At implementation `0f82c0e0`, the matrix has 101 cases: the original 55 and 46 Fn-site cases. Stock and owned each pass all 101, including all 46 Fn cases with the report at picker-entry SP+76. The cases use the same service scripts and report oracle in both roles. The generator's contract tests, production Rust class-agreement tests (164 native model entries and 50 boundary entries), and independent Haskell `render-pair-tests` pass.

Receipts are under `private-data/analysis/x-m1-1.50/native-case/picker-fn-r1-ROLE/results/batch-summary.json`:

| Role | Rejected Fn cases | Summary SHA-256 |
| --- | ---: | --- |
| `stock` | 0 | `c75055febf64128e019a1840eaf4352e0a1fc7e9fb5494e655f0c0fc32d3d05f` |
| `owned` | 0 | `3eda150abff21959a99ec859abe0286cb1d0850c11099185f25279c137cf237c` |
| `control-selected-unstyled` | 31 | `935683d22653162cd0e6656f0712ce01e257ad7bff88b538d4044ded3542eba7` |
| `control-scroll-ignored` | 3 | `f1190f17cf615f759b7077ab1ba3b8b13f7a394113ac11d5c69367a9337a316b` |
| `control-hidden-drawn` | 2 | `d52a8640c8571edc7be154f4afcbc51f778e3c62a7571f18ccf1e6c1a6400077` |
| `control-disabled-offered` | 8 | `40eacf11dc1e7e247c8311e57f108e7a41175607f43619b22db113758f97ac47` |
| `control-report-offset-wrong` | 4 | `f187ac7b990f19bdc3f2fa1d963b954a93c74aa992c864d3c4159a1a0659ab10` |

Build the targets in the reproduction block above, plus the five `value-picker-control-NAME-image` targets. Emit stock cases with `value-picker-cases --out OUTPUT`; emit owned cases with both `--candidate value-picker-case-image.bin` and `--candidate-bank value-picker-bank-case-image.bin`. For each control, replace only `--candidate` with its control image. Run each emitted batch through `native-case --runner native-runner --batch OUTPUT/batch.json --output OUTPUT/results`. These runs used this worktree's Bazel-built tools and output base `$HOME/.cache/bazel/latent-image-PickerSiteR1`, with `--jobs=2`. No guest or camera run was performed.

