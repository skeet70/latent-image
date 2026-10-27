# USB personality toggle (X-M1 1.50)

Design, implementation and host verification of an on-device control that switches the USB personality between the `04cb:ff80` service device (transport service mode 3, as with stored `0xf7 = 1`) and ordinary PTP `04cb:02b6`, with no jig session and no settings-area write ([USB-PERSONALITY-TOGGLE-01](https://github.com/skeet70/latent-image/issues/81)). It covers the stock decision path, the key and its census, the stub and patch sites, placement, the flash candidates and their composition with RC0 and Classic Chrome, the native and emulator evidence, the changed-byte explanation (section 10), the review record and reviewer packets (section 11) and the install session script (section 12). The owner approved the design on 2026-10-22 with the flip behaviour and chose RC0+toggle as the first flash; the install was abandoned that day when pin 2 of the card slot broke on reinsertion (E-STORED-F7-REENABLE-RUN-150). No camera or USB device was touched; the candidates are not installed.

Inputs, byte-identical to the pinned decodes under `private-data/analysis/x-m1-1.50/`: DAT SHA-256 `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`; image 4 `lz/id4-0061c400.bin` base `0x0061c400` SHA-256 `f2e2acab…dcff16`; image 5 `lz/id5-00bf1a00.bin` base `0x00bf1a00` SHA-256 `3cd501c0…fd892a`; plain `lz/plain-01100000.bin` base `0x01100000` SHA-256 `ee211bda…2d889d`; image 2, both loaders and both boot records under `filmsim/` and `decode/`; the fallback adjustment image at DAT payload `0xe0048` (`payload.bin`), equal to the body's adjustment bytes over `0xa000`-`0xcfff` (E-ADJ-READ-150).

Labels: **byte-read** (instruction, table or data read from the pinned images in this pass), **static-inference** (conclusion from those bytes), **inherited** (named ledger entry or report, not re-derived), **hardware-observed**, **unproved**.

## 1. Summary and decision

**Key: hold Q while plugging in the cable.** On an attach edge in an ordinary, playback or first-start boot, if the key driver reports Q held, the patch flips the RAM copy of adjustment byte `0xf7` (0 becomes 1, nonzero becomes 0) before the stock connect handler tests it. With stored `0xf7 = 0`, a Q attach gives `ff80` and every later attach in that boot stays `ff80`; a second Q attach in the same boot returns to PTP. A plain attach after power-on gives PTP. Nothing is written to storage, so every power-on starts from the stored byte.

- **One hook, one stub, image 5 only.** The `bl 0x0069f8fc` at `0x00ecf900` in connect handler A becomes a `bl` to a 22-word stub (`host/firmware/usb-toggle-stub.S`) placed in the unreferenced image-5 function extent `[0x00bf4454, 0x00bf44e4)` (sections 3.1 and 5).
- **Key state from the key driver.** The stub reads Q through `0x006a0d00(0x32)`, the driver's debounced row state, not the UI key bitmap, so a UI that misses a release cannot leave a later attach armed (section 4.2).
- **RAM only.** The stub writes `cache[0xf7]` through the stock byte setter `0x0069fc94`, the same RAM state the jig tools' gate write leaves until power-off. All 13 stock readers of `0xf7` read that RAM byte (section 2.4), so the debug channel, the `read-ram` gate check and the detach arm follow the toggle without further patching.
- **No persisted toggle.** The only host-reachable storage route is the 33-unit whole-area commit; a stored toggle would repeat that rewrite on every switch. None is needed (section 3.4).
- **RAM-push test variant.** Stored `0xf7 = 1` already provides the initial `ff80` channel. A temporary push can test both Q transitions within that boot without flashing; it cannot select the first USB personality of a later boot. Section 14 owns its publication contract and the [cardless session script](usb-toggle-ram-push-session-script.md) owns the battery-removal ending.
- **Candidates.** RC0+toggle `10feac47…2445` (on RC0 `acb0fbbb…ca8`) and CC-icon+toggle `c1bfc9aa…ad8f` (on the design-B Classic Chrome flash candidate `98a66780…1606`, which carries RC0, Classic Chrome, its icon and Q tiles), both built by `//host/firmware:build-usb-toggle` and `//extensions:build-extension`, pinned and reproduced. Each decoded image 5 differs from its base only in the stub bytes and the hook word; every other part equals the base (section 10). CC+toggle `507ecf32…0e1e`, on the superseded CC candidate `16c830ad…2c89`, is not a flash target. The owner chose RC0+toggle as the first flash (2026-10-22) and later planned "PRO Neg + toggle and Classic Chrome" once the slot is repaired; CC-icon+toggle carries all of it in one update, so one card trip.
- **Evidence.** 364 native cases and every full-system run the emulator can deliver pass: both candidates boot to SHOOTING MENU, a plain attach gives PTP, a Q attach gives `ff80` with a working service session, a second Q attach returns PTP in the same boot, stock behaviour is unchanged without Q, the Classic Chrome RAM push runs over a Q attach on RC0+toggle, and the updater install resets stored `0xf7` to 0 (section 9; E-USB-TOGGLE-DESIGN-150, E-USB-TOGGLE-EMU-150, E-USB-TOGGLE-CANDIDATES-150).
- **Review.** Independent review `toggle-review` approved RC0+toggle `10feac47…2445` on 2026-10-22 after its five must-fix items were closed in `0de0811` (section 11.5, E-USB-TOGGLE-REVIEW-150). Independent review `ToggleCompReview` approved CC-icon+toggle `c1bfc9aa…ad8f` as a composition of reviewed design-B and the reviewed toggle on 2026-10-24, with no must-fix (section 11.5, E-USB-TOGGLE-CCB-REVIEW-150). Gate 1 is closed for both DATs; CC-icon+toggle's open boot-path item needs an owner decision (11.4).
- **Install.** H3. The session script is section 12. It starts only after the card slot is repaired and passes the card checks of [#85](https://github.com/skeet70/latent-image/issues/85), and it inserts the card into the camera once.

Body state: stored `0xf7 = 1` since the 2026-10-22 `commit-f7-enable`, with stored `0x80 = 0x22` and `0x81 = 0xff` expected (E-STORED-F7-REENABLE-RUN-150); a plain attach in an ordinary boot gives `ff80`. No card is in the slot. The install stores `0xf7 = 0` and is expected to return `0x80` and `0x81` to `0x00` (section 12.2). The toggle also covers stored `0xf7 = 1` (section 3.2).

## 2. The USB personality decision, byte-read

### 2.1 Boot

Init action `0x0073e0b0` calls `0x0073cbfc` (`0x0073e0c0`) and then the mode decision `0x0073ccb8` (`0x0073e0c4`) (byte-read).

1. **Boot key records.** `0x0073cbfc` registers `0x0073ad44` as the key-driver callback through `0x006a1e5c(0x0073ad44, 0)` at `0x0073cc50`. `0x0073ad44` appends `{code, arg}` to the record array at `0x00913520` (76-byte stride, count at `+0x1300`, at most 64) and posts it as a message through `0x00726478`. `0x0073ad44` has one data reference, the `movw`/`movt` at `0x0073cc44` (byte-read).
2. **Boot USB key, `0x0073ae10`** (called at `0x0073cc64`). It reads `0xf7` at `0x0073ae20`. With `0xf7 = 0` (`0x0073aeec`-`0x0073af20`) it copies the raw attach bit `0xfff55104[31]` to `0x00914828` and, when it is 1, appends record `0x10` (USB). With `0xf7 != 0` (`0x0073ae2c`-`0x0073aee8`) it clears `0x00914828`, scans the records for code `0x15` (TELE2), 2 (UP) or 7 (S1) and only then samples the raw bit and appends `0x10` (byte-read).
3. **Fold.** `0x00639b1c` (from `0x0073cce0`) calls the fold `0x00634b40` at `0x00639bbc`: each record below `0x600` with qualifier `0x00` or `0x40` sets its key-state bit through `0x006329c4`, qualifier `0x80` clears it through `0x00632a00`, and only code `0x11` (CARD) is forwarded to the application (byte-read).
4. **Poller.** `0x00635280` at `0x00639bd0` requests pending mode 3 through `0x006330d8(1)` and, when `*(0x400a0144)` is 0, tests key states through `0x00632950`: POWER 6, BARR_OP `0xd`, BARR_CL `0xe`, `0x600`, USB `0x10` (request 8, application mode 7), JIG `0x339` and USB with UP and S2, or USB with halfword `0x0096b418 == 0xff80` (request `0xc`, mode `0xb`), DOOR `0x210`, DISP_B 9 with `0x0096b429` bit 1 (request `0xa`, the update screen). `0x0063975c` commits the pending mode at `0x00639bd8` (byte-read; E-JIG-ENTRY-KEY-BIND-150 and E-UPDATE-ENTRY-KEY-BIND-150 inherited for the request meanings).
5. **System-mode export.** `0x0073ccb8` passes the application mode to `0x00639b1c`, whose table `0x00639c4c` stores the system mode word `0x009570dc` once: application 3 to 1, 4 to 2, 5 to 3, 7 to 5, `0xb` to 7. `0x0073c15c` returns it; `0x0073aff0` stores it through a pointer (byte-read, inherited from E-STORED-F7-ORDINARY-READ-150).

`0x0096b418` is `cache[0x102..0x103]`, copied at boot by `0x0072b8c4` (`0x0072b938` calls `0x0069f8b8`, `0x0072b93c` stores it), reached through `0x0073acfc(0x0096b400)` at `0x00639ba0` (byte-read). The body's config window `0x000100` begins `cb 04 b6 02`, so the halfword is `0x02b6` and the `0xff80` arm is inactive on this body (byte-read from the private read-config capture).

### 2.2 USB-manager start

- **Handshake, handler B `0x00ecf9c8`.** Runs on queue-7 `0xB400` in group-1 state 1, once per boot (inherited, stored-f7 report 1.2). It reads `0xf7` at `0x00ecf9d8`. With `0xf7 != 0` and system mode not 5 or 7 it stores 0 to the latch `0x015c229c` (`0x00ecfa0c`); otherwise it stores the current attach bit (`0x00d82348`) to the latch, compares it with the boot value (`0x0073af68`) and posts application `0x10` or `0x90` on a difference (`0x00ecfa14`-`0x00ecfa50`) (byte-read).
- **Startup barrier, `0x00ec8014`.** Reads `0xf7` at `0x00ec811c`. With `0xf7 = 0` it posts application `0x10` when the raw attach bit is set and `0x90` otherwise (`0x00ec8128`-`0x00ec814c`); with `0xf7 != 0` it posts nothing (byte-read). It runs once, after the startup mask `0x015c1e5c` reaches `0x1ff` (inherited).

### 2.3 Attach and detach

Every attach or detach edge reaches connect handler A `0x00ecf8b8` through observer `0x00ecfe64`, queue-7 `0x1714` and group-1 row 12 in state 3 (inherited, E-CONNECT-EDGE-150). Byte-read:

| Address | Step |
| --- | --- |
| `0x00ecf8d8`-`0x00ecf8ec` | New bit `r1 = payload & 1`; returns if equal to the latch byte `0x015c229c` (`0x015c20c4 + 0x1d8`) |
| `0x00ecf8f8` | Latch := new bit; `r1` still holds it at the next call |
| `0x00ecf8fc`-`0x00ecf908` | `0x0069f8fc(0xf7)`; zero goes to the application arm `0x00ecf99c` |
| `0x00ecf910`-`0x00ecf924` | System mode through `0x0073aff0`; 5 or 7 go to the application arm |
| `0x00ecf93c`-`0x00ecf95c` | Attach arm: trace, `0x00ed117c(7, 0)` posts queue-7 `0x1705`, `0x00647398(0x00ed19e4)` registers the debug-text callback |
| `0x00ecf964`-`0x00ecf998` | Detach arm: `0x00ed11a4(7, 0)` posts the transport stop `0x1706`, or application `0x90` when `0x0073abec()` is nonzero |
| `0x00ecf99c`-`0x00ecf9b8` | Application arm: application `0x10` (attach) or `0x90` (detach) |

`0x1705` runs `0x00ecc104`, which fills `0x43360000`-`0x45a9ffff` with `0xff` and starts service mode 3 through `0x00d82478`; driver action `0x00d84af8` installs the vendor table and descriptor mode 3, so GET_DESCRIPTOR returns the fixed `04cb:ff80` descriptor at `0x00d7ef00` (inherited, stored-f7 report 1.2, 1.3 and 1.6; hardware-observed E-STORED-F7-ORDINARY-ENUM-150). On the application arm the application handles `0x10`; `0x00d0afe0`, reached from application handler `0x00c82b00` (table word `0x00cd82c8`), requests maintenance (`0xd350c4(0xc)`) when USB and either the `0xff80` halfword or UP with S2 are set in the live key state, and USB mode (`0xd350c4(8)`) otherwise (byte-read); USB mode starts the PTP class with the descriptor built from `cache[0x100..0x101]` and `cache[0x13e..0x13f]`, `04cb:02b6` on this body (inherited).

### 2.4 Readers of `0xf7`

The accessor `0x0069f8fc` returns `*(*(0x0096ca50) + index)`, and the setter `0x0069fc94` stores `r1` there with no other effect (byte-read). Of its 6900 calls, 13 pass `0xf7` (byte-read, matching the stored-f7 census):

| Site | Function | Runs | Sees a toggle |
| --- | --- | --- | --- |
| `0x0073ae20` | boot USB key `0x0073ae10` | before the mode decision | no, earlier |
| `0x00ecf9d8` | handler B | `0xB400` handshake, once | no, earlier |
| `0x00ec811c` | startup barrier `0x00ec8014` | once | only if it runs after a Q attach; it then posts nothing, as for stored 1 |
| `0x00ecf900` | handler A | every edge | patched site |
| `0x006473a8`, `0x006473b8` | `0x00647398` registration | attach arm | yes |
| `0x00646ed0`, `0x00647464` | log writers | after registration | yes |
| `0x00647120` | spawn test (`== 2`) | registration | value 1 spawns nothing |
| `0x00647958`, `0x00647b34`, `0x00647b90`, `0x00647c10` | debug functions 1/4, 2, 3, 5 | per D0 request | yes; a toggle back to 0 closes them |

No stock code calls `0x0069fc94` with index `0xf7` (byte-read). Loaders, boot records and the plain image do not read the byte (inherited).

### 2.5 What else selects the personality

The raw attach bit at boot and the boot key records (USB-cable boot, system mode 5, excludes the service route); the system mode word (5 and 7 excluded); the group-1 state, which must be 3 for `0x1714` to reach handler A (inherited); driver state 3 for the mode-3 start (inherited); live UP and S2, or `cache[0x102..0x103] == 0xff80`, for maintenance on the application arm; `cache[0x100..0x101]` and `cache[0x13e..0x13f]` for the PTP descriptor.

## 3. Design

### 3.1 Stub

`host/firmware/usb-toggle-stub.S`, linked at `0x00bf4454` by `host/firmware/usb-toggle-stub.ld` through `//host/firmware:usb-toggle-stub` (`usb-toggle-stub.bin`, 88 bytes, SHA-256 `35a7e9597f06bdc1dda250d991df4a468d2401e5ae3c24c22e37403318746d62`). Entry from `0x00ecf900` with `r0 = 0xf7`, `r1` = new attach bit, `lr = 0x00ecf904`; it returns the value handler A tests in `r0`, preserves `r4`-`r11` and the stack, and keeps 8-byte alignment. Linked words:

```asm
00bf4454  e92d4070  push    {r4, r5, r6, lr}
00bf4458  e1a04001  mov     r4, r1              @ attach bit
00bf445c  ebeaad26  bl      0x0069f8fc          @ r0 = cache[0xf7]
00bf4460  e1a05000  mov     r5, r0
00bf4464  e3540001  cmp     r4, #1
00bf4468  1a00000d  bne     0x00bf44a4          @ detach edge: stored value
00bf446c  e3a00032  mov     r0, #0x32           @ KEY_Q
00bf4470  ebeab222  bl      0x006a0d00          @ debounced driver state, 0 or 1
00bf4474  e3500000  cmp     r0, #0
00bf4478  0a000009  beq     0x00bf44a4
00bf447c  ebed1f36  bl      0x0073c15c          @ system mode word 0x009570dc
00bf4480  e3500005  cmp     r0, #5
00bf4484  13500007  cmpne   r0, #7
00bf4488  0a000005  beq     0x00bf44a4          @ USB-cable boot or maintenance
00bf448c  e3550000  cmp     r5, #0
00bf4490  03a05001  moveq   r5, #1
00bf4494  13a05000  movne   r5, #0
00bf4498  e3a000f7  mov     r0, #0xf7
00bf449c  e1a01005  mov     r1, r5
00bf44a0  ebeaadfb  bl      0x0069fc94          @ cache[0xf7] = r5
00bf44a4  e1a00005  mov     r0, r5
00bf44a8  e8bd8070  pop     {r4, r5, r6, pc}
```

The stub flips the byte only where handler A will take the service attach arm with the new value; on a detach edge, with Q released, or in system mode 5 or 7 it returns the stock read. Attach and detach therefore stay paired: a session's detach reads the value its attach set. Its deepest stack use is 28 bytes below its entry SP (16-byte frame, then `0x006a0d00` at 8 and `0x006a0b4c` at 4; measured in N1).

### 3.2 Behaviour

| Stored `0xf7` | Situation | Result |
| --- | --- | --- |
| 0 | power on, plain attach | PTP: application `0x10`, USB mode, `04cb:02b6` |
| 0 | power on, Q held at attach | `ff80`: RAM `0xf7 = 1`, `0x1705`, service mode 3 |
| 0 | later plain attach in that boot | `ff80` (RAM byte is 1) |
| 0 | later Q attach in that boot | PTP, RAM `0xf7 = 0` |
| 0 | cable present at power-on, Q or not | USB-cable boot (system mode 5), PTP; the stub never flips |
| 1 | power on, plain attach | `ff80`, stock stored behaviour |
| 1 | power on, Q held at attach | PTP, RAM `0xf7 = 0` |
| any | jig boot (system mode 7) | no flip; jig tools see the stored byte |

Static-inference over sections 2.2 to 2.4, emulated in section 9.3 except where the model has no disconnect event. The attach must follow the startup handshake: before group-1 reaches state 3 no `0x1714` reaches handler A, and handler B reports the attached cable to the application (PTP). The 41 MB `0xff` fill runs only on attaches that start `ff80`, not on every attach as with stored 1. A cable present at power-on selects maintenance instead of USB-cable boot when `cache[0x102..0x103]` reads `0xff80` (the firmware template's value, 2.1); this body holds `0x02b6`.

### 3.3 Why attach time and not power-on

A power-on variant is possible: a second hook at `0x0073ae20` could scan the boot records for Q, as stock scans for TELE2, UP and S1. It is not chosen:

- **The snapshot's content for Q is inferred, not shown.** Jig boots with `0xf7` stored started on this body (E-STORED-F7-COMMIT-RUN-150 and the 2026-10-22 restore session), which by the bytes of 2.1 requires an UP or S1 record at `0x0073ae10`, and the update screen's DISP_B test reads channel 9 like Q (static-inference). The 2026-10-09 capture also shows startup press forms for Fn and Q with no owner press (syscont ticks 344 and 658, initcont Fn at tick 31; reports/input-map-150.md section 3, source unproved), so a power-on Q could fire falsely.
- **The held key still reaches the application.** `0x006a282c`, which registers the application callback `0x0072b680` (at `0x00ec42c8` and `0x00ec5720`, two of its five call sites), copies the boot records and replays them through `0x006a1b40`, the caller of the registered callback (byte-read), so a Q held through boot is delivered as a Q press (static-inference).
- **A power-on choice cannot be undone within the boot.**

The attach-time test reads the key driver's debounced state of a key pressed during normal operation (section 4.2), the same layer whose press reports drive the running UI.

### 3.4 No persisted toggle

`cache[0xf7]` is already the single gate every reader uses (2.4). Persisting a toggle would need the whole-area commit, the only host-reachable or firmware writer of the byte: `0x0069fea8` (`0x006a9fdc(cache, 0x420000)`), called from `0x00dbc8e4` (service row 26, host `0x80 = 0x22`), `0x00db932c` (update completion) and `0x00d5cf0c` (byte-read). That is a 33-unit rewrite that also stores `0x80 = 0x22` (inherited), the cost the owner asked to avoid.

### 3.5 RAM-push scope

The push route writes its runtime with debug function 3, which needs `ff80` enumerated and `0xf7` nonzero (`0x00647b90`) before the first byte; the allocation and all publications are RAM state lost after full power removal (inherited, transport README). It cannot provide a first-attach switch on a stored-0 body. This body already has stored 1, so the section 14 variant can publish the hook over the existing service session, switch to PTP on the next Q attach and return to `ff80` on a second Q attach without a flash. The broken card slot is not used.

## 4. Key choice

### 4.1 Census

Byte-read over images 4, 5 and plain. `0x00632950` has 227 BL callers: 217 pass a constant key (45 distinct keys), 10 pass a computed key.

| Consumer | Keys it tests | Site |
| --- | --- | --- |
| Boot USB key | TELE2 `0x15`, UP 2, S1 7 (records) | `0x0073ae60`-`0x0073ae9c` |
| Boot fold | ADAPTER `0xf`, `0x601`-`0x603`, LENS_DET `0x38`; forwards CARD `0x11` | `0x00634b40` |
| Boot poller | POWER 6, BARR_OP `0xd`, BARR_CL `0xe`, `0x600`, USB `0x10`, JIG `0x339`, UP 2, S2 8, DOOR `0x210`, DISP_B 9 | `0x00635280` |
| Boot switch scans | mode dial `0x302`-`0x31d` (`0x007748f8`), I-M_A `0x400`, S-A `0x31e`, RING `0x204`-`0x206`, E-D `0x32c`-`0x338`, MET `0x20d`-`0x20f` | `0x00637478`, `0x0063a3c4`-`0x0063bbc0` |
| Attach-time maintenance choice | USB, `0xff80` halfword, UP, S2 | `0x00d0afe0` |
| Mode re-decision | USB, `0xff80` halfword, UP, S2, JIG, DISP_B, RING_STB/RING_ON, PLAY `0xb`, S1, POWER | `0x00d0ad8c` |
| Release synthesis | OK 1, FMODE `0xa`, Fn `0x28`, `0x30`, EVF_LCD `0x26`, UP-RIGHT 2-5, one table key | `0x00d3ad9c` |
| DOWN combination | DOWN 3 when `0x0096b480` is 1-3 | `0x00d5cfbc` |

Physical X-M1 controls and their condition rows (rows from the 36-row table `0x00828120`, byte-read; codes hardware-observed in E-PRESS-MAP-150):

| Control | Code | Row kind | Constant-key sites | Stock combination or press action |
| --- | --- | --- | ---: | --- |
| Q | `0x32` | analog, channel 9 | 0 | none; press opens the Q menu |
| Sub-command push | `0x2b` | port bit | 0 | none; holding risks turning the dial |
| Fn | `0x28` | port bit | 1 | assignable function; startup press forms |
| OK | 1 | analog, channel 9 | 0 (computed only) | opens MENU |
| DOWN, LEFT, RIGHT | 3, 4, 5 | analog, channel 8 | 8, 6, 6 | selector functions; DOWN combination |
| UP | 2 | analog, channel 8 | 10 | jig (UP with S2), boot USB key |
| DISP/BACK | 9 | analog, channel 9 | 8 | update screen at power-on |
| PLAY | `0xb` | port bit | 3 | playback start in `0x00d0ad8c` |
| MOVIE | `0x34` | analog, channel 9 | 2 | starts recording |
| S1, S2 | 7, 8 | shutter | 8, 13 | jig, full press at power-on |

Q is the only physical button with no key-state test anywhere and no role in any stock combination at power-on or attach. The driver-state getter `0x006a0d00` the stub uses has three stock callers, for LENS_DET `0x38` (`0x0073d064`), PLAY `0xb` (`0x00ec80ac`) and S1 7 (`0x00ec80e8`), none for Q (byte-read). Q is not in the release-synthesis list, and the bank sources in `menu/` reference neither `0x00632950` nor code `0x32`. The DOWN combination is inert on this body: `0x0096b480` is `cache[0xa902]` (copied at `0x0072bd88`), which reads `0x00` in the fallback image (byte-read) that equals the body over `0xa000`-`0xcfff` (E-ADJ-READ-150).

### 4.2 The key state the stub reads

`0x006a0d00(code)` finds the code's condition row through `0x006a0b4c` (36 rows of 28 bytes from `0x00828120`; Q is row 19 at `0x00828334`) and returns 1 exactly when row byte `+0x15` is 1 (byte-read). `+0x15` is the driver's debounced state. The scan loop (`0x006a3478`-`0x006a34a4`) debounces rows with flag `+0x04 & 0x100` through `0x006a11f4` and every other row through `0x006a2fa8`; Q's row has `+0x04 = 0x40`, so its debounce is `0x006a2fa8`. That step stores raw `+0x16` (written by the row evaluator `0x006a0888`, here the channel-9 ADC window) to `+0x15` after `+0x14` agreeing samples and posts the press (code) or release (code `| 0x80`) through `0x006a0ff4` (byte-read, independent review). The stub therefore reads the same state from which the press and release events are made, at the attach edge. The release-all step `0x006a27a8` (only caller `0x00ec8248` in the startup barrier) clears held states and reports releases; it can only make a held Q read 0.

Stock also offers the UI key bitmap: `0x00632950(code)` tests bit `idx & 31` of word `0x0094f8b8 + 4 * (idx >> 5)` (`0x00632860` maps the code through the range table `0x007748dc`; Q is word `0x0094f8bc`, bit 18), set and cleared by the boot fold and by the UI key handler `0x00d36ea4` (callers `0x00c231fc`, `0x00d5d0e4`), which skips the update when `0x00d35f68()` returns 1 or `[0x0094f5e0] == 2` (`0x00d36ee4`-`0x00d36f04`) (byte-read). A release skipped there would leave the bit set and arm every later attach, so the stub does not read it. N1 shows the stub ignores the bitmap (section 9.1).

## 5. Patch sites and placement

### 5.1 Words

| Image | Address | File offset | Current | New | Role |
| --- | --- | --- | --- | --- | --- |
| 5 | `0x00ecf900` | `0x002ddf00` | `ebdf3ffd` `bl 0x0069f8fc` | `ebf492d3` `bl 0x00bf4454` | handler A's `0xf7` read |
| 5 | `0x00bf4454` | `0x00002a54` | 144-byte extent, SHA-256 `a862a878075d05fd011efb724fdd7503fb406bddac8c1a00f27998486efd4ec0` | 88-byte stub (3.1); the remaining 56 bytes stay stock | stub |

The stub's calls reach `0x0069f8fc`, `0x006a0d00`, `0x0073c15c` and `0x0069fc94` in image 4 with A32 `bl` (displacements within range; the builder decodes every BL of the linked blob and refuses any other target). It uses only forms the R4F profile has; the native runner ran it with no admission change.

### 5.2 Ownership of `[0x00bf4454, 0x00bf44e4)`

The extent is one function, from its `push {r4-r9, lr}` at `0x00bf4454` to the `bx lr` at `0x00bf44e0`; the next function starts at `0x00bf44e4`. Byte-read census over images 4, 5, plain and 2, both loaders and both boot records:

- no A32 branch from outside the extent targets an address in it, and it has no internal branch target;
- no aligned or unaligned 32-bit value in `[0x00bf4454, 0x00bf44e4]` occurs in any of the eight inputs;
- no `movw`/`movt` pair and no `add`/`sub rX, pc` computes an address in it;
- Thumb listings of plain and both loaders branch nowhere into it; the image-5 Thumb listing yields only decodes of A32 upper halfwords (addresses `2 mod 4`).

It is the only image-5 function with no reference by this census. Ownership is by absence of references, the class of CC's `0x0073f684` verdict, not by replacement. The emulator watch (E2, section 9.3) adds the runtime check on the executed paths. None of the 13 private RAM-push manifests (12 CC builds of 131 publications, one bank build of 250) names an address in the extent or in handler A.

## 6. Composition

### 6.1 With RC0 and CC

RC0 changes only image 4. The CC candidate (`16c830ad…2c89`) changes image 4, image 5 (45 in-place words from `0x00d461ec` to `0x00eab32c`, no blob), the `0x83e` record and plain; none of its image-5 words is in the stub extent or at `0x00ecf900` (checked against the pinned `cc-image5.json`). `//extensions:build-extension` takes at most one patch set per image, so the toggle enters as part of a merged image-5 patch set:

| Step | Kind | Content |
| --- | --- | --- |
| 1 | `image-patch-set`, segment 4 | CC's `cc-image4.json`, unchanged (RC0's writes, prefix at `0x0061c200`) |
| 2 | `image-patch-set`, segment 5 | CC's 45 words, the hook word and the stub blob |
| 3 | `image2-record` | CC's `0x83e` record, unchanged |
| 4 | `plain-patch-set` | CC's `cc-plain.json`, unchanged |

Budget: the prefix window stays at CC's 484 of 512 bytes, the converter extents at 84 of 88 and 68 of 92, plain and image 2 unchanged. The toggle uses 88 of the 144 extent bytes and one in-place word; image 5 is re-encoded in place with no load or size change, so it adds no boot-path item. A toggle candidate on a CC image inherits CC's open base-alignment note (classic-chrome-patch.md 11.3 item 5): the hardware decompressor has not decoded an image-4 load at `0x0061c200`. The RC0+toggle candidate keeps RC0's image-4 load `0x0061c340`, which the body has decoded (E-FILMSIM-INSTALL-150).

The design-B candidate `98a66780…1606` (classic-chrome-icon.md section 8.1) adds to CC an `image2-record` for `0x83d`, an `image2-patch-set` (the Q glyph `0x37c` pointer), a `font-patch-set` and eight image-5 words (live view `0x00eee960`-`0x00eee970`, frames `0x00e424ac`-`0x00e424b4`). CC-icon+toggle keeps its seven steps in order and replaces only step 2 with the merged image-5 patch set: the 53 design-B words, the hook word and the stub blob. The design-B image-5 words run from `0x00d461ec` to `0x00eee970` and none lies in `[0x00bf4454, 0x00bf44e4)` or at `0x00ecf900`; its image-4 writes end at `0x007960e4` and touch none of the stub's callees, `0x0073c15c` or the condition-row table `[0x00828120, 0x00828510)` (byte-read, pinned parts).

Tooling, following CC's re-inclusion of RC0: `//host/firmware:build-usb-toggle [--output-sha256 HEX] BASE_EXTENSION_JSON NEW_OUTPUT_DIRECTORY` (`host/firmware/BuildUsbToggle.hs`, core in `host/FujiAF/UsbToggle.hs`):

- it takes the linked stub and its symbol table from `//host/firmware:usb-toggle-stub`, requires `usb_toggle` at `0x00bf4454` and decodes every BL of the blob to the four stock callees;
- it checks the stock image 5 against its pinned SHA-256, the extent against its pinned hash and the hook word against `ebdf3ffd`;
- it reads the base manifest and every step that names a file against its step SHA-256, refuses a base image-5 patch set that names another decode or moves the load, and refuses any base write that overlaps the hook word or any byte of the extent `[0x00bf4454, 0x00bf44e4)`, including the 56 stock bytes past the stub;
- it writes the merged image-5 patch set `usb-toggle-image5.json` (address order, contiguous write orders), copies every other base step and its file unchanged, and writes the manifest `usb-toggle.json` with the image-5 step after the image-4 step, plus `changed-words.json`/`.md` with the toggle as group `TOGGLE`.

`build-extension` is unchanged. The merge, the step assembly (`baseStep`, `assembleSteps`) and the stub checks are in `FujiAF.UsbToggle`; `BuildUsbToggle.hs` only reads files and writes the output. `//host/firmware:usb-toggle-tests` checks the merge and the linked blob (two toggle writes at the extent and the hook word, the hook encoding, address order of a disjoint merge, refusal at the hook word, inside the stub, at `0x00bf44ac` and at the extent's last word, acceptance just below the extent and at its end, an oversized blob refused, the four callees) and the step assembly over synthetic steps (image-4 steps first, the merged image-5 step next and the rest in base order, the base image-5 set dropped, every file copied byte-equal under its file name with its SHA-256 kept, a path-less record unchanged, refusal of a SHA-256 mismatch, a missing file, two files with one name and a name the builder writes itself). Because the CC parts carry the private data block, both candidates' parts and manifests stay under `private-data/analysis/usb-toggle/`, as CC's do. The RC0+toggle parts hold no private data and can move to `extensions/` once reviewed.

The merged image-5 set carries the toggle's own `tier` (`USB-TOGGLE`), `order_rule` (address order, every write in place), `unchanged_surfaces` and `totals`. For CC-icon+toggle that drops design-B's `order_rule` (its fragment orders), its `unchanged_surfaces` entry (the movie and live-view validator `FUN_00eee958`) and its `tier` `RC2`; they remain readable in `parts-b-1-pinned/cc-icon-image5.json`. They have no byte effect: every write is in place (review I1). Carrying them would change the pinned `usb-toggle-image5.json`, so the pinned parts keep the toggle's fields.

### 6.2 With the bank and the RAM-push route

- **RAM push (bank and CC payloads).** No publication touches the toggle words. `ram-alloc` and `read-ram` refuse unless config byte `0xf7` reads nonzero (transport README); on a toggle body with stored 0 the operator makes a Q attach first. The session scripts (reports/ram-push-session-script.md, classic-chrome-patch.md 8.4) and the transport README describe that premise as stored `0xf7 = 1` and need the Q-attach wording once the toggle is installed.
- **Recipe bank and D3 flash designs.** They use the image-4 prefix, plain and image-2 ids; no image-5 overlap with the toggle, and no key-state use.
- **`commit-f7-*`.** In jig boots (system mode 7) the stub never flips, so they see the stored byte. In an ordinary boot after a Q attach, `commit-f7-enable` refuses (start value reads 1) and `commit-f7-restore` would commit 0 over a stored 0.
- **Whole-area commit in a Q boot.** The commit `0x0069fea8` writes the RAM cache verbatim (`0x006a9fdc`), so a service row-26 commit (`0x80 = 0x22`) in a boot whose byte a Q attach flipped would persist that byte (byte-read). Today's `commit-f7-enable` refuses on a RAM 1 and `commit-f7-restore` writes 0 first, but no whole-area commit operation may run in a Q boot; run them in jig boots (system mode 7), where the stub never flips.
- **Gate-writing operations in a Q boot.** `probe-ram`, `probe-area2`, `probe-straps`, `probe-code`, `probe-ownership`, `probe-routes`, `dump-ram`, `arm-auto-act` and the `commit-f7` precondition and refusal paths write RAM `0xf7` mid-session and restore it to `00`. In a Q boot that leaves the service transport running with the byte at 0, so the next detach takes the application arm (`0x90`), a sequence no emulated or hardware run covers. Run them only in jig boots. The ordinary-boot operations that only read the byte (`read-ram`, `ram-alloc`, `ram-push`, `ram-unload`, `ram-free`) keep the pairing.
- **Install.** The update resets stored `0xf7`, `0x80` and `0x81` to 0 (section 8.4, byte-read and emulated). The toggle works with either stored value.

## 7. Review class

- **Install: H3** (firmware update, spec 07 section 3.2). The hook word redirects stock control flow and the stub runs outside any payload allocation, so both are new mechanism bytes: one independent review of the stub source and linked blob, the hook word, the merged image-5 patch set and the container diff; `//emulation:flash-boot-check` on the exact DAT; the owner's visual check in `//emulation:xm1-viewer -- --firmware`; the owner's per-run go naming the operation and the digest.
- **Use after install.** A Q attach writes only the RAM byte `0xf7`, the state the jig gate write leaves until power-off, and opens the same `ff80` channel stored `0xf7 = 1` opens today, including debug function 3 (arbitrary RAM write) for the attached host. Live commands over that channel keep their own classes (`read-ram` H1 under the read grant, `ram-push` H2).

## 8. Open questions, resolved

### 8.1 A missed release

The stub reads the key driver's debounced state `+0x15`, not the UI bitmap (section 4.2). `+0x15` of the condition rows is written only by key-driver code (`0x006a0cc4`-`0x006a3084`): the debounce steps `0x006a2fa8` (Q's) and `0x006a11f4`, the pair logic in `0x006a1038` and `0x006a1988`, the S2-to-S1 synthesis `0x006a1c50`, the timer toggle `0x006a2e00`, the init writes at `0x006a2680`/`0x006a26b4`, the pointer-table path at `0x006a138c`, the release-all step and `0x006a0cc4` (called twice in the startup barrier for S1). No reference to the row table exists outside `0x006a02ac`-`0x006a33f0` in images 4, 5 or plain (byte-read, independent review). The UI's skip states (`0x00d35f68() == 1`, `[0x0094f5e0] == 2`) do not touch it. The stub reads it once per attach edge, inside handler A, so a release the UI never processed cannot arm a later attach. N1 sets the UI bitmap bit with `+0x15 = 0` (no flip) and clears it with `+0x15 = 1` (flip). E4 reads `+0x15 = 1` with Q held and 0 after release (section 9).

### 8.2 usbcont stack headroom

usbcont is task 7. Its creation block, built at `0x00ed1120`, gives entry `0x00ecc058`, priority 6 and stack size `0xc00`. `0x00725dd8` allocates the stack, records it at `0x40a5b234` (size at `0x40a5b238`) and fills it with the task id byte through `0x007221c8` (byte-read). In emulation the stack is `[0x00a42200, 0x00a42e00)`. SP at `0x00ecf900` is `0x00a42db0`, 80 bytes below the top, in every run; SP at the stub's `0x006a0d00` call is `0x00a42da0`. The stub's path adds 28 bytes below its entry SP; stock reaches 4 at that call site, and handler A's own stock callees from the same frame reach more by direct calls alone (`0x00647398` 160 bytes, `0x00ecf894` 96, `0x00ed117c` 88). The cumulative high-water mark of the fill is 424 bytes on PTP-only runs (equal on stock), 1068 bytes on runs with an `ff80` start or service session, and 304 on the cable-at-reset boot, against 3072 (emulated). Static worst case over indirect calls is not bounded; the stub does not raise handler A's direct-call maximum.

### 8.3 PTP after an ff80 stop within one boot

Emulated, both candidates (E5u, section 9.3). A Q attach starts `ff80`. A detach takes the detach arm and posts `0x1706`, with no application `0x90`. A plain re-attach gives `ff80` again, enumerated with the full service session. A detach follows. A Q re-attach then flips the byte to 0, takes the application arm, posts application `0x10` and starts transport mode 2 with the PTP descriptor `04cb:02b6`. The model raises no controller disconnect event, so a detach after an enumerated session and the detach after the PTP start never reach handler A (driver state `0xa` and 8; the presence poll `0x00d7b98c` runs only in driver states 0 to 4 through `0x00d81d04`). The detach after an enumerated session therefore needs hardware.

### 8.4 Updater refresh flags for this container

Byte-read:

- The update completion `0x00db91e0` reloads the template through `0x0069fe20(0xff)`, applies the restore pairs and commits the whole area (`0x00db932c`) when the kind-0 flag word `0x014e3578` has bit 0 or 1 set.
- The header parser `0x00db9cd4`, kind 0 from `0x00ec8554`, clears that word at `0x00db9d08`. It sets bit 0 at `0x00dba1e8` when header `+0x244` is not `0xffffffff`, `+0x248` is in 1 to `0x10000` and `+0x24c` is nonzero, and bit 1 at `0x00dba2a8` for `+0x250`, `+0x254`, `+0x258`.
- At file offsets `0x244`-`0x25b` the stock, CC and both toggle DATs carry `0x024a0020 0x00000188 0xffffb2d6 0x024a01a8 0x00000018 0xfffffbff`, the descriptors of the two restore lists (98 and 3 pairs, `rebuild-update verify`), so both bits are set.

`0xf7`, `0x80` and `0x81` lie in no restore pair (inherited). Emulated (E9): through the RAM restore route, the updater installing either toggle DAT over the stored-1 template NAND reads the flag word as 3 at `0x00db91e0`, and stored `0x80`/`0x81`/`0xf7` go from 0/0/1 to 0/0/0. The committed logical block 0 moves from physical block `0x381` to `0x3a2`, and the updated image boots to the shooting screen with `cache[0xf7] = 0`.

### 8.5 Still open

For the power-on variant only: the content of the boot records at `0x0073ae10` for a held Q, and the source of the startup Fn and Q press forms. The detach after an enumerated `ff80` session, the detach after PTP and the held-Q UI effect are now hardware-observed (E-USB-TOGGLE-RAM-PUSH-SESSION-2-150); reading the key driver from usbcont remains open.

## 9. Verification results (host and emulated)

### 9.1 Native cases

`//emulation:usb-toggle-cases` (`emulation/UsbToggleCases.hs`) generates and runs 364 cases under `private-data/analysis/x-m1-1.50/native-case/usb-toggle/`; all pass. Expected values come from the behaviour rule and handler A's byte-read arms, not from the stub. Stock id4, stock id5 and the stub blob are pinned. The run refuses to start unless the stub and hook word equal those of both pinned image-5 patch sets.

- **N1, the stub with the stock closures of `0x0069f8fc`, `0x0069fc94`, `0x006a0d00`/`0x006a0b4c` and `0x0073c15c`: 44/44.** The 40-case matrix is stored `0xf7` × attach bit × Q `+0x15` × system mode {1, 2, 3, 5, 7}. The byte flips in exactly the six cases with attach bit 1, Q 1 and mode 1, 2 or 3, and every other case returns the stored byte unchanged. Four bitmap cases: bit 18 set with `+0x15 = 0` gives no flip, and bit 18 clear with `+0x15 = 1` flips, for stored 0 and 1. Deepest stack 28 bytes.
- **N2, handler A slice `[0x00ecf8b8, 0x00ecf9c8)`: 160/160 patched and 160/160 stock.** Latch × new bit × stored `0xf7` × Q × mode × `0x0073abec` result, with recording stubs for the trace, the `0x1705`/`0x1706` posts, the registration and the application event. All 80 Q = 0 pairs give identical events, `r0`, latch and cache byte on patched and stock.

### 9.2 Builds and full-system boot (E1)

Both candidates build pinned and reproduce their digests; `extensions/rc0.json` still reproduces `acb0fbbb…ca8`. `//emulation:flash-boot-check` passes for CC+toggle (receipt `private-data/flash-boot-checks/usb-toggle-cc-1/`, SHOOTING MENU at 450,017,580 instructions) and RC0+toggle (`usb-toggle-rc0-1/`, 450,019,375).

After the rebase of `agent/f7-toggle` onto main `0f0b694` (2026-10-22), on commit `030653e`, `build-usb-toggle --output-sha256 10feac47…2445 extensions/rc0.json` writes an image-5 patch set equal to the pinned one apart from `recorded_at` and an unchanged `rc0-patch-set.json`. `build-extension` on the pinned manifest and the stock DAT prints PASS with `10feac474eee5ac178ff454397f1b4f1bc1c9a01ae4da8de8f102fa606ca2445`, and the output is byte-identical to the pinned DAT. The stub blob still hashes `35a7e959…6d62`. `//emulation:flash-boot-check` on the pinned RC0+toggle DAT passes again (receipt `private-data/flash-boot-checks/usb-toggle-rc0-2/flash-boot-receipt.json`, SHA-256 `8c8c20ac…5e9f55`).

### 9.3 Attach scenarios (E2-E8)

`//emulation:usb-toggle-emu` (`emulation/UsbToggleEmu.hs`) drives 24 scenarios from shooting-screen checkpoints in one emulator slot. It pins every DAT and NAND hash, and its expectations come from the behaviour table of 3.2. The bridge additions in `continue_rec_scanout.py` are primitives only: `--toggle-server` (run, key press and release, ADC level, cable level, memory read, trace sites, descriptor enumeration without an expected id, transfer, capture, probe readout, checkpoint), `--cable-at-reset`, `--adc-at-reset`, `--access-probe-range`, and `--attach-hold-key` for the RAM-push attach. Inputs:

- stored-0 NAND `private-data/analysis/usb-toggle/emu/inputs/f7-0.nand` (SHA-256 `8e2b5243…478b`), the stored-1 template with byte `0xf7` restored to 0;
- its body-like variant `f7-0-pid02b6.nand` (`633944533…2e04`, `cache[0x102..0x103] = b6 02`, the body's value), used for E6;
- the stored-1 template `f7-1.nand` (`baf47535…4aa0`).

The template carries `cache[0x100..0x103] = cb 04 80 ff` and PID `0x02b6` at `0x13e`.

| Run | Observed on CC+toggle and RC0+toggle | Result |
| --- | --- | --- |
| E2 | Access probe on `[0x00bf4454, 0x00bf44e4)`, boot plus one attach: stock and CC base record nothing in the extent; each toggle DAT records 12 fetches at `0x00bf4454`-`0x00bf44a8`, entered once, at the instruction after the `0x00ecf900` call. All four also show the three image-wide model transfers that load image 5. | pass |
| E3 | Stored 0, plain attach: application arm, application `0x10`, `0xd350c4(8)`, no `0x1705`, no fill, `cache[0xf7] = 0`, `0x0146a0c0 = 2`, `0x01469810 = 2`, descriptor `04cb:02b6` | pass |
| E4 | Stored 0, Q held across the attach: Q `+0x15 = 1` before the edge; stub entered, `0x006a0d00(0x32)` returns 1; attach arm, `0x00ed117c(7,0)`, `0x00ecc104`, fill `0x007221a0(0x43360000, 0xffffffff, …)`, transport start mode 3; `0x0146a0c0 = 3`, `0x01469810 = 3`, `0x0146a0cc = 0x00d80cf0`, `cache[0xf7] = 1`; no application event; descriptor `04cb:ff80`; C0, identity `X-M1`, config window 0 with `0xf7 = 1`, one function-2 read equal to guest memory; after release `+0x15 = 0`. The frames 8 M and 40 M instructions after release show the shooting screen with no Q panel. | pass |
| E5 | Detach after the enumerated E4 session: no handler A edge (model boundary, 8.3). | blocked |
| E5u | The same edges with enumeration only where named: Q attach `ff80`; detach `0x1706`, no `0x90`, byte stays 1; plain re-attach `ff80` with the full session; detach; Q re-attach flips to 0, application `0x10`, transport 2, `04cb:02b6`. The final detach after the PTP start is not delivered (model boundary). | pass to the last step |
| E6 | Cable present from reset, body-like NAND, with and without Q held from reset: system mode 5, handler B, transport 2, `04cb:02b6`, `cache[0xf7] = 0`, stub never entered. Rerun at `030653e` (`run-4-e6/`, summary `8e015607…b5a0`): 4/4 pass. Superseded: the four E6 `fail` rows in `run-3/summary.json` ran on the template stored-0 NAND and selected maintenance (system mode 7, `ff80`) through its `cache[0x102..0x103] = 0xff80`, with no stub entry. The body keeps `0x02b6` through the install (restore pair `(0x102, 4)`). | pass |
| E7 | Stored 1: plain attach `ff80` with the full session; Q attach flips to 0, application arm, `0x10`, transport 2, `04cb:02b6` | pass |
| Stock controls | Stock DAT, stored 0, plain and Q attach: application arm, PTP both times, no extent fetch, `cache[0xf7] = 0` | pass |
| E8 | `--ram-push-e2e --classic-chrome --attach-hold-q`, RC0+toggle, stored-0 NAND: Q held across the attach, device `04cb:ff80`, `ram-alloc` reads `0xf7 = 1`, allocation `0x00a7e8c8`, 131 publications installed and validated, every CC segment matched, `ram-unload` restored and verified all 131 old words, and the cold restart (Q held) read all 131 old. The after-unload segment shows the known final-step mismatch of classic-chrome-patch.md 8.2.5. | pass |

Receipts: `private-data/analysis/usb-toggle/emu/run-3/` (summary SHA-256 `85b4f955…897f`, one receipt per scenario) and `e8-cc-2/` (`e2e-result.json` `95acaedd…c130`), both run at commit `5d34bfb` with a clean tree and identical in every observed value to the earlier runs, and `run-4-e6/` for E6. The bridge's existing Python unit tests (`test_usb_endpoint`, `test_usb_presence`, `test_portable_checkpoint`, `test_ram_push_park`, `test_ram_push_ui_entry`, 39 tests) pass at `5d34bfb`. The boots are first-start boots (system mode 3). Mode 1 passes the same not-5-or-7 test.

Tally, E1 to E9:

| Run | Scenarios | Pass | Blocked at a detach the model cannot deliver (8.3) |
| --- | ---: | ---: | ---: |
| E1 flash-boot-check | 3 (CC+toggle, RC0+toggle twice) | 3 | 0 |
| E2 access probe | 4 | 4 | 0 |
| E3 plain attach | 2 | 2 | 0 |
| E4/E5 enumerated cycle | 2 | 0 | 2, after E4's steps passed |
| E5u cycles | 6 | 4 | 2, at the final detach |
| E6 cable at reset | 4 | 4 | 0 |
| E7 stored 1 | 4 | 4 | 0 |
| Stock controls | 2 | 2 | 0 |
| E8 RAM push over a Q attach | 1 | 1 | 0 |
| E9 updater | 2 | 2 | 0 |
| Total | 30 | 26 | 4 |

No scenario fails; the four blocked ones stop where the emulated USB controller never reports the cable leaving after a session started, and every step before that passed.

#### Synthetic body-PID input projection

A separate host-only input check constructs the stored-1/body-PID projection needed by the momentary-selection research. It changes only committed data page `0xe040` payload bytes `0x102..0x103` from `80 ff` to `b6 02`, retaining stored `0xf7 = 1`. A fresh stored-0 control first decompressed byte-identically to the existing body-PID stored-0 projection. Reversing only the new stored-1 PID pair restored the complete parent canonical document byte for byte, including spare/OOB, base, overlays and provenance.

Reproduction uses owner-supplied committed NAND inputs and the checked-in devenv's `jq` and `gzip`. Select an original stored-0 template as `INPUT` with `F7=00` for the control first, then the original stored-1 template with `F7=01`. Use a distinct ignored `OUTPUT` for each. Refuse an existing output before writing. The JSON serialization is sorted and compact with no trailing newline; gzip uses level 9 and mtime 0.

Start `devenv shell`; within that shell run:

```sh
set -o pipefail
gzip -dc "$INPUT" |
  jq -cSj --arg f7 "$F7" '
    if ([.committed_data_pages[] | select(.[0] == 57408)] | length) != 1
    then error("nonunique adjustment page") else . end |
    .committed_data_pages |= map(
      if .[0] == 57408 then
        if (.[1] | length) != 4096 or .[1][494:496] != $f7 or .[1][516:520] != "80ff"
        then error("unexpected adjustment payload")
        else .[1] = (.[1][0:516] + "b602" + .[1][520:]) end
      else . end)' |
  gzip -9n |
  dd of="$OUTPUT" conv=excl status=none
```

Before producing stored 1, compare the complete decompressed stored-0 control against the pinned body-PID stored-0 input. For stored 1, compare the complete document after reversing only the PID pair against its original parent, and compare the decoded payloads: the only differing byte positions are `0x102` and `0x103`.

Classify gzip framing separately from document changes. The observed GNU-gzip output changes the OS header byte from `ff` to `03`, with mtime still zero. The control retains its CRC and uncompressed size despite different deflate bytes. The stored-1 projection changes the CRC and deflate bytes while retaining uncompressed size. Compressed identity is not required.

This is a synthetic PID projection, not a full body settings/calibration capture or observed hardware USB status. The historical Q-toggle inputs and results above are unchanged. Fixture construction does not qualify a stock-UI trajectory, alter the strict 512-stop accounting, or establish public CLI support for the unintegrated momentary-selection prototype.

### 9.4 Updater (E9)

`//emulation:updater-f7` (`emulation/UpdaterF7.hs`) runs the ordinary updater through the RAM restore route of E-RECOVERY-RC0-RESTORE-HOST-150: a `//recovery:build-image restore` card from the toggle DAT, the `ram-rescue-engine` raw handoff, then continuation over the stored-1 template NAND with OK, OK, UP, OK, OK as ordinary ADC pulses. It then boots the exported NAND. The observed values are in 8.4. The receipts are `private-data/analysis/usb-toggle/emu-updater/rc0-toggle-updater-2/` and `cc-toggle-updater-2/`, run at commit `5d34bfb`. The exported NANDs are `7da7fb61…9841` (RC0+toggle) and `2814b76f…e465` (CC+toggle), the same as in the earlier runs. The bridge additions are console commands `watch`, `hits`, `read` and `nandstat`, which share the trace class `TraceSites` with the toggle server.

### 9.5 Exact candidates (2026-10-24)

On commit `32101c9` (clean harness tree), after the 8M key-pulse pins and the emulator re-pin of E-MERGE-EMULATOR-REVERIFY-150, every route ran on the two flash candidates' exact bytes. `usb-toggle-emu` gained the design-B base and CC-icon+toggle as DAT roles and a `--dats` filter.

| Run | RC0+toggle `10feac47…2445` | CC-icon+toggle `c1bfc9aa…ad8f` |
| --- | --- | --- |
| E1 `flash-boot-check` | passed, SHOOTING MENU (`private-data/flash-boot-checks/usb-toggle-rc0-3/`, receipt `5b428e78…391d`) | passed, SHOOTING MENU (`usb-toggle-ccb-1/`, receipt `8ba65249…5a03`) |
| E2 access probe | pass: the stub path only, after the `0x00ecf900` call | pass; the design-B base records nothing in the extent |
| E3, E5u-ff80, E5u-q-ptp, E6 (two), E7 (two) | all pass | all pass |
| E4/E5 and E5u-cycle | every step passes up to the detach after an enumerated session or after a PTP start, which the model cannot deliver (8.3) | same |
| E8 Classic Chrome RAM push over a Q attach, stored 0 | pass: `ff80`, allocation `0x00a7e8c8`, 131 publications installed, 7 of 7 menu receipts, `ram-unload` verified 131 old words, the cold restart read 131 old (`private-data/analysis/usb-toggle/emu/e8-rc0-3/e2e-result.json`, `d00e18e1…65a1`) | not run: the reviewed CC RAM payloads publish over RC0 images, and a CC-flashed image already holds those words. E4's C0, identity, config and function-2 session is the service-channel check |
| E9 updater over stored 1 | flag word 3, stored `0x80`/`0x81`/`0xf7` 0/0/1 to 0/0/0, reboot `cache[0xf7] = 0`, hook word `0xebf492d3`; exported NAND `7da7fb61…9841`, equal to the 2026-10-22 runs (`emu-updater/rc0-toggle-updater-4/`, receipt `64a59a83…5809`) | the same values; exported NAND `c5bff9d8…d9b5` (`emu-updater/ccb-toggle-updater-3/`, receipt `90c5d546…e09f`) |

`usb-toggle-emu` with `--dats stock,rc0-toggle,cc-icon-base,cc-icon-toggle`: 24 scenarios, 20 pass and 4 blocked at the undeliverable detach, 598 checks passed and 4 not reached (`private-data/analysis/usb-toggle/emu/run-5-candidates/summary.json`, `4105ebd4…36ed`). Both restore cards carry the candidate DAT byte for byte (`emu-updater/restore-{rc0,ccb}-3/`).

### 9.6 Local StockClick prototype (unintegrated)

A local unintegrated stock-UI click prototype is not in main `699f59b` and is not part of this documentation change. It is recorded here as a bounded result; the implementation remains unintegrated and unqualified.

The ADC path matched the source path. The scanner uses a phase-dependent two-sample debounce, so a fixed 3M or 8M hold is not a universal release or latch law. The retained observer and source bindings are in `private-data/analysis/usb-select/cost-fast1-20261026/analysis.md#guest-row-and-adc-binding` and `emulation/README.md#interactive-x-m1-firmware-viewer`; the corrected2 receipt alone does not contain that separate observer result.

In the one recorded run, the first Up click at the FOCUS MODE root moved the offered cursor from 2 to 32, the debounced row reached 1, the release reached 0, and the full 15-field publication predicate drained.

| Phase | Runs | Retired instructions |
| --- | ---: | ---: |
| Latch | 48 | 196,164 |
| Release | 92 | 13,348,269 |

Two further Up clicks completed from 32 to 31 and 31 to 30. The fourth Up attempt started from cursor 30 and was blocked during release before release/drain completion was verified. It stopped at the existing 512 timer-normalizer stops; no landed cursor was recorded. The first S scenario did not run. The result establishes three completed clicks at cursors 32, 31 and 30 and one blocked fourth attempt, not a complete path. Legacy instruction parity is unmeasured, and six-scenario or 144-endpoint acceptance is not claimed.

The implementation remains unintegrated and unqualified. This section does not approve it or a camera action.

## 10. Changed-byte explanation

### 10.1 Candidates

| Item | RC0+toggle | CC-icon+toggle |
| --- | --- | --- |
| DAT | `private-data/analysis/usb-toggle/rc0-pinned-dat/FWUP0002.DAT`, 38,405,148 bytes, SHA-256 `10feac474eee5ac178ff454397f1b4f1bc1c9a01ae4da8de8f102fa606ca2445` | `private-data/analysis/usb-toggle/ccb-pinned-dat/FWUP0002.DAT`, 38,405,148 bytes, SHA-256 `c1bfc9aa995861ff8a90729bf0e0e5e7bfe0747c1bac69adec2a72d7f7f0ad8f` |
| Pinned manifest | `private-data/analysis/usb-toggle/rc0-pinned/usb-toggle.json` | `private-data/analysis/usb-toggle/ccb-pinned/usb-toggle.json` (`2fb8b01b…3df3`) |
| Image-5 patch set | `usb-toggle-image5.json`, the hook word and the stub blob | `usb-toggle-image5.json` (`c15ff823…c1da`), the 53 design-B words, the hook word and the stub blob |
| Base | RC0 `acb0fbbb…ca8`; `rc0-patch-set.json` copied unchanged | design-B `98a66780…1606` (classic-chrome-icon.md 8.1, manifest `parts-b-1-pinned/classic-chrome-icon.json`); its image-4, image-2, font and plain parts and both records copied unchanged |
| Image-4 load | `0x0061c340`, decoded on this body | `0x0061c200`, never decoded by the hardware engine |
| Replaces on the body | the installed RC0 container | the installed RC0 container |
| Revert | reinstall RC0 `acb0fbbb…ca8` or stock through the same update route; nothing the toggle stores survives a power-off | stock film in the shooting setting and in C first (classic-chrome-patch.md 11.1), then the same |

CC+toggle `507ecf32…0e1e` (`cc-pinned/`, on `16c830ad…2c89`) is superseded with its base and kept only as the history of E-USB-TOGGLE-EMU-150.

Pinning followed CC's: an unpinned `build-extension` build gave the digest, `build-usb-toggle --output-sha256` wrote it into a fresh manifest, and the pinned `build-extension` reproduced it (`PASS`, 45/45 contract checks). On 2026-10-24 at `32101c9`, and again after the L1/L2 change of 11.5, `build-usb-toggle` over `extensions/rc0.json` and over design-B's manifest writes parts equal to `rc0-pinned/` and `ccb-pinned/` apart from `recorded_at` (and through it the image-5 step's SHA-256), and `build-extension` on them gives DATs byte-identical to both pins.

### 10.2 Every changed byte

Toggle writes, both candidates (`changed-words.md` in each pinned directory):

| Address | Current | New | Changed bytes | Role |
| --- | --- | --- | ---: | --- |
| `0x00bf4454` | 88 bytes of the stock extent (hash in 5.1) | the 22 stub words of 3.1 | 78 | stub |
| `0x00ecf900` | `fd3fdfeb` | `d392f4eb` | 3 | `bl 0x00bf4454` replaces `bl 0x0069f8fc` |

Decoded images, byte-compared after `rebuild-update extract-record5` and `fflzd`: RC0+toggle image 5 (`cb987771…7739`) differs from stock image 5 only within `[0x00bf4454, 0x00bf44ac)` and at `0x00ecf900`, and CC-icon+toggle image 5 (`a9a994b6…cda5`) differs from design-B image 5 (`859ab5b0…6454`) in the same 81 bytes, 78 in the stub range and 3 in the hook word; the stub range equals the linked blob `35a7e959…6d62`, the rest of the extent stays stock, and the toggle bytes equal RC0+toggle's. `rebuild-update diff` and a raw byte compare of CC-icon+toggle against design-B: only the main checksum word `0x20c`-`0x20d`, the image-5 header word at `0x2c0260` (2 bytes, record length `0x2c66d1` to `0x2c670c`) and the re-encoded stream `[0x2c22e5, 0x586968)` differ; the window tail after the stream, identity records, selector, fill, segment table, image-4 header and stream, image-2 extent, font window, plain window, trailer and both restore lists are byte-equal. DAT header bytes `0x244`-`0x25b` equal stock (8.4). Private artifacts: `private-data/analysis/usb-toggle/ccb-review-diff/`.

## 11. Review records and reviewer packets

Sections 11.1 to 11.3 are the packet the independent review of RC0+toggle used, as it stood at `4c55c79`. 11.6 is the brief the independent review of CC-icon+toggle used. 11.5 holds both reviews' records. Both installs are H3 (spec 07 section 3.2); the reviews cover new mechanism bytes, because the hook word redirects stock control flow and the stub runs outside any payload allocation.

RC0+toggle: DAT SHA-256 `10feac474eee5ac178ff454397f1b4f1bc1c9a01ae4da8de8f102fa606ca2445`, replacing the installed RC0 container `acb0fbbb…ca8`.

### 11.1 What differs from the installed RC0 image

Byte-compared on commit `030653e`, against `extensions/rc0.json` rebuilt to `acb0fbbb…ca8`:

- `rebuild-update diff`: identity records, selector, fill, segment table, image-4 header and stream, the plain window, image-2 extent, trailer and both restore lists unchanged. The image-5 header (1 byte, the stream length) and the image-5 flash window (the re-encoded stream; the tail after `0x586954` stays `0xff`, equal to stock) differ, and so does the main checksum word at DAT `0x20c` (2 bytes). The relation `stored - bytesum(file[0x25c, +0x24a0020)) = 0xb849e01f` holds for stock, RC0 and the candidate (independent review N1). The diff tool's "main post head" count compares shifted offsets and is not a change.
- Decoded image 5 (`extract-record5`, `fflzd`): RC0's is stock (`3cd501c0…fd892a`); RC0+toggle's is `cb987771…7739` and differs in 81 bytes, all in `[0x00bf4454, 0x00bf44ac)` (78 bytes, the stub) and `[0x00ecf900, 0x00ecf904)` (3 bytes, the hook word).
- `rebuild-update verify`: 45/45 contract checks pass.
- The factory template the update refresh reads (raw `[0xe025c, 0x12225c)`) and both restore lists (raw `[0x24a027c, 0x24a041c)`) are byte-equal to stock (the `cmp` commands of M3, 12.1).

### 11.2 Inputs

- this report, sections 2 to 10;
- `host/firmware/usb-toggle-stub.S` and `.ld`, `host/FujiAF/UsbToggle.hs`, `host/firmware/BuildUsbToggle.hs`, `host/firmware/UsbToggleTests.hs`, the new targets in `host/BUILD.bazel` and `host/firmware/BUILD.bazel`;
- `private-data/analysis/usb-toggle/rc0-pinned/` (`usb-toggle.json`, `usb-toggle-image5.json`, `rc0-patch-set.json`, `changed-words.*`) and `rc0-pinned-dat/FWUP0002.DAT`;
- the stock decodes under `private-data/analysis/x-m1-1.50/lz/` and the listings command of section 13;
- the native cases (9.1, `emulation/UsbToggleCases.hs`), the emulator drivers (`emulation/UsbToggleEmu.hs`, `emulation/UpdaterF7.hs`, the `continue_rec_scanout.py` primitives) and their receipts (9.2 to 9.4).

Commands the reviewer can rerun without a camera:

```sh
devenv shell -- bazel test //host/firmware:usb-toggle-tests
devenv shell -- bazel run //host/firmware:build-usb-toggle -- --output-sha256 10feac474eee5ac178ff454397f1b4f1bc1c9a01ae4da8de8f102fa606ca2445 extensions/rc0.json private-data/analysis/usb-toggle/REVIEW_PARTS
devenv shell -- bazel run //extensions:build-extension -- private-data/analysis/usb-toggle/rc0-pinned/usb-toggle.json "$(readlink -f private-data/firmware/x-m1-1.50/FWUP0002.DAT)" private-data/analysis/usb-toggle/REVIEW_DAT
devenv shell -- bazel run //host/firmware:rebuild-update -- diff RC0_DAT private-data/analysis/usb-toggle/rc0-pinned-dat/FWUP0002.DAT
devenv shell -- bazel run //emulation:usb-toggle-cases
```

### 11.3 Questions for the reviewer

1. Does the stub keep handler A's contract at `0x00ecf904`: only `r0` consumed, `r4`-`r11` and SP restored, no flag relied on? Handler A reloads `r3` before its next use (`0x00ecf910` onward); check that no other caller-saved register is live across `0x00ecf900`.
2. Is ownership of `[0x00bf4454, 0x00bf44e4)` by absence of references (5.2) sufficient, together with the E2 runtime watch?
3. Is reading row byte `+0x15` through `0x006a0d00` from the usbcont context sound, given that the key driver writes it from its scan context? It is one byte read per attach edge; a stale read gives a plain attach or a flip, never a store outside `cache[0xf7]`.
4. Does a flip in system mode 1, 2 or 3 leave every other `0xf7` reader consistent (2.4), in particular the detach arm and the debug-callback registration `0x00647398`?
5. Is the hook encoding `ebf492d3` exactly `bl 0x00bf4454` from `0x00ecf900`, and do the stub's four BL words reach `0x0069f8fc`, `0x006a0d00`, `0x0073c15c` and `0x0069fc94`?
6. Does anything in the container differ from RC0 outside the image-5 header and flash window (11.1)?
7. Is the install session script of section 12 complete and its stop rules sufficient?

### 11.4 Gates before the install

| Gate | RC0+toggle `10feac47…2445` | CC-icon+toggle `c1bfc9aa…ad8f` |
| --- | --- | --- |
| 1. Independent review of the new bytes and the container diff | Approved by `toggle-review`, 2026-10-22 (11.5). Closed | Approved by `ToggleCompReview`, 2026-10-24, no must-fix (11.5); design-B bytes by CcIconReview, toggle bytes by `toggle-review`. Closed for this DAT only |
| 2. `//emulation:flash-boot-check` on the exact DAT | Passed three times, last at `32101c9` on 2026-10-24 (9.5) | Passed at `32101c9` on 2026-10-24 (9.5) |
| 3. The owner's visual check, `devenv shell -- bazel run //emulation:xm1-viewer -- --firmware IMAGE`: live view, SHOOTING MENU, FILM SIMULATION and the Q menu | Not done. Expect RC0's seven rows | Not done for this DAT. The design-B RAM push showed the same CC rows, icon and Q tiles on the body (E-CC-ICON-RAM-PUSH-SESSION-150); expect eight rows and the CHROME tile |
| 4. Boot path (spec 07 section 3.2: no open prerequisite) | None added: image-4 load `0x0061c340`, decoded on this body (E-FILMSIM-INSTALL-150); image 5 keeps its load and decoded size | Open: image-4 load `0x0061c200`, never decoded by the hardware engine (classic-chrome-patch.md 11.3 item 5). Owner decision needed, below |
| 5. Card slot | Repaired and the card checks of 12.3 passed (#85) | Same |
| 6. The owner's per-run go of 12.8 | Not given | Not given |

**Owner decision needed (CC-icon+toggle gate 4).** Spec 07 section 3.2 makes "no boot-path prerequisite is open" a precondition of every H3 run. CC-icon+toggle inherits from design-B an image-4 load at `0x0061c200` that the hardware decompressor has never decoded; the composition adds no boot-path item (ToggleCompReview N2, N4). The spec offers no substitute for that precondition, and a written acceptance in the session notes is not one. The two ways to clear it: (1) the owner records an explicit exception to spec 07 section 3.2 for this digest, naming the `0x0061c200` load, in the spec's section 3.4 style with date, scope and lapse; or (2) the `0x0061c200` load is proved first, on hardware, before this install. Until one is done, CC-icon+toggle is not a flash target. RC0+toggle avoids the question: it keeps RC0's `0x0061c340`, which this body has decoded.

After the install, the RAM-push session scripts and the transport README's ordinary-boot prerequisites change from a stored `0xf7 = 1` to a Q attach.

### 11.5 Review records

**RC0+toggle.**

`toggle-review` (`anthropic/claude-opus-5-5`, not the author) reviewed `agent/f7-toggle` at `4c55c79` on 2026-10-22. Record: `private-data/analysis/usb-toggle-review/review.md` (SHA-256 `a624c73c0a6847b88e2ccb72bac5f7eb75877c9df2e7eabc6a8f3f49247feab8`), with the reviewer's own rebuilds, decodes and native cases beside it.

- First verdict: approve with must-fix, no blocker. The reviewer rebuilt both DATs, scanned the raw diff against RC0, decoded image 5, checked the hook encoding, the stub's register and stack contract, the four callees, the extent census over the eight inputs, every storage writer of the adjustment area and the key census.
- Must-fix M1 to M5: must-shows after the A8, A9 and A10 unplugs; explicit stops for `ff80` at A3 or in boot 3 and for an A7 mismatch; no `gphoto2`, because PTP requests outside `//transport:camera` are unjournaled camera access; Q's debounce is `0x006a2fa8` and `+0x15` has more key-driver writers; Q-boot limits for whole-area commits and gate-writing operations.
- Notes N1 to N9: N1, N2, N6 and N7 are folded into 11.1, 12.6 and 9.3; N3 (builder checks only A32 BL and only the 88 written bytes), N4 (constant-index setter census), N5 (one unexplained model transfer), N8 (run provenance) and N9 (a computed-base caller would reach the stub, RAM-only effect) stand and do not gate the install.
- Closing: all five fixed in `0de0811` with the image bytes unchanged. **Final verdict: approve** for DAT `10feac47…2445`.

Since `0de0811` the stub sources (`usb-toggle-stub.S`, `.ld`) are unchanged. `BuildUsbToggle.hs` changed at `32101c9` in which base steps it copies; after ToggleCompReview the step assembly moved into `FujiAF.UsbToggle` with tests (L1) and the merge refuses the whole extent (L2). The emitted bytes are unchanged: both pinned candidates rebuild byte-identical (10.1).

**CC-icon+toggle.** `ToggleCompReview` (`anthropic/claude-opus-5-5`, not the author of the toggle, design-B or the composition) reviewed `agent/UsbToggle` at `90fdd93` on 2026-10-24 against the brief of 11.6. Record: `private-data/analysis/usb-toggle-ccb-review/review.md` (SHA-256 `9f0a71271b7e7af76dc8393621aef3bdaba40c56b72e3ea5bfc03072b1d25eff`), with its own rebuilds, decodes, diff evidence and emulator receipts beside it.

- Verdict: **approve**, no must-fix, for DAT `c1bfc9aa…ad8f`. Every byte that differs from design-B is one of the 81 toggle bytes approved on RC0+toggle, at the same addresses with the same values; no design-B write touches a toggle dependency (callees, handler A, the key driver, the condition rows, the extent tail, the 13 `0xf7` readers, handler B, the startup barrier, the boot USB key, the whole-area commit callers); CC code neither reads nor writes `cache[0xf7]`.
- The reviewer's own runs on the exact bytes: `build-usb-toggle` and `build-extension` reproduce the DAT; flash-boot-check passed (receipt `c2f5c418…cd7d`); `usb-toggle-emu` on CC-icon+toggle 8 pass and 2 blocked at the undeliverable detach, and E2 on the design-B base 14 of 14; `updater-f7` over stored 1 ends 0/0/0 with the same exported NAND `c5bff9d8…d9b5`.
- L1 (the step copy untested) and L2 (overlap checked against the 88 stub bytes, not the whole extent; review N3 of `toggle-review`): fixed with tests, 6.1.
- I1 (the merged set drops design-B's `order_rule`, `unchanged_surfaces` and `tier`): documented, 6.1. I2 (`usb-toggle-emu` shared a fixed runtime directory): it now stages a unique runtime per run.
- N3 (flash-boot-check stops at SHOOTING MENU, so FILM SIMULATION and Q on this DAT rest on byte identity until gate 3) and Q8 (photograph the list on a film-list stop): 12.7. N4 (gate 4 is not substitutable under spec 07): 11.4.
- The approval does not close gates 3 to 6.

### 11.6 Reviewer brief (CC-icon+toggle)

CC-icon+toggle, DAT SHA-256 `c1bfc9aa995861ff8a90729bf0e0e5e7bfe0747c1bac69adec2a72d7f7f0ad8f`, is the reviewed design-B image `98a66780…1606` plus the reviewed toggle. The review covers what neither earlier review saw: the composition, so that every byte differing from `98a66780…1606` is a toggle byte already approved on `10feac47…2445`, and the toggle still behaves on the CC code base. It does not re-review CC, the icon, the Q tiles or the toggle design.

Inputs:

- this report: 6.1 (composition), 10 (changed bytes), 9.5 (runs on the exact bytes);
- `host/firmware/BuildUsbToggle.hs` at `32101c9` (the step-copy change) and `git diff 9d28ba1 32101c9`;
- `private-data/analysis/usb-toggle/ccb-pinned/` (`usb-toggle.json`, `usb-toggle-image5.json`, the four copied parts, `changed-words.*`) and `ccb-pinned-dat/FWUP0002.DAT`;
- the base: `private-data/analysis/classic-chrome-icon/parts-b-1-pinned/` and `build-b-1-pinned/FWUP0002.DAT`;
- the reviewed toggle image `private-data/analysis/usb-toggle/rc0-pinned-dat/FWUP0002.DAT` and its decoded image 5 `private-data/analysis/usb-toggle-review/cand-id5.bin`;
- decodes and diff artifacts in `private-data/analysis/usb-toggle/ccb-review-diff/`;
- receipts of 9.5.

Commands, no camera:

```sh
devenv shell -- bazel test //host/firmware:usb-toggle-tests
devenv shell -- bazel run //host/firmware:build-usb-toggle -- --output-sha256 c1bfc9aa995861ff8a90729bf0e0e5e7bfe0747c1bac69adec2a72d7f7f0ad8f private-data/analysis/classic-chrome-icon/parts-b-1-pinned/classic-chrome-icon.json private-data/analysis/usb-toggle/REVIEW_PARTS
devenv shell -- bazel run //extensions:build-extension -- private-data/analysis/usb-toggle/ccb-pinned/usb-toggle.json "$(readlink -f private-data/firmware/x-m1-1.50/FWUP0002.DAT)" private-data/analysis/usb-toggle/REVIEW_DAT
devenv shell -- bazel run //host/firmware:rebuild-update -- diff "$PWD/private-data/analysis/classic-chrome-icon/build-b-1-pinned/FWUP0002.DAT" "$PWD/private-data/analysis/usb-toggle/ccb-pinned-dat/FWUP0002.DAT"
bazel-bin/host/firmware/rebuild-update extract-record5 DAT RECORD5 && bazel-bin/lz/fflzd RECORD5 0 ID5 5185120
```

Questions:

1. Does the merged `usb-toggle-image5.json` hold exactly the 53 words of `cc-icon-image5.json` with their current and new values, plus the hook word and the 88-byte stub blob, and nothing else?
2. Are the copied parts and the two image-2 records byte-equal to design-B's, in design-B's step order?
3. Does the raw DAT differ from `98a66780…1606` only in the main checksum word, the image-5 header word and the image-5 stream (10.2), with the checksum relation of 11.1 holding?
4. Does the decoded image 5 differ from design-B's only in the 81 toggle bytes, and are those bytes and the stock remainder of the extent identical to the reviewed RC0+toggle decode?
5. Does any design-B write (image 4 including the prefix at `0x0061c200`, image 5, plain, image 2, font) touch the stub's callees `0x0069f8fc`, `0x006a0d00`, `0x006a0b4c`, `0x0073c15c`, `0x0069fc94`, handler A `[0x00ecf8b8, 0x00ecf9c8)`, the key driver `[0x006a0cc4, 0x006a3084)`, the condition rows `[0x00828120, 0x00828510)` or `[0x00bf44ac, 0x00bf44e4)` (review N3)? Does CC code read or write `cache[0xf7]`?
6. Is the step-copy change in `BuildUsbToggle.hs` correct, and does it leave every refusal of 6.1 in force?
7. Do the 9.5 runs cover the composition, given that E8 (the Classic Chrome RAM push over a Q attach) does not apply to a CC-flashed image?
8. Is section 12, with the candidate rows of 12.0, complete for this image?

Gate: closed by `ToggleCompReview` (11.5).

## 12. Install session script

A script, not a run, for either candidate, modelled on [the install procedure](install-procedure.md), [the RC0 install session](filmsim-install-session-script.md) and the Classic Chrome flash script (classic-chrome-patch.md section 12.2). It follows [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs): the model runs host commands, the owner handles the camera. Nothing here authorizes a camera operation; the install runs only after the gates of 11.4 and the owner's per-run go of 12.8. Record every item in `private-data/sessions/<date>-usb-toggle-flash/notes.md`; any item not done is a stop.

The 2026-10-22 attempt stopped before the update when pin 2 of the card slot broke as the card went back in (E-STORED-F7-REENABLE-RUN-150); the slot had already been damaged on 2026-10-21 and seated a card only over a shim (E-SD-SLOT-DAMAGE-150). The update reads its file from the card, a repeat update after an interruption needs the card again, and the SD recovery route needs the slot (E-XM1-CARD-SLOT-UNAVAILABLE-150). This script therefore starts only on a repaired slot, checks the slot in the same session before the update, and puts the card into the camera once: from D5 to the end of the session the card stays in, and everything after D5 uses it in place.

### 12.0 Candidate

The go names one candidate. `STOCK` is `$(readlink -f private-data/firmware/x-m1-1.50/FWUP0002.DAT)`.

| Item | RC0+toggle | CC-icon+toggle |
| --- | --- | --- |
| `IMAGE` | `private-data/analysis/usb-toggle/rc0-pinned-dat/FWUP0002.DAT` | `private-data/analysis/usb-toggle/ccb-pinned-dat/FWUP0002.DAT` |
| `DIGEST` | `10feac474eee5ac178ff454397f1b4f1bc1c9a01ae4da8de8f102fa606ca2445` | `c1bfc9aa995861ff8a90729bf0e0e5e7bfe0747c1bac69adec2a72d7f7f0ad8f` |
| `MANIFEST` | `private-data/analysis/usb-toggle/rc0-pinned/usb-toggle.json` | `private-data/analysis/usb-toggle/ccb-pinned/usb-toggle.json` |
| Size | 38,405,148 bytes | 38,405,148 bytes |
| `FILMS` after the install | seven rows: PROVIA, Velvia, ASTIA, B&W, SEPIA, PRO Neg Std, PRO Neg Hi | eight rows: the seven, then CLASSIC CHROME with the CHROME cartridge icon |
| `QFILMS` after the install | as before the install | eight Q films; the CC tile and full-size PRO Neg Std and Hi tiles (E-CC-ICON-RAM-PUSH-SESSION-150) |
| Extra acceptance | none | classic-chrome-patch.md 12.2.8 A2b to A7, run in A11 |

### 12.1 Before the session (model)

| Code | Action | Must show |
| --- | --- | --- |
| M1 | On the session commit: `sha256sum "$IMAGE"`; `build-extension` from `MANIFEST` into a fresh directory and `cmp` against `IMAGE` | `DIGEST` both times; `cmp` prints nothing |
| M2 | The 11.4 gates for this candidate recorded: review, flash-boot receipt, visual check, boot path, slot | All present; for CC-icon+toggle gate 4 cleared by one of the two routes of the 11.4 owner decision |
| M3 | `cmp --ignore-initial=918108 --bytes=270336 "$STOCK" "$IMAGE"` and `cmp --ignore-initial=38404732 --bytes=416 "$STOCK" "$IMAGE"` | Both print nothing (template and restore lists equal stock, 12.2) |
| M4 | Confirm the adjustment backup (E-ADJ-BACKUP-DURABLE-150, `c82ac937…199c`, two locations) | Present; since then only `0x80`, `0x81` and `0xf7` changed |
| M5 | The owner reports the slot repaired (#85; the repair is invasive work, planned with the owner first) | Repair reported; a card latches home without a shim and the door closes over it |

### 12.2 What the update does to stored state

The update path is S4 of [the install procedure](install-procedure.md#2-what-the-camera-does-when-inside-the-update): `0x00db91e0` snapshots `[0, 0x42000)`, refreshes it from the template, restores the listed bytes and commits the area when the flag word `0x014e3578 & 3` is nonzero (byte-read; the setter `0x00db9cd4` sets both bits for both candidates, 8.4). The template holds `00` at `0x80`, `0x81` and `0xf7`, and no restore pair covers them. The executing updater is RC0's stock image 5, and for S4 both containers' parts equal RC0's (M3), so the RC0 install's outcome repeats (static-inference; hardware-observed for RC0 in E-FILMSIM-READBACK-150). The body now holds stored `0xf7 = 01` with `0x80 = 22` and `0x81 = ff` expected (E-STORED-F7-REENABLE-RUN-150); after the update all three read `00`. E9 emulated both candidates over stored `0xf7 = 1` and ended with 0/0/0 (9.4, 9.5). If the refresh did not run, stored `0xf7` stays 1 and A3 shows `ff80`, a stop.

### 12.3 Card and slot, one insertion

PTP offers no write path: the card store reports Access Capability Read Only with Object deletion (private session `20261022T060247Z-ptp-summary`). The file goes by card reader, before the card first enters the repaired slot.

| Code | Action | Must show |
| --- | --- | --- |
| D1 | Owner: camera off, USB unplugged. The card is in the host's reader, not in the camera | Card mounted read-write |
| D2 | Model: `ls -la "$CARD" "$CARD/DCIM"` | No `FWUP*.DAT`; free space well above 38,405,148 bytes; record whether `DCIM/963_FUJI/DSCF3149.RAF` is present (CC-icon+toggle A11 uses it). Do not format |
| D3 | Model: `cp --no-clobber "$IMAGE" "$CARD/FWUP0002.DAT" && sync` | Exit 0; the name exactly `FWUP0002.DAT` |
| D4 | Owner: unmount, remove and reinsert the card in the reader. Model: `ls -la "$CARD"` and `sha256sum "$CARD/FWUP0002.DAT"` | Size 38,405,148 and `DIGEST`, read after a fresh mount. Any other value: delete the copy and repeat D3-D4 |
| D5 | Owner: unmount, eject, insert the card in the camera with the camera off until the latch holds it; close the door. This is the session's only insertion | Card latched, door closed, no shim |
| C1 | Owner: ordinary power-on, no button held, no USB | No CARD ERROR or NO MEMORY CARD; the shooting screen shows a frame count. `FWUP0002.DAT` runs only at a DISP/BACK power-on (install procedure section 6), so an ordinary boot ignores it (static-inference) |
| C2 | Owner: one FINE+RAW shot on a fixed scene in PROVIA; play it back | Saved and played back with no error |
| C3 | Owner: switch off and on twice, opening nothing; play the C2 shot back each time | No shutdown, no card message, the shot plays back |

Any failure in D5 to C3 (the card does not latch, a card message, a shutdown, a save or playback error) ends the session before the update. The card stays where it is; report to the model. The C acts are the card-present boot and capture/save/playback of #85 acceptance; its card read-back follows at the next card removal, after this session.

A damaged copy fails D4. Inside the camera a damaged file fails one of the three sums (status 5) before S1 (byte-read, install procedure section 1). The cached-success byte `0x15c1e58` survives until power is removed, so after any aborted attempt switch off and cut the supply before another one (install procedure section 7).

### 12.4 Before the camera acts (owner)

| Code | Owner action | Must show |
| --- | --- | --- |
| P1 | Photograph the watched settings and the C (CUSTOM) values | Record in the notes |
| P2 | SHOOTING MENU > FILM SIMULATION: photograph the list; set a stock film in the shooting setting and in C (CC-icon+toggle revert precondition) | RC0's seven rows; a stock film in both |
| P3 | Dummy-battery supply on mains, connector secured, not on a shared switch; camera on a stable surface; lens mounted; USB unplugged | Install procedure section 5 |
| P4 | Camera off. Start a video of the screen and a clock | |

### 12.5 Camera acts

The screen order is the RC0 run's (E-FILMSIM-INSTALL-150, E-INSTALL-TIMING-150) and the vendor procedure. Do not advance an unfamiliar screen by guessing.

| Code | Owner action | Must show |
| --- | --- | --- |
| I1 | Hold DISP/BACK, switch on, release when the firmware screen appears | The firmware upgrade screen with body version 1.50 |
| I2 | Inspect before pressing anything | The update offered as 1.50 to 1.50, BODY route, no file error, low-battery or refusal. Anything else: abort (12.7) |
| I3 | If item selection appears, select BODY and press MENU/OK once | BODY, both versions 1.50 |
| I4 | At FIRMWARE UPGRADE OK?, select OK and press MENU/OK once; at a battery or power warning with BACK CANCEL, pause without pressing its OK | The last screen with a cancel |
| I5 | Press the final OK once, only under the go | PROCESSING with progress squares. This is the point of no return |
| I6 | Touch nothing: no button, card door, battery door, POWER switch, cable or supply | Progress; the RC0 install took 57.3 s from confirm to completion with a longest unchanged stretch of 13.7 s, which does not bound this image |
| I7 | Wait for the completion screen asking to turn the camera off | Completion; record the screen and elapsed time |
| I8 | Switch off as instructed and wait for shutdown. Leave the card in the camera | Ordinary shutdown |

`FWUP0002.DAT` stays on the card. It runs only at a DISP/BACK power-on; delete it with the reader at the next card removal, which is not part of this session.

### 12.6 Acceptance after install

Ordinary boots, the camera on the dummy supply, the card in place, at the shooting screen with no menu open before each attach. Each cable act is one plug or unplug of the camera end; the host end stays connected. PTP is checked by the `lsusb` product id and by the owner browsing the card in his own file manager (an owner action); the model sends no PTP request. The model's host operations are `lsusb -d 04cb:` (reads descriptors the OS already holds) and one `read-config`, which the go names (12.8); the read grant of spec 07 section 3.4 lapses at every unplug, so it does not cover them.

| Code | Act | Must show |
| --- | --- | --- |
| A1 | Boot normally, no USB | No setup menu; date, time, language, the P1 settings and C values unchanged; one shot and its playback normal |
| A2 | SHOOTING MENU > FILM SIMULATION; photograph the list; also the Q menu's film tile | `FILMS` and `QFILMS` of 12.0 |
| A3 | Boot 1. Plug the cable without touching Q. Model: `devenv shell -- lsusb -d 04cb:` | `04cb:02b6` and no `04cb:ff80`; record what the camera displays (stock application USB mode) |
| A4 | Owner: open the camera in the host's file manager and browse `DCIM` | The card's folders and recent frames are listed; one frame opens |
| A5 | Unplug. Switch off | Record the display after the unplug; then an ordinary shutdown |
| A6 | Boot 2. At the shooting screen, press and hold Q, plug the cable while holding it, keep holding about two seconds, release Q, then press DISP/BACK if the Q menu is open. Record whether the Q menu opened and when Q was pressed relative to the plug. Model: `lsusb -d 04cb:` | `04cb:ff80`; menus and live view stay usable (the 41 MB fill of 2.3 runs, as on every stored-`0xf7` attach before) |
| A7 | Model: `devenv shell -- bazel run //transport:camera -- read-config "$PWD/private-data/read-plans/gate-byte.json"` | Product `X-M1`, `55 aa` at `+0x202`; window 0 reads `0xf7 = 01` and equals the reference `20261016T044012Z-read-config` window 0 outside `0x80`, `0x81` and `0xf7`; record `0x80` and `0x81` (expected `00` and `00`, 12.2) |
| A8 | Unplug. Then wait five seconds and plug the cable without touching Q. Model: `lsusb -d 04cb:` | After the unplug (detach after an enumerated `ff80` session): record the display; MENU opens and closes, live view runs, no reboot. After the plug: `04cb:ff80` again (the RAM byte stays 1) |
| A9 | Unplug. Then wait five seconds, hold Q, plug, hold about two seconds, release, close the Q menu if open. Model: `lsusb -d 04cb:`. Owner: browse `DCIM` as in A4 | After the unplug (detach after a plain `ff80` session): record the display; MENU opens and closes, live view runs, no reboot. After the plug: `04cb:02b6` and no `ff80`, and the card browses as in A4. This is the second Q attach returning PTP |
| A10 | Unplug. Switch off. Boot 3, plug the cable without touching Q. Model: `lsusb -d 04cb:`. Unplug | After the first unplug (detach after a PTP start from a byte flipped back to 0): record the display; MENU opens and closes, live view runs, no reboot; then an ordinary shutdown. In boot 3: `04cb:02b6`, the power cycle returned to the stored byte; after its unplug the same display checks |
| A11 | Without USB: SHOOTING MENU, FILM SIMULATION and the Q menu as in A2; one shot in a PRO Neg film and its playback; for CC-icon+toggle the extra acceptance of 12.0, whose A4 RAW conversion uses `DSCF3149.RAF` if D2 found it and otherwise the C2 RAF without the comparison; switch off and on | Unchanged from A2; the shots normal; no setup menu |

In A6 or A8 only, a PTP enumeration is a finding for host analysis, not a safety stop: record the UI state and the timing of the press, and continue with the next act. In A6 a candidate cause is contact bounce at the plug (attach, detach, attach edges while Q is held flip the byte twice; independent review N6); record any visible flicker of the USB screen. Do not repeat an attach beyond the script.

### 12.7 Stop rules

Every stop ends camera operations for the session; nothing is retried; a new attempt needs a new go.

- D5 to C3: the card does not latch, a card message, a shutdown, a save or playback error. No update; the card stays in place.
- Before I5: a wrong version, model, route or file message, no equal-version offer, low or doubtful power, an unfamiliar screen: cancel where offered, switch off, cut the supply, report.
- During I6: never cut power, open a door or press a button, whatever the screen shows. A stalled or blank display is recorded, not interrupted.
- After I8, a body that does not boot or hangs: stop. Do not power-cycle repeatedly or follow generic battery-removal advice; keep the last display, the supply and card state, and the video (install procedure section 9 step 1).
- The setup menu, a lost setting or changed C values: stop for host analysis; do not re-enter settings to hide the difference.
- A film list other than `FILMS`: stop; do not reflash. Photograph the list and the shooting mode first. For CC-icon+toggle this is also the first look at the CC rows, icon and Q tiles on this DAT, since flash-boot-check stops at SHOOTING MENU and they rest on byte identity with design-B (ToggleCompReview N3).
- During acceptance: any freeze, error dialog, unexpected reboot, stock-mode change, card message, `read-config` refusal or stop line, or an enumeration other than `04cb:02b6` or `04cb:ff80` (spec 07 section 3.3).
- `04cb:ff80` at A3 or in boot 3 of A10: stored `0xf7` is nonzero after the install, or the byte flipped without Q. Stop, make no host operation over that `ff80`, unplug and report (spec 07 section 15).
- An A7 window 0 that differs from the reference outside `0x80`, `0x81` and `0xf7`: stop and report.

### 12.8 What the go names

The owner's per-run H3 go, with the candidate's digest written out:

- RC0+toggle: "Go: firmware update of XM1-body-01 from the camera's update menu with FWUP0002.DAT SHA-256 10feac474eee5ac178ff454397f1b4f1bc1c9a01ae4da8de8f102fa606ca2445 (RC0 with the USB personality toggle), copied by card reader to the card root, followed by the 12.6 acceptance acts with their host reads: lsusb listings and one read-config of the gate-byte plan in the Q boot."
- CC-icon+toggle, valid only once gate 4 is cleared by the 11.4 owner decision: "Go: firmware update of XM1-body-01 from the camera's update menu with FWUP0002.DAT SHA-256 c1bfc9aa995861ff8a90729bf0e0e5e7bfe0747c1bac69adec2a72d7f7f0ad8f (Classic Chrome design B with the USB personality toggle), copied by card reader to the card root, followed by the 12.6 acceptance acts with their host reads: lsusb listings and one read-config of the gate-byte plan in the Q boot."

A go that names another digest, another file, another delivery or other host operations is not this go.

### 12.9 Recovery and revert

- Interruption residues and which a repeat repairs: [install procedure section 8](install-procedure.md#8-interruption-residues-and-which-are-recoverable-by-repeating-the-update). Between S1 and the end of S3 the body cannot boot and no repair is demonstrated; the owner accepted that residual on 2026-10-09 ([install procedure section 3](install-procedure.md#3-preconditions)). A repeat after S3 needs the card, which is in the camera already.
- CC-icon+toggle: a decode failure of the image-4 base `0x0061c200` at boot has the same symptom as that window and the same coverage (classic-chrome-patch.md 12.2.7).
- Coverage by failure class: [recovery status](recovery-status.md#failure-coverage-and-smallest-next-experiment).
- Revert from a working body: reinstall RC0 `acb0fbbb…ca8` (or stock) through this same route, with a stock film in the shooting setting and in C first on a CC body. The toggle stores nothing; a power-off already returns the USB personality to the stored byte.

## 13. Reproduction

Host reads only, from the repository root; `R=private-data/analysis/x-m1-1.50`.

```sh
for p in "id4 lz/id4-0061c400.bin 0x61c400" "id5 lz/id5-00bf1a00.bin 0xbf1a00" "plain lz/plain-01100000.bin 0x1100000"; do set -- $p
  devenv shell -- llvm-objcopy -I binary -O elf32-littlearm --rename-section .data=.text,alloc,load,code,readonly $R/$2 /tmp/$1.elf
  devenv shell -- llvm-objcopy --strip-all /tmp/$1.elf
  devenv shell -- llvm-objdump -d --triple=armv7a-none-eabi --mcpu=cortex-r4f --adjust-vma=$3 /tmp/$1.elf > /tmp/$1.dis
done
D5="devenv shell -- llvm-objdump -d --triple=armv7a-none-eabi --mcpu=cortex-r4f --adjust-vma=0xbf1a00 /tmp/id5.elf"
D4="devenv shell -- llvm-objdump -d --triple=armv7a-none-eabi --mcpu=cortex-r4f --adjust-vma=0x61c400 /tmp/id4.elf"
$D5 --start-address=0xecf8b8 --stop-address=0xecfa60     # handlers A and B
$D5 --start-address=0xbf4454 --stop-address=0xbf44e4     # stub extent
$D5 --start-address=0xd0ad8c --stop-address=0xd0b0f0     # attach-time and re-decision key tests
$D5 --start-address=0xd36ea4 --stop-address=0xd37340     # UI key handler, bitmap writer
$D5 --start-address=0xec80dc --stop-address=0xec8158     # startup barrier f7 branch
$D4 --start-address=0x73cbfc --stop-address=0x73cce8     # key callback, boot USB key call, mode decision
$D4 --start-address=0x73ac60 --stop-address=0x73af30     # record copier, appender, boot USB key
$D4 --start-address=0x634b40 --stop-address=0x634d20     # fold
$D4 --start-address=0x635280 --stop-address=0x635474     # poller
$D4 --start-address=0x632860 --stop-address=0x632a3c     # key index and key state
$D4 --start-address=0x69f8b8 --stop-address=0x69f920     # 0x102 halfword, byte getter
$D4 --start-address=0x69fc94 --stop-address=0x69fcb8     # byte setter
od -An -tx1 -j $((0x828120-0x61c400)) -N $((36*28)) $R/lz/id4-0061c400.bin   # 36 condition rows
od -An -c -j $((0xed2df8-0xbf1a00)) -N $((178*16)) $R/lz/id5-00bf1a00.bin    # key-name table
od -An -tx1 -j $((0xe0048+0xa902)) -N 1 $R/payload.bin                        # cache[0xa902]
dd if=$R/lz/id5-00bf1a00.bin bs=1 skip=$((0xbf4454-0xbf1a00)) count=144 status=none | sha256sum
```

Caller, word, pair, ADR and Thumb scans over the same listings and the eight raw inputs were run in a host session; they miss register-indirect references whose base is computed at run time, which E2 (9.3) covers on the executed paths.

Builds, cases and emulator runs (`D` is the resolved stock DAT, `$(readlink -f private-data/firmware/x-m1-1.50/FWUP0002.DAT)`):

```sh
devenv shell -- bazel test //host/firmware:usb-toggle-tests
devenv shell -- bazel run //host/firmware:build-usb-toggle -- --output-sha256 c1bfc9aa995861ff8a90729bf0e0e5e7bfe0747c1bac69adec2a72d7f7f0ad8f private-data/analysis/classic-chrome-icon/parts-b-1-pinned/classic-chrome-icon.json private-data/analysis/usb-toggle/NEW_CCB_PARTS
devenv shell -- bazel run //host/firmware:build-usb-toggle -- --output-sha256 10feac474eee5ac178ff454397f1b4f1bc1c9a01ae4da8de8f102fa606ca2445 extensions/rc0.json private-data/analysis/usb-toggle/NEW_RC0_PARTS
devenv shell -- bazel run //extensions:build-extension -- private-data/analysis/usb-toggle/ccb-pinned/usb-toggle.json "$D" private-data/analysis/usb-toggle/NEW_CCB_DAT
devenv shell -- bazel run //extensions:build-extension -- private-data/analysis/usb-toggle/rc0-pinned/usb-toggle.json "$D" private-data/analysis/usb-toggle/NEW_RC0_DAT
devenv shell -- bazel run //emulation:usb-toggle-cases
devenv shell -- bazel run //emulation:flash-boot-check -- --firmware private-data/analysis/usb-toggle/ccb-pinned-dat/FWUP0002.DAT --output private-data/flash-boot-checks/NEW
devenv shell -- bazel run //emulation:usb-toggle-emu -- --receipt-output private-data/analysis/usb-toggle/emu/NEW --runs all --dats stock,rc0-toggle,cc-icon-base,cc-icon-toggle
devenv shell -- bazel run //emulation:xm1-viewer -- --ram-push-e2e --classic-chrome --attach-hold-q --firmware private-data/analysis/usb-toggle/rc0-pinned-dat/FWUP0002.DAT --installed-nand private-data/analysis/usb-toggle/emu/inputs/f7-0.nand --sd-card-image private-data/analysis/classic-chrome-patch/ram-push/inputs/card.img --receipt-output private-data/analysis/usb-toggle/emu/NEW_E8
devenv shell -- bazel run //emulation:updater-f7 -- --recovery-ram RESTORE/terminal-valid-ram.bin --sd-image RESTORE/firmware-ram-recovery-sd.img --installed-nand private-data/analysis/x-m1-1.50/stored-f7/emulation/inputs/f7-1.nand --receipt-output private-data/analysis/usb-toggle/emu-updater/NEW
```

`RESTORE` is a `//recovery:build-image restore` output from the candidate DAT followed by the `ram-rescue-engine` raw handoff (emulation/README.md, the updater section).

## 14. RAM-push variant

**Status:** Two hardware RAM-push sessions installed the payload and restored stock after full power removal. The first stopped before A1, leaving the Q-toggle cycle unexercised (E-USB-TOGGLE-RAM-PUSH-SESSION-1-150). The second ran the full sequence: A1 and A4 detaches, A2 and A5 Q attaches, A6 reads, A7 and stock restoration (E-USB-TOGGLE-RAM-PUSH-SESSION-2-150). Its held-Q attach also toggled the stock long-press-Q sun indicator, which persisted as a stored setting.

### 14.1 Placement and lifetime

The stub remains at its reviewed image-5 address `0x00bf4454`, with the exact 88 bytes of section 3.1 and unchanged calls, stack contract and hook encoding. This retains section 5.2's ownership census and the native/full-system behavior evidence. Relinking it into an allocation would change its BL words, entry target and placement argument without changing the toggle behavior. The push overwrites 22 aligned words in `[0x00bf4454,0x00bf44ac)` and the hook word at `0x00ecf900`; the remaining 56 bytes of the reviewed extent stay stock.

The `UsbToggle` payload kind reserves `0xd20` bytes (3360) for the existing shared installer, publication table, rendezvous and mailbox. Its installer starts at `align32(base)` and occupies `0xd00` bytes; even an eight-byte-aligned base requiring 24 bytes of leading padding fits. Count-specialized entries are installer `+0x600` (install), `+0x83c` (rollback) and `+0x868` (status). The payload ABI is 6. Neither the payload allocation nor the publication words survive the approved full-power-removal ending. A switch-only cycle is not the removal proof.

The relinker is `host/firmware/build-usb-toggle-ram.sh`, builder `//host/firmware:build-usb-toggle-ram`. The builder pins stock image 5, the full 144-byte old extent, the old hook word, the linked stub and its stock callees. It emits the shared manifest/runtime format. `ram-alloc`, `ram-relink` and `ram-push` select this payload by allocation size and retain the existing authenticated-rebuild, upload-readback, old-word, CPU identity, callback-slot and mailbox checks. The new pins and their verdicts live only in `transport/ram-push-toggle-review.json`; the camera binary has no raw-write escape.

### 14.2 Masked-window and activation-order argument

Publication is **22 setup words, then one activation hook**, under the existing **CPU1 PARKED/COMMIT acknowledgement plus CPU0 IRQ-masked publication window** of the [transport publication contract](../transport/README.md#ram-push-review-record). Before that window the host uploads and verifies only the retained allocation. The installer synchronizes runtime and publication lines on both cores, parks CPU1 with its IRQs masked, masks CPU0 IRQs, validates all 23 old words, obtains the peer acknowledgement, stores the 22 stub words, then stores aligned hook word `0xebf492d3`, performs instruction/data-cache maintenance and releases the cores. An old-word failure publishes nothing.

Handler A may be suspended on either core at entry to the window. The old hook is a BL to the byte getter, with return `0x00ecf904`, which the new stub preserves. A suspended task before the BL resumes into either the fully stock or the fully published call, never an incomplete stub. One already in the stock getter returns to the same continuation with the stock value; a plain edge's result is unchanged. One after the call consumes its already-read `r0` exactly as before. No existing frame points into the stub extent because the hook was stock and section 5.2 finds no other reference. The allocation remains owned after installation, and no rollback/free runs during the live experiment.

This is not an idle-screen-only argument and does not claim the entire connect handler is quiescent. Idle shooting mode is the operational precondition; the masked publication window, activation-last order, cache maintenance and unchanged call/return ABI are the safety boundary.

### 14.3 Observability and hardware-only boundaries

Installation leaves RAM `0xf7 = 1` and the existing `ff80` session intact. After detach, Q attach flips the RAM byte to 0 and selects PTP `04cb:02b6`. `read-ram` and `ram-alloc` refuse with the byte at 0, and service commands cannot run over PTP. The script therefore uses the OS-cached enumeration descriptor as the observation, does not attempt an intermediate RAM read, and checks the RAM byte and installed words only after a second Q attach restores `ff80` and RAM 1.

The complete requested sequence is push, detach, Q attach to PTP, detach, Q attach to `ff80`, optional plain-attach retention, full power removal and a fresh ordinary-boot read. The emulator's configured-session disconnect boundary of section 8.3 is hardware-observed in E-USB-TOGGLE-RAM-PUSH-SESSION-2-150; an undelivered detach still stops a scenario and is never replaced by a guessed controller event. Reading the key driver from usbcont remains a hardware-only check. Model RAM inspection is not counted as an available host read while the gate is 0.

The Q-flip variant is being replaced: a momentary select, designed on a separate branch, gives PTP on every plain attach and `ff80` only while the sub-command dial push is held at attach, which avoids the Q hold's stock long-press-Q sun-indicator side effect.

The [session script](usb-toggle-ram-push-session-script.md) contains stop rules, the exact four-window private read plan and the standard battery-removal ending. The [independent reviewer brief](review/usb-toggle-ram-push.md) covers the new publication mechanism separately from the reviewed stub bytes.

### 14.4 RAM-push emulation evidence

The exact-payload FujiMod run `private-data/analysis/usb-toggle/ram-push/session-4` allocated at `0x00a7e8c8`, uploaded and read back the authenticated runtime, and returned `RamPushInstalled`. Guest readback confirmed all 88 reviewed stub bytes, hook `0xebf492d3`, 23 publication records and unchanged RAM `0xf7 = 1`. Runtime SHA-256 is `b6b86ec6a0409cf4419c2d7c5a0b4ecb3ce301a59b2d652533cf92a9e59257ad`; manifest SHA-256 is `0a37e6aa0a511608f267701730455a87bb2a4adafd2bfa0f58e18d9255024bb1`. The publication-contract digest is `9689704f21b660df2806a52a481cda1909182cac593007e094d569d69764334c`.

The next planned detach did not reach handler A within 80 million instructions. Driver state remained `0x0a`, status `0x11`, matching section 8.3's missing configured-session disconnect event. The receipt is **blocked**, with one failed detach-delivery check; neither Q attach ran in that continuous pushed session. A complete pushed-cycle pass is not claimed.

Native cases consumed the stub read back from that installed payload, rather than a fresh build: `private-data/analysis/usb-toggle/ram-push/native-exact-2`, held inside an emulator slot. All 364 cases passed: 44 stub cases, 160 patched handler-A cases and 160 stock controls, including both Q toggle directions. The 80 plain-edge stock/patched pairs had identical boundary events, `r0`, latch and RAM `0xf7`. Maximum stack depths were 44 bytes patched and 36 bytes stock.

FujiMod component runs in `private-data/analysis/usb-toggle/ram-push/transitions-1` passed `e5u-ff80-rc0-toggle`, `e5u-q-ptp-rc0-toggle`, `e7-rc0-toggle-plain` and `e7-rc0-toggle-q`. They exercise the identical stub and hook installed by the flash-toggle composition, not a continuation of the RAM-pushed session. `e5u-cycle-rc0-toggle` stopped at a PTP detach with driver state `0x08`, status `0x01`; its one failed detach-delivery check is also hardware-only. The requested continuous cycle completed on hardware in the second session, including both configured detach edges and the held-Q UI effect; the key-driver read from usbcont remains a physical-body acceptance item.
