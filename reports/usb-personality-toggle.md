# USB personality toggle (X-M1 1.50)

Design, implementation and host verification of an on-device control that switches the USB personality between the `04cb:ff80` service device (transport service mode 3, as with stored `0xf7 = 1`) and ordinary PTP `04cb:02b6`, with no jig session and no settings-area write ([USB-PERSONALITY-TOGGLE-01](https://github.com/skeet70/latent-image/issues/81)). It covers the stock decision path, the key and its census, the stub and patch sites, placement, the two flash candidates and their composition with RC0 and Classic Chrome, the native and emulator evidence, the changed-byte explanation and the reviewer packet (sections 10 and 11). The owner approved the design on 2026-10-22 with the flip behaviour. No camera or USB device was touched; the candidates are not installed.

Inputs, byte-identical to the pinned decodes under `private-data/analysis/x-m1-1.50/`: DAT SHA-256 `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`; image 4 `lz/id4-0061c400.bin` base `0x0061c400` SHA-256 `f2e2acab…dcff16`; image 5 `lz/id5-00bf1a00.bin` base `0x00bf1a00` SHA-256 `3cd501c0…fd892a`; plain `lz/plain-01100000.bin` base `0x01100000` SHA-256 `ee211bda…2d889d`; image 2, both loaders and both boot records under `filmsim/` and `decode/`; the fallback adjustment image at DAT payload `0xe0048` (`payload.bin`), equal to the body's adjustment bytes over `0xa000`-`0xcfff` (E-ADJ-READ-150).

Labels: **byte-read** (instruction, table or data read from the pinned images in this pass), **static-inference** (conclusion from those bytes), **inherited** (named ledger entry or report, not re-derived), **hardware-observed**, **unproved**.

## 1. Summary and decision

**Key: hold Q while plugging in the cable.** On an attach edge in an ordinary, playback or first-start boot, if the key driver reports Q held, the patch flips the RAM copy of adjustment byte `0xf7` (0 becomes 1, nonzero becomes 0) before the stock connect handler tests it. With stored `0xf7 = 0`, a Q attach gives `ff80` and every later attach in that boot stays `ff80`; a second Q attach in the same boot returns to PTP. A plain attach after power-on gives PTP. Nothing is written to storage, so every power-on starts from the stored byte.

- **One hook, one stub, image 5 only.** The `bl 0x0069f8fc` at `0x00ecf900` in connect handler A becomes a `bl` to a 22-word stub (`host/firmware/usb-toggle-stub.S`) placed in the unreferenced image-5 function extent `[0x00bf4454, 0x00bf44e4)` (sections 3.1 and 5).
- **Key state from the key driver.** The stub reads Q through `0x006a0d00(0x32)`, the driver's debounced row state, not the UI key bitmap, so a UI that misses a release cannot leave a later attach armed (section 4.2).
- **RAM only.** The stub writes `cache[0xf7]` through the stock byte setter `0x0069fc94`, the same RAM state the jig tools' gate write leaves until power-off. All 13 stock readers of `0xf7` read that RAM byte (section 2.4), so the debug channel, the `read-ram` gate check and the detach arm follow the toggle without further patching.
- **No persisted toggle.** The only host-reachable storage route is the 33-unit whole-area commit; a stored toggle would repeat that rewrite on every switch. None is needed (section 3.4).
- **No RAM-push variant.** The push route needs `ff80` already enumerated and `0xf7` nonzero before it can write anything, and a pushed hook is gone at power-off, so it cannot select the personality of a boot (section 3.5).
- **Candidates.** CC+toggle `507ecf32…0e1e` (on the CC flash candidate `16c830ad…2c89`) and RC0+toggle `10feac47…2445` (on RC0 `acb0fbbb…ca8`), both built by `//host/firmware:build-usb-toggle` and `//extensions:build-extension`, pinned and reproduced. Each decoded image 5 differs from its base only in the stub bytes and the hook word; every other part equals the base (section 10).
- **Evidence.** 364 native cases and every full-system run the emulator can deliver pass: both candidates boot to SHOOTING MENU, a plain attach gives PTP, a Q attach gives `ff80` with a working service session, a second Q attach returns PTP in the same boot, stock behaviour is unchanged without Q, the Classic Chrome RAM push runs over a Q attach, and the updater install resets stored `0xf7` to 0 (section 9).
- **Review class.** H3 install with new mechanism bytes: one independent review, the owner's visual check and a per-run go naming the digest remain (section 11).

Body state: commit-f7-restore stored `0xf7 = 0` on 2026-10-21 (E-STORED-F7-RESTORE-RUN-150), and the install would store 0 again (section 8.4). The toggle also covers stored `0xf7 = 1` (section 3.2).

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

### 3.5 No RAM-push variant

The push route writes its runtime with debug function 3, which needs `ff80` enumerated and `0xf7` nonzero (`0x00647b90`) before the first byte; the allocation and all publications are RAM state lost at power-off (inherited, transport README). A pushed hook could only change the personality of a boot that already has `ff80`, so it cannot provide the switch.

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

`0x006a0d00(code)` finds the code's condition row through `0x006a0b4c` (36 rows of 28 bytes from `0x00828120`; Q is row 19 at `0x00828334`) and returns 1 exactly when row byte `+0x15` is 1 (byte-read). `+0x15` is the driver's debounced state: the debounce step `0x006a11f4` counts identical raw samples of `+0x16` (written by the row evaluator `0x006a0888`, here the channel-9 ADC window) in `+0x18`, and once `+0x14` samples agree it stores the raw value to `+0x15` and reports the press (code) or release (code `| 0x80`) through `0x006a1038` (`0x006a11fc`-`0x006a1248`, byte-read). The stub therefore reads the same state from which the press and release events are made, at the attach edge. The release-all step `0x006a27a8` (only caller `0x00ec8248` in the startup barrier) clears held states and reports releases; it can only make a held Q read 0.

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

Budget: the prefix window stays at CC's 484 of 512 bytes, the converter extents at 84 of 88 and 68 of 92, plain and image 2 unchanged. The toggle uses 88 of the 144 extent bytes and one in-place word; image 5 is re-encoded in place with no load or size change, so it adds no boot-path item. The CC+toggle candidate inherits CC's open base-alignment note (classic-chrome-patch.md 11.3 item 5). The RC0+toggle candidate keeps RC0's image-4 load `0x0061c340`, which the body has decoded (E-FILMSIM-INSTALL-150), and can go ahead of CC's flash review.

Tooling, following CC's re-inclusion of RC0: `//host/firmware:build-usb-toggle [--output-sha256 HEX] BASE_EXTENSION_JSON NEW_OUTPUT_DIRECTORY` (`host/firmware/BuildUsbToggle.hs`, core in `host/FujiAF/UsbToggle.hs`):

- it takes the linked stub and its symbol table from `//host/firmware:usb-toggle-stub`, requires `usb_toggle` at `0x00bf4454` and decodes every BL of the blob to the four stock callees;
- it checks the stock image 5 against its pinned SHA-256, the extent against its pinned hash and the hook word against `ebdf3ffd`;
- it reads the base manifest and each part against its step SHA-256, refuses a base image-5 patch set that names another decode or moves the load, and refuses any base write that overlaps the stub or the hook word;
- it writes the merged image-5 patch set `usb-toggle-image5.json` (address order, contiguous write orders), copies every other base part unchanged, and writes the manifest `usb-toggle.json` with the image-5 step after the image-4 step, plus `changed-words.json`/`.md` with the toggle as group `TOGGLE`.

`build-extension` is unchanged. `//host/firmware:usb-toggle-tests` checks the merge and the linked blob: two toggle writes at the extent and the hook word, the hook encoding, address order of a disjoint merge, refusal at the hook word and inside the extent, acceptance just below it, an oversized blob refused, the four callees. Because the CC parts carry the private data block, both candidates' parts and manifests stay under `private-data/analysis/usb-toggle/`, as CC's do. The RC0+toggle parts hold no private data and can move to `extensions/` once reviewed.

### 6.2 With the bank and the RAM-push route

- **RAM push (bank and CC payloads).** No publication touches the toggle words. `ram-alloc` and `read-ram` refuse unless config byte `0xf7` reads nonzero (transport README); on a toggle body with stored 0 the operator makes a Q attach first. The session scripts (reports/ram-push-session-script.md, classic-chrome-patch.md 8.4) and the transport README describe that premise as stored `0xf7 = 1` and need the Q-attach wording once the toggle is installed.
- **Recipe bank and D3 flash designs.** They use the image-4 prefix, plain and image-2 ids; no image-5 overlap with the toggle, and no key-state use.
- **`commit-f7-*`.** In jig boots (system mode 7) the stub never flips, so they see the stored byte. In an ordinary boot after a Q attach, `commit-f7-enable` refuses (start value reads 1) and `commit-f7-restore` would commit 0 over a stored 0.
- **Install.** The update resets stored `0xf7`, `0x80` and `0x81` to 0 (section 8.4, byte-read and emulated). The toggle works with either stored value.

## 7. Review class

- **Install: H3** (firmware update, spec 07 section 3.2). The hook word redirects stock control flow and the stub runs outside any payload allocation, so both are new mechanism bytes: one independent review of the stub source and linked blob, the hook word, the merged image-5 patch set and the container diff; `//emulation:flash-boot-check` on the exact DAT; the owner's visual check in `//emulation:xm1-viewer -- --firmware`; the owner's per-run go naming the operation and the digest.
- **Use after install.** A Q attach writes only the RAM byte `0xf7`, the state the jig gate write leaves until power-off, and opens the same `ff80` channel stored `0xf7 = 1` opens today, including debug function 3 (arbitrary RAM write) for the attached host. Live commands over that channel keep their own classes (`read-ram` H1 under the read grant, `ram-push` H2).

## 8. Open questions, resolved

### 8.1 A missed release

The stub reads the key driver's debounced state `+0x15`, not the UI bitmap (section 4.2). `+0x15` changes only in the driver: the debounce step, the release-all step and `0x006a0cc4`, called twice in the startup barrier for S1. The UI's skip states (`0x00d35f68() == 1`, `[0x0094f5e0] == 2`) do not touch it (byte-read). The stub reads it once per attach edge, inside handler A, so a release the UI never processed cannot arm a later attach. N1 sets the UI bitmap bit with `+0x15 = 0` (no flip) and clears it with `+0x15 = 1` (flip). E4 reads `+0x15 = 1` with Q held and 0 after release (section 9).

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

For the power-on variant only: the content of the boot records at `0x0073ae10` for a held Q, and the source of the startup Fn and Q press forms. On hardware: the detach after an enumerated `ff80` session and the UI effect of the held Q.

## 9. Verification results (2026-10-22, host and emulated)

### 9.1 Native cases

`//emulation:usb-toggle-cases` (`emulation/UsbToggleCases.hs`) generates and runs 364 cases under `private-data/analysis/x-m1-1.50/native-case/usb-toggle/`; all pass. Expected values come from the behaviour rule and handler A's byte-read arms, not from the stub. Stock id4, stock id5 and the stub blob are pinned. The run refuses to start unless the stub and hook word equal those of both pinned image-5 patch sets.

- **N1, the stub with the stock closures of `0x0069f8fc`, `0x0069fc94`, `0x006a0d00`/`0x006a0b4c` and `0x0073c15c`: 44/44.** The 40-case matrix is stored `0xf7` × attach bit × Q `+0x15` × system mode {1, 2, 3, 5, 7}. The byte flips in exactly the six cases with attach bit 1, Q 1 and mode 1, 2 or 3, and every other case returns the stored byte unchanged. Four bitmap cases: bit 18 set with `+0x15 = 0` gives no flip, and bit 18 clear with `+0x15 = 1` flips, for stored 0 and 1. Deepest stack 28 bytes.
- **N2, handler A slice `[0x00ecf8b8, 0x00ecf9c8)`: 160/160 patched and 160/160 stock.** Latch × new bit × stored `0xf7` × Q × mode × `0x0073abec` result, with recording stubs for the trace, the `0x1705`/`0x1706` posts, the registration and the application event. All 80 Q = 0 pairs give identical events, `r0`, latch and cache byte on patched and stock.

### 9.2 Builds and full-system boot (E1)

Both candidates build pinned and reproduce their digests; `extensions/rc0.json` still reproduces `acb0fbbb…ca8`. `//emulation:flash-boot-check` passes for CC+toggle (receipt `private-data/flash-boot-checks/usb-toggle-cc-1/`, SHOOTING MENU at 450,017,580 instructions) and RC0+toggle (`usb-toggle-rc0-1/`, 450,019,375).

### 9.3 Attach scenarios (E2-E8)

`//emulation:usb-toggle-emu` (`emulation/UsbToggleEmu.hs`) drives 28 scenarios from shooting-screen checkpoints in one emulator slot. It pins every DAT and NAND hash, and its expectations come from the behaviour table of 3.2. The bridge additions in `continue_rec_scanout.py` are primitives only: `--toggle-server` (run, key press and release, ADC level, cable level, memory read, trace sites, descriptor enumeration without an expected id, transfer, capture, probe readout, checkpoint), `--cable-at-reset`, `--adc-at-reset`, `--access-probe-range`, and `--attach-hold-key` for the RAM-push attach. Inputs:

- stored-0 NAND `private-data/analysis/usb-toggle/emu/inputs/f7-0.nand` (SHA-256 `8e2b5243…478b`), the stored-1 template with byte `0xf7` restored to 0;
- its body-like variant `f7-0-pid02b6.nand` (`633944533…2e04`, `cache[0x102..0x103] = b6 02`), used only for E6;
- the stored-1 template `f7-1.nand` (`baf47535…4aa0`).

The template carries `cache[0x100..0x103] = cb 04 80 ff` and PID `0x02b6` at `0x13e`.

| Run | Observed on CC+toggle and RC0+toggle | Result |
| --- | --- | --- |
| E2 | Access probe on `[0x00bf4454, 0x00bf44e4)`, boot plus one attach: stock and CC base record nothing in the extent; each toggle DAT records 12 fetches at `0x00bf4454`-`0x00bf44a8`, entered once, at the instruction after the `0x00ecf900` call. All four also show the three image-wide model transfers that load image 5. | pass |
| E3 | Stored 0, plain attach: application arm, application `0x10`, `0xd350c4(8)`, no `0x1705`, no fill, `cache[0xf7] = 0`, `0x0146a0c0 = 2`, `0x01469810 = 2`, descriptor `04cb:02b6` | pass |
| E4 | Stored 0, Q held across the attach: Q `+0x15 = 1` before the edge; stub entered, `0x006a0d00(0x32)` returns 1; attach arm, `0x00ed117c(7,0)`, `0x00ecc104`, fill `0x007221a0(0x43360000, 0xffffffff, …)`, transport start mode 3; `0x0146a0c0 = 3`, `0x01469810 = 3`, `0x0146a0cc = 0x00d80cf0`, `cache[0xf7] = 1`; no application event; descriptor `04cb:ff80`; C0, identity `X-M1`, config window 0 with `0xf7 = 1`, one function-2 read equal to guest memory; after release `+0x15 = 0`. The frames 8 M and 40 M instructions after release show the shooting screen with no Q panel. | pass |
| E5 | Detach after the enumerated E4 session: no handler A edge (model boundary, 8.3). | blocked |
| E5u | The same edges with enumeration only where named: Q attach `ff80`; detach `0x1706`, no `0x90`, byte stays 1; plain re-attach `ff80` with the full session; detach; Q re-attach flips to 0, application `0x10`, transport 2, `04cb:02b6`. The final detach after the PTP start is not delivered (model boundary). | pass to the last step |
| E6 | Cable present from reset, body-like NAND, with and without Q held from reset: system mode 5, handler B, transport 2, `04cb:02b6`, `cache[0xf7] = 0`, stub never entered. On the template stored-0 NAND the boot selects maintenance (system mode 7, `ff80`) through `cache[0x102..0x103] = 0xff80`, again with no stub entry and `cache[0xf7] = 0`. | pass on the body-like input |
| E7 | Stored 1: plain attach `ff80` with the full session; Q attach flips to 0, application arm, `0x10`, transport 2, `04cb:02b6` | pass |
| Stock controls | Stock DAT, stored 0, plain and Q attach: application arm, PTP both times, no extent fetch, `cache[0xf7] = 0` | pass |
| E8 | `--ram-push-e2e --classic-chrome --attach-hold-q`, RC0+toggle, stored-0 NAND: Q held across the attach, device `04cb:ff80`, `ram-alloc` reads `0xf7 = 1`, allocation `0x00a7e8c8`, 131 publications installed and validated, every CC segment matched, `ram-unload` restored and verified all 131 old words, and the cold restart (Q held) read all 131 old. The after-unload segment shows the known final-step mismatch of classic-chrome-patch.md 8.2.5. | pass |

Receipts: `private-data/analysis/usb-toggle/emu/run-3/` (summary and one receipt per scenario) and `e8-cc-2/`. The boots are first-start boots (system mode 3). Mode 1 passes the same not-5-or-7 test.

### 9.4 Updater (E9)

`//emulation:updater-f7` (`emulation/UpdaterF7.hs`) runs the ordinary updater through the RAM restore route of E-RECOVERY-RC0-RESTORE-HOST-150: a `//recovery:build-image restore` card from the toggle DAT, the `ram-rescue-engine` raw handoff, then continuation over the stored-1 template NAND with OK, OK, UP, OK, OK as ordinary ADC pulses. It then boots the exported NAND. The observed values are in 8.4; receipts are under `private-data/analysis/usb-toggle/emu-updater/`. The bridge additions are console commands `watch`, `hits`, `read` and `nandstat`, which share the trace class `TraceSites` with the toggle server.

## 10. Changed-byte explanation

### 10.1 Candidates

| Item | CC+toggle | RC0+toggle |
| --- | --- | --- |
| DAT | `private-data/analysis/usb-toggle/cc-pinned-dat/FWUP0002.DAT`, 38,405,148 bytes, SHA-256 `507ecf326119110713c34f15bb07d0f38bd6946127011835980f6e4d39ad0e1e` | `private-data/analysis/usb-toggle/rc0-pinned-dat/FWUP0002.DAT`, 38,405,148 bytes, SHA-256 `10feac474eee5ac178ff454397f1b4f1bc1c9a01ae4da8de8f102fa606ca2445` |
| Pinned manifest | `private-data/analysis/usb-toggle/cc-pinned/usb-toggle.json` | `private-data/analysis/usb-toggle/rc0-pinned/usb-toggle.json` |
| Image-5 patch set | `usb-toggle-image5.json`, 45 CC words, the hook word and the stub blob | `usb-toggle-image5.json`, the hook word and the stub blob |
| Base | CC flash candidate `16c830ad…2c89` (reports/classic-chrome-patch.md 11.1); its image-4, `0x83e` record and plain parts copied unchanged | RC0 `acb0fbbb…ca8`; `rc0-patch-set.json` copied unchanged |
| Replaces on the body | the installed RC0 container | the installed RC0 container |
| Revert | reinstall CC (`16c830ad…2c89`), RC0 or stock through the same update route; nothing the toggle stores survives a power-off | the same, RC0 `acb0fbbb…ca8` |

Pinning followed CC's: an unpinned `build-extension` build gave the digest, `build-usb-toggle --output-sha256` wrote it into a fresh manifest, and the pinned `build-extension` reproduced it (`PASS`, 45/45 contract checks).

### 10.2 Every changed byte

Toggle writes, both candidates (`changed-words.md` in each pinned directory):

| Address | Current | New | Changed bytes | Role |
| --- | --- | --- | ---: | --- |
| `0x00bf4454` | 88 bytes of the stock extent (hash in 5.1) | the 22 stub words of 3.1 | 78 | stub |
| `0x00ecf900` | `fd3fdfeb` | `d392f4eb` | 3 | `bl 0x00bf4454` replaces `bl 0x0069f8fc` |

Decoded images, byte-compared after `rebuild-update extract-record5` and `fflzd`: CC+toggle image 5 differs from CC's image 5 only within `[0x00bf4454, 0x00bf44ac)` and at `0x00ecf900`. RC0+toggle image 5 differs from stock image 5 at the same places. `rebuild-update diff` against the CC candidate: identity records, selector, fill, segment table, image-4 header and stream, image-2 extent, trailer and both restore lists unchanged; image-5 header (2 bytes) and stream re-encoded. The extracted plain window equals the base's (CC `7e60e04e…`, RC0 and stock `ee211bda…`). DAT header bytes `0x244`-`0x25b` equal stock (8.4).

## 11. Reviewer packet and remaining gates

The install is H3 (spec 07 section 3.2). The review covers new mechanism bytes: the hook word redirects stock control flow and the stub runs outside any payload allocation.

Reviewer inputs:

- this report;
- `host/firmware/usb-toggle-stub.S` and `.ld`, `host/FujiAF/UsbToggle.hs`, `host/firmware/BuildUsbToggle.hs` and `UsbToggleTests.hs`;
- both pinned directories under `private-data/analysis/usb-toggle/` (manifest, `usb-toggle-image5.json`, `changed-words.*`);
- the native cases (9.1) and the emulator receipts (9.2 to 9.4).

Questions for the reviewer:

1. Does the stub preserve handler A's contract at `0x00ecf904`: only `r0` consumed, `r4`-`r11` and SP preserved, flags not relied on?
2. Is ownership of the extent by absence of references (5.2) enough, with E2's runtime watch?
3. Is reading `+0x15` through `0x006a0d00` from usbcont context sound, given that the debounce step writes it from the key-scan context?
4. Does a flip in system mode 1, 2 or 3 leave every other `0xf7` reader consistent (2.4)?
5. Does the merged image-5 patch set carry CC's 45 words unchanged?

Gates before an install:

1. One independent review of the bytes above and the container diff. Not started.
2. `//emulation:flash-boot-check` on the exact DAT. Passed for both (9.2).
3. The owner's visual check: `//emulation:xm1-viewer -- --firmware <candidate>`, live view and SHOOTING MENU (for CC+toggle also FILM SIMULATION, classic-chrome-patch.md 11.3). Not started.
4. The owner's per-run go naming the operation (firmware update) and the digest. Not given.
5. Boot path: the toggle adds none. CC+toggle inherits CC's image-4 base note; RC0+toggle does not.

After install (owner):

- plain attach after the shooting screen: the host lists `04cb:02b6` and files read over PTP;
- Q held while attaching: the host lists `04cb:ff80`, and `read-config` with the gate-byte plan reads `0xf7 = 1` (H1 under the read grant);
- detach, then plain re-attach: `ff80`;
- power cycle and plain attach: `04cb:02b6` again;
- shooting, playback and settings normal;
- a `read-config` window-0 capture in a Q boot equals the reference outside `0xf7`.

The RAM-push session scripts and the transport README's ordinary-boot prerequisites change from a stored `0xf7 = 1` to a Q attach once a toggle candidate is installed.

## 12. Reproduction

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
devenv shell -- bazel run //host/firmware:build-usb-toggle -- --output-sha256 507ecf326119110713c34f15bb07d0f38bd6946127011835980f6e4d39ad0e1e private-data/analysis/classic-chrome-patch/candidate-pinned/classic-chrome.json private-data/analysis/usb-toggle/NEW_CC_PARTS
devenv shell -- bazel run //host/firmware:build-usb-toggle -- --output-sha256 10feac474eee5ac178ff454397f1b4f1bc1c9a01ae4da8de8f102fa606ca2445 extensions/rc0.json private-data/analysis/usb-toggle/NEW_RC0_PARTS
devenv shell -- bazel run //extensions:build-extension -- private-data/analysis/usb-toggle/cc-pinned/usb-toggle.json "$D" private-data/analysis/usb-toggle/NEW_CC_DAT
devenv shell -- bazel run //extensions:build-extension -- private-data/analysis/usb-toggle/rc0-pinned/usb-toggle.json "$D" private-data/analysis/usb-toggle/NEW_RC0_DAT
devenv shell -- bazel run //emulation:usb-toggle-cases
devenv shell -- bazel run //emulation:flash-boot-check -- --firmware private-data/analysis/usb-toggle/cc-pinned-dat/FWUP0002.DAT --output private-data/flash-boot-checks/NEW
devenv shell -- bazel run //emulation:usb-toggle-emu -- --receipt-output private-data/analysis/usb-toggle/emu/NEW --runs all
devenv shell -- bazel run //emulation:xm1-viewer -- --ram-push-e2e --classic-chrome --attach-hold-q --firmware private-data/analysis/usb-toggle/rc0-pinned-dat/FWUP0002.DAT --installed-nand private-data/analysis/usb-toggle/emu/inputs/f7-0.nand --sd-card-image private-data/analysis/classic-chrome-patch/ram-push/inputs/card.img --receipt-output private-data/analysis/usb-toggle/emu/NEW_E8
devenv shell -- bazel run //emulation:updater-f7 -- --recovery-ram RESTORE/terminal-valid-ram.bin --sd-image RESTORE/firmware-ram-recovery-sd.img --installed-nand private-data/analysis/x-m1-1.50/stored-f7/emulation/inputs/f7-1.nand --receipt-output private-data/analysis/usb-toggle/emu-updater/NEW
```

`RESTORE` is a `//recovery:build-image restore` output from the candidate DAT followed by the `ram-rescue-engine` raw handoff (emulation/README.md, the updater section).
