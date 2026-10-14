# X-M1 1.50 physical input map

## 1. Scope and integrity

**Hardware-observed:** the owner's act-logged session is `private-data/sessions/20261009T064945Z-read-log`, paired with the filled note sheet in [input-map-session-script.md](input-map-session-script.md#3-the-note-sheet). The body showed a black screen and green LED throughout the recorded acts; the flash physically popped up. This analysis is host-only. E-PRESS-MAP-150 records the result. **Byte-read** labels captured or firmware bytes; **static-inference** labels their interpretation; **unproved** labels unresolved attribution or effects.

**Byte-read, integrity PASS before interpretation:** all 17,097 journal lines and 3,104 outgoing lines parse as version-1 camera tuples. The 4,140 dispatched transfers equal the fixed `FujiAF.Log.logRequests` plan in kind, bytes, lengths and order, including C0, gate enable/readback, 1,030 debug half-reads, gate restore/readback and C4. Every native outcome has status 0 and its requested count. Every outgoing `bytes=` field agrees with its bracketed bytes and the plan. All 1,030 accepted half-replies decode against their requested addresses and equal their 128-byte captured slices. All 515 window hashes and both gate-file hashes match the files. `read-log` journals hashes per 256-byte window, not per half-request; the per-request check is reply-to-slice equality. There is one `515 windows` completion and no refusal or uncertainty record.

**Byte-read:** the two 256-byte gate readbacks are the known pair from the earlier captures: enabled SHA-256 `2264fcfdb55e94372ae267100ef670066e48f64f76101d00b100ea078efa0c4b`, restored SHA-256 `409d56adca05fb4f8d2353f152a60c53b2904bb806ab18076ede6f62f6af98a3`, with index `0xf7` reading 1 then 0. There is no transport-integrity deviation.

**Byte-read:** the ring capacity is 8,188; its captured header next index is 1,212, while the rolling read contains 1,219 contiguous occupied slots, 0 through 1,218, followed by zeros. The twelve bytes beyond the store are zero. **Static-inference:** the seven-slot difference is consistent with traffic appended after the header was read, not wrap or missing early acts. All named acts precede the read. Of the 1,219 typed records, 1,162 are type `0x0f`; that type is a multiplexed event record, not a guarantee that the packed field is a physical input code.

## 2. Main correction: the ring has several input sources

**Byte-read:** the shutter press edges are present in this ring, and also in the earlier jig-A capture. They were absent only from the earlier mining's keytimer view. The entry combo and POWER also have press-form state publications on other threads. The prior statement that those press forms lie outside the ring is withdrawn. The ring does not preserve a physical edge that happened before its boot clear, but it does publish the resulting held state after the clear.

Select type `0x0f` first, then the thread and emitter signature in `arg`, before interpreting `slot+0x0a` as an input code. Slots below are zero-based; the code is the upper halfword of `slot+8`, parameter the lower halfword, and tick the word at `slot+0xc`. **Byte-read:** input publications in this capture have parameter 0 and these signatures:

| Thread | Publication scope in this capture | `arg` | Evidence |
| --- | --- | --- | --- |
| `0x25`, keytimer | Scanned button events and releases, both dial rotations, pop-up state, mode-detent changes | `0x00002501` for ordinary events, `0x00002507` for mode changes | 60 entries, including the 13-event early prefix |
| `0x00`, unnamed context | Shutter S1/S2 press forms during the two shutter acts | `0x00000001` | slots 1132, 1134, 1135 |
| `0x3b`, initcont | Early condition/state publication | `0x00003b01` | slots 12-16; other type-`0x0f` initcont records have different `arg` meanings |
| `0x02`, syscont | Later startup state publication, including the held entry combo, POWER, AUTO and USB | `0x00000201` | slots 1013-1018 at tick 1095; additional early state sets at ticks 344 and 658 |
| `0x07`, usbcont | USB publication and copies of mode changes | `0x00000701` | slot 1029 and the twelve mode-walk copies |

