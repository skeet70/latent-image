# Generic root and pane draw contract

## Evidence and scope

Pinned source is x70view `a72c95a3572562fc05f359b4f35915f3a0243b19`, read with `//build-support:xm1-decomp -- lookup|show ADDR --rev REV`. Exactness follows [the decomp rules](../xm1-decomp.md). `FUN_0115815c` is exact. `FUN_01166200`, `FUN_011662a0`, and snapshot preparation `FUN_01165aa0` are NONMATCHING; their facts below are confirmed in `private-data/analysis/x-m1-1.50/decode/img6.dis` at the cited instruction addresses. The complete pane BL-site inventory is included below, not inferred from the draft's 909-instruction comment.

The contract was derived by static source inspection before the [host qualification](#host-qualification) below. The static route diagnosis is `private-data/scratch/ControllerStrategy/render-pairs/static-coverage/draw-coverage-diagnosis.json`. Its historical receipts have zero old R1 draw-entry hits; the node-1 callback path below explains that observation statically, not by a newly observed guest trace.

| Function | Stock extent | Bytes | Stock SHA256 | Exactness |
| --- | --- | ---: | --- | --- |
| Root wrapper `0115815c` | `[0115815c,01158188)` | 44 | `683613907021a4c9c82a4fdffb87aa84dbc1f7993d565e7d15827b3999d6f93d` | exact, `tu_0115609c_exact.c:113` |
| Preparation dispatcher `01166200` | `[01166200,011662a0)` | 160 | `e1675855b124c76515d7d99a101055bd31ecdac149a58182b7b9274e608be4b0` | NONMATCHING, `tu_01163b08.c:1358` |
| Pane `011662a0` | `[011662a0,011675ec)` | 4,940 | `f2358629ce63cc2162be94cfcce4f5e2eb5ac1e683a51ed433b2c13b2a48ad75` | NONMATCHING, `tu_01163b08.c:1385` |

Addresses in tables and inventories omit leading zeroes. Names, including font and resource names, remain inferred labels; register transfers and call arguments are the contract.

## ABI, seams, and callers

Let E be entry SP. All entries are ARM-state A32, with no meaningful return value promised.

- `115815c`: r0 is root/node. Entry word `e92d4830` saves r4,r5,r11,lr; fp=E-4 and SP=E-16. It saves r0 in r5, reads `015e46c4+1d0 = 015e4894`, calls `1166200(preparation)` at `1158174`, then calls `11662a0(root,[015e489c])` at `1158180`. It restores its 16-byte frame at `1158184`.
- `1166200`: r0 is preparation code. Entry word `e92d4800`, saves r11,lr; fp=E-4, SP=E-8, no other local allocation. The in-body PC-relative jump table is `1166218..1166254`, reached by conditional `ldr pc,[pc,r0,lsl #2]` at `1166210` after subtracting 1 and checking unsigned <=15. Input 1 calls `112db04(2)`; 3 or 8 calls `(3)`; 7 calls `(7)`; 9 calls `(9)`; 13 calls `(13)`; 16 calls `(16)`; all other inputs return without a callee. There is one distinct BL target, six sites (`116625c,1166268,1166274,1166280,116628c,1166298`). This is notification/animation preparation, not a row drawer.
- `11662a0`: r0 node and r1 mode are consumed by the first call `1165aa0` at `11662ac`, unchanged. Entry word `e92d4ff0` saves r4-r11,lr; fp=E-4, SP=E-168 after `sub sp,sp,#132`. No stack arguments are consumed. The draft `void(void)` and `1165aa0(0,0)` are incorrect. Exit is `sub sp,fp,#32; pop {r4-r11,pc}` at `11675e4..11675e8`.

Static BL callers in img6 are exhaustive for these three entry addresses:

| Target | BL sites |
| --- | --- |
| `115815c` | `111121c,1111254,1117628,1117658,1120d90,1120e30,1120e44,1120e58,1122b98,11240f4,1124acc` |
| `1166200` | `112420c,1158174` |
| `11662a0` | `1158180,11586e4,1158808,1167a10,1168c68,116a784` |

No direct B reference to those entries was found in img6, id4, or id5; no direct BL reference was found in id4/id5. An aligned little-endian word search of the complete image4, image5, and plain-image binaries found no word equal to any of these three entry addresses. That negative inventory does not prove the absence of dynamically constructed pointers. The relevant callback table word instead names `1123f2c`: root-1 draw table word `1110178 = 1123f2c`, whose ordinary branch calls `115815c(1)` at `11240f4` (diagnosis above). It is an indirect ancestor, not a table reference to `115815c` itself.

Bank-created references are separate: the publication at `115815c` is a non-linking B to `rb_bank_draw_root`; `rb_call_stock_draw_root` pushes exactly `{r4,r5,r11,lr}` and B-resumes at `1158160` (`menu/recipe_bank_arm.S:122..127`, linker `rb_stock_draw_root_continue`). Bank trampoline `rb_call_draw_root_prepare` targets `1166200`; `rb_call_draw_pane` targets `11662a0`. The root resume skips any new hook installed only at `115815c`.

## Published draw state and snapshot boundary

Define D=`015e64d8`, N=`015e64f4` (24-byte root summary), and R=`015e650c` (68-byte report). The pane itself does not acquire a private snapshot; it first calls retained `1165aa0(node,mode)`, which publishes D/N/R. Keeping that call retains snapshot selection and report construction as one boundary.

`1165aa0` saves 28 bytes and allocates 108, total 136 bytes, fp=E-4 (`1165aa0..1165ab0`). It initializes its 44-byte snapshot at fp-72 and calls live reader `61cd14(&snapshot)` at `1165b24`. Modes 2,7,13 additionally read saved slot 2 with `61ce1c(node,2,&snapshot)`; modes 5,11 read slot 1; mode 5 also calls `61d134()` (`1165b7c..1165c44`). Other modes keep the live snapshot. It then writes D words +0,+4,+8,+c,+10,+14,+18, clears R and N using `7221c8`, and calls `bf8e64(node,N)` (`1165d48..1165dc4`). D+14 is the input mode, D+18 snapshot word 1; D+10 is 1 for snapshot type 3 or 9, otherwise 0 (`1165d08..1165d30`). For node other than 1 or 0x60, a calculated D+4=2 becomes 0 (`1165d34..1165d44`).

For default/live mode, snapshot depth 1 produces D `{0,0,0,2,...}`; depth 0 `{1,0,1,2,...}`; depth 2 with type other than 9 `{2,2,1,0,...}` before node adjustment; depth 2/type 9 `{3,3,1,0,1,...}`. Other depths default to zeros in the first four words. Snapshot type 3 or 9 overrides D+0 to 3 and D+10 to 1 (`1165c5c..1165d60`). Animation modes override those first four words before that type override; mode 2 `{2,2,1,1}`, mode 3 `{2,1,1,1}`, modes 6/11/12 `{2,1,1,2}`, modes 7/9/13 `{2,2,1,0}`, mode 5 generally `{2,1,1,2}` but uses +4=2 for node 1/0x60 when `61d134()==2`. Values in braces are D+0,+4,+8,+c, not inferred enum names.

R fields consumed by the pane: +0 depth/kind, +4 node, +c root child id, +10 root selection id, byte +1c root selected row ordinal, byte +1f zero-based root page, +20 nested parent child, +24 nested raw count, +28 nested visible count, +2c nested capacity, +30 nested scroll top, +34 current nested selection, +38 remembered selection, bytes +3c current nested ordinal, +3d remembered ordinal, +3e centering rows. N+0 is raw root count, N+4 page count, N+8 root capacity. Preparation fills these through the same `bf8d20`, `bf8f4c`, `bf90f0` accessors as the tree/picker contracts; it computes page and root ordinal excluding hidden state 3 and computes nested ordinals after scroll (`1165e2c..1166198`). R+3c/+3d use 255 as absent sentinel. Preparation's summary/snapshot setup remains observable and must not be replaced by merely preloading the pane's expected draw rows.

The pane hook record H starts fp-124. Defined offsets: +0 node, +4 root child, +8 variant/selection, +12..+28 copied D+0..+16, +32 first coordinate, +36 second coordinate, +40 alignment/mode, +44 selected, +48 resource. Five D words are copied with LDM/STM (`1166464..1166478`, `1166708..116671c`, `1166914..1166928`). Unlike the tree contract's 64-byte zeroed record, this is not cleared: label construction leaves H+8 unspecified; nested construction leaves H+40 inherited from the root pass or private stack. Do not compare unspecified bytes as semantic outputs. Root child C starts fp-60: +0 record, +4 child id, +8 selection id, +12 variant, +16 state, +20 suppress-default-value. Nested record reuses that area: +0 selection, +4 state, +8 resource-data. No fourth argument is established for `bf8d20` at `1166380`.

## Root rows, page and selected value

The pane skips the root loop when D+4=3. Otherwise it selects font 13 for R+0=1, font 15 for R+0=0 or 2, leaving other kinds unchanged (`11662b0..1166304`). With N+0 and N+8 nonzero, skip count is `low8(N.capacity * R.page)`; raw index and drawn count begin zero, y begins 4. Every raw row calls `bf8d20(R.node,index,&C)`. State 3 skips without decrementing skip count or moving y. Nonhidden skipped rows decrement the eight-bit skip count; after it reaches zero, rows draw until raw count or capacity. Selection is C.child==R+0xc, not the raw index or the current stored property value (`1166308..11663bc`, `11667d0..11667ec`). Drawn count and y wrap as eight-bit values, y+=2. Grid-call coordinates are sign-extended low bytes.

For each drawn root row, the service order is:

1. `bf9360(node,index,(D+4==2 && !selected)?1:0)` obtains label; `1163e04(selected,C.state)` applies label style; `116cf50(node,C.child)` obtains the special-format predicate (`11663c4..1166400`).
2. For mode 11/12 and selected, `1112da4()` then `1113514(5,y,32,s8(y+1),17,56,153)` (`1166408..1166450`).
3. Fill H with label, x=6, y, H+40=0, selected and D; call label hook `116dce0(&H)`. A nonzero return suppresses default label. Zero draws `11121d4(6,y,label,1)`, except unselected D+4=2 uses aligned leaf `111278c(6,0,y,s8(y+1),0,2,label)`, preceded by `1112ce4(8)` if the format predicate is nonzero (`1166454..1166524`).
4. For mode 11/12 and selected, `1112d84()` (`1166528..1166540`). Skip current value if D+4=1, or selected and D+0=2 (`1166544..1166568`).
5. Otherwise obtain value `bf93e0(node,C.child,C.variant,0)`. Unselected rows use BL `116657c`; selected rows use cold block BL `11675c4`, then rejoin at `1166590`. Unselected D+4=2 sets x=9, alignment=1; all other cases x=-20, alignment=0 (`116656c..11665b0`, `11675b4..11675cc`).
6. Value palette: with D+8=0, selected uses `(120,255,0,0)`, unselected disabled state 2 `(107,255,0,0)`, other unselected grid palette 104. With D+8!=0 and D+0 in {2,3}, selected uses `(121,255,0,0)`, unselected disabled `(107,76,0,0)`, other unselected `(118,255,0,0)`. With D+8!=0 and D+0 outside {2,3}, disabled uses `(122,255,0,0)` and other state `(118,255,0,0)` regardless of selection (`11665b4..11666a8`).
7. If format predicate: D+0=0 or (D+0=3 and D+4=0) calls `1112e38()` then grid palette 113; otherwise `1112ce4(8)` (`11666ac..11666ec`). Fill H with variant, value, x/alignment and call value hook `116e264(&H)`. Only when C+20=0, value!=0, and hook returns zero draw `111278c(x,0,y,s8(y+1),alignment,2,value)`. End angle-bracket format with `1112e58()` in the same bracket-enabled case (`11666f0..11667cc`).

The hooks are retained special-rendering boundaries, not accessors or owned row loops. Their suppression semantics apply even to a zero resource. The two patched value sites do not cover the nested value call at `11668e8`.

## Nested picker and second-level list

When D+c!=2 and mode is neither 2 nor 3, select font 11 even if the nested list is empty. Enter the nested loop only for R+24!=0 and R+2c!=0. Skip `low8(R+30)` nonhidden records, start y=`low8(2*R.byte3e+3)`, enumerate raw indices with `bf90f0(node,R+20,index,&C)`, skip state 3, and draw at most R+2c rows (`11667f0..11668c4`). Selected means C.selection==R+34. Hidden and scrolled rows do not advance y; drawn count and y wrap eight bits (`1166a30..1166a4c`).

Per drawn nested row: `bf93e0(node,R+20,C.selection,1)`, `1163c1c(selected,C.state)`, fill H with x=13,y,D and resource, then `116f948(&H)`. Zero hook draws `111236c(14,y,resource)`; nonzero suppresses that draw. For a selected row under mode 7/13, enable `1177e10(1113bf4,1,0)`, clear descriptor highlight with `1163c1c(0,state)`, redraw default resource if the first hook returned zero, otherwise set H.selected=0 and call `116f948` again, then disable `1177e10(1113bf4,0,0)`. A selected row then calls `116d4e4(node,parent,R+34)` unless mode 2/3, and always `1159168(node,parent,R+34)`. The mode-2/3 footer path is a cold continuation at `11675d0`, not the next function (`11668c8..1166a2c`, `11675d0..11675e0`).

This draws the right-hand choices beside the left root rows, including node 1's ordinary property pickers and the bank edit screen's open field lists. The root loop alone draws the ordinary shooting list. A second-level list under the generic pane path uses the nested loop; it is not automatically `overlay_draw_choice_list`. Other second-level callback families may instead use `116768c -> 1167a5c -> 1164e1c` or `1168bb8 -> 1168ce0 -> 1163f2c`. Those old tree/picker contracts remain separate and are not evidence of coverage for this pane.

## Chrome, tabs, markers, palette and font effects

After rows the following blocks execute in order. Drawing calls remain in this order even when later background/title calls cover earlier commands.

1. D+0=1 clears the screen: if `[956754+18]==2`, `1112d64(bounds)`, `11130e0(rectangle,18,0,255)`, `1112d64(0,0,0,0)`; `[957aa0+2bf0]==2` selects bounds (-160,-30,960,540) and rectangle (-160,-30,800,510), otherwise (0,0,960,540) for both. Other display modes use `1113514(1,1,32,20,18,0,255)` (`1166a50..1166b48`).
2. Save font `[15e46c0]`; call `1135db0()`. If zero, D+10=0 selects font 18 and fills `(1,1,32,20,18,0,255)`, then title by R.node. Node 1: font15, palette `(109,255,0,0)`, grid `(9,1,0x27a)`, font15, grid palette6, `113f1e8()`, grid `(5,1,result)`. Node 0x2a: font17, grid `(5,1,ff0000ca)`, palette109, grid `(9,1,0x5c3)`; node0x3f analogous `ff0000cc` and `0x12`. Nodes0x2b/0x60: font15, `1130e68(0)`, fill `(1,1,32,2,16,[110ba74+style*48+8],204)`, palette7, grid `(3,1,0x27b)`. Restore saved font, only within this zero-return block (`1166b4c..1166db0`).
3. Always call getters `bf9614`, `bf963c`, `bf9660`, `bf9688`, in that order. If D+0!=3, select font15. Nodes1,0x2a,0x3f draw page tabs: node1 count is 1 if second getter==0x2c, otherwise5; node0x2a count3; node0x3f count3 if first getter in {4,6}, otherwise1/5 by second getter==0x2c. Palette/table bank index is 1 for D+0=0, 2 for D+0=2, otherwise0 (`1166db4..1166e84`).
4. For tab i at `(1,2*i+3)`, use three-word entries: node1 table `116cd34` with bank stride6 words if second getter==0x2c, otherwise `116ce9c` with stride15; node0x2a table `116cd7c` with stride9. Select word0 for current page i==R.byte1f, word1 otherwise. Node0x3f uses word1 from short/long tables if first getter in {3,5}, otherwise word1 from `116cd7c`. Then three extra tabs from `116cde8`, bank stride15 words, at successive y+=2: default word1, or for node0x3f word0 on current page; force word2 when fourth getter==1 or third getter!=0 and second getter==0x1f (`1166e84..1167058`).
5. If D+4!=3, font17 and grid `(4,3,resource)` where resource is ff0000e3 for D+8=0, ff0000e7 for D+8!=0 and D+0 in{2,3}, otherwise ff0000e2. If N+4>1 and D+8=0: font16, components `(116,255,0,0)`, grid `(18,18,ff000617)` then `(18,3,ff000660)` (`116705c..1167138`).
6. If D+c!=2: enable IA for mode2/3, font14, grid `(12,2,ff0000e8)`, disable IA for mode2/3; for mode outside{1,2,3} and D+4!=3, font13, grid palette116, grid `(12,9,ff00035f)` (`116713c..11671f0`).
7. Save current font. If D+0=1 skip all marker work and its restore. Otherwise D+c=2 and D+4=0: font16, `1130e68(0)`, grid `(5,2*(R.byte1c+2),[110f528+style*8])`, font13, components `(117,255,0,0)`, grid same position ff00062f unless D+0=3, and always grid x32 same y ff00062e. D+4 in{1,2} instead draws font16 `(5,2*(R.byte1c+2),ff0000f3)`. With D+c!=2, draw that f3 marker whenever D+4!=3 (`11671f4..1167370`).
8. With D+c!=2 and mode not2/3, use current y=`low8(2*(R.byte3c+R.byte3e)+3)`, font13; if current ordinal!=255 call `1130e68(0)` and draw `(13,y,[110f528+style*8+4])`. Always call `1138a00(8)`. If remembered ordinal!=255, fill `(13,remembered_y,13,remembered_y+1,11,colour,255)` using colour from that result; if current and remembered y coincide and mode neither7 nor13 use colour7. For mode7/13, enable `1177e10(1113bf4,1,0)`, fill `(13,current_y,32,current_y+1,15,124,255)`, disable format. Restore saved font (`1167374..11674f0`).
9. If D+c!=2 and R+28>R+2c: font13, components `(116,255,0,0)`, grid `(23,2,ff000660)`, `(23,17,ff000617)`. Finally if D+0 in{0,1}, or D+0=3 and D+4=0, call `112eb38(0x78c,1,19)`; otherwise return (`11674f4..11675ac`).

There is no function-wide restoration of entry font or palette. The two local font saves restore only their blocks' entry state; later arrows and final text may change it again. Palette/IA/bracket/bounds calls are observable retained side effects. The pane's own non-stack stores build H/locals/outgoing arguments; global D/N/R writes occur through preparation, and font/palette/render-object writes through callees. An oracle must check those resulting state bytes and ordered external draw events, not assume tree-list's final entry-font restoration.

## Callee boundary classification and complete order

There are 37 distinct direct BL targets in the pane, 124 BL sites, no BLX sites. The root wrapper has two distinct callees; preparation dispatcher has one. “Draw leaf” here means the retained drawing API boundary, not a machine leaf. `11121d4`, `111236c`, `111278c`, `1113514`, `11130e0`, and `112eb38` draw resources/boxes/text but call deeper stock services. Font, palette, bounds, and formatting setters also call deeper services.

Machine leaves among the 37 are `bf9360`, `116cf50`, `1135db0`, `bf9614`, `bf963c`, `bf9660`, `bf9688`, confirmed across their complete img6/id5 extents. The other 30 have calls or an outgoing tail branch. In particular `113f1e8` has a tail B to `113f670`, not a BL. Leaf status does not imply a pure function: `116cf50` reads settings through `[94de4]`; the four getters read `[94de4]+bb6c`, `[[93ade4]]`, signed byte `[94de4]+cf00`, and `[94de4]+cf44`, respectively. `1135db0` checks byte `[15e4b3f]` and, only if 1, word `[[94de4]+10080]`.

The dispatcher's sole boundary `112db04` is NONMATCHING (`ov44_notification_meters.c:724`) and is not a machine leaf: its bytes call `69f8fc` at `112de80,112de98,112dee4,112defc` and `1112ab8` at `112e020`. Neither root-wrapper callee is a machine leaf.

Shared with tree/picker contracts: root label hook `116dce0`, root value hook `116e264`, nested hook `116f948`, style helpers `1163e04/1163c1c`, special-format predicate `116cf50`, resource accessors `bf9360/bf93e0`, child enumeration `bf8d20/bf90f0`, selected detail/footer `116d4e4/1159168`, and the font/palette/format/draw APIs. Snapshot readers and summary accessors are indirect through `1165aa0`, not direct pane callees. Generic grid wrappers differ from tree-list's descriptor leaf `1112e78`; do not substitute one oracle's arguments for the other.

Exactness of each retained boundary at the pinned revision:

| Address | Inferred name | Exactness |
| --- | --- | --- |
| `01165aa0` | `FUN_01165aa0` | nonmatching |
| `01112b2c` | `overlay_select_panel_font` | exact |
| `00bf8d20` | `menu_tree_describe_child` | nonmatching |
| `00bf9360` | `FUN_00bf9360` | nonmatching |
| `01163e04` | `FUN_01163e04` | nonmatching |
| `0116cf50` | `FUN_0116cf50` | nonmatching |
| `01112da4` | `overlay_disable_format_option_ia` | exact |
| `01113514` | `overlay_fill_grid_cell_box` | nonmatching |
| `0116dce0` | `FUN_0116dce0` | nonmatching |
| `01112ce4` | `overlay_select_primary_palette_color` | exact |
| `0111278c` | `overlay_draw_aligned_grid_resource` | nonmatching |
| `011121d4` | `overlay_draw_grid_resource_with_mode` | exact |
| `01112d84` | `overlay_enable_format_option_ia` | exact |
| `00bf93e0` | `ui_menu_read_child_record_word` | nonmatching |
| `01137cf8` | `overlay_set_palette_components` | exact |
| `01137d2c` | `overlay_apply_grid_palette` | nonmatching |
| `01112e38` | `overlay_begin_angle_bracket_format` | exact |
| `0116e264` | `FUN_0116e264` | nonmatching |
| `01112e58` | `overlay_end_angle_bracket_format` | exact |
| `00bf90f0` | `ui_menu_find_child_selection` | nonmatching |
| `01163c1c` | `overlay_set_descriptor_highlight` | nonmatching |
| `0116f948` | `overlay_draw_selection_value` | nonmatching |
| `0111236c` | `overlay_draw_grid_resource` | exact |
| `01177e10` | `ov48b_parse_text_format` | nonmatching |
| `0116d4e4` | `FUN_0116d4e4` | nonmatching |
| `01159168` | `overlay_draw_choice_resolution_footer` | nonmatching |
| `01112d64` | `overlay_set_drawing_bounds` | exact |
| `011130e0` | `overlay_fill_pixel_rectangle` | exact |
| `01135db0` | `FUN_01135db0` | exact |
| `0113f1e8` | `FUN_0113f1e8` | nonmatching |
| `01130e68` | `ov44_level_bar_style_index` | nonmatching |
| `00bf9614` | `FUN_00bf9614` | exact |
| `00bf963c` | `FUN_00bf963c` | exact |
| `00bf9660` | `FUN_00bf9660` | exact |
| `00bf9688` | `FUN_00bf9688` | exact |
| `01138a00` | `FUN_01138a00` | nonmatching |
| `0112eb38` | `ov44_grid_text_0112eb38` | nonmatching |

Complete BL sites in increasing instruction order follow. Conditional paths are specified above; this table includes repeated targets, including cold continuation blocks. In every row, the instruction is direct `bl TARGET`.

| Site | Target |
| --- | --- |
| `011662ac` | `01165aa0` |
| `011662ec` | `01112b2c` |
| `011662f8` | `01112b2c` |
| `01166304` | `01112b2c` |
| `01166380` | `00bf8d20` |
| `011663e4` | `00bf9360` |
| `011663f4` | `01163e04` |
| `01166400` | `0116cf50` |
| `01166420` | `01112da4` |
| `01166450` | `01113514` |
| `011664a8` | `0116dce0` |
| `011664e0` | `01112ce4` |
| `0116650c` | `0111278c` |
| `01166524` | `011121d4` |
| `01166540` | `01112d84` |
| `0116657c` | `00bf93e0` |
| `011665d8` | `01137cf8` |
| `01166600` | `01137cf8` |
| `01166620` | `01137cf8` |
| `01166648` | `01137cf8` |
| `01166654` | `01137d2c` |
| `01166688` | `01137cf8` |
| `011666a8` | `01137cf8` |
| `011666d8` | `01112e38` |
| `011666e0` | `01137d2c` |
| `011666ec` | `01112ce4` |
| `0116674c` | `0116e264` |
| `0116679c` | `0111278c` |
| `011667cc` | `01112e58` |
| `01166820` | `01112b2c` |
| `011668a8` | `00bf90f0` |
| `011668e8` | `00bf93e0` |
| `011668f8` | `01163c1c` |
| `0116694c` | `0116f948` |
| `01166968` | `0111236c` |
| `0116699c` | `01177e10` |
| `011669b0` | `01163c1c` |
| `011669b8` | `0116f948` |
| `011669c8` | `01177e10` |
| `011669dc` | `01177e10` |
| `011669e8` | `01163c1c` |
| `011669f8` | `0111236c` |
| `01166a08` | `01177e10` |
| `01166a1c` | `0116d4e4` |
| `01166a2c` | `01159168` |
| `01166aac` | `01112d64` |
| `01166b04` | `011130e0` |
| `01166b18` | `01112d64` |
| `01166b48` | `01113514` |
| `01166b58` | `01135db0` |
| `01166b7c` | `01112b2c` |
| `01166ba8` | `01113514` |
| `01166bf0` | `01112b2c` |
| `01166c04` | `01137cf8` |
| `01166c14` | `0111236c` |
| `01166c1c` | `01112b2c` |
| `01166c24` | `01137d2c` |
| `01166c28` | `0113f1e8` |
| `01166c38` | `0111236c` |
| `01166c44` | `01112b2c` |
| `01166c58` | `0111236c` |
| `01166c6c` | `01137cf8` |
| `01166c7c` | `0111236c` |
| `01166c88` | `01112b2c` |
| `01166c9c` | `0111236c` |
| `01166cb0` | `01137cf8` |
| `01166cc0` | `0111236c` |
| `01166ccc` | `01112b2c` |
| `01166cd4` | `01130e68` |
| `01166d10` | `01113514` |
| `01166d24` | `01137cf8` |
| `01166d34` | `0111236c` |
| `01166d40` | `01112b2c` |
| `01166d48` | `01130e68` |
| `01166d84` | `01113514` |
| `01166d98` | `01137cf8` |
| `01166da8` | `0111236c` |
| `01166db0` | `01112b2c` |
| `01166db4` | `00bf9614` |
| `01166dbc` | `00bf963c` |
| `01166dc4` | `00bf9660` |
| `01166dcc` | `00bf9688` |
| `01166dec` | `01112b2c` |
| `01166f9c` | `0111236c` |
| `01167048` | `0111236c` |
| `011670bc` | `01112b2c` |
| `011670cc` | `0111236c` |
| `011670fc` | `01112b2c` |
| `01167110` | `01137cf8` |
| `01167124` | `0111236c` |
| `01167138` | `0111236c` |
| `01167168` | `01112d84` |
| `01167170` | `01112b2c` |
| `01167184` | `0111236c` |
| `011671a0` | `01112da4` |
| `011671d4` | `01112b2c` |
| `011671dc` | `01137d2c` |
| `011671f0` | `0111236c` |
| `0116724c` | `01112b2c` |
| `01167254` | `01130e68` |
| `01167278` | `0111236c` |
| `01167280` | `01112b2c` |
| `01167294` | `01137cf8` |
| `011672bc` | `0111236c` |
| `011672d0` | `0111236c` |
| `011672f0` | `01112b2c` |
| `01167318` | `0111236c` |
| `01167348` | `01112b2c` |
| `01167370` | `0111236c` |
| `011673b8` | `01112b2c` |
| `011673d4` | `01130e68` |
| `011673f0` | `0111236c` |
| `011673f8` | `01138a00` |
| `01167478` | `01113514` |
| `011674a8` | `01177e10` |
| `011674d8` | `01113514` |
| `011674e8` | `01177e10` |
| `011674f0` | `01112b2c` |
| `01167524` | `01112b2c` |
| `01167538` | `01137cf8` |
| `0116754c` | `0111236c` |
| `01167560` | `0111236c` |
| `011675ac` | `0112eb38` |
| `011675c4` | `00bf93e0` |

## What bank publications change

`host/firmware/BuildRecipeBank.hs:634..644` checks root entry word `e92d4830` and replaces it with a tail B to `rb_bank_draw_root`. It also checks `116657c=ebea4b97` and `11675c4=ebea4785` and retargets both BLs from `bf93e0` to `rb_bank_current_value_label`. The assembly helper returns zero only for node r0=1, r3=0, and child r1 in{0x281,0x285}; otherwise it tail-BX delegates to `bf93e0`, preserving the original LR and r0-r3 arguments (`menu/recipe_bank_arm.S:129..150`). These are the package navigation rows' absent current-setting labels. They neither bypass a full pane nor intercept nested r3=1 choice resources.

`rb_bank_draw_root` consults the UI frame and inferred bank page (`menu/recipe_bank_actions.c:100..137`). Dialog uses preparation plus the bank action-list drawer, not this pane. Field/select pages adjust the current-row marker before calling stock. Subpage sets the marker to verbs-per-slot, draws a bank backdrop, invokes preparation from `015e4894`, and calls pane(frame.node,0). Slot/select/edit/field call the stock root trampoline with frame.node. Everything else calls it with the incoming root. Thus the ordinary node-1 root and its pickers still reach this pane, and package edit field lists also share it. Bank preparation of markers/frame state must precede the owned pane just as it precedes stock.

## Proposed R1 owned class and feasibility

The smallest new owned-function set is **`FUN_011662a0` alone**, replacing its row loops and chrome for the ordinary node-1 live root/picker class. Retain `115815c`, `1166200`, and `1165aa0` as named wrapper/preparation/snapshot boundaries. Owning only `115815c` is insufficient: the bank resumes at `1158160`, and forwarding from a new wrapper to stock pane does not own drawing. Owning only `1166200` owns no rows. Owning the generic pane entry catches the root-wrapper path, bank resume path, and direct pane callers without duplicating the bank root decision.

Classify at the pane boundary using typed node/mode/snapshot state, initially ordinary node1, live root or picker depth/kind supported by preparation, non-package nodes/records, with no saved-report mode ownership. Include node1 matrix properties 15,26,36,211,239,260,271. The extra two node1 records are `789ea4` (property260) and `77d514` (property271), from `private-data/scratch/ControllerStrategy/menu-tables.json`, confirmed against image4 words +0=1 and +8=0x104/0x10f. Each property also has records under other node ids, so property id alone is not the class discriminator. Do not infer membership from a property label or assume the historical diagnosis reconstructed all transient callback states. A route is qualified only when its armed draw entry and owned-class decision actually hit. Package pages and unowned modes preserve stock pane fallback through displaced-entry replay at `11662a4`, not recursion through the hook. A full pane implementation can expand that class later; this report does not claim those additional modes qualified.

The complete stock pane needs 37 stubs if every direct target is stubbed, exceeding `MAX_STUBS=32` in `native_case.rs`. This is a fixture design constraint, not an impossibility or a reason to prune a branch from the contract. A concrete unchanged-limit native fixture can execute the five exact, call-free getter bodies `1135db0,bf9614,bf963c,bf9660,bf9688` as admitted stock functions and stub the remaining **32** targets. It must map their pointed-to state and preserve their real register/stack effects. The getter `bf9660` introduces register-offset LDRSB, and these leaf prologues use predecrement STR and LDM SP writeback; these forms must be admitted explicitly. Alternatively, per-case stub subsets can declare only reached targets and leave undeclared unexpected calls refused, with cases covering the union of all 37; that requires recording each case's exact boundary set, not claiming a 37-stub case passed.

Static pane forms are ADD (including conditional and shifted operands), AND, B/BL, CMP (including CMPNE), EOR, LDM/STM with writeback, LDR/LDRB (immediate and scaled-register, conditional), LSL, MOV/MOVW/MOVT (including conditional halves), MUL, PUSH/POP, RSB, STR (including conditional), SUB/SUBS, SXTB/SXTH, UXTB (including conditional). Conditions include EQ/NE/HI/LS/HS/LO/LT. No divide, coprocessor, exception, exclusive, VFP, or device instruction appears in the pane. The dispatcher adds the PC-relative LDR-PC jump-table form, with 16 embedded data words that must not be scanned as executed instructions. No static observation here constitutes a successful native admission run.

The pane stock private frame is 168 bytes; its retained preparation can add 136 before deeper accessors, making at least 304 bytes below pane entry before deeper calls. The wrapper adds 16. An owned candidate must derive its emitted stack bound plus retained-service depth against the task-33 headroom, not reuse old tree/picker figures. Compare the published D/N/R, externally meaningful font/palette/format state, ordered calls with pointed H/C records excluding undefined bytes, and restored ABI registers/SP. Cover empty/all-hidden rows, off-page selection, page multiplication truncation, scroll/centering, disabled rows, hook suppression, both preview call paths, and title/tab/marker branches. Selected-row, wrong-page/scroll, skipped-hidden and preview-label mutation controls must fail on cases where those behaviors are observable.

Bank coexistence requires retaining its existing `115815c` publication and trampoline. Add a separately owned seam at `11662a0` with displaced word `e92d4ff0`; do not republish the root entry as a second owner. The two in-body bank publications remain meaningful for stock fallback, but are bypassed by an owned pane. The owned resource adapter must explicitly apply the same preview rule or call the bank's preview entry for r3=0, while leaving nested r3=1 lookups unchanged. It cannot call unpatched `bf93e0` for those two navigation children and claim composed equivalence. Preserve package preparation/marker ordering, bank backdrop/action-list paths, overlay residency, publication expected words and allowlist entries, fallback target and frame replay. Keep the selected-drawer interposition at `11089d8` intact. A revised archive must account for the pane entry and the two retained interior publications rather than silently deleting or double-owning either.

This revises R1's coverage target; it does not replace the already documented tree/choice-list contracts or turn their host qualification into node1 guest evidence. The next qualification must trace `11662a0` and its owned adapter/resume and require root plus picker hits on the actual matrix routes. Second-level screens using the separate tree/picker callers still require their own ownership evidence.

## Host qualification

`PaneDrawerCases.hs` derives 411 native cases from this contract. Stock and `menu_widgets/src/pane_drawer.rs` each pass all 411 with status `returned`, no mismatches and preserved SP/r4-r11. The five exact call-free getters execute their pinned bytes; the remaining 32 targets are scripted service boundaries. Preparation publishes the expected synthetic D/N/R. Cases check those published bytes, resulting font/palette memory, ordered service arguments and meaningful hook words. Undefined hook words and only the emitted private frame are excluded. No guest or camera run is established.

The matrix covers root/nested counts, pages, selected and off-page rows, hidden/disabled states, hook suppression, scroll, centering, both root preview-value paths, all seven matrix properties, pixel-display bounds, titles, short/long/forced tabs and marker branches. It includes every combination of snapshot depth `{0,1,2}`, type `{0,1,2,3,9}` and mode `{0,1,3,4,6,8,9,10,12,14,255}`. Snapshot preparation itself remains scripted, not qualified by these draws.

All five controls run the same 411 cases. Selected-row styling fails 329 cases; wrong root page fails 381; ignored nested scroll fails 36; drawing hidden rows fails 3; wrong preview labels fail both preview cases. Negative outcomes are mismatches after return or `stub_contract` at a differing retained-service call, not unsupported instructions.

The candidate entry `owned_pane_drawer` is A32 at `0x01800000`. Its emitted body frame is 280 bytes (36 saved plus 244 local); retained-call thunks add at most 24, making the ignored native private-frame bound 304 bytes. The selected/page/scroll/hidden/preview control bounds are 280/304/296/296/296 bytes respectively. Stock uses 172 bytes including the getters' four-byte frame. These native bounds do not replace the composed task-33 stack check.

| Artifact | SHA256 |
| --- | --- |
| Owned native image | `4fbdfa1fed6a6be3a4d177d0ae4fd6446fdfdbce00da44242a83fff2070da996` |
| Owned native ELF | `430c04f9914100408748d1d62ec46034a903993809edb4b3ad271d8cf7d91d9c` |
| Native runner | `d3977d0e97b53445d160054c01d3c3e32efaab5d1531a6338bc4e0f2904c93d3` |
| Generator executable | `637f9a6c4a306d44e22f455643e1b1267fb6651e20845f1b2519323d5382c818` |
| Stock report checksum inventory | `038f97538d9cad4338d0c9b02c9edef258212599e6a0fc7987bb70f03959a004` |
| Owned report checksum inventory | `8ee27539c45666b678e37a0b8b262c39e56117ef34adc0b0617617111251c6d4` |

Reports are under `private-data/analysis/x-m1-1.50/native-case/pane-drawer`, `pane-drawer-owned` and `pane-drawer-control-{selected-unstyled,page-off-by-one,scroll-ignored,hidden-drawn,preview-label-wrong}`. Each report-inventory hash is `sha256sum *.json.report | sha256sum`, run within its report directory.

Build with `devenv shell -- bazel --output_base=$HOME/.cache/bazel/latent-image-pane-drawer-r1 build --jobs=2 //replacement/menu_widgets:pane-drawer-case-image //emulation:pane-drawer-cases //emulation:native-runner`; the five `pane-drawer-control-NAME-image` targets build mutations. Run `pane-drawer-cases --image IMAGE6 --output OUTPUT --candidate IMAGE.bin --symbols IMAGE.symbols --frame-bytes 304 --runner NATIVE-RUNNER`; omit the three candidate options for stock and use the control-specific frame bounds for mutations. `//emulation:native-runner-tests` passes, including 85 Rust native-case tests and the new signed register-offset LDRSB, predecrement STR/LDM frame, and out-of-range byte-access fixtures. Existing admission already supports these forms; no runner profile widening was needed.
