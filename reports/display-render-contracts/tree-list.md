# Tree-list draw contract

Source: x70view `a72c95a3572562fc05f359b4f35915f3a0243b19`, lookup `0x01164e1c`: NONMATCHING definition `app/ui/menus/tu_01163b08.c:650`, range `[0x01164e1c,0x01165aa0)`, 3,204 bytes, SHA256 `785cbbbfbd3a4542c8798970e5038eecb7c4b5e694e84331d6b75334379a84d9`. C is a reading aid only. Addresses below cite `decode/img6.dis` instruction bytes. The owned class is live snapshot, effective kind 1, non-package list node, caller BL `0x1167a5c`. Other kinds below describe the stock boundary, not owned behavior.

## ABI and records

At entry `E=sp`, push saves r4-r11 and lr, fp becomes `E-4`, sp becomes `E-352` (`1164e1c-1164e24`). Arguments are node r0, first grid coordinate r1 (low 16 bits), second grid coordinate r2 (low 16 bits), first-coordinate step r3; stack `E+0` arg4 second-coordinate step, `E+4` arg5 nested first coordinate, `E+8` arg6 nested second coordinate, `E+12` arg7 nested first step, `E+16` arg8 nested second step, `E+20` arg9 mode, `E+24` arg10 report (`1164e28-1164e3c`, `1165138`, `1165154-116515c`, `1165618-1165628`, `11656b0`, `1165704`, `1165a40-1165a64`). No meaningful return value is specified. Panel font at `0x15e46c0` is saved before snapshot acquisition (`1164eac-1164eb8`), restored by `0x1112b2c(saved)` on exit (`1165a78-1165a84`).

The owned caller supplies first step 0, second step 2, nested second coordinate 4, nested first step 0 and nested second step 2. Node, first coordinate, second coordinate, nested first coordinate and mode come from its r6/r8/r4/r5/r7; report is its fp-104 (`1167a24-1167a5c`). Live modes are 0,1,2,3,4,8,10 and >=14; saved-slot modes are excluded from R1 ownership.


Snapshot S is 11 words at fp-80, initialized zero (`1164e40-1164e6c`). Mode <=13 tests `1<<mode`: mask `0x2280` chooses `0x61ce1c(node,2,&S)`, otherwise mask `0x1860` chooses slot 1; every other mode uses `0x61cd14(&S)` (`1164ebc-1164f14`). S[0] is forced to 1 for node 0x25 with S[4]=0 and S[3]=0x27, or node 0x38 with S[4]=11 and S[3]=0x228 (`1164f18-1164f60`).

Summary N starts fp-104. `0xbf8e64(node,&N)` fills it; N[0] is raw row count, N[2] is page capacity P, normalized from zero to 1 (`1164fb0-1164fc8`, `1165630-1165638`). Child C starts fp-128; `0xbf8d20(node,raw_index,&C)` fills at least five words: C[0] record, C[1] child id, C[2] selection id, C[3] variant, C[4] state. State 3 skips a row (`1165198-11651a4`, `11651f4`, `1165228-1165238`, `1165388-1165398`). The third argument is the only out pointer established at this call site; an extra fourth C argument in the draft is not established by these bytes.

The 64-byte hook record H at fp-264 is zeroed by `0x43550(&H,0,64)` (`1164ea0-1164ea8`). Its defined words are +0 node, +4 child id, +8 variant, +32 signed first grid coordinate, +36 signed second grid coordinate, +40 zero, +44 selected, +48 resource, +52 effective kind, +56 mode, +60 raw index (`1165388-11653c4`, `116553c-1165570`). The initial label call leaves +56 unchanged; value calls set it to mode. Unassigned bytes persist across rows and hook invocations, not a newly zeroed record each time.

The 32-byte resource descriptor D at fp-200 starts zero (`1164e80-1164e9c`). Fields: +0 resource word, +4 signed pixel x halfword, +6 signed pixel y halfword, +8/+10 geometry halfwords, +20 zero, +24 layout word, +28 initially zero. Label descriptor: x=20*(first-1), y=24*(second-1), geometry=0, layout=0 (`1165264-11652b4`). Value descriptor: x=-20, same y, +8=0, +10=y+48, layout=2 (`11654f4-1165524`). Halfword arithmetic truncates; grid arguments passed through SXTB truncate and sign-extend eight bits.

## Owned flat control flow and service order