**Static-inference:** an `ON`/press-form state publication is not proof of a fresh physical rising edge. Neither a missing code in one thread nor a duplicate code in another proves a missing act or a repeated act. `RingCheck.hs`'s keytimer press/release counts and same-code runs are thread-scoped. Its earlier conclusions about release-only shutter/POWER/entry-combo events, mode-only counts, unnamed codes and the absence of repeats cannot be promoted to whole-ring claims without the other sources.

**Byte-read, prior-capture cross-check:** in `20261008T045338Z-dump-ram-high-known`, thread `0x00` publishes S1 at ticks 2614 and 2857 and S2 at 3036; initcont publishes POWER at 31; syscont publishes POWER at 4013 and S1/S2/POWER at 107147. The earlier release-only classification missed those records. That older capture still has no act log, so its physical acts remain unattributed.

## 3. Alignment by order and tick gaps

**Hardware-observed:** note-sheet times are minute-resolution labels, not a synchronized tick origin. Their heading also uses a different clock notation from the act rows. **Byte-read:** tick samples are 72436, 72461 and 87394; the first-to-last completed-window interval is 149.759494172 seconds for 14,958 ticks. **Static-inference:** the long-baseline rate is about 99.88 ticks/second, close to 100. Relative order and gaps, not an assumed UTC start, align the acts. The held-entry releases occur at 1287, the first OK at 4072, Fn at 17087, the push at 22342, shutter acts near 28700/30200, the held OK near 31700, the walk near 41500-43900, and the main dial near 65600. Their long pauses match the recorded progression from 12:37 to 12:48; the read follows near tick 72436.

The following is **byte-read** for slots/codes/ticks and **hardware-observed** for the act labels. The correspondence is **static-inference** from the ordered acts, distinct codes and gaps, independently checked against the firmware names in section 4. `P/R` is press-form/release-form. Unless a thread is specified, the entry is from keytimer `0x25`.

| Note row / act | Recorded time | Entries: slot, code, tick | Result |
| --- | --- | --- | --- |
| 1, entry combo | 12:37 | syscont 1013-1018: `0007,0008,0006,0002,0302,0010` at 1095; releases 1075 `0088`, 1076 `0087`, 1077 `0082`, all at 1287 | Held S1/S2/UP state is published before its three releases; POWER/AUTO/USB are in the same state set |
| 2, OK | 12:38 | 1083 `0001` @4072; 1087 `0081` @4104 | One P/R pair, 32 ticks |
| 3, UP | 12:38 | 1088 `0002` @6457; 1089 `0082` @6484 | One P/R pair, 27 ticks |
| 4, DOWN | 12:39 | 1093 `0003` @8004; 1094 `0083` @8032 | One P/R pair, 28 ticks |
| 5, LEFT | 12:39 | 1095 `0004` @9439; 1096 `0084` @9462 | One P/R pair, 23 ticks |
| 6, RIGHT | 12:39 | 1100 `0005` @10882; 1101 `0085` @10904 | One P/R pair, 22 ticks |
| 7, DISP/BACK | 12:39 | 1102 `0009` @12472; 1103 `0089` @12502 | One P/R pair, 30 ticks |
| 8, PLAY | 12:40 | 1107 `000b` @14249; 1108 `008b` @14277 | One P/R pair, 28 ticks |
| 9, Q | 12:40 | 1109 `0032` @15774; 1110 `00b2` @15804 | One P/R pair, 30 ticks |
| 10, Fn | 12:40 | 1114 `0028` @17087; 1115 `00a8` @17124 | One P/R pair, 37 ticks |
| 11, sub-command push | 12:41 | 1122 `002b` @22342; 1123 `00ab` @22367 | One P/R pair, 25 ticks |
| 12, sub-command direction A | 12:41 | 1124 `002d` @24830 | One rotation pulse, no release |
| 13, sub-command direction B | 12:41 | 1128 `002c` @26004 | One rotation pulse, no release |
| 14, shutter half | 12:41 | thread `00`: 1132 `0007` @28699; keytimer 1133 `0087` @28817 | One S1 P/R pair across two sources, 118 ticks |
| 15, shutter full | 12:42 | thread `00`: 1134 `0007` @30233, 1135 `0008` @30236; keytimer 1136 `0088` @30259, 1137 `0087` @30262 | Four entries: S1 and S2 P/R pairs, not just S2 |
| 16, OK held five seconds | 12:42 | 1141 `0001` @31654; 1142 `0081` @32237 | One P/R pair, 583 ticks; no intervening record |
| 17, flash lever | 12:43 | 1143 `001c` @33532 | One POP event, no `009c` release; mechanical pop-up observed |
| 18, MOVIE | 12:43 | 1147 `0034` @37044; 1148 `00b4` @37079 | One P/R pair, 35 ticks; control exists and registers |
| 19, mode walk | 12:44 | Twelve keytimer entries at 41467-43897, each followed by a usbcont copy at the same tick; section 4 lists them | 24 ring entries for twelve changes; no release forms |
| 20, main command A then B | 12:48 | 1203 `001e` @65631; 1204 `001d` @65803 | One pulse per direction, no releases |
| 21, host read | 12:50 | Tick samples and transport journal, section 1 | Reads the preceding acts; its traffic also occupies ring slots |
| 22, supply cut | 12:52 | None in this capture | Occurs after the read; absence is not an input-scan result |

