# Scanout commit and frame scheduler contract

## Scope and evidence

This is the stock contract and current CPU replacement boundary for `replacement/display_scanout` in [target design §2.4, S8 and S9](../display-render-target-design.md). Software effects are separate from device laws L1-L5. Source implementation and host comparison sources do not establish runtime ownership or authorize camera work.

The current source pin is x70view `55436fc85c0560dd252b0410af6f2427fe198cd3`. The four display translation units below are unchanged from the earlier report pin `a72c95a3572562fc05f359b4f35915f3a0243b19`, confirmed by a path-limited Git comparison. Evidence follows [xm1-decomp.md](../xm1-decomp.md): every requested function is **NONMATCHING**, except the auxiliary selector getter `FUN_006c8850`, which is **exact, source-inspected**. The implementation was checked against image-4 instructions, including the recursively required packing, unique-value collection and float conversion helpers. Names, packet interpretations and “blanking” are not independent evidence. Byte citations `B[address-address]` refer to `private-data/analysis/x-m1-1.50/lz/id4.dis`, backed by `lz/id4-0061c400.bin`, base `0x0061c400`, SHA-256 `f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16`. Addresses are inclusive instruction addresses.

The principal C reading aids are `app/drivers/display/tu_006c7b9c.c` (commit, capture, timing, formatter configuration and blanking), `tu_006c6d6c.c` (scheduler and ISRs), `display_saved_state_more.c` (setters, reset and wait), and `tu_006ce0d8.c` (formatter strobe). All paths are below `decomp/firmwares/xm1-150/` at the pin.

The existing x70view working tree is at `c44204c8c1c87e00ed15a7d33572968afadd1b29`. Its C was used as a reading aid without changing that checkout. A path-limited comparison confirms the four listed display units are also unchanged between `55436fc85c0560dd252b0410af6f2427fe198cd3` and that working-tree revision. Pinned `git show 55436:.../tu_006c7b9c.c` was additionally inspected; image-4 byte confirmation is the behavioral evidence for NONMATCHING functions.

## State and engine identity

For valid stock engine IDs 0 and 1, define `R(e) = 0x009553cc + e*0x58`, `S(e) = 0x0096b9bc + e*0x1a4`, `I(e) = 0x009554b8 + e*0x23c`, and `T(e) = 0x009429f4 + e*0x58`. These are runtime, saved register image, input/preparation image and timing state, respectively. Register banks are scanout `D(0)=0xfff6b000`, `D(1)=0xfff6e000`; timing `Q(0)=0xfff6a000`, `Q(1)=0xfff6d000`; formatter `F(0)=0xfff69000`, `F(1)=0xfff6c000`. The functions commonly select bank 0 for zero and bank 1 for any nonzero argument, while their RAM arithmetic still uses the argument. This is not engine-range validation. Owned boundaries must represent the two valid engines explicitly, not make invalid stock indexing part of a safe API. B[6ca3f0-6ca420], B[6ca7bc-6ca7e8], B[6c7ba4-6c7bb0], B[6c9d08-6c9d24].

`0x00955af8` is a selection/control record, not an engine ID cell. Wrappers select engine 1 only when its `+0x28` word is zero and its `+0x10` word equals 1; otherwise they select engine 0. For example `0x006cfc6c` preserves its caller argument in r1 then calls `0x006c8fac` with this selected engine; `0x006cfcac` preserves its caller argument in r2 and calls `0x006c8f38` with selected engine and r1=5. B[6cfc70-6cfcec]. This selection is distinct from the scheduler's timing-source selector `T(0)+0x58`, and from the global alternate-mask byte `0x00916610`.

## Commit: `FUN_006ca3e0`, 0x006ca3e0

### Invocation and processing flag

The engine wrappers `0x006cb0c4` and `0x006cb0d8` call commit with 0 and 1, respectively. The capture routine registers the former persistently with scheduler phase 0 and the latter with interrupt source 14. Commit therefore runs on each delivery of its registered frame interrupt/callback, not only when a new image is pending. Only the preparation and register-write body is conditional: it executes exactly when the word `R(e)+0x0c` equals 1 and clears that word to 0 before any preparation call. Values 0, 2 and other non-1 values bypass the body. The two callback-list drains are unconditional. B[6cb0c4-6cb0e8], B[6ca3f0-6ca410], B[6caea0-6caeec], B[6c96bc-6c96ec], B[6c991c-6c9944]. This does not establish that every timing phase invokes commit.

### Preparation before register writes

The pending branch clears a 36-byte local default packet, calls `0x006c896c(e, packet)`, converts four default floating values through `0x0089be38`, and calls `0x006ca328` with the derived packet. Scale is 1000 only if `R+4 == 2` and `R+8 != 0`, otherwise 10. B[6ca414-6ca4ac].

It processes ten records at `I+0xf4+n*0x20`, then two at `I+0xb4+n*0x20`. Each forms four halfword coordinates through `0x0089bddc`, two signed halfwords scaled by scale/10 with truncation toward zero, three selector bytes and a copied flag byte, then calls `0x006ca04c(e,0,n,packet)` or `(e,1,n,packet)`. Selector bytes encode the first matching default among four values (else 4), four integer values (else 4), and two bytes (else 2). B[6ca4cc-6ca7b8]. The owned implementation includes these float and packing helpers; unmatched selector values retain the source's spill outside the cleared two-bit or one-bit selector field.

Before emitting the map below, commit copies `I+0x3c` to `S+4`; copies low 12 bits of 40 halfwords from `I+0x58..+0xa6` to `S+0xfc..+0x14a`, preserving each high nibble; and does the same for eight halfwords `I+0x44..+0x52` to `S+0xec..+0xfa`. B[6ca7bc-6ca8c0]. If byte `0x00916611` is nonzero, per-record nonzero flags at `I+0xa8..+0xb1` and `I+0x54..+0x55` replace the two vertical halfwords with low12(`low12(S halfword +0x12)+1 - low12(opposite input halfword)`), preserving their high nibbles. It toggles bit 0 of byte `S+4`. B[6ca8c4-6ca9c4].

It copies `I+0x0c` to `S+0x34`, conditionally adds `I+0x34` to low14 of `S` halfword `+0x36` when `S+4` bit 0 is set, and `I+0x38` to low14 of halfword `S+0x34` when bit 1 is set. It copies the eight words at `I+0x14..+0x30` into the four `S+0x5c+16*n` and four `S+0xa8+16*n` words. If either addition word is nonzero, it calls `0x006c8870` twice, using the group flag bits and selector bits already in `S`. B[6ca9c8-6caaf0]. These mutations mean a test must not assume that the incoming saved image is emitted unchanged.