Report R, when nonnull, is zeroed by `0x7221c8(R,0,68)`. Initial writes: +0=S[0], +4=S[1], bytes +1d/+3d=255, +20=S[3], +30=S[10], +38=S[5] (`1164f64-1164fac`). For kind 1: selected visible index A=S[7], page Q=S[9], visible count V=S[8]. Report +8=S[3], +10=S[4], byte +1f=Q, word +14=low8(V), +18=P (`1164ffc-1165038`).

Second coordinate begins low16(arg2). Unless node is 0x53/0x57, V<P adds `arg4 * low8((P-V)>>1)` to that coordinate modulo 65536 and writes the centering byte at R+1e (`116510c-1165144`). This write is unconditional on report nonnull. The owned caller supplies a report.

For each raw row i from 0 to N[0]-1, issue `0xbf8d20(node,i,&C)`. Hidden state 3 advances only i. Let k be the eight-bit count of nonhidden rows seen, initially zero. Issue `0x4d978(k+P,P)` for page p. If Q>p, increment k modulo 256 and continue. If Q<p, stop. Equality draws the row (`116518c-11651d8`). On a drawn row with k=A, write report +c=C[1]; byte +1c is drawn-row ordinal when p<=1, otherwise r1 remainder from `0x4da6c(k,P)` (`11651dc-1165224`). When C[2]=S[5], write byte +1d=k (`1165228-1165238`). Selection flag T=(k=A) (`116523c-1165248`).

Each drawn row issues the following calls, inline in this order:

1. `0xbf9360(node,i,0)` returns label resource L (`116524c-116525c`). Set label descriptor and hook record as above.
2. `0x1112b2c(15)` if mode in {5,6,11,12} or effective kind 2, otherwise `(13)`. Write side state `0x15e64d8+8` to 1 in the first branch or 0 otherwise; write +0=3 in both branches (`11652b8-116531c`).
3. `0x1163e04(T,C[4])` (`1165320-1165328`).
4. If mode in {11,12} and T: `0x1112da4()` then `0x1113514(s8(first-1),s8(second),30,s8(second+1),17,56,153)` (`116532c-1165384`).
5. `0x116dce0(&H)`. Any nonzero result skips the label leaf; zero issues `0x1112e78(&D)` (`11653c4-11653d8`). In particular the branch lands after the BL, not on it.
6. Special node-0x38 override only: when S[4]=11,S[3]=0x228,T and either mode=13 or side+14=13: `0x1177e10(0x1113bf4,1,0)`, `0x1163c1c(0,local_zero)`, `0x111236c(13,s8(second),L)`, `0x1113514(s8(first-1),s8(second),31,s8(second+1),15,124,255)`, `0x1177e10(0x1113bf4,0,0)` (`11653dc-116549c`). The local at fp-160 was initialized zero and is not filled on the flat path.
7. Modes 11/12: if T issue `0x1112d84()`, then finish row (`11654a0-11654b8`). Other modes: issue `0x116cf50(node,C[1])`; if nonzero issue `0x1112ce4(8)` (`11654bc-11654d4`).
8. `0xbf93e0(node,C[1],C[3],0)` returns value resource W. Set value descriptor, update H resource and mode, issue `0x1112b2c(kind==2 ? 15 : 13)`, then `0x116e264(&H)` (`11654d8-1165574`). Any nonzero result ends the row. Zero issues `0x1112e78(&D)` (`1165578-1165584`).
9. If kind 1, W!=0 and mode neither 5 nor 6: `0x1112b2c(16)`, `0x1137cf8(28,204,107,204)`, `0x1138060(24,30,s8(second),1,2,0xff000101)` (`1165588-11655ec`).

Finish drawn row: increment k modulo 256, drawn ordinal by 1, second coordinate by arg4 modulo 65536, first coordinate by arg3 modulo 65536 (`11655f0-1165628`). Then advance raw index. Hidden rows and earlier-page rows do not advance the drawn coordinates (`116562c-1165638`). Kind 1 exits directly through font restoration (`1165648-116565c`, `1165a78`). Snapshot scroll top S[10] is copied to report +30 but does not choose flat rows. Report +3e stays zero. There are no page-tab draw calls on the flat path.

## Other stock kinds and complete callee inventory

Kind 2 first enumerates children, skipping state 3; calls `0x11730dc(node,kind,&word_fp268,&word_fp272,&selected_index)` for nonhidden children and uses its out words when nonzero, otherwise matches C[1]=S[3]. It counts nonhidden rows, divides selected index by capacity and adds 1 to determine page (`1164fd8-1165108`). Unknown kinds use page/count zero. The first row loop remains shared.

