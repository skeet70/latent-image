# AUTO-ACT-150: the AUTO_ACT.SCR script path on this body (1.50)

Host static analysis on the pinned official 1.50 images (id4 `id4-0061c400.bin` base 0x0061c400 SHA-256 `f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16`, id5 `id5-00bf1a00.bin` base 0x00bf1a00 SHA-256 `3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a`) and this body's three adjustment-area captures.
Labels are **byte-read** for bytes read from a pinned artifact, **static-inference** for a conclusion drawn from decompiled code or bytes, **hardware-observed** for captured camera state, **reported** for a sibling-camera or wiki claim used as a cross-check only, and **unproved** where the named fact is unavailable.
S22/S23 sibling claims (spec/09-sources.md) appear only as cross-checks against this body's bytes.

## 1. Question and verdict

The question is whether the `AUTO_ACT.SCR` path exists on this body, what reads it, when it runs, what it accepts, and whether it reaches code execution or is limited to a command vocabulary.

**Headline result: the facility exists in full on this body and is disabled as shipped.** The loader, parser and handshake executor are all present in the pinned 1.50 images (static-inference over byte-read anchors), but the card file is never opened, because the path's only enable gate is adjustment-cache byte `0xa2` equal to 2, and that byte reads `0x00` in all three captures of this body: the official-1.50 capture (20261008T175358Z-read-config, window `000000.bin`), the 2026-10-07 capture (20261007T060812Z-read-config) and the S03 1.00 capture (read-03) (byte-read, hardware-observed).
The route is one reviewed config write of `0xa2 = 2` away from live, not a card-only trigger, and it is not a RAM-write or code-execution route.

## 2. Anchors (byte-read)

The string at `0x00ed3ac4` (id5) is `A:\DCAA\auto_act.scr`, lowercase, and is referenced by one site: the `movw`/`movt` pair at `0xed1e18` inside the loader handler at `0x00ed1de0`.
The string at `0x00d92020` (id4) is `\AUTO_ACT.SCR`, 13 characters plus NUL, and is referenced by two `movw`/`movt` pairs, `0xd91a84` and `0xd91aa4`, both inside the record check at `0x00d91988`.
The lowercase path string and the uppercase name entry are distinct strings with distinct referencers; the former is the file path and the latter is the canonical base name of an object-format record.

The record check at `0x00d91988..0x00d91f4c` (file 0x19ff88..0x1a054c) takes a u32 index and a 60-byte record pointer (static-inference): it uppercases the record's 28-byte name field unless the index is 2, requires the record's `+0x20` word to be 0, then matches the record type at `+0x1c` against a name: `0x3002` maps to `\AUTO_ACT.SCR` (`0xd92020`), `0x3800`/`0xb103` to `\DSCF0000.RAF` (`0xd920b8`), `0x3801` to `\DSCF0000.JPG` (`0xd920a8`), `0xb802` to `\FPUPDATE.DAT` (`0xd91fe8`) and `0xc802` to `\XFUPDATE.DAT` (`0xd92098`).
On a mismatch it returns `0x80000000`; on a match it copies the record into the DCAA object table at `0x46b5612c + index*60` and, if the record's `+0x28` word is unset, stamps the date `20060101120000` (byte-read at `0xd91fc8`).
It is entry 19 of the 21-entry function-pointer array at `0xd920c8`, registered by the registry init at `0xd90968` (static-inference).
Format `0x3002` is the script container format: the registry accepts it only under the base name `\AUTO_ACT.SCR`, uppercased, and the registry subsystem is enabled by the active volume letter at its init `0xd90968` (static-inference).

## 3. The loader (static-inference)

The loader is `0x00ed1de0..0x00ed1f80` (file `0x2e03e0`), reached only as the handler for DCAA message `0x3302` (internal code `0xb`); nothing else enters it.
It does, in order:
1. Clears the script area: `memset(0x166704c, 0, 0x10764)` (the movw/movt pair at `0xed1e04`) before every load.
2. Builds the path from the 21-byte template at `0x00ed3ac4`, overwriting its first byte with the active volume letter from id4 `0x71f708`, forming `<volume>:\DCAA\auto_act.scr`; the literal `A` in the template is decorative, matching the byte-read conclusion the project already reached for every other path builder (E-SRECORD-ROUTE-150, inherited).
3. Opens the file through id4 `0x71c648` with callback `0x00ed1828`, which stores its two arguments at `0x16777b4`/`0x16777b8`.
4. Queries the size via id4 `0x71ce84` (read from `stack_info+0x54`); a size above `0x100000` (1 MiB) is rejected.
5. Reads the file into the fixed RAM buffer `0x43e6ff00` through id4 `0x71c6fc` with callback `0x00ed17d8`.
6. When the completion count equals the requested size, stores the byte count at `0x16777ac`, which is the parser's input-length word at base `0x166704c` + `0x10760`.
7. Calls the parser driver `0xed1dcc`, then sets `[0x166704c+4] = -1` and `[0x166704c+8] = 0` (statement index 0), sets the dispatch-table selector to 2, and sends DCAA message `0x3305`.