**Byte-read:** every exercised control act has at least one entry. The starting AUTO detent at row 19 creates no additional change entry; its held-state publication is at tick 1095 and returning to AUTO produces `0302` at 43897. Flash and rotations have fewer records than a button pair. Full shutter has four records rather than the script's S2-only expectation. The mode walk has twelve primary records plus twelve forwarded copies, not twenty-four physical changes. Landed-position gaps are 167-330 ticks, so the suggested three-second pause was not a measured constant. The named acts contribute 59 input publications across the selected sources: 44 keytimer, three shutter press records and twelve mode copies. No named control act produces an unexplained extra event.

### Startup values and the unlogged prefix

**Byte-read:** initcont publishes ADAPTER `000f`, LENS_DET `0038`, POWER `0006`, FUNCTION `0028` and MANUAL `0312` at tick 31. Syscont publishes AUTO `0302` at tick 1095, together with the owner's held entry keys, POWER and USB. The twelve-position walk subsequently binds M to `0312` and AUTO to `0302` in the recorded order.

**Byte-read, both publishers:** these are kernel message-trace records, not a separate input log. The store's type-`0f` records have one writer, loader A's `0x41c8c`, whose only caller is `FUN_00726478` (E-AF-TRACE-MECH-150); the word at record `+4` is `{receiver, sender, message id}` and the byte at `+1` is the writer's own task. The five tick-31 records read `{01, 3b, 0000}` (receiver 1, sender initcont) and the six tick-1095 records `{01, 02, 0000}` (sender syscont), each carrying one condition code in the record's code field. Both batches therefore publish the same quantity in the same encoding, from two different tasks.

**Byte-read, the dial:** no mode code is a discrete input. Rows 24-35 are analog ranges over ADC channel 3, with windows held in the adjustment cache (`0x3700`-`0x372e`) and read live through `FUN_0069fec8`: AUTO `[0x11c, 0x169]` for row 27 (`0302`), M `[0x368, 0x3cb]` for row 34 (`0312`). `FUN_006a0a78` keeps only the first true row of a run that shares a class byte, so one channel value selects one mode row. The two windows are disjoint, so no single channel value produces both codes: the dial reading changed between tick 31 and tick 1095 while the dial itself did not (hardware-observed note sheet).