After that loop, only kind 2 with mode other than 2/3 enters the nested loop (`1165648-116565c`). It calls `0xbf8f4c(node,S[3],&nested_summary)`; report +20=S[3], +24=summary[0], +28=S[8], +2c=summary[2], +30=S[10], +38=S[5], +34=the fp-164 local initialized zero (`1165660-11656ac`). It centers the nested second coordinate from arg6 with step arg8, writes +3e, skips S[10] nonhidden children, stops after report +2c drawn rows or raw count, and uses `0xbf90f0(node,S[3],index,&nested_child)` with words {selection,state,resource-data} (`11656b0-116577c`, `1165a38-1165a74`). Report +3d records previous selection, +3c current selection, +40 selected resource-data (`1165780-11657b8`).

Nested row order: `0xbf93e0(node,S[3],selection,1)`, font 11, palette `0x1137cf8(selected?105:state==2?107:106,255,0,0)`, hook `0x116f948(&H)`, default resource leaf on zero (`11657bc-11658c4`). A selected row under mode 7/13 or side+14=13 enables format at `0x1177e10(0x1113bf4,1,0)`, descriptor highlight `0x1163c1c(0,state)`, then either redraws hook with selected=0 (original hook nonzero) or grid resource `0x111236c(node==0x38?21:14,s8(second),W)`; both issue cell box `(s8(first-1),s8(second),32,s8(second+1),15,124,255)` and disable format (`11658c8-1165a10`). Selected rows then call `0x116d4e4(node,S[3],S[4])` unless side+14 is 2/3 and always call footer `0x1159168(node,S[3],S[4])` (`1165a18-1165a34`, `1165a88-1165a9c`).

Nested summary occupies fp-152 onward; the body consumes words +0 and +8 (`1165680-1165694`). Nested child occupies fp-164 onward; its +0 selection, +4 state and +8 resource-data are consumed (`1165754-1165760`, `11657b4`). Nested H writes only +0=node,+4=S[3],+8=selection,+32=first,+36=second,+44=selected,+48=W; the remaining H fields retain earlier values (`116587c-11658ac`). Nested D sets resource=W, x=20*(first-1), y=24*(second-1), geometry=0, +20=0,+24=0 (`11657e0-1165830`). First starts low16(arg5); nested steps wrap modulo 65536 (`1165704`, `1165a40-1165a64`). The remembered/current index bytes subtract the count of hidden records from the eight-bit raw offset relative to scroll top (`116570c-1165718`, `1165764-116576c`, `1165794-11657b0`). The format/hook redraw does not modify side state +0/+8.


All 31 distinct BL targets in the 3,204-byte body: `43550`, `61ce1c`, `61cd14`, `7221c8`, `bf8e64`, `bf8d20`, `11730dc`, `4d978`, `4da6c`, `bf9360`, `1112b2c`, `1163e04`, `1112da4`, `1113514`, `116dce0`, `1112e78`, `1177e10`, `1163c1c`, `111236c`, `1112d84`, `116cf50`, `1112ce4`, `bf93e0`, `116e264`, `1137cf8`, `1138060`, `bf8f4c`, `bf90f0`, `116f948`, `116d4e4`, `1159168`. There are no register BLX sites. Every site is covered above; nested-only targets remain declared but unused by owned flat cases.

## Static instruction scan

The complete body has ADD (including conditional and shifted operands), AND, B/BL, CMP, LDR/LDRH/LDRSH, LSL, MLA, MOV/MOVW/MOVT, MVN, PUSH/POP, RSB, STR/STRB/STRH, SUB, SXTB, TST, UBFX, UXTB, UXTH. Conditions include EQ/NE/HI/HS/LO/LS. Signed halfword loads include immediate negative offsets at `1165268`, `1165280`, `11657ec`, `1165808`. No A32 divide instruction, coprocessor, exclusive, exception, VFP or device instruction appears. Admission must cover these exact forms before host qualification.

## Host qualification

The stock body and owned A32 entry passed the same 63 contract-derived cases. Every stock and owned report has `status=passed`, `runner.status=returned` and empty `mismatches`. No expected service event or report/side-state byte changed after execution. Reports are `private-data/analysis/x-m1-1.50/native-case/tree-list/*.json.report` for stock and `tree-list-owned-final/*.json.report` for owned. Cases bind all initial artifacts and executable bytes by SHA256. All 31 callees are declared scripted stubs; record reads are checked relative to the current argument pointer, not a stock private-frame address.

