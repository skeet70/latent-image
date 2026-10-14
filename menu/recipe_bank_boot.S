@ The recipe bank's boot copy, publication-hardened edition: validate the
@ carried package, relocate it into the carved heap span, validate the
@ bank's publication destinations, then hand image 5 its stock entry.
@ This is the only bank code that runs before the entry and the only bank
@ code outside the package: it lives in the four stock array runs the
@ D3/Q retargets free, except the store pass, which is image code.
@
@ Attachment: image 4's entry launcher at 0x0069d2b4 loads image 5's entry
@ from the stage header word at 0x00bf1a0c and reaches it through the
@ `blx r3` at 0x0069d2e0 (stock word 0xe12fff33). The container replaces
@ that one word with `b rb_boot_entry`.
@
@ Entry register contract, identical to the replaced `blx r3` on exit:
@ r0-r3, r12 and r4-r11 hold their launcher values, LR becomes 0x0069d2e4
@ (immediate halves 0xd2e4 and 0x0069), and the jump target is r3.
@
@ Failure contract: any failed gate or check stores 2 into the status cell
@ and takes the same stock branch; the failure path does not run the cache
@ pass. No store happens before the hash matches; pass one checks every
@ destination against the allowlist with no stores, and the store pass
@ runs only after the whole table validated. Success leaves the cell
@ UNCHANGED (the first budget trade; a warm-boot stale 2 reads as a fresh
@ refusal and diagnostics corroborate against the bank's presence).
@
@ Trust boundary (chosen byte source (a), pending the review's ruling):
@ the store pass is image content, executed from the source copy of the
@ hashed image at RB_STORE_ENTRY after the hash has matched. The trusted
@ stub keeps the gates, the hash and the destination validation; the
@ executed bytes' integrity is the matched hash, and the stub never writes
@ the source span, so no D-cache clean is owed for them. The source span
@ is never fetched as code before this jump. The store pass's own bulk
@ cache pass covers the carve copy (the bank's runtime, fetched after the
@ entry) and the patched stock words.
@
@ Package layout at RB_SRC_BASE:
@
@   header, 32 bytes little-endian:
@     +0x00 u32 magic        0x42524a46
@     +0x04 u16 format       1 (the stub reads only the magic word; the
@                               builder refuses any other format)
@     +0x06 u16 flags        0
@     +0x08 u32 image_bytes  >= 0xa018 (name-table end), 4-aligned,
@                              < 0xe600 (effective max 0xe5fc)
@     +0x0c u32 hash         32-bit rotating word hash of the image
@     +0x10 u32 reserved     0 (unread)
@     +0x14 u32 reserved     0
@     +0x18 u32 reserved     0
@     +0x1c u32 reserved     0
@   image at +0x20:
@     image offset 0x000  allowlist: 16 slots of {u32 lo, u32 span}; a
@                        destination is allowed when lo <= addr < lo+span
@                        and the sum does not wrap; unused slots {0,0}
@     image offset 0x080  publication table: 64 slots of {u32 addr, u32
@                        value}; {0,0} ends the walk; unused slots {0,0};
@                        code words before data words at the builder's
@                        obligation
@     image offset 0x280  store pass: 15 instructions, 60 B, emitted by
@                        the builder from this file's .text.rb_boot_store
@                        and executed at RB_STORE_ENTRY
@     image offset 0x9d84 name table: 10 records of 66 bytes (runtime
@                        0x00beb784, the stock record base's low half
@                        0xb784, so each record-base update is one atomic
@                        MOVT store)
@   The metadata region [0x000, 0x280) and the name table end 0xa018 must
@   lie inside image_bytes; the coverage gate enforces both.
@
@ Validation order: gate 1 (no source read while regions 5-7 protect an
@ unloaded span), magic, image_bytes (4-aligned, >= 0xa018, < 0xe600),
@ gate 2 (stage-relative bytes copied), copy and hash, hash compare,
@ pass-one walk bound and allowlist, store pass, bulk cache pass, stock
@ exit. A package shorter than the coverage minimum cannot reach the
@ table: the walk reads only the carve copy of a copied, hashed image.
@ Address 0 is never a store target: it ends the table walk and lies in no
@ window (an unused {0,0} slot has span 0).
@
@ Hash: h = 0; for each image word in order, h = ror32(h + word, 7).
@ Each step is a bijection in the accumulator, so a single-word change
@ always changes h, and permutation invariance (addition commutes) is
@ gone: order enters through the rotation. Deterministic compensating
@ collisions exist: two words a multiple of 32 positions apart whose
@ value changes sum to zero cancel exactly while the propagation stays
@ carry-free, because 7 is invertible mod 32 and both contributions
@ return to the same bit. The reachable case is a zero-filled stretch
@ (unused slots, zero fill) with two words raised to 0x80000000; outside
@ carry-free stretches the carries perturb the pair (measured about one
@ in six random contexts). Residual: that structured class plus the
@ map's 32-bit collision class (a modeled random corruption passes with
@ probability 2^-32). The header words sit outside the hash, so the
@ magic, coverage and length gates stand on their own. The class does
@ not reopen store-at-zero: address 0 terminates both walks before any
@ window check or store.
@
@ Cache contract: the span is cacheable write-back through MPU region 0.
@ All copy and store-pass stores complete first; then one bulk pass
@ cleans and invalidates the D-cache (whole-cache set/way fallback of the
@ image-4 helper 0x0061c4f4) and invalidates the I-cache (0x0061c4ec),
@ bracketed by DSB/ISB, so the bank's carve code and the patched stock
@ words are coherent for the execution that follows. One zero argument
@ selects the whole-cache fallback: at 0x61c50c..0x61c51c the helper sets
@ r3 = (r0==0) | (r1==0) and branches when r3 == 1, so zeroing r0
@ suffices. The two calls are made through absolute symbols
@ (rb_cache_dclean_inval / rb_cache_iinval) assigned in the .ld. CP15
@ cache maintenance is core-local: the sequence is coherent for the core
@ that runs this hook, and the core question is an open prerequisite of
@ the contract. The source span is not written by the stub; source
@ coherence (the source's memory content equals the bytes the copy reads)
@ remains an open prerequisite and is load-bearing for the store pass's
@ own fetch.

.syntax unified
.arm

@ Stock cache helpers. The two symbols are absolute assignments in
@ menu/recipe_bank_boot.ld, not here: a numeric `bl` operand and a
@ same-unit `.set` symbol both assemble as a raw imm24 field and the call
@ lands at pc+8+operand (verified: both forms link to 0xeb18713d), while a
@ linker-script absolute symbol makes lld resolve the call pc-relatively
@ (the delivered link's first call, at 0x00789e94, is 0xebfa4996). A link
@ without the script fails on the undefined symbols instead of
@ mis-encoding them.

.equ RB_STATUS_CELL,   0x00befffc   @ top word of the bank span, reserved
.equ RB_HDR_BYTES,     32
.equ RB_IMAGE_MIN,     0xa018       @ name table 0x9d84 + 10*66 bytes
.equ RB_IMAGE_LIMIT,   0xe600       @ first refused length; 4-alignment
                                    @ makes the effective maximum 0xe5fc
.equ RB_ALLOW_BYTES,   0x80         @ 16 allowlist slots
.equ RB_TABLE_OFF,     0x80         @ publication table in the image
.equ RB_TABLE_SLOTS,   64           @ table slots, in the image region
.equ RB_TABLE_LAST,    0x278        @ last table slot start (region end
                                    @ 0x280, inside image_bytes by the
                                    @ coverage gate)

@ ------------------------------------------------- entry, gate 1, header ptr
@ Runs from the film-values run [0x0078319c, 0x007831cc). 12 instructions,
@ 48 B: the run's full capacity. Gate 1 reads the plain-load MPU protect
@ regions: while any of regions 5-7 is enabled the load machinery still
@ protects an unloaded span and the source must not be read.

.section .text.rb_boot_entry,"ax",%progbits
.global rb_boot_entry
.type rb_boot_entry,%function
rb_boot_entry:
    push    {r0-r12, lr}
    mov     r4, #5
1:
    mcr     p15, 0, r4, c6, c2, 0       @ select MPU region r4
    mrc     p15, 0, r0, c6, c1, 2       @ read its DRSR
    tst     r0, #1                      @ enable bit
    bne     rb_boot_fail
    add     r4, r4, #1
    cmp     r4, #8
    blo     1b
    movw    r4, #:lower16:RB_SRC_BASE
    movt    r4, #:upper16:RB_SRC_BASE   @ r4 = package header
    b       rb_boot_check
.size rb_boot_entry, .-rb_boot_entry

@ --------------------- validation, covered length, gate 2, carve base
@ Runs from the film-labels run [0x0078d4f8, 0x0078d558). 24 instructions,
@ 96 B: the run's full capacity. The coverage gate keeps the metadata
@ region and the name table inside image_bytes, so the hash covers both
@ tables and the walk can never read bytes the copy did not write.

.section .text.rb_boot_check,"ax",%progbits
.global rb_boot_check
.type rb_boot_check,%function
rb_boot_check:
    ldr     r0, [r4]                    @ magic
    movw    r1, #0x4a46
    movt    r1, #0x4252
    cmp     r0, r1
    bne     rb_boot_fail
    ldr     r5, [r4, #8]                @ image bytes
    tst     r5, #3
    bne     rb_boot_fail
    movw    r0, #:lower16:RB_IMAGE_MIN  @ covered length: name-table end
    cmp     r5, r0
    blo     rb_boot_fail
    cmp     r5, #RB_IMAGE_LIMIT
    bhs     rb_boot_fail
    movw    r0, #:lower16:RB_SRC_LIMIT  @ gate 2: bytes-copied threshold
    movt    r0, #:upper16:RB_SRC_LIMIT
    movw    r1, #0x4834
    movt    r1, #0x0091                 @ reporter high-water 0x00914834
    ldr     r1, [r1]
    cmp     r1, r0
    blo     rb_boot_fail
    mov     r6, #0                      @ hash accumulator
    movw    r10, #0x1a00
    movt    r10, #0x00be                @ carve base 0x00be1a00
    b       rb_boot_sum
.size rb_boot_check, .-rb_boot_check

@ -------------------------------------- copy and hash, one pass
@ Runs from the Q-values run [0x00782348, 0x00782378). 12 instructions,
@ 48 B: the run's full capacity. r5 counts down from image_bytes
@ (multiple of 4, >= 0xa018); r9 is the copy cursor so r10 keeps the carve
@ base for the walk. The hash is complete and compared before the
@ publication walk begins.

.section .text.rb_boot_sum,"ax",%progbits
.global rb_boot_sum
.type rb_boot_sum,%function
rb_boot_sum:
    add     r8, r4, #RB_HDR_BYTES       @ image source
    mov     r9, r10                     @ copy destination cursor
1:
    ldr     r0, [r8], #4
    add     r6, r6, r0                  @ h += word
    ror     r6, r6, #7                  @ h = ror32(h, 7)
    str     r0, [r9], #4
    subs    r5, r5, #4
    bne     1b
    ldr     r0, [r4, #0x0c]             @ header hash
    cmp     r6, r0
    bne     rb_boot_fail
    b       rb_boot_publish
.size rb_boot_sum, .-rb_boot_sum

@ ----------- destination validation walk, status, stock exit
@ Runs from the Q-labels run [0x00789e2c, 0x00789e8c). 24 instructions,
@ 96 B: the run's full capacity. Pass one bounds the walk and checks every
@ destination against the allowlist with no stores; when the whole table
@ has validated it branches to the image-resident store pass. Each store
@ in that pass therefore follows a validated address.

.section .text.rb_boot_publish,"ax",%progbits
.global rb_boot_publish
.type rb_boot_publish,%function
rb_boot_publish:
    add     r7, r10, #RB_TABLE_OFF      @ pass-one cursor
    add     r9, r10, #RB_TABLE_LAST     @ last table slot
    add     r11, r10, #RB_ALLOW_BYTES   @ allowlist region end
.Lcheck:
    cmp     r7, r9
    bhi     rb_boot_fail                @ no terminator inside the region
    ldr     r0, [r7], #8                @ addr; skip the value in pass one
    cmp     r0, #0
    beq     rb_store_entry              @ table validated: image-resident store pass
    mov     r8, r10                     @ allowlist cursor
.Lallow:
    cmp     r8, r11
    bhs     rb_boot_fail                @ no window admits the address
    ldmia   r8!, {r3, r12}              @ window lo, window span
    sub     r3, r0, r3
    cmp     r3, r12                     @ lo <= addr < lo + span
    bhs     .Lallow
    b       .Lcheck
.global rb_boot_fail
.type rb_boot_fail,%function
rb_boot_fail:
    movw    r0, #:lower16:RB_STATUS_CELL
    movt    r0, #:upper16:RB_STATUS_CELL
    mov     r1, #2                      @ RB_ST_REFUSED
    str     r1, [r0]
.global rb_boot_done
.type rb_boot_done,%function
rb_boot_done:
    pop     {r0-r12, lr}
    movw    lr, #0xd2e4
    movt    lr, #0x0069                 @ LR = 0x0069d2e4, the stock return
    bx      r3
.size rb_boot_publish, .-rb_boot_publish

@ ---------------- store pass and bulk cache pass (image code, hash-covered)
@ Placed at RB_STORE_ENTRY = RB_SRC_BASE + RB_HDR_BYTES + boot.store_off
@ so its pc-relative branches and helper calls are encoded for the address
@ it executes at; the builder emits these bytes into the image at
@ boot.store_off. 15 instructions, 60 B. Executed only after the hash
@ matched and pass one validated every destination. Reads the table from
@ the carve copy (r10), stores to the validated addresses, then runs the
@ bulk cache pass that covers the carve and the patched stock words.

.section .text.rb_boot_store,"ax",%progbits
.global rb_store_entry
.type rb_store_entry,%function
rb_store_entry:
    add     r7, r10, #RB_TABLE_OFF      @ pass-two cursor
1:
    ldr     r0, [r7], #4                @ addr (validated in pass one)
    cmp     r0, #0
    beq     .Lcache
    ldr     r1, [r7], #4                @ value
    str     r1, [r0]
    b       1b
.Lcache:
    dsb     sy
    mov     r0, #0
    bl      rb_cache_dclean_inval       @ whole-cache D clean + invalidate
    mov     r0, #0
    bl      rb_cache_iinval             @ whole-cache I invalidate
    dsb     sy
    isb     sy
    b       rb_boot_done
.size rb_boot_store, .-rb_boot_store
