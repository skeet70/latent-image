// X-M1 1.50 mode-1 guard for compact stock segment-4 staging.
.syntax unified
.arch armv7-r
.arm

.equ XM1_ROM_STACK_TOP,       0xfff00f00
.equ XM1_FAT_RETURN_OFFSET,   0x0000015c
.equ XM1_COMPACT_SOURCE,      0x00040000
.equ XM1_SEGMENT_TABLE,       0x0061c000
.equ XM1_SEGMENT4_BSS,        0x0089ec00
.equ XM1_SEGMENT4_BSS_END,    0x00bf1a00

.section .text,"ax",%progbits
.balign 4
.global xm1_seg4_image_guard
.type xm1_seg4_image_guard,%function
xm1_seg4_image_guard:
    movw r0, #:lower16:XM1_ROM_STACK_TOP
    movt r0, #:upper16:XM1_ROM_STACK_TOP
    cmp sp, r0
    bne .Lhalt
    ldr r0, [sp, #-4]
    bic r0, r0, #0x80000000
    cmp r0, #XM1_FAT_RETURN_OFFSET
    bne .Lhalt

    mov r0, #0
    ldr r1, [sp, #-4]
    bl xm1_report_stage

    movw r0, #:lower16:XM1_COMPACT_SOURCE
    movt r0, #:upper16:XM1_COMPACT_SOURCE
    movw r1, #:lower16:XM1_SEGMENT_TABLE
    movt r1, #:upper16:XM1_SEGMENT_TABLE
    movw r2, #:lower16:XM1_SEGMENT4_BSS
    movt r2, #:upper16:XM1_SEGMENT4_BSS
.Lcopy:
    ldmia r0!, {r4-r11}
    stmia r1!, {r4-r11}
    cmp r1, r2
    blo .Lcopy

    mov r4, #0
    mov r5, #0
    mov r6, #0
    mov r7, #0
    mov r8, #0
    mov r9, #0
    mov r10, #0
    mov r11, #0
    movw r2, #:lower16:XM1_SEGMENT4_BSS_END
    movt r2, #:upper16:XM1_SEGMENT4_BSS_END
.Lzero_bss:
    stmia r1!, {r4-r11}
    cmp r1, r2
    blo .Lzero_bss

    movw r0, #:lower16:XM1_SEGMENT_TABLE
    movt r0, #:upper16:XM1_SEGMENT_TABLE
.Lclean:
    mcr p15, 0, r0, c7, c10, 1
    add r0, r0, #32
    cmp r0, r2
    blo .Lclean
    dsb sy
    mov r0, #0
    mcr p15, 0, r0, c7, c5, 0
    dsb sy
    isb sy

    mov r0, #0
    msr APSR_nzcvq, r0
    movw r12, #0x1800
    bx r12
.Lhalt:
    wfi
    b .Lhalt
.size xm1_seg4_image_guard, .-xm1_seg4_image_guard