**Static-inference:** the tick-31 code is an early, pre-operational reading of the dial, not a stored default and not a second quantity. The same batch's ADAPTER record proves the adjustment cache was already loaded: `FUN_006a16bc` writes that row's lower threshold from cache `0x38a << 2` and sets `0xffff` (a row that can never be true) when the byte reads zero, so a zeroed cache could not fire the row. The windows in force at tick 31 were the body's own calibrated windows, and the M code then requires a channel-3 value inside M's window, which the dial never had in this session. **Unproved:** which mechanism produced that value. The result registers at `0xfff8a030 + channel*4` are read-only for the CPU (the only reader is `FUN_006495b8`; the only writer found in the images programs the two 14-word channel-config blocks at `0xfff28088`/`0xfff280c0` through `FUN_00688730`), so a channel map that was not yet programmed and a result register that had not yet updated both fit the bytes, and this capture does not separate them.

**Byte-read, the session's other dial records:** `0312` appears once in the whole capture, in the tick-31 batch, and no release form `0392` appears at all; the later `0312` at tick 42867 is a keytimer mode-change record (`{07, 25, 0000}`), not this row's level event. The owner-confirmed AUTO reading is the syscont record at tick 1095.

**Byte-read:** thirteen keytimer records precede the entry releases: `0200@80,00a8@167,0086@237,0028@342,0200@375,00b2@458,00a8@463,0086@551,0028@651,0200@690,00b2@780,00a8@790,0086@862` (slots 197, 246, 248, 375, 533, 561, 562, 564, 681, 859, 877, 878, 880). They use the same `00002501` emitter as the later buttons. Syscont also publishes POWER/Fn/Q press forms at tick 344 and again at 658, and initcont supplies POWER/Fn press forms at 31. Thus Fn/POWER/Q are not whole-ring release-only here. `0200` is the debug table's `HVS_0` name, not a demonstrated extra owner act.

**Static-inference:** this is the earlier mining's same keytimer-release/state-publication class, now exposed as a multi-source startup prefix, not new evidence of thirteen physical presses. The boot clear is earlier than these retained startup records; it is not a boundary at the entry combo's release. The stored prefix does not show another loader clear, and the repeated publications do not establish extra power cycles. **Unproved:** which startup scan/reinitialization paths produce the repeated Fn/Q/POWER states. They cannot be assigned to undocumented owner acts, discarded, or used to shift the later act alignment.

## 4. Code-to-control map and firmware comparison

**Byte-read:** image 4's input/condition table is 36 rows at `0x00828120`, 28-byte stride. The named-key table containing the S1 row at `0x007a01b0` is a different table: its named run begins at `0x007a018c`, uses 12-byte rows `{logical event id, input code, name pointer}`, and ends with `UNKNOW_KEY` at `0x007a09f0`. Names below use its input-code field, not its logical-event-id field. For example logical event `0601` means `KEY_DIAL_UP` with input code `001d`; condition-row code `0601` is not that dial code.

The ordinary-control code map is the alignment table above. Its firmware confirmation is **byte-read**, and its physical binding is **hardware-observed** through that alignment:

| Physical input | Input code | Firmware name | 36-row index | Pairing |
| --- | --- | --- | ---: | --- |
| OK | `0001` | `KEY_OK` (`EXE_M` in debug text table) | 18 | P/R, including hold |
| UP / DOWN / LEFT / RIGHT | `0002 / 0003 / 0004 / 0005` | `KEY_UP / KEY_DOWN / KEY_LEFT / KEY_RIGHT` | 14 / 15 / 16 / 17 | P/R |
| DISP/BACK | `0009` | `KEY_DISPBACK` | 20 | P/R |
| PLAY | `000b` | `KEY_PLAY` | 12 | P/R |
| Q | `0032` | `KEY_Q` | 19 | P/R |
| Fn | `0028` | `KEY_FUNCTION` | 13 | P/R for its named act |
| Sub-command push | `002b` | `KEY_CMDL_PUSH` | 22 | P/R |
| Sub-command A / B | `002d / 002c` | `KEY_CMDL_RIGHT / KEY_CMDL_LEFT` | Not rows | Pulse only |
| Shutter half / full stage | `0007 / 0008` | `KEY_S1 / KEY_S2` | 5 / 6 | P/R across thread `00` and keytimer; full also crosses S1 |
| Flash pop-up | `001c` | `KEY_FLASH_POPUP` | 7 | POP only in this act |
| MOVIE button | `0034` | `KEY_MOVIE_REC` | 21 | P/R |
| Main command A / B | `001e / 001d` | `KEY_DIAL_DOWN / KEY_DIAL_UP` | Not rows | Pulse only |