### Full saved-image to scanout offset map

All entries are 32-bit accesses. Offsets are hex. Scalar rows retain program order; indexed rows expand in increasing index, with the destinations in their listed order inside each iteration. All accesses assume **L4** (readable/programmed register storage and the device's latch semantics); their scheduling relative to a frame additionally assumes **L1**, and usable time before the next frame assumes **L3**. The instruction stream alone does not prove atomic latching.

| Saved offset in S(e) | Register offset in D(e) | Byte evidence |
| --- | --- | --- |
| 00,04,08,0c,10,14 | 00,04,08,0c,10,14 | B[6caaf4-6cab2c], B[6caef4-6caf24] |
| 18, engine 1 only | 18 | B[6cabcc-6cabd4], B[6caf28-6caf2c] |
| 1c,20,24,28,2c,30 | 1c,20,38,3c,88,8c | B[6cab30-6cab5c], B[6caf30-6caf60] |
| 34,38,3c,40,44,48,4c | 24,28,2c,30,d4,d8,dc | B[6cab60-6cab94], B[6caf64-6caf98] |
| 50,54,58,9c,a0,a4 | 34,40,e0,84,90,e4 | B[6cab98-6cabe0], B[6caf9c-6cafcc] |
| 5c+16*n,60+16*n,64+16*n,68+16*n; n=0..3 | 44+4*n,54+4*n,64+4*n,74+4*n | B[6cabe4-6cacd8], B[6cafd0-6cafec] |
| a8+16*n,ac+16*n,b0+16*n,b4+16*n; n=0..3 | 94+4*n,a4+4*n,b4+4*n,c4+4*n | B[6cac8c-6cacd8], B[6caff0-6cb010] |
| ec,f0,174,f4,f8,178 | 10c,110,194,114,118,198 | B[6cace4-6cad5c] |
| fc+8*n,100+8*n; n=0..9 | 11c+8*n,120+8*n | B[6cad60-6cadb0], B[6cb014-6cb020] |
| 14c,150,154 | 16c,170,174 | B[6cadb4-6cade0], B[6cb024-6cb040] |
| 158+4*n; n=0..3 | 178+4*n | B[6cade4-6cae18] |
| 168,16c,170,e8 | 188,18c,190,108 | B[6cae1c-6cae44], B[6cb044-6cb064] |
| 17c,180,184,188,18c,190,194,198,19c,1a0 | e8,ec,f0,f4,f8,fc,100,1bc,9d0,9d4 | B[6cae48-6cae9c], B[6cb068-6cb0b8] |

The two indexed four-item groups are interleaved per item: first group's four words, then second group's four words, then the next item. They are not eight independent register runs. There is no access to engine-0 `D+0x18` in this map.

### Completion callbacks

After the pending branch, drain `first[6]` at `R+0x24..+0x38`, then `second[6]` at `R+0x3c..+0x50`, each in ascending slot order. For each nonzero callback, clear that slot before calling it with no explicitly prepared arguments. Empty slots do not stop scanning. A callback may modify later slots, which are read when reached, rather than from a snapshot. The caller observes completion callbacks even when no register image was pending. B[6caea0-6caeec]. Callback execution is software behavior; interpreting it as “visible on the panel” additionally requires L1/L4 and is not established.

## Capture and registration: `display_save_context`, 0x006c926c

The pin names this `FUN_006c926c`; the descriptive name here identifies its role. When byte `R+0x54` is zero, clear the 0x58-byte runtime record, set `R+4` from input word 0 (if zero, default 1 for engine 1 and 3 otherwise), set `R+0` from input word 1 and `R+8` from word 2, then call reset. B[6c9280-6c92f0].

Capture the reverse of the commit map above except `S+0xe8` / `D+0x108`: that pair is written by commit but is not captured here. Set bits 1 and 2 in the low byte of captured `S+0`. Engine 1 captures `D+0x18`; engine 0 does not. Capture covers the scalar words, the two four-item groups, the two rectangles, the ten indexed pairs, the tail words and `D+0x9d0/+0x9d4`. B[6c92f4-6c96a8], B[6c9754-6c9918]. Each capture access assumes **L4**, including readback validity after reset; a RAM-backed register file establishes software copying only.

If `R+0x14` has no registration, engine 1 calls `driver_configure_boot_handle` (`0x0064e918`) with source 14, the direct callback address `0x006cb0d8` in r1 and output handle at `R+0x14`. Its local `{callback,1,1,0}` tuple is not passed to this primitive. Engine 0 passes `{0x006cb0c4,1,1,0}` to `FUN_006c7318`. B[6c96ac-6c96ec], B[6c991c-6c9944]. The scheduler tuple is callback, persistent selector, unsigned order, phase. Primitive `0x64e918` forwards its second argument directly to `0x723870`, B[64e928], B[64eaa4-64eaa8], B[723888-7238b8]. Registration assumes **L1**; source 14 is not scheduler source 9/10. The first initialization path increments byte `R+0x54` even if registration fails and propagates its result through r0. No rollback occurs. B[6c9740-6c9748].

If already initialized, compare all three configuration words; identical words return 0 without recapture or increment. Changed words update `R+4`, `R+0`, `R+8`, return 0 and increment the initialization byte. No register recapture occurs on this path. B[6c96f4-6c9748].

## Per-frame service: `FUN_006c7318`, 0x006c7318

### Registration and ordering

The boundary packet is four words `{callback, persistent_selector, unsigned_order, phase}` and an output handle pointer. The routine enters a critical section, zeroes the output handle and rejects a null callback with -30. Nonzero persistent selector chooses the four persistent slots at `0x00917ee4`; zero chooses four one-shot slots at `0x00917f24`. A slot is `{handle, order, callback, phase}`, 16 bytes. Normalize packet phase in place: exactly 1 stays 1, every other value becomes 0. B[6c7318-6c7394], B[6c75e8-6c75ec].

Insertion is before the first slot whose unsigned order is strictly lower than the new order. Thus order is descending; equal orders stay ahead of the new entry. Shift later slots toward the tail and drop the old last slot when insertion succeeds. If no slot has lower order, return -20 with output handle still zero. This is not simply “reject the fifth registration”: a higher-order fifth registration evicts the tail, and order zero cannot beat a zero-initialized slot. B[6c7398-6c7444], B[6c7598-6c75e4].

Handle counter is `0x00917ed0+8`. Before insertion, repair zero to 1. Successful insertion publishes that counter as the output handle and slot handle. Accepted phase-1 registration increments `0x00917ee0`, including persistent registrations. The counter increments on both successful insertion and -20, modulo 32 bits; null-callback rejection returns before this increment. B[6c7350-6c736c], B[6c73c0-6c7440], B[6c7574-6c7594]. No eviction callback or phase-count repair is visible in the insertion loop.

### Source selection and failure behavior

`FUN_006c8850` returns word `0x009429f4+0x58` (exact C at the pin; also B[6c8850-6c886c]). Zero selects ISR `0x006c70e4`, source 11 for phase 0 or 12 for phase 1, handle cell `0x00917ed4`. Nonzero selects ISR `0x006c6eb0`, source 9 or 10, handle cell `0x00917ed0`. On successful insertion with no existing selected interrupt handle, call `0x0064e918`; registration failure clears that handle cell but does not undo the inserted callback or change the insertion return code. A persistent selector exactly 1 sets activity `0x00917edc` after successful interrupt registration or on the existing-handle path. It also sets activity when insertion returns -20. Other nonzero selectors choose the persistent list without this exact-1 behavior. B[6c7448-6c7574], specifically B[6c74c8-6c74d4], B[6c755c-6c7568]. Source registration and critical exclusion assume **L1**.

## ISRs: 0x006c70e4 and 0x006c6eb0

Both implement the same state machine using different timing banks and interrupt handle cells:

| ISR | Status read | Handle cell | Sources |
| --- | --- | --- | --- |
| `0x006c70e4` | `0xfff6a030` | `0x00917ed4` | 11/12 |
| `0x006c6eb0` | `0xfff6d030` | `0x00917ed0` | 9/10 |

The listing reads status once into a local, then tests that saved word, not a second MMIO read as the NONMATCHING C suggests. B[6c6ebc-6c6ecc], B[6c70f0-6c7100]. Read polarity assumes **L2**; delivery and source switching assume **L1**.

When status bit 1 is clear, or the phase-1 count `0x00917ee0` is nonzero, enter critical, attempt release of a live handle, clear it only on release success, select lower source for clear status or upper source for set status, and register it only if handle is now zero and activity `0x00917edc` is nonzero. Clear the handle on registration failure. Dispatch phase is 1 for clear status and 0 for set status. When bit 1 is set and count is zero, dispatch phase 0; release a live handle if activity is zero, without the surrounding critical section used by the switching branch. B[6c6ed0-6c6fdc], B[6c7104-6c720c].

Scan all four one-shot slots first, ascending address. For nonzero callback and matching phase, decrement phase-1 count if phase is 1, clear all four slot words before invoking callback. Then scan persistent slots in order, stopping at the first zero handle or after four slots, calling nonzero matching-phase callbacks without clearing them. Finally activity becomes 1 if the persistent scan traversed any slot, otherwise 0; this is not a census of one-shot work. B[6c6fe0-6c70dc], B[6c7210-6c730c]. Both loops read the live arrays as they progress; callback re-registration can affect later iterations. One-shot-before-persistent order is part of S8's observable contract, independent of the relative order of the commit's own two six-slot lists.

## Shadow setters

These functions make RAM writes, not immediate MMIO writes, and do not set the processing word `R+0x0c`. Engine/group validity is a caller precondition unless an explicit check is stated. Physical interpretation of the eventual fields depends on **L4**, but the mutations below need no peripheral model.

### 0x006c9cf4, `display_set_item_configuration`

Reject unsigned item >3 with -30. At `S+0x64+group*0x4c+item*16`, replace low 30 bits from the value and preserve bits 31:30; return 0. B[6c9cfc-6c9d34].

### 0x006c9d44, `display_write_range_packet`

Reject unsigned item >3 with -30. The bytes establish the packet layout, which must not be copied from the NONMATCHING C's guessed fields:

| Packet access | Effect |
| --- | --- |
| u16 +0 | Replace bits 29:16 at `I+0x14+group*16+item*4` |
| u16 +2 | Replace bits 13:0 in that same word |
| u16 +4 | Store `(value-1)` low13 in halfword `S+0x62+group*0x4c+item*16`, preserving high3 |
| u16 +8 | Store `(value-1)` low13 in halfword `S+0x60+group*0x4c+item*16`, preserving high3 |
| u16 +6 | Store low12 of `max(packet.u16(+6)-packet.u16(+4),0)` in halfword `S+0x68+group*0x4c+item*16`, preserving high4 |
| u8 +0x0a | Clamp to 128 and store at `S+0x58+group*0x4c+item` |

Return 0. Zero dimensions wrap through subtract-and-mask, not validation. B[6c9d58-6c9e68]. The halfword at packet +6 is the padding comparison operand, not the second dimension.

### 0x006c9e7c, `display_set_range_enabled`

If byte `R+0x54` is zero return -10; then reject item outside 0..3 and 0xff with -30. If byte `0x00916610` is zero, choose halfword `S+0x54+group*0x4c`; otherwise choose `I+4+group*4`. Exactly value 1 enables, every other value disables. Single-item mask is `1 << (4*item)`; all-item enable is OR 0x1111 and disable is AND 0xeeee. Return 0. B[6c9e84-6c9f60].

### 0x006c9f70, `display_write_group_options`

Replace low3 of byte `S+0x50+group*0x4c` from input word 0. If group is 1: replace low4 of `S+3` from word 1, bits 5:4 of `I+0x3c` from word 2, bits 3:2 of `S+9` from word 3 and bits 1:0 of `S+9` from word 4. Otherwise clear bits 7:4 of `S+2` (word 1 is not loaded), replace bits 3:2 of `I+0x3c` from word 2, bits 7:6 of `S+8` from word 3 and bits 5:4 of `S+8` from word 4. Preserve all other bits; return 0. B[6c9f78-6ca03c].

## Blanking: `FUN_006cb8b4`, 0x006cb8b4

An uninitialized `R+0x54` returns -10 in r0, although the draft C declares void. Otherwise store packet byte 0 to global byte `0x00916610`, and packet bytes 3,2,1 to `S+0x180,+0x181,+0x182`. For a nonzero byte 0, save `S+0` bit 0 to `I+0` bit 0 and clear it in S; move halfwords `S+0xe8,+0x54,+0xa0` to `I+0x40,+4,+8`, zeroing the S halves. For zero byte 0, perform the reverse transfer and clear the corresponding I bit/halves. B[6cb8bc-6cb9bc]. It does not check for an already-blank state: repeating nonzero blanking can overwrite the saved values with zeros. It does not write registers or request a commit. The eventual scanout control/mask/color effects assume **L4**; “black on the physical display” is not established by these RAM moves.

## Reset: `display_reset_register_bank`, 0x006c9160

Read and save `D+0`, write exactly `0x00010000`, read-modify-write clearing bit 16, then poll until read bit 16 is **set**, and write `saved | 0x00010000`. No delay, timeout or error result is present. B[6c9168-6c9210]. Every access assumes **L4**. In particular completion polarity is set, not clear; a plain memory cell holding the last written zero bit hangs forever. Reset completion and preservation of unrelated device state are distinct from merely observing these writes.

## Timing configuration

### `display_engine_flush_configuration`, 0x006c7b9c

If word `T+0x10` is zero, return 0 without MMIO. Otherwise clear it before writes and publish `T+0x28,+0x2c,+0x30,+0x34,+0x38,+0x3c,+0x40` to `Q+8,+0xc,+0x10,+0x14,+0x18,+0x1c,+0x20`; engine 1 additionally publishes `T+0x44,+0x48` to `Q+0x24,+0x28`; then publish `T+0x4c,+0x50,+0x54` to `Q+0x2c,+0x34,+0x38`. B[6c7ba4-6c7c60], B[6c7d3c-6c7da4]. These are **L4** register-storage/latch assumptions; the resulting period is **L3**, and interrupt timing is **L1/L2**.

Read `Q+0`, replace bit 8 in the read value from bit 8 of `T+0x20`, and compare with `T+0x20`. If equal, read `Q+4`; if it equals `T+0x24` and force word `T+0x14` is not 1, return 0 despite the preceding parameter writes. Otherwise clear force word, write `T+0x20` to `Q+0`, write `T+0x24` to `Q+4` only if nonzero, and return 1. B[6c7c64-6c7d2c]. Return 0 does not mean “no device writes.”

### `FUN_006c8790`, 0x006c8790

Store packet byte +4 to `T+0x1c` and call `0x006c7da8(e,T+0x50)`, which supplies defaults for fields not explicitly selected. Then packet +4 bit 0 selects replacement of bits 27:16 from packet u16 +0; bit 1 selects bits 2:0 from packet byte +2; bit 2 selects bit 28 from packet byte +3. Write the resulting word `T+0x50` immediately to `Q+0x34`, return 0. B[6c8798-6c8848]. Default acquisition in `0x006c7da8` uses service `0x0069f8fc`; it is an external input to the contract, not constant zero. B[6c7dcc-6c7e1c]. Device storage and interpretation assume **L4**; any timing consequences assume **L1-L3**.

### `display_wait_input_ready`, 0x006c8710

Read-modify-write `Q+0` clearing bit 16, then read `Q+0x30` repeatedly until bit 1 is **set**. Between unsuccessful polls call `0x00723ce8(0)`; the draft C names a different wrapper, so the byte address is the contract. No timeout is present. B[6c8718-6c878c]. Control write assumes **L4**; readiness polarity/progress assumes **L2**, and time to progress assumes **L1/L3**. Neither a zero-initialized register page nor a permanently-ready page proves the real handshake.

## Formatter

### Configuration: `FUN_006cc1f0`, 0x006cc1f0

Let `U=0x00917c10+e*0x128`, `P=U+0x38`. If word `U+0x34` is zero do nothing; otherwise clear it before writes. Engine exactly 1 uses F(1), all other arguments use F(0). Copy these 32-bit offsets from P to the same offsets in F, in the following order: `0,4,0x20,0xc8,0x34`; then `0x38..0x48` (five words); `0x64..0x74` (five words); `0x78`; `0x7c..0x90` (six words); `0x94..0xa8` (six words); `0xac..0xc0` (six words); finally `0xc4`. B[6cc1f8-6cc428]. No strobe call occurs here. Configuration accesses assume **L5** (writable configuration and its relationship to the strobe); register readback/latching remains unmeasured, not established by plain storage.

### Strobe: `FUN_006ce2dc`, 0x006ce2dc

Enter critical; read-modify-write F+0 OR bit 28; leave critical; poll F+0 bit 28 until it is **clear**. Zero chooses F(0), nonzero F(1). The polling loop is outside the critical section and has no yield, timeout or error return. B[6ce2e8-6ce380]. Every access assumes **L5**, including self-clear progress; a memory-backed word without scripted behavior will hang. The software sequence does not establish strobe latency, write refusal while pending, or when the new format affects output.

## Device-law ledger and hardware status

The labels below retain [target design §3](../display-render-target-design.md) meanings. Status is from [fidelity audit §2.2 and §2.4](../emulator-device-fidelity-audit.md), model FujiMod `3d90035d`; no new hardware result is asserted.

| Law | Accesses or dependency in this contract | Declared model | Hardware status |
| --- | --- | --- | --- |
| L1 Frame delivery/phase order | Source 14 registration and commit wrappers; scheduler sources 9/10 or 11/12; ISR release/re-register and callback ordering relative to commit | Three timing phases; bank 0 lines 0x34/0x34/0x35, bank 1 lines 0x37/0x37/0x38 at p/2, 3p/4, p | None for phase order/delivery; interrupt-controller delivery also ungrounded. Static source mapping is not camera evidence |
| L2 Timing status polarity | Both ISR reads of Q+0x30; wait-input-ready polling | Bit 1 set at phase 0, clear at phase 1; reset value 2 in target design | None; byte tests establish the firmware's expectation, not device polarity |
| L3 Frame period | Timing register programming, scheduler latency, time available to complete a commit and client waits | 400,000 ticks/frame, about 30 Hz at declared 12 MHz [INFERENCE] | None; the 30 fps install video cannot resolve refresh |
| L4 Scanout latch/reset | Every D read/write in capture, reset and full commit map; Q control and timing programming/readback | Register file and two-tick reset completion | None for latch/reset or timing-register semantics. Held RAM shadows and buffer geometry are firmware state, not register captures |
| L5 Formatter strobe | Every F configuration write and F+0 bit-28 read/write/poll | Latch after two ticks, pending-write refusal declared by target design | None; static self-clear polling is not a measurement of completion or pending-write policy |

L1-L5 are not an exhaustive hardware specification. In particular, L4 groups the timing/scanout register-storage assumptions for this contract but does not establish pixel composition, cache/DMA visibility, bus ordering, electrical output or panel routing. Composition/pixel format is target law L6, outside this assignment. No instruction here proves that a sequence of register writes becomes one atomic visible frame.

## Proposed host contract cases

These are pre-run expectations derived from the instructions above, not exercised results. Generate proprietary stock extents/cases under `private-data/analysis/x-m1-1.50/native-case/scanout/` using the surrounding Haskell native-case pattern. Run stock and candidate on identical initial state and external-service outcomes. Compare return values, retained public state, ordered register accesses and callback-visible effects; use distinguishable values per source word and sentinel bits outside each mask. Callback recording must capture caller-visible effects rather than assert private helper structure.

The native-case runner maps no peripherals. Do not claim that an ordinary native case executes live MMIO. A separate host oracle or runner facility with explicit memory-backed pages at D/Q/F can check the software accesses below; read-dependent handshakes need a scripted register model or access interception. Fixed register RAM alone cannot establish device laws or terminate every loop.

| Cases | Expected observation | Required environment |
| --- | --- | --- |
| Commit flag 0, 2, 1 for both engines | Only 1 clears processing and emits the full ordered map; all values drain both callback lists | Memory-backed D and real/stubbed deterministic preparation dependencies; flag-0/2 paths need no device page |
| Map coverage with distinct source words | Every destination above receives the correct prepared word, engine 0 never touches +0x18, bank isolation and untouched holes preserved | Memory-backed D plus access trace; seed preparation inputs so rewritten words have independently computed expected values |
| Preparation branch matrix | 10 then 2 record processing, scale choice, low12/high4 preservation, vertical reversal, bit toggle, 14-bit additions and two group-address updates | Host oracle or native dependencies with controlled default/helper results; no claim about pixels |
| Empty/sparse/full commit lists, callback requeue | First before second, six slots each, slot cleared before callback, later-slot mutations visible, already-passed slot waits for next invocation | Native RAM and callbacks; no peripherals for no-pending branch |
| Capture first/repeated/changed configuration | Reverse map excluding +0x108, low-byte OR6, correct engine source registration, byte +0x54 behavior, registration failure without rollback | Memory-backed D with scripted reset and registration service outcomes |
| Scheduler null callback, zero/equal/high unsigned orders, fourth/fifth insertion | -30, -20, stable ties, tail eviction, output handle and wrap/zero-repair behavior as specified | Native RAM with critical/interrupt service boundaries recorded |
| Scheduler persistent selectors 0/1/2 and phases 0/1/2 | Separate list choice versus exact-1 activity, phase normalization in caller packet, correct count and source | Native RAM and registration outcomes; no timing device needed |
| Both ISR polarities, phase-count zero/nonzero, release/register success/failure | One status read, proper source switch, handle effects; matching one-shots cleared before calls and before persistent callbacks | Scripted or memory-backed Q+0x30 and service outcomes; no autonomous interrupts required |
| Setters for every item/group, invalid item and uninitialized runtime | Exact masks, packet byte offsets, dimension wrap, opacity saturation, all-item mask, preserved reserved bits; no MMIO or processing-word change | Native RAM only |
| Blank, unblank, repeated blank, uninitialized | Byte order, global alternate-mask byte, saved halfword transfer and repeated-blank loss, -10 path | Native RAM only |
| Timing dirty clear/set, equality, force and zero/nonzero control word | Parameter writes before equality return, bit-8 comparison, engine-1-only words, return 0/1 | Memory-backed Q and access trace; defaults controlled for 0x6c8790 |
| Reset bit transitions and never-completes control | Save, exact 0x10000 write, clear, poll-set, restore saved OR0x10000; bounded harness reports nontermination for held clear | Scripted D, not plain RAM; does not measure two-tick latency |
| Wait ready immediately/delayed/held clear | Control bit16 cleared, failed polls invoke 0x723ce8(0), exit only on set status | Scripted Q, bounded harness for held-clear |
| Formatter dirty and clean, each engine, delayed/held strobe | Exact ordered configuration writes, dirty cleared before writes; strobe critical region ends before polling; exit only after clear | Memory-backed F for configuration; scripted F for strobe |

After software qualification, emulator evidence must cover autonomous interrupt arrival, status phase progression, registration switching over multiple frames, reset/strobe progress and integrated publication. Such evidence establishes behavior of that declared model only. Camera evidence is required to ground L1-L5: phase/status correspondence and actual period, register readback and reset progress/polarity, commit-to-visible-frame relationship, and formatter strobe completion/latching. Existing `read-ram` DRAM admission does not directly admit 0xfff6xxxx MMIO; any hardware sampler requires its own approved route. No camera operation is proposed as part of this document's execution.

## What is not established

- No stock/candidate host cases, emulator runs or camera measurements were performed for this contract; the table is a qualification proposal.
- No hardware evidence grounds L1-L5, and a stock/owned pair sharing the same model cannot independently validate those laws.
- CPU preparation, default collection, packing and bitwise float conversions are implemented in the owned package. Native qualification of those implementations remains required. Calibration values are external inputs read through the owned calibration package. IRQ registration/release, critical exclusion, MMIO and scheduler yield remain explicit primitive boundaries.
- Callback cancellation APIs, handle uniqueness across wrap, guarantees against reentrant slot mutation, interrupt latency bounds and registration cleanup are not derived from these entry points. Successful insertion can evict a live tail.
- Register names beyond their software offsets, physical layer alpha/color meaning, panel choice, memory coherency, MMIO ordering barriers and atomic hardware latch points are not established by the listing.
- S8 can retire only when retained clients no longer use the stock scheduler or the owned service preserves their request and callback contract. S9 stays until timing, reset and formatter laws are grounded and owned bring-up is qualified. This document does not remove either shim.

## Current owned CPU interface

`replacement/display_scanout/src/lib.rs` defines the two valid engines and register-bank identities. The safe CPU modules use named runtime, preparation, plane, rectangle, saved-register, timing and formatter records; callback slots, scheduler lists and scheduler counters are semantic types. Stock software byte offsets exist only in the ARM layout/publication adapter. `shadow.rs` owns the four setter families above and blanking over semantic field reads/writes. `commit.rs` owns preparation, the complete ordered map, live completion callbacks, capture and both engine wrappers. `scheduler.rs` owns insertion and both ISR state machines over shared typed fields, allowing callback re-registration without aliasing mutable Rust state. `timing.rs` owns staging/default acquisition, immediate timing, flush, reset, readiness wait and formatter configuration/strobe.

`timing_keys.rs` expresses the recovered calibration selectors as ten semantic mode profiles and four route/rate profiles, with named pulse, interval, output and adjustment keys typed as validated `CalibrationOffset` values. Safe staging does not compute original ROM addresses or index flattened firmware table layouts. Selector identities are distilled executable configuration, not captured calibration values or bundled proprietary input bytes.

The ARM adapter exports `xm1_scanout_*` entry points corresponding to the stock entries below. Engine/group/item parsing occurs there. `layout.rs` maps semantic fields to stock RAM addresses and access widths; `publication_edge.rs` publishes only the fields each operation owns. Shadow setters perform their individual volatile field operations directly, without replaying unrelated runtime or preparation snapshots. The scheduler uses live shared fields at `0x00917ed0`; phase/output publication and source selection occur inside the original critical interval. Commit publishes preparation before MMIO, clears the exact completion slot before each callback and reloads runtime fields after it. Capture uses the owned scheduler for engine 0 and direct IRQ source 14 registration for engine 1. The adapter retains only actual MMIO, critical enter `0x7235d8`, critical leave `0x7236b0`, interrupt configure `0x64e918`, interrupt release `0x64eb20` and yield `0x723ce8` primitives. It does not forward CPU preparation to stock.

All ARM arguments are AAPCS words or guest pointers. The prefix in the following table is `xm1_scanout_`. Engines accept only 0/1, groups 0/1, item indices 0..3 and all-item selection 255 where specified. Unsupported identity values return -30 rather than reproducing unchecked stock RAM arithmetic. Void stock entries' r0 is not a comparison observation.

| Owned suffix and arguments | Stock entry |
| --- | --- |
| `item_configuration(e,g,item,value)` | `0x6c9cf4` |
| `range_packet(e,g,item,packet11bytes)` | `0x6c9d44` |
| `range_enabled(e,g,item,value)` | `0x6c9e7c` |
| `group_options(e,g,packet5words)` | `0x6c9f70` |
| `blank(e,packet4bytes)` | `0x6cb8b4` |
| `commit(e)`, `commit_zero()`, `commit_one()` | `0x6ca3e0`, `0x6cb0c4`, `0x6cb0d8` |
| `capture(e,configuration3words)` | `0x6c926c` |
| `scheduler_register(packet4words,out_handle)` | `0x6c7318` |
| `isr_zero()`, `isr_one()` | `0x6c70e4`, `0x6c6eb0` |
| `timing_flush(e)`, `timing_defaults(e,out_word)` | `0x6c7b9c`, `0x6c7da8` |
| `timing_stage(e,configuration3words)` | `0x6c7e50` |
| `timing_immediate(e,packet5bytes)` | `0x6c8790` |
| `reset(e)`, `wait(e)` | `0x6c9160`, `0x6c8710` |
| `formatter_flush(e)`, `formatter_strobe(e)` | `0x6cc1f0`, `0x6ce2dc` |

Timing default bytes and selector-addressed configuration use `calibration_liveview`'s typed `CalibrationOffset`/`CalibrationByte` read boundary. Its `InitializedCalibrationBase` performs individual volatile byte reads without an exclusive cache borrow, copied cache or assumed lock. The caller precondition is the same initialized, readable `0x420000`-byte cache whose base the owned calibration initializer publishes at `0x96ca50`; this does not promise an atomic multi-byte calibration snapshot during reload. The added library target disables only that package's standalone ARM ABI and panic handler; existing cache/publication APIs and standalone profiles remain unchanged. Firmware calibration values and proprietary image bytes are not tracked as fixtures.

The source targets are `//replacement/display_scanout:display-scanout`, `:host-tests` and `:native-suite`. The freestanding ELF is read-only in `0x02000000..0x02040000` and retains stock public RAM layout at its edge. Host tests use synthetic distinct-field inputs and ordered hardware receipts. Native comparison sources require owner-supplied image 4 and the candidate ELF, compare stock and owned public effects, and assert independent expected effects. Scripted host reads establish software polling order only; a held handshake remains nonterminating and is stopped by the harness, not by a candidate timeout.

`native-suite list` enumerates the synthetic catalog without inputs or filesystem output. `native-suite stage|execute RUNNER IMAGE4 ELF PRIVATE_OUTPUT [CATEGORY|CASE_ID]` and the two-artifact form `native-suite stage|execute RUNNER IMAGE4 ELF PANEL_ELF PRIVATE_OUTPUT [CATEGORY|CASE_ID]` admit named selections against a prospective 256 MiB proof bound, including existing bytes below that output root. Serialized execution reports have an enforced pre-write file bound of 8 MiB, or 40 MiB for whole-cache cases; the same bound is reserved before staging. This applies after both runner streams and JSON escaping are represented. The complete catalog is not an authorization to stage it all when that bound is exceeded. Successful first-capture cases isolate reset through a declared receipt at both implementations' reset function boundary; actual reset-held, wait-held and strobe-held cases execute the real CPU loops and require instruction-budget exhaustion. The capture receipt does not qualify reset progress, L4 or panel behavior. Candidate cases omit original scanout, preparation, float and calibration software extents.

The panel cohort uses a second artifact, `//replacement/display_scanout:display-scanout-panel`, built from the same crate with `feature="panel"` and linked by `display-scanout-panel.ld`, which keeps only the standalone exports plus the RAM-only panel entries. Its stock extent `0x006cfe14-0x006d0f28` carries the mode frontends. Cases cover `engine_initialize`, `operation_release` and the eight mode frontends whose bodies touch no device page and no retained service: `window`, `backing`, `enabled`, `range`, `colour`, `window_size` and `colour_packet` in group `mode`, plus `engine` and `lease`. Entries that call a retained service, a device page, or the boot-image unsigned divide at `0x0004d978` remain uncovered; the per-export coverage table is in `private-data/scratch/DisplayNativeQual/coverage.md`.

The native case contract admits a multi-byte access only at its natural alignment. `xm1_scanout_range_packet` read the eleven-byte packet into a stack array whose zero-initialisation the compiler widened to an unaligned word store at `SP+0x1f`; the runner refused it at `0x02002144` before the call completed, while stock `0x006c9d44` loads only aligned halfwords. The adapter now reads the packet halfwords directly from the guest pointer.

The shared native loader permits empty expected general-register observations only for a typed kernel profile expecting `instruction_budget` with nonempty independent expected memory. Every other case retains the independent-register requirement. The held cases assert the real control-word writes and unchanged public memory; wait additionally asserts its yield receipt. Source tests include a real synthetic store followed by an unconditional loop, rejection of returned/non-kernel/empty-memory/wall-budget alternatives, and refusal of a wrong expected memory effect. None of these assertions treats an interrupted function as having returned or a held device handshake as complete.

Source, native, integrated-model and hardware scopes remain separate: source is implemented; source review and the first cohesive host gate are required before accepting native ownership; no integrated-model or hardware result is asserted. No complete live-view claim follows from this unit. The frame publisher, VGLIB/raster/font producers and rendering assets remain their existing owners' interfaces. S7 retires only when the integrated publisher uses this driver's acknowledged publication; S8 retires only after retained-client request/callback parity is qualified; S9 still requires grounded L1-L5 and qualified bring-up. L6 pixel composition, DMA/cache visibility, panel routing and physical presentation remain outside this CPU unit.

Prospective private accounting is `private-data/scratch/LiveViewDisplayOwnership/storage-plan.json`: selected compiler 3 GiB, selected proof 256 MiB, minimum reserve 108 GiB and physical growth guard equal to measured pre-large-phase available space minus 4 GiB. No build is admitted before complete independent source review. Every later Bazel command must use the current main host launcher through `devenv shell --`, the assigned output base and a fresh physical/selected-product admission check. No deletion, reserve reduction, SDK realization or firmware guest execution is part of this source change.

## Whole panel CPU composition

Panel bring-up includes the LCDDEVF actor, not just a queued request. `panel.rs` owns the `0x6ce54c`/`0x6d293c` constructor: command 0..6, opcode `0x3200`, current-task sender byte, destination `0x22`, eight-byte `{command, completion}` payload, and queue **0x22, decimal 34**. A nonzero post result becomes -255; an invalid command becomes -30. Queue acceptance is not completion.

`panel_actor.rs` owns command selection, the queue consumer, event priority 1 before 2 before 4, continuation and software-step dispatch, error notification and completion. Unlike the scanout commit lists, the panel's `0x6d30f4` invokes completion before clearing it, reloads the payload pointer after the call, then clears the reloaded callback slot and busy state. The actual shared asynchronous executor is PowerLifecycle's owned ten-slot `0x64dc9c` implementation. LCD recipe actions and all fourteen terminal actions resolve through an owned action registry, never an original software function pointer. Unknown action identities refuse rather than complete.

`panel_device.rs` owns the callback recipes, calibration decisions, panel register packing, serial programming, PWM and HDMI control. Native serial writes use a source-bound residue input instead of replacement stack contents. Successful descriptor-1 configuration stores zero at original caller `SP-20`; configuration errors -20/-12 preserve the preceding successful open's semaphore **0x16, decimal 22**. The status write therefore retains the distinct `0x3311`/`0x3317` values. An open failure does not reach the write. These are CPU effects, not an assertion that the peripheral ignores unspecified bits.

`panel_route.rs` owns `0x6d20a4`, lookup `0x6d1b3c`, profile application `0x6d1c68` and AVout request `0x649ab0`. The source argument is recorded after routing; lookup uses the existing controller state, not that argument. The route performs two enabled GPIO writes before resource handling, five separately IRQ-protected mux updates, then the third GPIO write. AVout retains the complete request word and the original request/ready handshake.

`panel_formatter.rs` owns the live source frontend `0x6ce460`, its transform/preparation helpers, full 847-instruction base stage `0x6cd3d8`, configuration `0x6ce0d8` and enable frontend `0x6ce1dc`. The enable frontend stores the value in the engine-zero record at `+0x138` and `+0x10`, and engine one then stages the four-word packet at `+0x128` and applies its slot; engine zero only stores. The full stage is reachable from bring-up at `0x6cf9d4` and mode paths at `0x6d1564`, `0x6d1580` and `0x6d159c`; it is not a device-law exemption. It reuses the existing typed formatter image. Source history and uninterpreted configuration identities preserve their raw values; slot-mode validation is not applied to unrelated metadata. Source-invalid engine/slot combinations that hold in the original stage are not made ready by the replacement.

`bootstrap.rs` owns `0x6c7b1c` with clock staging/flush helpers, timing frontend `0x6c8684`, live timing `0x6c86e8`, engine dimensions `0x6cbb88` and unconditional formatter frontend `0x6ce058`. Its ARM exports are `xm1_scanout_bootstrap(configuration)`, `xm1_scanout_timing_configuration(engine, configuration)`, `xm1_scanout_timing_live(engine, configuration, flushed)`, `xm1_scanout_engine_initialize(engine, route, mode)` and `xm1_scanout_formatter_slot(engine, configuration)`. The configuration frontend zeroes the record and forces publication; the live frontend stages and flushes the existing record without forcing it and publishes the flush result. The formatter frontend forwards its input packet and does not take configuration's equality fast path. Extended second-pulse calibration reads descend from the interval pair at key K to pulse pairs K-2 and K-4. Clock MMIO `0xfff68000..0xfff68018` remains a device boundary, not a clock-progress claim.

`mode_frontends.rs` owns completion/window/backing/scale/enabled/range/colour frontends `0x6cfd3c`, `0x6cfed0`, `0x6cffbc`, `0x6d0000`, `0x6d01bc`, `0x6d0498` and `0x6d0ee4`, plus window-size `0x6cfe14`, colour-packet `0x6d0244`, select `0x6d1520` and event-zero commit `0x6cfcf4`, exposed as `xm1_scanout_mode_*`. Window size reads packet word 1 and the selected engine, then returns the zero of `0x6c994c` without a write. Colour packet splits packet bytes 4 and 5 into the record nibble byte `+0x19c`, stores byte 6 at `+0x19e`, word 0 bit 0 at `+0x19f` and word 8 at `+0x1a0`. Select applies the panel device at `0x96bd74` through its second flag, enables the selected formatter engine and strobes it unless the engine is forced, combining both leaf results. Commit uses event 0. The completion frontend uses event 2 of the actual commit producer. Panel frontends `0x6ce68c`, `0x6ce69c`, `0x6ce6ac` and `0x6ce6bc` reuse owned programming/transfer or reach the authenticated zero-return leaves `0x6d4068` and `0x6d4050`. They are not unimplemented hardware callbacks. Playback preset `0x6d1294`, exported as `xm1_scanout_mode_preset`, reads ROM `0x7cee28 + 12*selector + 4`, retains the original unchecked-selector precondition, rejects a loaded source above 11, skips an unchanged source and otherwise invokes the owned timing setter with t…

`operation_lease.rs` owns the per-engine operation lease byte at runtime record `+0x55` behind the initialized byte `+0x54`. Acquire `0x6c9214` refuses an uninitialized engine with -10, increments a held lease and returns -12, and otherwise stores one and runs the timing reset `0x6c9160`. Release `0x6c8ee8` refuses an uninitialized engine with -10, decrements the lease with unsigned wrap and returns -12 while a remainder holds. Dispatcher `0x6d16c8` releases the selected engine for value one and acquires it for any other value. ARM exports are `xm1_scanout_operation_acquire(engine)`, `xm1_scanout_operation_release(engine)` and `xm1_scanout_operation_lease(value)`.

The composed Rust component is `//replacement/display_scanout:libdisplay_scanout_panel`, crate `display_scanout_panel`, with `library` and `panel` features. `//replacement/display_scanout/host:display-scanout-panel` produces `libdisplay_scanout_panel.rlib` and its `libcalibration_liveview.rlib` from the pinned `@lab_xm1_rust` rustc for host consumers that link this crate; the ARM rlib stays the firmware target. The composed component excludes its standalone panic handler but includes the full scanout ARM ABI and panel ABI. Library IRQ/critical/yield adaptation reaches Power's owned `xm1_power_interrupt_lock`, `xm1_power_interrupt_unlock`, `xm1_power_task_delay`, `xm1_power_timer_irq_attach` and `xm1_power_timer_release_handle`. The default standalone build retains its existing primitive bindings. The Power root also links the actual owned message transport, receive/event/priority/timer services, shared executor, serial descriptor 1, resources and diagnostics. Downstream linkage must retain referenced scanout/panel/action symbols, supply the calibration rlib metadata and have one root-owned panic handler. The standalone scanout ELF and its native suite do not qualify this larger composition by themselves.

The commit producer APIs are `completion_queue::event` and `completion_queue::packet`, with ARM exports `xm1_scanout_completion_event(engine, event, callback)` and `xm1_scanout_completion_packet(engine, callbacks)`. The latter is the actual `0x6c8fac` six-word packet, not a single callback appended to a free list. Under the IRQ critical section it reads source word `i` only if destination secondary slot `i` is empty; a null packet bypasses all six slots. `0x6c8f38` selects event 0..6, with event 6 aliasing secondary slot zero. Both reject a zero capture-count byte with -10 and request pending commit only when runtime `+0x10` is zero; occupied slots are preserved while returning zero. Initialization rejection precedes the event-range -30 rejection. Native layout is runtime base `0x9553cc`, stride `0x58`. These accepted requests and callbacks do not acknowledge physical presentation.

Additional genuine premises remain: serial device completion/error status, PWM/GPIO electrical effects, AVout `0xfff88048` request bit 28 and completion bit 24, shared timer progress and physical panel sequencing. No scripted response proves those laws. Authentic malformed recipe cells have wrapped unsigned copy lengths. The safe model classifies these as faults; the legacy ARM edge instead executes the owned original copy algorithm, including its prefix effects, without a clamp, timeout or original `0x722054` forwarding. Refusing a safe-model fault is not the same observation as the original memory-copy outcome. That distinction requires a separately admitted expected-fault case and is not stock-equivalence evidence by itself.

The renderer, font/raster assets and frame-buffer lifetime owner remain outside the driver. A CPU completion callback alone is not a newer-frame scanout acknowledgment. Until a genuine device/model acknowledgment is admitted, the publisher must retain the relevant pins rather than synthesize `FramePresented` or reuse a scanning buffer. This interface, caller migration and grounded device laws remain the S7-S9 removal conditions.

## Lower-cohort host gate checkpoint

Five retained generations preceded the corrected pass: an invalid global `GHCRTS=-N1` environment, a missing host-test `Vec` import, removal of an indexing method used only by the ARM panel profile, Haskell name-shadowing errors under `-Werror`, and one environment launch without `devenv shell`. The source repairs preserve scenario values, gate the indexing method with its actual ARM panel users and use one twelve-byte ARM service descriptor type.

The corrected fresh-phase gate passed as one invocation `e5deeefa-39a1-4616-986e-0a3c686e292e` in scope `xm1-host-build-3219573.scope`, exit 0, verdict `passed`, 57.293 s wall, 81.690 s scope CPU, 1885356032-byte memory peak under 6G RAM/2G swap and an 8589934592-byte reservation. It built `//replacement/display_scanout:libdisplay_scanout_panel`, `//replacement/display_scanout:display-scanout` and `//replacement/display_scanout:native-suite`, and passed `//replacement/display_scanout:host-tests`, `//replacement/calibration_liveview:host-tests` and `//emulation:native-runner-tests` (schema validation plus the shared native-loader tests, 51.1 s). The native suite was built, not executed.

The produced default calibration ELF has SHA256 `8d2414296bfce43ea14f46339f1cca713dcabc72cdcd8b720d1906bf1a63c4d3`. This does not inherit the historical `c7cd2102...` artifact's native qualification. Compiler and SDK paths were evaluated and checked already held before admission; no new SDK realization was authorized.

This phase used a fresh floor of `147629465600` bytes, the greater of 108 GiB and the `151924432896`-byte preflight minus 4 GiB; the finished phase's `160010330112` floor is retained in its own record. The gate added 14970880 compiler bytes to the output base (145899520 total, within 3 GiB) and left `150821203968` bytes available. Failed generations and the pass remain recorded under `private-data/scratch/LiveViewDisplayOwnership`; no deletion was performed. This checkpoint is not a whole-panel native, integrated-model or hardware result.

The lower ImageInit leaf cohort passed a second fresh-phase gate, invocation `cada31f7-3709-41db-a1c9-04d071d2dc15` in scope `xm1-host-build-3346292.scope`, exit 0, 6.046 s wall, 30.619 s scope CPU and a 1014136832-byte memory peak. It built the ARM rlib, display ELF, native suite and host rlib package and passed `//replacement/display_scanout:host-tests`; the floor was `145842987008` bytes and `150063058944` bytes remained. The host rlib package produced `libcalibration_liveview.rlib` and `libdisplay_scanout_panel.rlib` for consumers that link the crate with the pinned `@lab_xm1_rust` rustc.

The branch merged `origin/main` `249fadd2` without conflicts at `d29d4f1a`. The post-merge targeted gate used the shared floor `118111600640`, invocation `90b63eb1-6c0e-4475-b0b1-b6af6fd8c2f7` in scope `xm1-host-build-3513288.scope`, exit 0, 17.017 s wall, 36.490 s scope CPU and a 1286463488-byte memory peak. It rebuilt the ARM rlib, display staticlib and ELF, native suite and host rlib package against the merged emulation libraries and passed `//replacement/display_scanout:host-tests`. A label search found no Bazel consumer of this package outside itself; the output base was `884199424` bytes and `139169271808` bytes remained.