The read cap is 1 MiB; nothing smaller is checked.

## 4. The parser and its grammar (static-inference)

The parser is the tokenizer `0x00ed1bcc..0x00ed1cfc` and the code-and-label pass `0x00ed1d54..0x00ed1dc4`, driven by `0x00ed1dcc` (file `0x2e01cc` onward).
The grammar:
- Plain text. Whitespace (space `0x20`, tab `0x09`) separates tokens; `//` to end of line is a comment; lines end with CR, LF or CRLF.
- One statement per line: token 0 is the statement keyword and tokens 1..9 are arguments. Each token is copied into a 20-byte slot and truncated at 10 characters.
- Statement `n` is stored at `0x166704c + n*0xe0` (record stride `0xe0`, 224 bytes); its keyword text is slot 0 at record `+0x14`, argument `k` at record `+0x14 + k*20`, and the statement's resolved code goes to record `+0x10`. The statement count is written to `[0x166704c+0xc]`. The `0x10764`-byte cleared area holds 301 such records (`n = 0`..`300`, each `0xe0` bytes); the region's tail holds the variable array `0x16776dc` (26 words, `a`-`z`), the per-initial label table `0x1677744` (26 words, `A`-`Z`) and the length word `0x16777ac` (base + `0x10760`). Record 300 (`0x16776cc`..`0x16777ac`) overlaps both the variable array and the label table, and record 301 begins at the length word, so the practical bound is lower than 301: a script should stay under about 300 statements.
- The keyword lookup (`0xed1d00`) searches the first 13 entries of the table at `0xed2d28` by string comparison and returns the entry's u32 at `+0xc`, or 25 when nothing matches.
- The second pass stores each record's code at `+0x10` and, for LABEL records (code `0x10`), writes the statement index into the per-initial label table at `0x1677744` (26 words, one per initial `A`-`Z`), at the word for the label's first letter.

The 13 statement keywords, byte-read from the table at `0xed2d28`: `wait` `0xc`, `WAITSET` `0xd`, `if` `0xe`, `func` `0xf`, `LABEL` `0x10`, `key` `0x11`, `calc` `0x12`, `set` `0x13`, `jump` `0x14`, `random` `0x15`, `tp` `0x16`, `END` `0x17`, `WT_LOG` `0x18`. This list is identical to the sibling wiki's keyword list (S22), which is a cross-check, not evidence for this body.

**The missing bounds check.** Neither the statement counter nor the per-line token counter is compared against any limit anywhere in the tokenizer (static-inference). A file with more than about 300 statements, or with more than about 9 tokens on a line, writes parse output past the cleared `0x10764`-byte area, sequentially upward from `0x166704c`, 10 controlled bytes per 20-byte stride.
The earlier framing of this as an overrun past a `0x1764`-byte area was built on the wrong clear size and is corrected here: with the `0x10764`-byte clear, the statement overflow lands first on the script's own variable array and label table and then the length word. The consequence is bounded, not a memory corruption: the statement code that overflow corrupts is one the dispatch table does not map, so the walker stops at the `{0x1a, 0}` terminator, posts nothing, and the task re-enters the blocking receive and stalls quietly (static-inference, matching section 11's no-match behaviour). The sweep is confined to the script area above `0x166704c`, far above the image 4 and image 5 extents (base `0x0061c400` and `0x00bf1a00`), so it cannot touch the loaded images or their data structures. Its reach and effect are static-inference only (unproved in practice).

## 5. The executor and its handshake clock (static-inference)

