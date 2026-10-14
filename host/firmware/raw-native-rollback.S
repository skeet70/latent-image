// X-M1 1.50 primary BSS ends at 0x5280; companion boot starts at 0x10000.
.syntax unified
.arch armv7-r
.arm
.section .text,"ax",%progbits
.balign 4

.equ XM1_ROM_STACK_TOP,     0xfff00f00
.equ XM1_RAW_RETURN_OFFSET, 0x00000174
.equ XM1_MEDIA_RECEIVE_TAP, 0x0071feb4
.equ XM1_RECORDER_TAP_WORD, 0xeb34a9c9
.equ XM1_STOCK_TAP_WORD,    0xebffffdc

.global raw_native_guard
.type raw_native_guard,%function
// Successful ROM loader pop-to-PC leaves its caller return at SP minus four.
raw_native_guard:
    movw r0, #:lower16:XM1_ROM_STACK_TOP
    movt r0, #:upper16:XM1_ROM_STACK_TOP
    cmp sp, r0
    bne .Lguard_halt
    ldr r0, [sp, #-4]
    bic r0, r0, #0x80000000
    cmp r0, #XM1_RAW_RETURN_OFFSET
    bne .Lguard_halt
    mov r0, #0
    msr APSR_nzcvq, r0
    b xm1_native_reset
.Lguard_halt:
    wfi
    b .Lguard_halt
.size raw_native_guard, .-raw_native_guard

.org 0x40, 0
.global raw_native_rollback
.type raw_native_rollback,%function
raw_native_rollback:
    push {r0-r3, r12, lr}
    bl xm1_primary_discriminator
    cmp r0, #0
    bne .Lrollback_halt
    movw r0, #:lower16:XM1_MEDIA_RECEIVE_TAP
    movt r0, #:upper16:XM1_MEDIA_RECEIVE_TAP
    ldr r1, [r0]
    movw r2, #:lower16:XM1_STOCK_TAP_WORD
    movt r2, #:upper16:XM1_STOCK_TAP_WORD
    cmp r1, r2
    beq .Lcohere
    movw r3, #:lower16:XM1_RECORDER_TAP_WORD
    movt r3, #:upper16:XM1_RECORDER_TAP_WORD
    cmp r1, r3
    bne .Lrollback_halt
    str r2, [r0]
.Lcohere:
    mcr p15, 0, r0, c7, c10, 1
    dsb sy
    mov r0, #0
    mcr p15, 0, r0, c7, c5, 0
    dsb sy
    isb sy
    pop {r0-r3, r12, lr}
    b xm1_native_close
.Lrollback_halt:
    wfi
    b .Lrollback_halt
.size raw_native_rollback, .-raw_native_rollback
