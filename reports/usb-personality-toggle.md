# USB personality toggle (X-M1 1.50, design)

Design for an on-device control that switches the USB personality between the `04cb:ff80` service device (transport service mode 3, as with stored `0xf7 = 1`) and ordinary PTP `04cb:02b6`, with no jig session and no settings-area write. This report covers the stock decision path, the chosen key and its census, the patch sites, placement, composition with RC0 and the Classic Chrome flash candidate, the review class and the verification plan. Patch bytes are not written yet; the stub below is the design sequence for review. No camera or USB device was touched.

Inputs, byte-identical to the pinned decodes under `private-data/analysis/x-m1-1.50/`: DAT SHA-256 `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`; image 4 `lz/id4-0061c400.bin` base `0x0061c400` SHA-256 `f2e2acab…dcff16`; image 5 `lz/id5-00bf1a00.bin` base `0x00bf1a00` SHA-256 `3cd501c0…fd892a`; plain `lz/plain-01100000.bin` base `0x01100000` SHA-256 `ee211bda…2d889d`; image 2, both loaders and both boot records under `filmsim/` and `decode/`; the fallback adjustment image at DAT payload `0xe0048` (`payload.bin`), equal to the body's adjustment bytes over `0xa000`-`0xcfff` (E-ADJ-READ-150).

Labels: **byte-read** (instruction, table or data read from the pinned images in this pass), **static-inference** (conclusion from those bytes), **inherited** (named ledger entry or report, not re-derived), **hardware-observed**, **unproved**.

## 1. Summary and decision

**Key: hold Q while plugging in the cable.** On an attach edge in an ordinary or playback boot, if the Q key reads held, the patch flips the RAM copy of adjustment byte `0xf7` (0 becomes 1, nonzero becomes 0) before the stock connect handler tests it. With stored `0xf7 = 0`, a Q attach gives `ff80` and every later attach in that boot stays `ff80`; a second Q attach in the same boot returns to PTP. A plain attach after power-on gives PTP. Nothing is written to storage, so every power-on starts from the stored byte.

- **One hook, one stub, image 5 only.** The `bl 0x0069f8fc` at `0x00ecf900` in connect handler A becomes a `bl` to a 23-word stub placed in the unreferenced image-5 function extent `[0x00bf4454, 0x00bf44e4)` (section 5).
- **RAM only.** The stub writes `cache[0xf7]` through the stock byte setter `0x0069fc94`, the same RAM state the jig tools' gate write leaves until power-off. All 13 stock readers of `0xf7` read that RAM byte (section 2.4), so the debug channel, the `read-ram` gate check and the detach arm follow the toggle without further patching.
- **No persisted toggle.** The only host-reachable storage route is the 33-unit whole-area commit; a stored toggle would repeat that rewrite on every switch. None is needed (section 3.4).
- **No RAM-push variant.** The push route needs `ff80` already enumerated and `0xf7` nonzero before it can write anything, and a pushed hook is gone at power-off, so it cannot select the personality of a boot (section 3.5).
- **Delivery.** The stub and hook word merge into the CC candidate's image-5 patch set; the prefix window, converter extents, image 2 and plain are untouched (section 6).
- **Review class.** H3 install with new mechanism bytes: one independent review, `//emulation:flash-boot-check`, the owner's visual check and a per-run go naming the digest (section 7).

Body state: the latest session record, `20261022T044649Z-commit-f7-restore` (private, not yet in the ledger), ends `committed;f7=0x01->0x00;result=0x00`, so the body is expected to hold stored `0xf7 = 0` at its next boot. The toggle also covers stored `0xf7 = 1` (section 3.2).

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

Design sequence; the linked bytes are written after this design is reviewed. Entry from `0x00ecf900` with `r0 = 0xf7`, `r1` = new attach bit, `lr = 0x00ecf904`; it returns the value handler A tests in `r0`, preserves `r4`-`r11` and the stack, and keeps 8-byte alignment.