The interpreter is handshake-paced, not free-running. The task loop at `0x00ed2c90` receives a DCAA message (id4 `0x725318`) and dispatches it: `0xed138c` reads the message's first u16 and maps `0x3300` to `0`, `0x3301` to `1`, `0x3302` to `0xb`, `0x3303` to `0xa`, `0x3305` to the current statement's code from record `+0x10`, and anything else to `0x1a` (null).
The dispatch tables hold 8-byte `{u32 code, u32 handler}` pairs and `0xed142c` walks the selected one for a code; the selector word at `0x16777c0` picks `0xed39e8` (selector 0), `0xed3c04` (selector 1) or `0xed3a00` (selector 2) through the triple at `0xed2cf4`. The selector is written only by this module, through `0xed15ec`: task entry sets 0 (`0xed164c`), the `0x3301` handler sets 0 (`0xed1634`), the arm path sets 1 (`0xed16e8`), `END` sets 1 (`0xed1610`) and the loader sets 2 (`0xed1f54`); section 11 gives the three table contents and the corrected roles (selector 0 is the pre-arm table and selector 2 is the statement table, not the other way round).
Message handlers: code 1 (`0x3301`) goes to `0xed1634`; code `0xb` (`0x3302`) goes to the loader `0xed1de0`; code `0xa` (`0x3303`) goes to `0xed1928`.
So the camera drives its own handshake: every pacing message is addressed to task id 35, the id the mujin task itself is created under, so the counterpart is that same task and no external `0x3301` or `0x3305` is needed to advance a loaded script (section 11, resolved 2026-10-09; static-inference over byte-read anchors). Send and receive go through id4's id-addressed message API (open `0x724cdc`, send `0x72503c`, receive `0x725318`).

The statement handlers: `wait` `0xed2924`, `WAITSET` `0xed28ec`, `if` `0xed2724` (an operator byte compared with `0x3d` `=`, jump table at `0xed27f0`/`0xed2818`/`0xed2840`), `func` `0xed1fe8` (a 17-arm jump table on a status name), `LABEL` `0xed16a0` (advance only), `key` `0xed1a98`, `calc` `0xed2648`, `set` `0xed2a00`, `jump` `0xed1f90` (label table to `[0x166704c+8]`), `random` `0xed24e8`, `tp` `0xed2548`, `END` `0xed1610`, `WT_LOG` `0xed28b4`. Every handler except `END` increments `[0x166704c+8]` and finishes by posting `0x3305` through `0xed1670` (byte-read at thirteen sites: `0xed16bc` LABEL, `0xed1ae0` key, `0xed1f5c` loader, `0xed1fb8` jump, `0xed22ec` func, `0xed2540` random, `0xed263c` tp, `0xed271c` calc, `0xed28ac` set, `0xed28e4` WT_LOG, `0xed291c` WAITSET, `0xed2954` wait, `0xed2af8` if). The file-using handlers bracket their I/O with `0xed1704` (the `0x16777b0` interlock) and `0xed178c` (reset).

The task is created by id5's own init chain: `0x00ed2b00`, called from `0x00d98660` inside `FUN_00d985a4`, sends an id4 op-35 request `{0x30, 0x0c}` and, only if it returns 0, creates a task named `mujin` (name pointer `0xed3adc` at `0xed2b38`, entry `0xed2ce0` at `0xed2b54`, priority 9, `0xc00` stack). The earlier `0x1a6c60` here was objdump's symbol-relative column (image base `0x00bf1a00` plus `0x1a6c60` is the call site itself) read as an absolute address, and the whole-image BL scan finds this one call site only. The sibling wiki's `mujin task` name is therefore bound to this body's code (static-inference).

## 6. The five gates, in order