**Static-inference:** both dials register rotation in maintenance mode. The `CMDL` push/left/right family belongs to the manual's sub-command dial, not its non-push main command dial. That contradicts the original script's literal main-command-push label, not the firmware's code values. Main direction A maps to the table's DOWN and B to UP; sub-command A maps to RIGHT and B to LEFT. The note records A/B, not clockwise/counterclockwise, and this session displays no edited value. Physical clockwise sense and ordinary-operation increase/decrease effects remain **unproved**. No firmware numeric or control-name binding is contradicted by the acts.

The mode walk is **hardware-observed** for the recorded detent order and **byte-read** for the following entries. Every firmware name agrees, including the owner's action/sports detent, C as C1, and SR+ as SR_AUTO:

| Recorded detent | Code | Keytimer slot / tick | usbcont copy slot | Row | Firmware name |
| --- | --- | --- | ---: | ---: | --- |
| AUTO, startup held state | `0302` | syscont 1017 / 1095 | No same-tick copy | 27 | `KEY_AUTO_MODE` |
| Adv. | `031c` | 1155 / 41467 | 1156 | 28 | `KEY_ADVANCE_MODE` |
| SP | `0304` | 1157 / 41797 | 1158 | 29 | `KEY_SP_MODE` |
| Action | `0309` | 1159 / 42002 | 1160 | 30 | `KEY_SP_SPORTS` |
| Landscape | `0308` | 1161 / 42222 | 1162 | 31 | `KEY_SP_LANDSCAPE` |
| Portrait | `0307` | 1163 / 42427 | 1164 | 32 | `KEY_SP_PORTRAIT` |
| C | `0317` | 1165 / 42647 | 1166 | 33 | `KEY_C1_MODE` |
| M | `0312` | 1167 / 42867 | 1168 | 34 | `KEY_MANUAL` |
| A | `0310` | 1172 / 43127 | 1173 | 35 | `KEY_APERTURE` |
| S | `030f` | 1174 / 43294 | 1175 | 24 | `KEY_SHUTTER` |
| P | `030e` | 1176 / 43502 | 1177 | 25 | `KEY_PROGRAM` |
| SR+ | `0306` | 1178 / 43702 | 1179 | 26 | `KEY_SR_AUTO_MODE` |
| AUTO, return | `0302` | 1180 / 43897 | 1181 | 27 | `KEY_AUTO_MODE` |

### What the 36 rows still do not bind

**Byte-read and static-inference:** 26 of 36 rows are bound by deliberate control acts: fourteen button/stage/pop-up rows and twelve mode rows. Three further rows have named boot-state observations, POWER (`0006`, row 11), ADAPTER (`000f`, row 4) and LENS_DET (`0038`, row 9). They are condition/state bindings, not independent toggle experiments. This session therefore reaches 29/36 row codes, with 26 act-bound and seven unreached. The four rotation codes and USB `0010` lie outside this table; do not count them as extra bound rows.

Each row's own bytes fix what it reads, so the seven are now answered rather than left open. The row layout is `{code, flag word, {class byte, kind byte}, operand, lower/upper, timing, counter}`; `FUN_006a0888` evaluates kind 0 as an analog range over the live channel table `DAT_0096b570`, kinds 2 and 4 as a port bit, and kind 5 as a callback compared with the upper field. The channel table is filled by `FUN_006a03a8` from the result registers at `0xfff8a030 + channel*4` through `FUN_006495b8` (`0x006495b8`, address built at `0x6495f4` as `movw #0x280c`/`movt #0x3ffe` then `lsl #2`). The shared port-bit form is `0xfff89014 + bank*0x40`, bit `operand & 0xf`, bank `operand >> 16` (byte-read at `0x6a08c4-0x6a08e4`).