```asm
usb_toggle:                     @ 0x00bf4454
    push    {r4, r5, r6, lr}
    mov     r4, r1              @ attach bit
    bl      0x0069f8fc          @ r0 = cache[0xf7]
    mov     r5, r0
    cmp     r4, #1
    bne     1f                  @ detach edge: stored value
    mov     r0, #0x32           @ KEY_Q
    bl      0x00632950          @ live key state, 0 or 1
    cmp     r0, #0
    beq     1f
    bl      0x0073c15c          @ system mode word 0x009570dc
    cmp     r0, #5
    cmpne   r0, #7
    beq     1f                  @ USB-cable boot or maintenance
    cmp     r5, #0
    moveq   r5, #1
    movne   r5, #0
    mov     r0, #0xf7
    mov     r1, r5
    bl      0x0069fc94          @ cache[0xf7] = r5
1:  mov     r0, r5
    pop     {r4, r5, r6, pc}
```

23 words, 92 bytes. The stub flips the byte only where handler A will take the service attach arm with the new value; on a detach edge, with Q released, or in system mode 5 or 7 it returns the stock read. Attach and detach therefore stay paired: a session's detach reads the value its attach set.

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

Static-inference over sections 2.2 to 2.4. The attach must follow the startup handshake: before group-1 reaches state 3 no `0x1714` reaches handler A, and handler B reports the attached cable to the application (PTP). The 41 MB `0xff` fill runs only on attaches that start `ff80`, not on every attach as with stored 1.

### 3.3 Why attach time and not power-on

A power-on variant is possible: a second hook at `0x0073ae20` could scan the boot records for Q, as stock scans for TELE2, UP and S1. It is not chosen:

- **The snapshot's content for Q is inferred, not shown.** Jig boots with `0xf7` stored started on this body (E-STORED-F7-COMMIT-RUN-150 and the 2026-10-22 restore session), which by the bytes of 2.1 requires an UP or S1 record at `0x0073ae10`, and the update screen's DISP_B test reads channel 9 like Q (static-inference). The 2026-10-09 capture also shows startup press forms for Fn and Q with no owner press (syscont ticks 344 and 658, initcont Fn at tick 31; reports/input-map-150.md section 3, source unproved), so a power-on Q could fire falsely.
- **The held key still reaches the application.** `0x006a282c`, which registers the application callback `0x0072b680` (at `0x00ec42c8` and `0x00ec5720`, two of its five call sites), copies the boot records and replays them through `0x006a1b40`, the caller of the registered callback (byte-read), so a Q held through boot is delivered as a Q press (static-inference).
- **A power-on choice cannot be undone within the boot.**

The attach-time test reads the live key state that the running UI maintains (section 4.2), the same source stock's attach-time maintenance choice in `0x00d0afe0` reads.

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

Q is the only physical button with no key-state test anywhere and no role in any stock combination at power-on or attach. It is not in the release-synthesis list and the bank sources in `menu/` reference neither `0x00632950` nor code `0x32`. The DOWN combination is inert on this body: `0x0096b480` is `cache[0xa902]` (copied at `0x0072bd88`), which reads `0x00` in the fallback image (byte-read) that equals the body over `0xa000`-`0xcfff` (E-ADJ-READ-150).

### 4.2 The live key state

`0x00632950(code)` tests bit `idx & 31` of word `0x0094f8b8 + 4 * (idx >> 5)`, where `0x00632860` maps the code to `idx` through the range table `0x007748dc`; for Q, `idx = 50`, word `0x0094f8bc`, bit 18 (byte-read). Two functions set and clear single bits: the boot fold and the UI key handler `0x00d36ea4` (callers `0x00c231fc`, `0x00d5d0e4`), which sets the bit for qualifier `0x00`/`0x40` and clears it for `0x80` (`0x00d372e4`-`0x00d37310`); `0x00638f54` zeroes the words (byte-read). `0x00d36ea4` skips the update when `0x00d35f68()` returns 1 or `[0x0094f5e0] == 2` (`0x00d36ee4`-`0x00d36f04`); in those states a Q press is not seen and the attach is plain (unproved which UI states those are; section 9).

