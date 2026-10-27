@ X-M1 1.50 recipe boot publication. The four stock-array fragments are
@ trusted loader code. Validation, copy and publication code execute from the
@ checked source image; instruction fetch must observe the stable CRC input.
@
@ Format 2: 32-byte header {magic, format/flags, image_bytes, crc32, reserved[4]}.
@ image_bytes is exactly 0xa120. CRC-32/ISO-HDLC covers every image byte.
@ CRC detects accidental corruption, not maliciously chosen replacements.
@ Image offsets: validator [0,0x80), table [0x80,0x480), store [0x480,0x4bc),
@ copy [0x4bc,0x4dc), kernel [0x500,0x8800), data and names through 0xa120.
@
@ No payload RAM store occurs before CRC and the complete table validate.
@ Destination windows are instruction constants, not carried allowlist data.
@ Failure writes status 2 and resumes the stock image-5 entry with the launch
@ registers restored and LR=0x0069d2e4. Success leaves status unchanged.
@ Physical source lifetime, source fetch coherence, core ownership and carve
@ ownership remain placement prerequisites.
.syntax unified
.arm

.equ RB_STATUS_CELL, 0x00befffc
.equ RB_HDR_BYTES, 32
.equ RB_IMAGE_BYTES, 0xa120
.equ RB_TABLE_OFF, 0x80
.equ RB_TABLE_END, 0x480

.section .text.rb_boot_entry,"ax",%progbits
.global rb_boot_entry
.type rb_boot_entry,%function
rb_boot_entry:
    push    {r0-r12, lr}
    mov     r0, #0x47
    movw    r1, #1000
    bl      rb_plain_load_wait
    cmp     r0, #0
    bne     rb_boot_fail
    mov     r0, #0x47
    bl      rb_plain_load_release
    movw    r4, #:lower16:RB_SRC_BASE
    movt    r4, #:upper16:RB_SRC_BASE
    b       rb_boot_check
    nop
.size rb_boot_entry, .-rb_boot_entry

.section .text.rb_boot_check,"ax",%progbits
.global rb_boot_check
.type rb_boot_check,%function
rb_boot_check:
    movw    r0, #:lower16:RB_SRC_LIMIT
    movt    r0, #:upper16:RB_SRC_LIMIT
    movw    r1, #0x4834
    movt    r1, #0x0091
    ldr     r1, [r1]
    cmp     r1, r0
    blo     rb_boot_fail
    ldr     r0, [r4]
    movw    r1, #0x4a46
    movt    r1, #0x4252
    cmp     r0, r1
    bne     rb_boot_fail
    ldr     r0, [r4, #4]
    cmp     r0, #2
    bne     rb_boot_fail
    ldr     r5, [r4, #8]
    movw    r0, #RB_IMAGE_BYTES
    cmp     r5, r0
    bne     rb_boot_fail
    mvn     r6, #0
    movw    r10, #0x1a00
    movt    r10, #0x00be
    b       rb_boot_sum
    nop
.size rb_boot_check, .-rb_boot_check

.section .text.rb_boot_sum,"ax",%progbits
.global rb_boot_sum
.type rb_boot_sum,%function
rb_boot_sum:
    add     r8, r4, #RB_HDR_BYTES
    movw    r12, #0x8320
    movt    r12, #0xedb8
.Lword:
    ldr     r0, [r8], #4
    eor     r6, r6, r0
    mov     r2, #32
    b       rb_boot_publish
    nop
    nop
    nop
    nop
    nop
.size rb_boot_sum, .-rb_boot_sum

.section .text.rb_boot_publish,"ax",%progbits
.global rb_boot_publish
.type rb_boot_publish,%function
rb_boot_publish:
.Lbit:
    lsrs    r6, r6, #1
    eorcs   r6, r6, r12
    subs    r2, r2, #1
    bne     .Lbit
    subs    r5, r5, #4
    bne     .Lword
    mvn     r6, r6
    ldr     r0, [r4, #12]
    cmp     r6, r0
    bne     rb_boot_fail
    b       rb_validate_entry
.global rb_boot_fail
.type rb_boot_fail,%function
rb_boot_fail:
    movw    r0, #:lower16:RB_STATUS_CELL
    movt    r0, #:upper16:RB_STATUS_CELL
    mov     r1, #2
    str     r1, [r0]
.global rb_boot_done
.type rb_boot_done,%function
rb_boot_done:
    pop     {r0-r12, lr}
    movw    lr, #0xd2e4
    movt    lr, #0x0069
    bx      r3
    .space 20, 0
.size rb_boot_publish, .-rb_boot_publish

.section .text.rb_boot_validate,"ax",%progbits
.global rb_validate_entry
.type rb_validate_entry,%function
rb_validate_entry:
    add     r7, r4, #(RB_HDR_BYTES + RB_TABLE_OFF)
    add     r9, r4, #(RB_HDR_BYTES + RB_TABLE_END)
.Lcheck:
    cmp     r7, r9
    bhs     rb_boot_fail
    ldmia   r7!, {r0, r1}
    cmp     r0, #0
    bne     .Ldestination
    cmp     r1, #0
    bne     rb_boot_fail
    b       rb_copy_entry
.Ldestination:
    tst     r0, #3
    bne     rb_boot_fail
    movw    r3, #0xc400
    movt    r3, #0x0061
    sub     r3, r0, r3
    movw    r12, #0x2800
    movt    r12, #0x0028
    cmp     r3, r12
    blo     .Lcheck
    movw    r3, #0x1a00
    movt    r3, #0x00bf
    sub     r3, r0, r3
    movw    r12, #0x0020
    movt    r12, #0x004d
    cmp     r3, r12
    blo     .Lcheck
    sub     r3, r0, #0x01100000
    cmp     r3, #0x00080000
    blo     .Lcheck
    b       rb_boot_fail
.size rb_validate_entry, .-rb_validate_entry

.section .text.rb_boot_store,"ax",%progbits
.global rb_store_entry
.type rb_store_entry,%function
rb_store_entry:
    add     r7, r10, #RB_TABLE_OFF
1:
    ldr     r0, [r7], #4
    cmp     r0, #0
    beq     .Lcache
    ldr     r1, [r7], #4
    str     r1, [r0]
    b       1b
.Lcache:
    dsb     sy
    mov     r0, #0
    bl      rb_cache_dclean_inval
    mov     r0, #0
    bl      rb_cache_iinval
    dsb     sy
    isb     sy
    b       rb_boot_done
.size rb_store_entry, .-rb_store_entry

.section .text.rb_boot_copy,"ax",%progbits
.global rb_copy_entry
.type rb_copy_entry,%function
rb_copy_entry:
    add     r8, r4, #RB_HDR_BYTES
    mov     r9, r10
    movw    r5, #RB_IMAGE_BYTES
1:
    ldr     r0, [r8], #4
    str     r0, [r9], #4
    subs    r5, r5, #4
    bne     1b
    b       rb_store_entry
.size rb_copy_entry, .-rb_copy_entry