1. **Task creation.** The mujin task exists only if the id4 op-35 request `{0x30, 0x0c}` at `0x00ed2b00` returns 0 (static-inference; corroborated in the field by the 2026-10-09 16:08 run, whose first baseline read already carried the prologue's store at `0x016777d4`, E-AUTOACT-160839-150).
2. **Enable byte `0xa2`.** The task's prologue reads adjustment-cache byte `0xa2` once, after `sleep(1000)`, before the receive loop (static-inference: a single prologue check, not a loop check). The read at `0x00ed2b78` is literally id4 `0x69f8fc(162)`, the same adjustment-cache u8 accessor the project already mapped for the `0xf7` gate (E-GATE-ENABLE-SEQUENCE, inherited). Only on equality with 2 does it sleep 4000 delay units (about 4 s) and call `0xed2c34`, which sends DCAA messages `0x3300` and `0x3302` once per boot behind the guard flag `0x1667048` and re-checks the byte non-zero at call time; `0xed2c34` itself only requires the byte to be non-zero, so a non-zero test and an equal-2 test both exist, with the equal-2 prologue the one that fires. `0xed2c34` has a second caller at `0x00d0cd10`, reached from the application event pump `0x00c22f60` when event id 9 (payload 0) dispatches, gated on `0x0096b477 != 0` and its own once flag `0x01467840`; `0x0096b477` is assigned once by the key-state init `0x72b8c4` as a boot-time copy of cache byte `0x265`, which reads `0x00` in every capture (byte-read), so that route is closed on this body as shipped. This is the enable byte the sibling sources call the EEPROM `0xa2` script flag (S23, reported), now read from this body's firmware rather than inherited.
3. **The `0x3002` name check.** Object format `0x3002` is the script container format; the registry accepts it only under the base name `\AUTO_ACT.SCR` (uppercased), the check at `0x00d91988` (byte-read, section 2). This is the same format code fujihack's PTP code uses with `FUJI_CREATE_FILE`/`WRITE_FILE` on siblings (reported cross-check).
4. **The file.** `<active volume>:\DCAA\auto_act.scr` must exist on the mounted volume (the SD card on this body per E-SRECORD-ROUTE-150, inherited) and be 1 MiB or less (byte-read cap in the loader).
5. **Body state; the decisive gate.** Adjustment index `0xa2` reads `0x00` in all three captures of this body (byte-read, section 1): the equal-2 branch at `0xed2cb0` never runs, no `0x3302` message is generated, the handler at `0xed1de0` is never entered, and the card file is never opened. The path is dead as shipped on this body.

## 7. Statement, key and func vocabularies (byte-read)

The statement vocabulary is the 13-keyword table of section 4, with the handler addresses of section 5.
The key vocabulary is the table at `0xed2df8` (16-byte `{name[12], u32 code}` entries), at least 101 entries with codes 1..`0x31e`: EXE_M, UP, DOWN, LEFT, RIGHT, POWER, S1, S2, BARR_OP, BARR_CL, BARRIER, CRADLE `0x214`, ADAPTER, USB, DOOR `0x210`, CARD, FMODE, EXP, METER `0x21d`, TELE/TELE2, WIDE/WIDE2, AF, VIDEO, HDTV, CONT, POP, C_DIAL +/-, W_DIAL +/-, MF_AF_A/C/M, 1P_AF+, F_RING +/-, Z_RING +/-, AE-LOCK, AEAF_LOCK, EVF_LCD, VIEW, K_HOME/K_PLAY/K_PS/K_Q, PLAY/REC/AUTO/QUICK/SP/SP2/SR, scene modes, PROGRAM, SHUTTER, APERTURE, MANUAL, C1-C3, FSB, ISO, ADVANCE and more. A second table at `0xed3a80` supplies the `ON` = 0 / `OFF` = `0x80` modifier that the key handler ORs into the code. Key presses are delivered as application events through id4 `0x72b680`, not as raw GPIO.
The func vocabulary is the 17 names at `0xed3ae4`, one jump-table arm each in `0xed1fe8`: DISP_ERR 0, MED_FULL 1, RD_ERR 2, WT_ERR 3, F_ERR 4, Z_ERR 5, F_Z_ERR 6, STANDBY 7, GLS_WAIT 8, P_MULTI 9, V_AVG `0xa`, PIX_STILL `0xb`, PIX_MJ `0xc`, GET_MODE `0xd`, GET_FILES `0xe`, L_DET_ON `0xf`, L_DET_OFF `0x10`.
The same module carries the DCAA message-id table `0xed2d00` (`0x3300` to 0, `0x3301` to 1, `0x3302` to `0xb`, `0x3303` to `0xa`, terminated by `0xffff`), a `{name,code}` table at `0xed3c24` (ON `0xb00`, OFF `0xb01`, MOVE `0xb02`, SWEEP `0xb04`, 2ON/2OFF/2MOVE/2SWEEP, MULTI `0xb0a`), a mode/function table at `0xed3600`-`0xed3760` (I-M_F11 `0x501`, I-M_POS30 `0x41f`, F_R_+4 `0x101`, SETUP, M_SEL, VOICE, JIG, EXR, VREC, MJPEG, SCEN_*, MMOD_*, APERT, POS0-POS30), and the text interface the project already mapped at `0xed19e4`, which shares the key table.

## 8. What the path can and cannot do

It can: synthesize input events (`key`), call the 17 named camera routines (`func`), assign the script's own variables (`set`/`calc`), delay (`wait`/`WAITSET`/`tp`/`random`), branch and jump within the statement array (`if`/`jump`/`LABEL`), and write a log file (`WT_LOG`, no operand; about 4 KiB into the active volume's DCIM tree through the frame-name builder).
It cannot write RAM or execute code through the script, with this bound (static-inference):
- No statement and no vocabulary entry carries an address: operands are names resolved in the fixed tables plus numeric literals.
- `set`/`calc` write the script's own variable array at `0x16776dc` (26 words, single-letter `a`-`z`, one word each); `jump`/`if` index the statement array only.
- The file read always targets the fixed buffer `0x43e6ff00` under the fixed 1 MiB cap.
- The only indirect calls in the module are the fixed dispatch-table entries (the `0xed142c` branch) and four fixed callbacks (`0xed1828`/`0xed17d8`/`0xed17b0`/`0x16777c8`); nothing loads a call target from file data, and nothing can execute the script bytes.
- The parse-output overrun of section 4 is the one write-shaped surface, and it cannot reach the image data (section 4).