| Unreached row | Address | Code | What the row reads (byte-read) | Answer |
| ---: | --- | --- | --- | --- |
| 0 | `00828120` | `0601` | Kind 0, channel 5, thresholds `[0x231, 0x264]` in the 2026-10-08 live dump | A power-source voltage band, not a control |
| 1 | `0082813c` | `0602` | Kind 0, channel 5, thresholds `[0x219, 0x230]` | A power-source voltage band, not a control |
| 2 | `00828158` | `0603` | Kind 0, channel 5, threshold `<= 0x218` | A power-source voltage band, not a control |
| 3 | `00828174` | `0600` | Kind 0, channel 5, lower threshold `0x3fc` with upper `0x3ff` | A power-source voltage band, not a control |
| 8 | `00828200` | `0011` | Kind 2, port `0xfff89314` bit 7, asserted at 0; state 1 with the card in | A body input the session did not try |
| 10 | `00828238` | `0039` | Kind 5, callback `0x006a00f8` → `0x006c2c08`, compared with 1 | Not a physical control |
| 23 | `008283a4` | `0035` | Kind 2, port `0xfff892d4` bit 2, asserted at 1; state 0 in the dump | A body interface the session did not try |

**Byte-read, rows 0-3:** the four rows are the only members of their class (class byte 6) and carry no key-table name; `FUN_006a0b4c` resolves them by code and `FUN_006a16bc` writes their thresholds at startup from the adjustment cache: index `0x3a9` then `0x386` for `0600` (minimum of the two only when cache `0xcf` bit 1 is set), cache `0x384/0x383/0x385` for `0601/0602/0603` through the three-row index table at `0x007cb130`, selected by `DAT_0089f7c0` (`FUN_006a1ad0`, values 0-2). The 2026-10-08 image-2 dump holds the configured result. **Static-inference:** channel 5 is the body's power input, not a control line: `FUN_006a1e5c` mirrors it into `DAT_0096cfec`, the firmware-upgrade path `FUN_00ec797c` compares that mirror against cache `0x38a << 2` as its AC-adapter test, `FUN_006b6844` debounces the same channel against a threshold `<< 2` for the power-loss flag, and `FUN_00d82178` turns the four band states (plus the ADAPTER row) into the one-to-three level value the UI publishes. **Unproved:** the physical quantity on channel 5 (rail voltage or an accessory-sense divider) and what each band means.

**Byte-read, row 8:** `KEY_CARD_DET` is the firmware's own name for code `0011` (name pointer `0x00780f5c` in the table at `0x007a018c`). Its port bit has two other readers, `FUN_00ec78c8` in the update path and `FUN_006eddc0`. **Static-inference:** the owner's card stayed in its slot for the whole session, so the condition read 1 and never changed; ejecting or reinserting the card is a physical act the session did not perform.

**Byte-read, row 10:** the callback's body is `0x006c2c08` read from the disassembly: it returns 1 only when the lens module's state cell `DAT_0096aff4` reads 5 and bit 7 of port `0xfff89014` is clear, and 0 otherwise; the row's upper field is 1, so the condition *is* that state. The lens shutdown `FUN_006c244c` raises the off-form event `0xb9` when the same cell read 5. The dump holds state 0. **Static-inference:** the row is the lens-interface power-up state, not a user input; nothing a button press does can assert it.

**Byte-read, row 23:** `KEY_FLASH_EXT` is the firmware's name for code `0035` (name pointer `0x00780fa8`). The port bit has one other reader, `FUN_0069ef1c`, which is gated on exactly that bit and then reads the card file `C:\ff_ext_f.bin` (string `0x00781984`, with `flashext` adjacent); no other code in images 4, 5 or the plain image reads it. **Static-inference:** the row is the external-flash attach input. The body has a hot shoe; whether the contact that drives this pin is wired on this body is **unproved**, and nothing was mounted on it in the session, which is why the condition read 0.