## 5. Patch sites and placement

### 5.1 Words

| Image | Address | File offset | Current | New | Role |
| --- | --- | --- | --- | --- | --- |
| 5 | `0x00ecf900` | `0x002ddf00` | `ebdf3ffd` `bl 0x0069f8fc` | `ebf492d3` `bl 0x00bf4454` | handler A's `0xf7` read |
| 5 | `0x00bf4454` | `0x00002a54` | 144-byte extent, SHA-256 `a862a878075d05fd011efb724fdd7503fb406bddac8c1a00f27998486efd4ec0` | 92-byte stub (3.1); the remaining 52 bytes stay stock | stub |

The stub's calls reach `0x0069f8fc`, `0x00632950`, `0x0073c15c` and `0x0069fc94` in image 4 with A32 `bl` (displacements within range, byte-read). It uses only forms the R4F profile has.

### 5.2 Ownership of `[0x00bf4454, 0x00bf44e4)`

The extent is one function, from its `push {r4-r9, lr}` at `0x00bf4454` to the `bx lr` at `0x00bf44e0`; the next function starts at `0x00bf44e4`. Byte-read census over images 4, 5, plain and 2, both loaders and both boot records:

- no A32 branch from outside the extent targets an address in it, and it has no internal branch target;
- no aligned or unaligned 32-bit value in `[0x00bf4454, 0x00bf44e4]` occurs in any of the eight inputs;
- no `movw`/`movt` pair and no `add`/`sub rX, pc` computes an address in it;
- Thumb listings of plain and both loaders branch nowhere into it; the image-5 Thumb listing yields only decodes of A32 upper halfwords (addresses `2 mod 4`).

It is the only image-5 function with no reference by this census. Ownership is by absence of references, the class of CC's `0x0073f684` verdict, not by replacement; the emulator watch in 8.2 adds the runtime check. None of the 13 private RAM-push manifests (12 CC builds of 131 publications, one bank build of 250) names an address in the extent or in handler A.

## 6. Composition

### 6.1 With RC0 and CC

RC0 changes only image 4. The CC candidate (`16c830ad…2c89`) changes image 4, image 5 (45 in-place words from `0x00d461ec` to `0x00eab32c`, no blob), the `0x83e` record and plain; none of its image-5 words is in the stub extent or at `0x00ecf900` (checked against the pinned `cc-image5.json`). `//extensions:build-extension` takes at most one patch set per image, so the toggle enters as part of a merged image-5 patch set:

| Step | Kind | Content |
| --- | --- | --- |
| 1 | `image-patch-set`, segment 4 | CC's `cc-image4.json`, unchanged (RC0's writes, prefix at `0x0061c200`) |
| 2 | `image-patch-set`, segment 5 | CC's 45 words, the hook word and the stub blob |
| 3 | `image2-record` | CC's `0x83e` record, unchanged |
| 4 | `plain-patch-set` | CC's `cc-plain.json`, unchanged |

Budget: the prefix window stays at CC's 484 of 512 bytes, the converter extents at 84 of 88 and 68 of 92, plain and image 2 unchanged. The toggle uses 92 of the 144 extent bytes and one in-place word; image 5 is re-encoded in place with no load or size change, so it adds no boot-path item. The combined candidate inherits CC's open base-alignment note (classic-chrome-patch.md 11.3 item 5).

Tooling, following CC's re-inclusion of RC0: a Haskell builder `//host/firmware:build-usb-toggle BASE_MANIFEST NEW_OUTPUT_DIRECTORY` links the stub from `host/firmware/usb-toggle-stub.S` and `.ld` at `0x00bf4454` (assembly, as CC's stubs: a register-contract shim at a stock call site), checks the hook word and the extent hash against the stock decode, refuses a base image-5 write that overlaps either, and writes the merged image-5 patch set, the manifest reusing the base's other parts by path, and a changed-word listing that groups the toggle as `TOGGLE`, so it can be reviewed and reverted apart from CC. `build-extension` stays unchanged. Because the CC parts carry the private data block, the merged manifest and digest stay under `private-data/`, as CC's do. The same builder over `extensions/rc0.json` gives an RC0 plus toggle image whose image-4 load stays at `0x0061c340`, if the toggle should not wait for CC's flash review.