| Image | Passed | Rejected | Private frame excluded | Binary SHA256 |
| --- | ---: | ---: | ---: | --- |
| Owned | 63 | 0 | 328 | `bfe4a0cc3f734a208a3255434970a31d6d59261b3c01e4a630a515eafc16e91f` |
| Selected unstyled | 28 | 35 | 288 | `3244534c31b599364a1911524172551ccb5c13a5e04a4b5739296486d5dc2c1c` |
| Page off by one | 0 | 63 | 320 | `b4f38455da6b80adb90e454bb187b1c776aa9d6685cd26e809703f626bbd1680` |
| Side-state font flag wrong | 2 | 61 | 328 | `9c4831f43a86b21cfb7fa073c3fee9338a84f85e4c2daad916531fab607f47fa` |

Each control ran all 63 cases and returned nonzero from the qualifier. None was an `invalid_case`. Reports are in `tree-list-control-{selected-unstyled,page-off-by-one,side-state-wrong}-final/`. The side-state control passes only the empty and all-hidden lists. Selection outside the drawn page leaves the selected-unstyled control indistinguishable in those cases. The page control changes page selection, not nonexistent flat-list tab draw calls. The picker controls cover the scroll, hidden-row, disabled-row and report-`+0x3e` mutations in the shared R1 acceptance set.

The excluded private spans end at entry SP and do not cover any caller argument. They come from the emitted entry PUSH plus local allocation plus the deepest internal helper PUSH: owned `36+276+16=328`, selected-unstyled `36+236+16=288`, page-off-by-one `36+268+16=320`, side-state `36+276+16=328`. Internal clearing helpers pop before tail transfer. Stock excludes its 352-byte frame. This is a native harness scratch comparison rule, not a UILIB task-stack qualification.

`//replacement/menu_widgets:tree-list-tests` passes five behavioral tests; `//emulation:tree-list-cases-tests` passes its contract-generator assertions. The four A32 images build with the bank's `armv7r-none-eabi` toolchain. The native package build passes all 82 `native_case` Rust tests, including exact negative-immediate LDRSH sign-extension words. No ISA admission widening was needed. Scripted stubs and private-frame exclusions are shared native-case features with public fixtures, not proprietary test implementations.

```sh
devenv shell -- bazel --output_base=$HOME/.cache/bazel/latent-image-RenderTreeR1 test --jobs=2 //replacement/menu_widgets:tree-list-tests //emulation:tree-list-cases-tests //emulation:tree-list-cases
devenv shell -- bazel --output_base=$HOME/.cache/bazel/latent-image-RenderTreeR1 build --jobs=2 //emulation:native-case //emulation:native-runner //replacement/menu_widgets:tree-list-case-image //replacement/menu_widgets:tree-list-control-selected-unstyled-image //replacement/menu_widgets:tree-list-control-page-off-by-one-image //replacement/menu_widgets:tree-list-control-side-state-wrong-image
devenv shell -- bazel-bin/emulation/tree-list-cases --image private-data/analysis/x-m1-1.50/decode/img6-01100000.bin --output private-data/analysis/x-m1-1.50/native-case/tree-list --runner bazel-bin/emulation/native-runner
devenv shell -- bazel-bin/emulation/tree-list-cases --image private-data/analysis/x-m1-1.50/decode/img6-01100000.bin --output private-data/analysis/x-m1-1.50/native-case/tree-list-owned-final --candidate bazel-bin/replacement/menu_widgets/tree-list-case-image.bin --symbols bazel-bin/replacement/menu_widgets/tree-list-case-image.symbols --frame-bytes 328 --runner bazel-bin/emulation/native-runner
```

For each control, the same emitter command used its corresponding `tree-list-control-NAME-image.bin/.symbols`, the frame size in the table and output `tree-list-control-NAME-final`. The expected semantic events and memory stayed unchanged.

The first candidate emission was refused before execution because the generator put `ignored_memory` at the root rather than inside `expected` and omitted executable caller-return bytes. Those metadata were corrected without changing expectations. Non-threaded emitter invocations blocked on child-process I/O and were cancelled; the final emitter links `-threaded`, matching the existing native adapter convention.

This establishes owned flat/live loop behavior behind scripted stock-service boundaries. It does not establish drawing-leaf implementation, actual menu accessor behavior, UI task stack headroom, package-node ownership, saved snapshots, unowned tree-kind loops, RAM-push installation, guest composition, device timing or camera behavior. No adapter or entry seam is installed by this work.