**Static-inference:** against the three possibilities, four of the seven rows read a power-source band and one reads an internal lens state, so five are not physical controls at all; the other two (card detect and external flash) are inputs this body has and the session did not try; none of the seven is a control the body lacks, and no one of them is a missing body button. The body has no dedicated Home or EVF/eye-sensor control, but their generic key-table names are not rows in this 36-row scan table.

**Byte-read:** the generic named-key table also contains controls the session did not exercise. Outside the two interface rows above, its unreached named button/control families are ZOOM_TELE/WIDE (`0013/0014`), FMODE (`000a`), AF (`0017`), CONTINUOUS (`001b`), METER (`021d`), EXPOSURE (`0012`), MACRO (`0212`), INFO (`021c`), ONEPUSH_AF (`0022`), AE_LOCK (`0025`), EVF_LCD (`0026`), HOME (`021b`), PS (`0213`), BARRIER_OPEN/CLOSE (`000d/000e`), FACEDETECT (`0211`), ISO (`0027`), ZOOM_TELE2/WIDE2 (`0015/0016`), WB (`002e`), AEAF_LOCK (`0031`), AF_CSM (`002a`), EV (`0218`), EYE_SENSOR (`0033`), WLAN (`0037`), FOCUS_AF/MF/CAF (`001f/0021/0020`), ring states/movement, zoom-ring and wheel-dial families (`0204-020a`, `0023/0024`, `0101-0109`, `020b`), METER_SPOT/MULTI/AVE (`020d/020e/020f`), AE (`002f`), VIEW (`0217`), RAW (`0030`), EVF_OVF (`0029`), DRIVE (`0018`), shutter-dial/exposure-dial families (`031e-0338`) and iris/focus-distance families (`0400-041f`, `0424-0427`, `0500-0506`). The generic table's non-body modes MOVIE, NIGHTVIEW, NATURALPHOT, BLURRE, SETUP, AS, NATURFLA, SP2, C2, FSB, EXR, PANORAMA, C3 and PLAY/REC mode names are not additional physical detents reached by the twelve-position walk. CMOS-temperature state names are also not physical controls. **Unproved:** a dedicated physical source for any generic name not bound above; generic vocabulary alone does not add hardware to this body.

## 5. Held-control answer and instrument bounds

**Byte-read:** held OK emits `0001` at tick 31654 and `0081` at 32237. The slots are consecutive, 1141 and 1142, so there is no intervening record from any thread, no repeat and no periodic held event. The gap is 583 ticks, about 5.84 seconds at this capture's measured long-baseline rate, versus 22-37 ticks for the ordinary button taps. **Hardware-observed:** the owner intended a five-second hold. **Static-inference:** this exercise confirms edge/state-change reporting without hold repeats for OK in this maintenance session. It does not prove every control, every operating mode or the downstream UI is repeat-free.

**Static-inference:** this instrument binds controls to codes and records state publications, durations and forwarding. It does not report the effect of a button in ordinary photography, menu values, settings persistence, or a settings-preserving ending. Flash latches mechanically, so lack of a POP release in its act is not absence of the flash lever. The note confirms a supply cut after the capture, not a firmware clean-exit path. Every future input read must include all relevant emitter/thread views and distinguish physical edges, startup-held states and forwarded copies.

## 6. Reproduction

The first command performs integrity checks before decoding any input and enumerates the pinned tables. The second reproduces the original thread-scoped census; it is not a substitute for the cross-thread view. Both read host files only.

```sh
devenv shell -- runghc -ibroker -itransport \
  private-data/analysis/x-m1-1.50/input-map/PressMapCheck.hs \
  private-data/sessions/20261009T064945Z-read-log \
  private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin
devenv shell -- runghc \
  private-data/analysis/x-m1-1.50/dump/RingCheck.hs quiet \
  private-data/sessions/20261009T064945Z-read-log
```
