// X-M1 1.50 mode-1 entry guard for a preloaded RAM image.
.syntax unified
.arch armv7-r
.arm

.equ XM1_ROM_STACK_TOP,     0xfff00f00
.equ XM1_FAT_RETURN_OFFSET, 0x0000015c

.section .text,"ax",%progbits
.balign 4
.global xm1_full_image_guard
.type xm1_full_image_guard,%function
xm1_full_image_guard:
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
    mov r0, #0
    msr APSR_nzcvq, r0
    movw r12, #0x1800
    bx r12
.Lhalt:
    wfi
    b .Lhalt
.size xm1_full_image_guard, .-xm1_full_image_guard
