# RTOS interrupt and cache services: contract and fixed-bank form

Part of [#156](https://github.com/skeet70/latent-image/issues/156). Sources: x70view `55436fc85c0560dd252b0410af6f2427fe198cd3`, `decomp/firmwares/xm1-150/kernel0/src/` (`irq/intc.c`, `irq/irq_enter.c`, `irq/irq_exit.c`, `irq/irq_initializer.c`, `irq/irq_registration.c`, `irq/irq_controller_enable.c` (NONMATCHING), `irq/irq_controller_disable.c`, `cpu/cache_maintenance.c`, `cpu/interrupt_mask.c`) and `symbols/kernel0.txt`, and the pinned loaders `loader-40000` (A) and `loader-d0000` (B). Every address, constant and caller below was read from the loader disassembly of both domains (`private-data/analysis/x-m1-1.50/decode/loader-{40000,d0000}.dis`); where a decomp name disagrees with the bytes, the bytes are used. The root-slot services `+0x158/+0x15c/+0x160` are already owned in stage form by [`rtos_irq_control`](../replacement/rtos_irq_control/irq-control-contract.json); the acknowledge and end-of-interrupt decisions are owned in place by [`rtos_exception`](rtos-exception-entry-exit.md). This report fixes the family boundary and its fixed-bank form.

## Extents

| Extent | A | B | Note |
| --- | --- | --- | --- |
| mask, cache, identity | `[0x42560,0x427d4)` | `[0xd2560,0xd27d4)` | byte-identical between loaders |
| controller | `[0x42854,0x434f0)` | `[0xd2854,0xd34f0)` | differs only in `movw`/`movt` immediates (51 words) |
| slot thunks `+0x158/+0x15c` | `[0x41108,0x41140)` | `[0xd1108,0xd1140)` | byte-identical |
| define `+0x160` | `[0x4d8fc,0x4d978)` | `[0xdd8c0,0xdd93c)` | |

Outside the family: the global-lock helpers `0x427d4`, `0x427fc`, `0x42828` and the literal words `0x42830 = 0x3ff`, `0x42834 = 0x7fff` (read by the stock set/way loop), the empty handler `0x42840` / `0xd2840` (a published marker value stored in the table and the controller and compared by enable and enter), `kernel_zero_words` `0x434f0`, and the vector bodies `0x40100..0x40368`, which `rtos_exception` owns in its carrier and the native runner cannot execute (banked and mode-changing status writes).

## Services

Callers are direct `bl` in the loader of the same domain (148 call sites per domain into the two extents, the same set in A and B) or image4 calls into loader A counted by translation units in the x70view decomp (`app/`).

| Service | A | B | Entered by |
| --- | --- | --- | --- |
| save I+F, `FUN_00042560` | `0x42560` | `0xd2560` | 15 sites: define `0x4d910` and the object-configuration services `0x4d0a4..0x4d8b4` |
| restore I+F, `FUN_00042568` | `0x42568` | `0xd2568` | the 15 matching sites |
| `kernel_save_irq_mask` | `0x42574` | `0xd2574` | 37 sites (device setup `0x40e00..0x410c8`, `0x41260`, configure, enable, disable, `0x493d8`, `0x495f8`); image4 13 TUs |
| `kernel_restore_irq_mask` | `0x4257c` | `0xd257c` | the 37 matching sites; image4 13 TUs |
| `kernel_save_fiq_mask`, `kernel_restore_fiq_mask` | `0x42588`, `0x42590` | `0xd2588`, `0xd2590` | no caller found |
| `icache_enable`, `dcache_enable` | `0x425c8`, `0x425d0` | `0xd25c8`, `0xd25d0` | image4 `drivers/display/tu_00ec797c.c` |
| `icache_disable`, `dcache_disable` | `0x425d8`, `0x425e0` | `0xd25d8`, `0xd25e0` | the same TU |
| `icache_invalidate_range` | `0x4262c` | `0xd262c` | no caller found |
| `dcache_clean_invalidate_range` | `0x42634` | `0xd2634` | image4 38 TUs, including the phase-0 installer `0x725a3c` (`0x42634(0x96bd20, 32)`) and the image5 producer `0x6a9828` (`0x42634(0, 0)`) |
| `dcache_clean_range` | `0x4263c` | `0xd263c` | image4 1 TU |
| `dcache_invalidate_range` | `0x42644` | `0xd2644` | image4 6 TUs |
| `kernel_core_index` (CP14 identity) | `0x427b8` | `0xd27b8` | 9 sites: loader install `0x40024`, vector lock `0x40268`/`0x402a0`, `kernel_start` `0x409b8`, `0x40b3c`, `0x40ba4`, `0x412c4`, `0x413c8`, core index |
| core index, `FUN_00042854` | `0x42854` | `0xd2854` | initialize, enable, disable, enter, leave, clear pending |
| `kernel_initialize_irqs` | `0x4286c` | `0xd286c` | `0x43a44` (from `kernel_start` through `0x43a3c`) |
| `kernel_configure_irq(irq, desc)` | `0x429ac` | `0xd29ac` | `kernel_start` IRQ-120 registration `0x40dd0`, reset, define |
| `kernel_reset_irq(irq)` | `0x42a98` | `0xd2a98` | define (NULL packet) |
| handler lookup `(irq)` | `0x42adc` | `0xd2adc` | no caller found |
| `kernel_enable_irq(irq)` | `0x42b00` | `0xd2b00` | `0x40dd8` (IRQ 120), `+0x158` thunk |
| `kernel_disable_irq(irq)` | `0x42eb0` | `0xd2eb0` | `+0x15c` thunk |
| `kernel_enter_irq` | `0x430a8` | `0xd30a8` | acknowledge wrapper `0x40d58` |
| `kernel_leave_irq` | `0x431d8` | `0xd31d8` | end-of-interrupt wrapper `0x40d68` |
| enable-set `(core, irq)` | `0x432f8` | `0xd32f8` | no caller found |
| clear `(core, irq)` | `0x43358` | `0xd3358` | no caller found |
| `kernel_raise_irq(core, irq)` | `0x433e4` | `0xd33e4` | inter-core notify `0x44030` (IRQ 120); image4 1 TU |
| clear pending `(irq)`, decomp `FUN_000433f4` | `0x43444` | `0xd3444` | interrupt-120 callback `0x40b0c` |
| active-slot snapshot `(buffer)` | `0x434a4` | `0xd34a4` | no caller found |
| `+0x158` thunk, `+0x15c` thunk | `0x41108`, `0x41124` | `0xd1108`, `0xd1124` | root slots |
| `kernel_define_irq`, `+0x160` | `0x4d8fc` | `0xdd8c0` | root slot |

`0x433f4` is inside `kernel_raise_irq`, not an entry; the decomp's `FUN_000433f4` is the clear-pending service at `0x43444`. "No caller found" covers the loader listings and the x70view decomp; a code pointer from an unexamined image would not be found, so those entries stay in the family rather than being filled.

## Records and raw consumers

Per domain: registration table A `0x400538fc` / B `0x400e38bc` (128 entries of `{handler, +0x4, priority, +0xc, mode, +0x14}`), active slots A `0x40054500` / B `0x400e44c0` (16 pairs `{controller +0x1c0 word, irq}`), unhandled counter A `0x400544fc` / B `0x400e44bc`, IRQ nesting counter A `0x400545a4` / B `0x400e4564` (read by leave, written by the vector). The bank is selected by the executing core (CP14 `c2,c0,0` equal to `0x4003` selects `0xfffb3000`, otherwise `0xfffb4000`), except raise, enable-set and clear, which take the core as an argument.

Controller words (byte offsets from the bank): `+0x80` group enable, `+0xa0` group clear, `+0xc0` set pending (raise), `+0xe0` clear pending, `+0x60`/`+0x100` group configuration, `+0x120` written with the line bit by enable, disable and leave, `+0x140`/`+0x180` two-bit fields, `+0x1c0`/`+0x1c4` priority masks, `+0x200` acknowledged vector, `+0x230..+0x23c` active bitmap, `+0x400 + 4*irq` vector, `+0x800 + 4*irq` priority. The names are the stock code's use of each word, not a controller model.

- Initialize writes every table entry to `{marker, 0, 15, 0, 1, 0}`, every active slot to `{0, -1}`, then the bank: `+0x1c0 = 0xffff`, then for each irq `+0x400+4*irq = marker` followed by `+0x800+4*irq = 15`, then `+0x1c4 = 0xffff`, then the unhandled counter to 0.
- Enter reads `+0x200`; zero or the marker increments the unhandled counter and calls the retained log `0x41668` / `0xd1668` with `(6, count)` and returns 0. Otherwise it scans `+0x230..+0x23c` for the first nonzero group and its highest bit, sets TPIDRURW bit 31, and stores `{+0x1c0, irq}` at the active slot of the line's registered priority; it returns the `+0x200` word.
- Leave re-derives the line from the active bitmap, writes its bit to `+0x120 + 4*group`, clears TPIDRURW bit 31 only when the nesting counter is 1, clears the slot to `{0, -1}`, and always writes 0 to `+0x200`.
- Enable, disable, configure, reset and define are as in the `rtos_irq_control` contract; configure, enable and disable take the signed `cmp r0, #127; bgt` bound.
- Raise, enable-set, clear and clear pending compute `irq / 32` and `irq % 32` with signed division; each writes only the stated words.

The mask helpers return `cpsr & mask` and restore by writing only the masked bits back with `msr CPSR_c`. Interrupt state is the only shared record they touch.

## Cache maintenance

The stock helpers issue CP15 operations with no memory access:

- Enable (`r0` = `0x1000` I or `0x4` D): read SCTLR, OR the bit, DSB SY, then `tst r0, #0x1000; mcreq` `ICIALLU` and `tst r0, #4; mcreq` `p15,0,c15,c5,0`, both with operand 0, write SCTLR, ISB SY. The conditions are `eq`, so enabling the I-cache invalidates all data and enabling the D-cache issues `ICIALLU`; the decomp comment states the opposite pairing. Disable: read SCTLR, clear the bit, DSB SY, write SCTLR, ISB SY.
- Range `(start, length)`: a zero start or zero length selects the whole-cache path. Otherwise end = start + length, start is rounded down to 32 bytes, and one operation per 32-byte line is issued while `line < end` under a signed compare (`blt`), at least once. Operation 1 is `DCCIMVAC` (`c7,c14,1`), 2 `DCCMVAC` (`c7,c10,1`), 3 `DCIMVAC` (`c7,c6,1`), 4 `ICIMVAU` (`c7,c5,1`). No barrier is issued.
- Whole cache: operation 3 issues `p15,0,c15,c5,0` with operand 0, operation 4 `ICIALLU` with operand 0. Operations 1 and 2 walk CLIDR's levels below the level of coherency: for each level whose CLIDR type is 2 or more, write CSSELR = `level << 1`, ISB SY, read CCSIDR, then for each set descending and, within it, each way descending, issue `DCCISW` (`c7,c14,2`) or `DCCSW` (`c7,c10,2`) with `level << 1 | way << clz(ways - 1) | set << ((CCSIDR & 7) + 4)`. With one way, `clz(0) = 32` and the register shift contributes 0. No final barrier.

## Declared differences

- Negative line in `kernel_enable_irq`, `kernel_disable_irq` and their slots: stock passes the signed bound and writes below the table and the bank; the owned bodies return -1 (slots -5) with no access. This is the `valid-irq-number` correction of `rtos_irq_control`. Configure, reset and the handler lookup keep stock's unchecked index.
- Enter and leave index the active slots by the registered priority `& 15`, through `rtos_exception`'s decisions; stock uses the raw word. Only configure, reset, define and initialize write the table, and each keeps the priority at 15 or below.
- Mask restore sets and clears I and F with at most two `cps` instructions, masking before unmasking, instead of one `msr CPSR_c`. The final CPSR is equal; the intermediate state differs only when I and F move in opposite directions.
- Reset and define do not build the stock descriptor on the stack, and every body has its own frame. Stack bytes are not part of the contract.
- The set/way loop takes its masks from the instruction stream, not from `0x42830`/`0x42834`.
- The unhandled path of enter calls the retained log at `0x41668` / `0xd1668` by absolute address.

## Fixed-bank form

`replacement/rtos_irq_control/stage/abi.rs` with feature `bank` exports every service as `xm1_rtos_<name>_{a,b}`: `mask_{save,restore}_{irq_fiq,irq,fiq}`, `cache_{enable,disable}_{instruction,data}`, `cache_invalidate_instruction_range`, `cache_clean_invalidate_data_range`, `cache_clean_data_range`, `cache_invalidate_data_range`, `irq_identity`, `irq_core_index`, `irq_initialize`, `irq_configure`, `irq_reset`, `irq_handler`, `irq_enable`, `irq_disable`, `irq_enter`, `irq_leave`, `irq_set_enable`, `irq_clear`, `irq_raise`, `irq_clear_pending`, `irq_copy_active`, `irq_slot_enable`, `irq_slot_disable` and `irq_define`. The semantics live in `src/mask.rs`, `src/cache.rs`, `src/controller.rs` and the existing `src/lib.rs` (enable, disable, define); enter and leave are `rtos_exception`'s `acknowledge` and `end_of_interrupt` behind its `Machine` trait. Bank builds set and clear I and F only with `cps`. `//replacement/rtos_irq_control:irq-bank-{a,b}` are the rlibs a bank root links. The in-place `irq-a`/`irq-b` stage is unchanged.

In a bank image each stock entry keeps a four-byte `b` veneer to its domain's bank body and the rest of each extent is `udf` fill; `0x42840` and the lock helpers stay stock. `//replacement/rtos_irq_control:irq-bank-probe-{a,b}` link the bodies at the fixed body origin `0x00ba0020` behind those veneers, and `-split` extracts one carrier binary per extent (`-slots`, `-mask-cache`, `-controller`, `-define`, each starting at its extent base) and `-bank.bin`. The bank calls the retained log `0x41668` / `0xd1668` by absolute address. `rtos_exception`'s in-place stage also patches `0x430a8..0x432f8`; a bank image takes one of the two, not both.

## Native comparison

NATIVE

## Open

- Bank placement and the from-reset pair need joint bank/loader admission, which is not on main.
- The vector bodies stay in `rtos_exception`'s carrier; the native runner refuses their banked and mode-changing status writes.
- Device regions in the native runner are memory-backed: equal reads need equal prior writes, so the cohort compares access order and values, not controller semantics.
- Cache maintenance is recorded, not modeled; CLIDR and CCSIDR are case inputs, so the cohort qualifies the operation sequence for the declared geometries, not X-M1's cache geometry.