## 9. Refuted leads

**Card-only trigger (the sibling framing) is refuted.** The sibling sources frame the auto-act script as a card-driven execution mechanism that a card file can trigger on its own (S22/S23, reported). On this body the card file is read only after the camera dispatches DCAA message `0x3302`, which the camera generates only behind the enable byte: when adjustment byte `0xa2` equals 2, with the mujin task present. The fujihack note that merely creating the file changes nothing is explained by this gate, not by an absent reader.
**RAM write and code execution through the script are refuted**, with the bound of section 8: no operand carries an address, assignments reach the script's own variable array, and the file always lands in a fixed buffer under a 1 MiB cap.
These refutations close routes the project would otherwise keep hoping for: the path cannot be activated by a card alone, and arming it takes one reviewed config write of `0xa2 = 2` (an H2-class operation requiring the owner's go), after which it runs the card script by itself, with no external counterpart (section 11), and its vocabulary has no memory write, so it does not reach the film-simulation demonstration.

## 10. Open questions

- **Resolved 2026-10-09 (section 11), the counterpart:** it is the mujin task itself, task/op id 35, and the handshake is a self-paced loop; none of the four ids reaches a jig, card or host protocol (static-inference over byte-read anchors). Ids `0x3301` and `0x3303` are constructed nowhere in image 4, image 5 or the plain image, so no producer for them exists in the pinned artifacts.
- **Resolved 2026-10-09 (section 11), the selector-2 state:** table `0xed3a00` is the statement table and holds all thirteen statement codes; the earlier reading that it lacked them was wrong. The loader's own `0x3305` at `0x00ed1f5c` triggers the first statement, so no external `0x3301` is needed to start execution.
- The record type codes `0x3800`/`0x3801`/`0xb802`/`0xc802`/`0xb103` and their canonical name pairs are byte-read but their formats are unbound.
- The parse-overrun's reach and effect are static-inference only (unproved in practice).
- The key-vocabulary table enumeration above is truncated at 101 of the entries present (the table continues past `0x31e`).

## 11. The handshake counterpart and the selector-2 state, resolved (2026-10-09)

The counterpart the binding analysis was looking for is the mujin task itself: every pacing message is posted to task/op id 35, the id the task is created under, and no other producer or consumer of these ids exists in the pinned images (static-inference over byte-read anchors).
The facility is id4's id-addressed message API: `0x724cdc` opens or attaches an id, `0x72503c` sends, `0x725318` receives with a timeout, and the caller's record is `{u16 id, u8 source, u8 destination, u32 length, u32 payload}` (byte-read: the field copies at `0x725144`..`0x725180` in the send and at `0x7253c0`..`0x7253f8` in the receive).
Ids 1..104 map to one of two classes through the table at `0x7cad9c` (all class 1 except 45, 46, 62, 65, 66, 88, 89, 90, 93 and 103, which are class 2); the class descriptor table at `0x96bd20` is only read, lies inside the boot-zeroed range `[0x89ec00,0xbf1a00)`, and is therefore runtime-filled, with its writer not located in image 4, image 5 or the plain image (byte-read reads, static-inference for the fill).
The selector triple is byte-read at `0xed2cf4`: selector 0 uses `0xed39e8`, selector 1 uses `0xed3c04`, selector 2 uses `0xed3a00`, selected by the word at `0x16777c0` through `0xed14c4`..`0xed14d4`.
Selector 0 (`0xed39e8`) is the pre-arm table: `{0 -> 0xed16e8, 1 -> 0xed1508, 0x1a -> 0}`.
Selector 1 (`0xed3c04`) is the message table: `{1 -> 0xed1634, 0xb -> 0xed1de0 loader, 0xa -> 0xed1928 file job, 0x1a -> 0}`.
Selector 2 (`0xed3a00`) is the statement table: `{1 -> 0xed1634, 0x10 -> 0xed16a0 LABEL, 0xd -> 0xed28ec WAITSET, 0xc -> 0xed2924 wait, 0x14 -> 0xed1f90 jump, 0xe -> 0xed2724 if, 0x13 -> 0xed2a00 set, 0x12 -> 0xed2648 calc, 0x11 -> 0xed1a98 key, 0x17 -> 0xed1610 END, 0xf -> 0xed1fe8 func, 0x15 -> 0xed24e8 random, 0x16 -> 0xed2548 tp, 0x19 -> 0xed16a0, 0x18 -> 0xed28b4 WT_LOG, 0x1a -> 0}`.
So the earlier reading of the three tables is corrected: selector 0 is the pre-arm table, not the statement table, and selector 2 (`0xed3a00`) does hold all thirteen statement codes plus `0x19` (the keyword lookup's no-match code 25) and the `0x3301` code 1 entry. The selector-2 state after a load is not a dead end, nothing needs to set the selector back, and the first statement needs no external message: the loader posts `0x3305` itself at `0xed1f5c`, immediately after `mov r0, #2; bl 0xed15ec`.
The binding is byte-read at `0xed2b00`: it calls `0x724cdc` with op 35 and the parameter pair `{0x30, 0x0c}`, then calls `0x725dd8` with the creation struct `{35, "mujin" at 0xed3adc, priority 9, stack 0xc00, entry 0xed2ce0, 0}`, and `0x725dd8` reads the struct's first halfword as the id whose class it looks up in the same op-to-class table.
Every send on this path is addressed to 35 with source 35: the `0x3300` sender `0xed2b8c` and the `0x3302` sender `0xed2be4` store 35 in the destination byte and the current id in the source byte and post through `0x72503c`, and the executor's helper `0xed1554` is called as `(35, 35, id)` by the loader and by every statement handler through `0xed1670`.
The id census over image 4, image 5 and the plain image (every MOVW/MOV immediate forming `0x3300`..`0x3305`, plus the 4-aligned halfword occurrences and the decimal renderings `#13056`..`#13061`): `0x3300` is constructed as a message id only at `0xed2ba8` (`mov r3, #0x33, lsl #8`), `0x3302` only at `0xed2c00`, `0x3305` only at `0xed1690` for sending and at `0xed1398` for the receive comparison; `0x3301`, `0x3303` and `0x3304` are constructed nowhere. Their only data occurrences are the id table `0xed2d00` and the tables `0x7cad9c`/`0xed2d00`-style lookups; the other immediates with the same low bits are address builds, for example `0xe64084` (`0x6023300`).
The sender gate is unchanged: the task prologue reads adjustment byte `0xa2` once after `sleep(1000)` and, on equality with 2, sleeps 4000 delay units (about 4 s) and calls `0xed2c34`, which posts `0x3300` then `0x3302` once per boot behind the guard byte at `0x1667048` and re-checks the byte non-zero at call time (a single prologue check, not a loop check; the second caller and its gate are in section 6).
Consequence for the facility, and the answer to whether a loaded script can advance without an external counterpart: it can. `0x3300` arms the engine (selector 0 to 1), `0x3302` loads the file and enters statement mode, the loader's `0x3305` executes statement 0, and each handler increments the statement index and posts the next `0x3305` until `END` (`0xed1610`: index -1, selector back to 1). No statement or handler waits on an external `0x3301`: the pending-callback slot at `0x16777c8` is only cleared (`0xed16d8`) and read (`0xed14f0`) in the module, and nothing in the images sets it, so the callback those messages carry is empty.
Host reachability: no code reachable from the USB, jig or card interface constructs any of the four ids, and the path's only external inputs are the adjustment byte `0xa2` and the card file, so as far as the pinned artifacts show the channel is internal. A producer for `0x3301` or `0x3303` would have to exist outside image 4, image 5 and the plain image (unproved, and it is not needed for the self-driven path).

Anchors for this section:

```sh
# the three dispatch tables, the selector triple, the task binding and the boot senders
xxd -s 0x2e12f0 -l 16  private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin   # {1, 0xed39e8, 0xed3c04, 0xed3a00}
xxd -s 0x2e1fe8 -l 24  private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin   # selector 0
xxd -s 0x2e2000 -l 128 private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin   # selector 2, all statement codes
xxd -s 0x2e2204 -l 32  private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin   # selector 1
xxd -s 0x2e1100 -l 96  private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin   # 0xed2b00 open(35,{0x30,0x0c}) and the task struct
xxd -s 0x2e118c -l 176 private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin   # 0xed2b8c posts 0x3300, 0xed2be4 posts 0x3302
xxd -s 0x1ae99c -l 420 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin   # op-to-class table 0x7cad9c, 105 entries

# the API and the executor
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0x0061c400 \
  --start-address=0x724cdc --stop-address=0x7254c0 private-data/analysis/x-m1-1.50/lz/id4.elf
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0x00bf1a00 \
  --start-address=0xed138c --stop-address=0xed1720 private-data/analysis/x-m1-1.50/lz/id5.elf
devenv shell -- llvm-objdump -d --triple=armv7-none-eabi --adjust-vma=0x00bf1a00 \
  --start-address=0xed1de0 --stop-address=0xed1f80 private-data/analysis/x-m1-1.50/lz/id5.elf
```

## 12. The first script and the ranked follow-ups (2026-10-09)

With the self-queue execution model of section 11, the smallest runnable file that can be distinguished from a not-parsed file by a host-side check is two statements: one observable effect, then `END`.

**The first script.** Its bytes are `WT_LOG` then `END`, each terminated by CR LF, 13 bytes, SHA-256 `bdd37b75ad3e2664ea8fc42fc9611d4fd761abde4fe487b0d357348beddd11e8`, placed at `DCAA\auto_act.scr` on the card root (the loader's template `A:\DCAA\auto_act.scr` at `0x00ed3ac4` overwrites the volume letter with the active volume, so the runtime path is `<active volume>:\DCAA\auto_act.scr`). Written with `printf 'WT_LOG\r\nEND\r\n' > <card>/DCAA/auto_act.scr`.
It parses to record 0 `WT_LOG` (code `0x18`), record 1 `END` (code `0x17`), and a statement count of 3 (the parser stores one more record than source lines; the trailing phantom record is code 25 and `END` never lets dispatch reach it). Execution: `WT_LOG` (handler `0xed28b4`) writes its file, increments the index and posts the next `0x3305`; `END` (`0xed1610`) sets the index to -1 and the selector to 1, posts nothing, and the task blocks in the receive. A loaded script runs to its end unattended (section 11).
Why it is the smallest file that distinguishes parsed from not parsed: it is the shortest file whose execution has an external, host-readable effect. `WT_LOG` writes a file of about 4 KiB into the active volume's DCIM tree through the camera's own frame-name builder, so a new `.msg` file appears on the card. The result is a file, not camera state: no settings, calibration or configuration byte is written, and the file is readable on the host with a card reader with no second camera operation. A file with only `END` (5 bytes) parses and executes but has no external effect, and a file that fails to parse leaves no file at all, so `WT_LOG`+`END` is the smallest pair that separates the two.
If any card write is disallowed on the first run, the smallest runnable file is `END\r\n` (5 bytes, SHA-256 `d1426a572f3750bbc93e1d7fc0d6d5c2e837d11fa6537447d091c0a4251d6375`); it parses and executes (count 2, index -1, selector 1) but its proof is the script-area state, observable only through the existing reviewed dump-ram operation in a separate camera run.

**What appears on the card, and timing.** A new ~4 KiB `.msg` file in the active volume's DCIM tree, named by the camera's frame allocator (the retained header on this body reads `C:\DCIM\963_FUJI\DSCF3087.JPG`). Timing, corrected 2026-10-09: the delay argument is not the 10 ms log tick. The kernel's delay entry adds its argument to the scheduler clock, and that clock advances 10 per timer interrupt, so one argument unit is about 1.0 ms (E-AUTOACT-UNIT-150, [the arming report's section 13.2](auto-act-arming.md#13-the-delay-unit-the-host-command-latency-and-the-window-verdict-2026-10-09)): the prologue `sleep(1000)` is about 1 s, the `sleep(4000)` about 4 s, and the script runs about 5 s after the mujin task's creation. Back up the card before the first run and confirm it is not write-protected.
If nothing appears, three causes are indistinguishable from the card alone: the arming byte was not live at the test, the loader did not find the file (directory absent, wrong case on a strict FS, wrong volume, or over 1 MiB), or the write failed silently. The existing reviewed dump-ram segment `id5-bss-2` (`0x01660000`-`0x0171bf00`) contains the whole script area and separates them: never armed gives selector `[0x16777c0] = 0` and a zeroed area; armed but the file failed gives selector 1 and a still-zeroed area (the loader memsets before it opens); loaded and executed gives record text at `0x1667060`, count `[0x1667058] = 3`, index `[0x1667054] = 0xffffffff`, selector 1.

**The ranked follow-up scripts, in the analysis's order.**
1. **`set`/`calc`/`if`/`jump`/`LABEL` exercise**, e.g. `set a 12345` + `if a = 12345` + `WT_LOG` + `END`. Teaches the real grammar: variables `a`-`z` at `0x16776dc` (single-letter, any byte writes), labels bucketed by first letter at `0x1677744` (26 words), and argument forms decimal, `0x` hex, `E_`5-hex config read, lowercase variable. Risk: script-owned RAM only, plus one card file if `WT_LOG` is used; no camera state. Also verifies the corrected capacity: keep every script under about 300 statements.
2. **`func NAME`**, one name per run after reading all 17 arms. Teaches the DCAA routine vocabulary (`DISP_ERR`, `MED_FULL`, `RD_ERR`, `WT_ERR`, `F_ERR`, `Z_ERR`, `F_Z_ERR`, `STANDBY`, `GLS_WAIT`, `P_MULTI`, `V_AVG`, `PIX_STILL`, `PIX_MJ`, `GET_MODE`, `GET_FILES`, `L_DET_ON`, `L_DET_OFF`; names byte-read, bodies unread in this pass). Risk: bounded only after each arm is read host-side; several names look like status setters and the `PIX_*` like pixel-mode changes, so one name per script with the body reviewed first.
3. **`key NAME ON/OFF`**, starting with one press/release pair watched physically. Teaches the scripted-input facility: `0xed19e4` looks up `NAME` in the key table `0xed2df8` (codes 1..`0x31e`) and the modifier in `0xed3a80` (`ON` = 0, `OFF` = `0x80` only), then posts the event through `0xed1518` to id4 `0x72b680`. Risk: highest of the list: it is an application event, not a physical key, so it can drive menus, request mode changes or the shutdown-class exit; the mapped consumer for the chosen name in the current mode must be stated before the run, and the owner should watch the body.
4. **`set E_000A2 <value>`** or another `E_` index. Teaches that the script itself is a cache writer: token1 `E_` + 5 hex chars is the index, the value from argument 3, written by `0x69fc94`, the same single-byte cache writer the service function-3 path uses. Risk: volatile (no commit, no queue post for starts other than `0x80`), but it writes calibration-area bytes in RAM; only justified with a stated purpose and the same calibration-risk note the `0xf7` gate carries.
5. **`tp`** with a table name and arguments. Teaches the structured event/command post: handler `0xed2548` reads up to 10 arguments and posts a named event built from the `{name,code}` table at `0xed3c24` (`ON` `0xb00`, `OFF` `0xb01`, `MOVE` `0xb02`, `SWEEP` `0xb04`, `2ON`/`2OFF`/`2MOVE`/`2SWEEP`, `MULTI` `0xb0a`) through `0x72b680`. Risk: arms unread; `MOVE`/`SWEEP` suggest actuation, so read the consumer first and treat it like `key` with less knowledge.
6. **Two `WT_LOG` statements separated by `wait N`**. Teaches the delay unit and the event-paced loop: two `.msg` files whose spacing and ordering measures the tick-to-second factor and confirms the handler's synchronous file I/O. Risk: one extra card file.

## Reproduction

All commands read host files only.

```sh
# id5 anchors: path template 0x00ed3ac4, record check 0x00d91988, loader 0x00ed1de0, parser 0x00ed1bcc
xxd -s 0x2e20c4 -l 24  private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin
xxd -s 0x19ff88 -l 48  private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin
xxd -s 0x2e03e0 -l 48  private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin
xxd -s 0x2e01cc -l 48  private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin

# id5 tables: statement keywords 0xed2d28, key vocabulary 0xed2df8, DCAA message table 0xed2d00,
# func names 0xed3ae4, dispatch 0xed39e8
xxd -s 0x2e1328 -l 208 private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin
xxd -s 0x2e13f8 -l 256 private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin
xxd -s 0x2e1300 -l 40  private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin
xxd -s 0x2e20e4 -l 120 private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin
xxd -s 0x2e1fe8 -l 32  private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin

# id5 name string 0x00d92020 (file 0x1a0620) and the enable accessor id4 0x69f8fc (file 0x834fc)
xxd -s 0x1a0620 -l 16 private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin
xxd -s 0x834fc  -l 32 private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin

# enable byte 0xa2 = 0x00 in all three body captures (window 000000, adjustment page 0)
xxd -s 0xa0 -l 16 private-data/sessions/20261008T175358Z-read-config/windows/000000.bin
xxd -s 0xa0 -l 16 private-data/sessions/20261007T060812Z-read-config/windows/000000.bin
xxd -s 0xa0 -l 16 private-data/sessions/S03/read-03/windows/000000.bin
```