### 6.2 With the bank and the RAM-push route

- **RAM push (bank and CC payloads).** No publication touches the toggle words. `ram-alloc` and `read-ram` refuse unless config byte `0xf7` reads nonzero (transport README); on a toggle body with stored 0 the operator makes a Q attach first. The session scripts (reports/ram-push-session-script.md, classic-chrome-patch.md 8.4) and the transport README describe that premise as stored `0xf7 = 1` and need the Q-attach wording once the toggle is installed.
- **Recipe bank and D3 flash designs.** They use the image-4 prefix, plain and image-2 ids; no image-5 overlap with the toggle, and no key-state use.
- **`commit-f7-*`.** In jig boots (system mode 7) the stub never flips, so they see the stored byte. In an ordinary boot after a Q attach, `commit-f7-enable` refuses (start value reads 1) and `commit-f7-restore` would commit 0 over a stored 0.
- **Install.** The ordinary updater resets stored `0xf7`, `0x80` and `0x81` to 0 when its refresh flags are set (inherited, stored-f7 report section 4, corroborated for RC0 by E-FILMSIM-READBACK-150); the flag value for this container is unproved (8.2 item E9). The toggle works with either stored value.

## 7. Review class

- **Install: H3** (firmware update, spec 07 section 3.2). The hook word redirects stock control flow and the stub runs outside any payload allocation, so both are new mechanism bytes: one independent review of the stub source and linked blob, the hook word, the merged image-5 patch set and the container diff; `//emulation:flash-boot-check` on the exact DAT; the owner's visual check in `//emulation:xm1-viewer -- --firmware`; the owner's per-run go naming the operation and the digest.
- **Use after install.** A Q attach writes only the RAM byte `0xf7`, the state the jig gate write leaves until power-off, and opens the same `ff80` channel stored `0xf7 = 1` opens today, including debug function 3 (arbitrary RAM write) for the attached host. Live commands over that channel keep their own classes (`read-ram` H1 under the read grant, `ram-push` H2).

## 8. Verification plan

### 8.1 Native harness

Cases under `private-data/analysis/x-m1-1.50/native-case/usb-toggle/`, generated by a new `//emulation:usb-toggle-cases`. Expected values come from the behaviour table of 3.2, not from the stub.

- **N1, stub matrix.** The linked stub with the stock closures of `0x0069f8fc`, `0x0069fc94`, `0x00632950` (`0x00632920`, `0x00632934`, `0x00632860`, table `0x007748dc`, loader-A copy `0x0004357c`) and `0x0073c15c`. Inputs: cache pointer `0x0096ca50` to a synthetic page with `0xf7` in {0, 1}; `r1` in {0, 1}; word `0x0094f8bc` bit 18 in {0, 1}; `0x009570dc` in {1, 2, 3, 5, 7}. 40 cases; each checks `r0` and the final cache byte, and the permitted change set is that byte and the stack.
- **N2, handler A.** Image-5 slice `[0x00ecf8b8, 0x00ecf9c8)` with the hook word, boundary stubs recording `0x00723a14`, `0x00ed117c`, `0x00ed11a4`, `0x00647398`, `0x0073abec` and `0x00ecf894` with their arguments, latch `0x015c229c`, payload word through `0x015c20cc`. Expected: the boundary call sequence per edge, stored value, Q and system mode. Control: the stock slice gives the same sequence for every Q = 0 case.

### 8.2 Full-system emulator

Each run holds one emulator slot and starts from a checkpoint saved at the shooting screen of the named DAT and NAND. "Stored-0 NAND" is the viewer's erased-settings baseline or the template NAND with `0xf7 = 0`; "stored-1 NAND" is `stored-f7/emulation/inputs/f7-1.nand`. Q is `MenuKey.Q` (ADC channel 9); the cable is `PowerDevices.set_usb_connected`. Enumeration uses the endpoint transport of the `--ff80-rehearsal` driver.

| Run | Inputs | Expected |
| --- | --- | --- |
| E1 | candidate DAT | `//emulation:flash-boot-check` passes |
| E2 | stock, CC and toggle DATs; access probe on `[0x00bf4454, 0x00bf44e4)` along the flash-boot route and one attach | stock and CC: no fetch, load or store; toggle: fetches only after the call from `0x00ecf900` |
| E3 | toggle, stored 0, plain attach | handler A application arm (`0x00ecf99c`), no `0x1705`, no fill, `cache[0xf7] = 0`, `0x0146a0c0 = 2`, `0x01469810 = 2`; GET_DESCRIPTOR returns the template descriptor patched from `cache[0x100]`/`cache[0x13e]` |
| E4 | toggle, stored 0, Q held, attach | `0x0094f8bc` bit 18 set before the edge; `0x1705`, the fill, `0x0146a0c0 = 3`, `0x01469810 = 3`, `0x0146a0cc = 0x00d80cf0`, `cache[0xf7] = 1`; descriptor `04cb:ff80` (`0x00d7ef00`); C0, identity `X-M1`, config window 0 with `0xf7 = 1`, one function-2 read; after Q release the bit clears; SP at `0x00ecf900` recorded against the usbcont stack extent |
| E5 | E4 continued | detach: `0x1706`, transport stop, no application `0x90`; plain re-attach: `ff80`; detach; Q re-attach: `cache[0xf7] = 0`, application `0x10`, `0x0146a0c0 = 2`, PTP descriptor; detach: application `0x90` |
| E6 | toggle, stored 0, cable present from reset, with and without Q held from reset | system mode 5, PTP; `cache[0xf7]` stays 0 |
| E7 | toggle, stored-1 NAND | plain attach `ff80`; Q attach PTP with `cache[0xf7] = 0` |
| E8 | toggle, stored 0, `--ram-push-e2e` with the attach step holding Q | CC payload alloc, push, checks and unload pass |
| E9 | emulated updater installing the candidate over the stored-1 NAND | stored `0xf7` after the update, which settles whether the install resets it |

Harness work: the `--ff80-rehearsal` driver stages stock 1.50 and attaches without input; it needs `--firmware`, a key held across the attach, and a PTP-expectation mode that checks the descriptor and transport mode instead of running C0. The e2e attach step needs the same key hold.

### 8.3 After install (owner, later)

Plain attach: the host lists `04cb:02b6`. Q attach after the shooting screen: the host lists `04cb:ff80` and `read-config` of the gate-byte plan reads `0xf7 = 1`. Power cycle and plain attach: `04cb:02b6` again. Shooting and playback normal after closing the Q menu.

## 9. Open questions

1. Which UI states make `0x00d36ea4` skip the key-state update (`0x00d35f68() == 1`, `[0x0094f5e0] == 2`); a Q press in those states is not seen, and a press seen with its release skipped would leave the bit set (E4 checks the shooting screen and the Q menu).
2. usbcont stack headroom: the stub's deepest path (`0x00632950` through `0x00632860` and the copy) adds about 100 bytes at `0x00ecf900` (E4 records SP).
3. Whether a PTP start after an `ff80` stop in the same boot behaves on the body as in E5.
4. For the power-on variant only: the content of the boot records at `0x0073ae10` for a held Q, and the source of the startup Fn and Q press forms.
5. The updater refresh flags for this container (E9).

## 10. Reproduction

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

Caller, word, pair, ADR and Thumb scans over the same listings and the eight raw inputs were run in a host session; they miss register-indirect references whose base is computed at run time, which 8.2 E2 covers on the executed paths.
