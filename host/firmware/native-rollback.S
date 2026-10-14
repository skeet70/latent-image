// X-M1 1.50: decode/boot-40048.dis and decode/ghidra/boot-40048.c.
// Primary BSS is [0x4300,0x5280); the companion record starts at 0x10000.
// Native ids 0/1 load at 0x40000/0xd0000; id 4 loads at 0x61c400.
// Entry replaces the primary vector literal at 0x20, formerly 0x1800.
// Rollback replaces the call at 0x1fdc, after 0x1d08 -> 0x2250 succeeds.
// The post-close hook halts before the stock peer release at 0x1ff8.
// Lifetime is pre-kernel only. Cache maintenance affects this processor only.

.syntax unified
.arch armv7-r
.arm
.section .text,"ax",%progbits
.balign 4

.equ XM1_ROM_STACK_TOP,     0xfff00f00
.equ XM1_FAT_RETURN_OFFSET, 0x0000015c
.equ XM1_MEDIA_RECEIVE_TAP, 0x0071feb4
.equ XM1_RECORDER_TAP_WORD, 0xeb34a9c9
.equ XM1_STOCK_TAP_WORD,    0xebffffdc

.global native_rollback_guard
.type native_rollback_guard,%function
// ROM loader pop-to-PC leaves the caller return word at SP minus four.
native_rollback_guard:
    movw r0, #:lower16:XM1_ROM_STACK_TOP
    movt r0, #:upper16:XM1_ROM_STACK_TOP
    cmp sp, r0
    bne .Lguard_halt
    ldr r0, [sp, #-4]
    bic r0, r0, #0x80000000
    cmp r0, #XM1_FAT_RETURN_OFFSET
    bne .Lguard_halt
    mov r0, #0
    ldr r1, [sp, #-4]
    bl xm1_report_stage
    mov r0, #0
    msr APSR_nzcvq, r0
    b xm1_native_reset
.Lguard_halt:
    wfi
    b .Lguard_halt
.size native_rollback_guard, .-native_rollback_guard

.org 0x48, 0
.global native_rollback_stub
.type native_rollback_stub,%function
native_rollback_stub:
    // Native 0x1e4c supplies an eight-byte-aligned supervisor stack.
    push {r0-r3, r12, lr}
    bl xm1_primary_discriminator
    cmp r0, #0
    bne .Lrollback_halt
    mov r0, #1
    mov r1, #0
    bl xm1_report_stage

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
    mov r0, #2
    bl xm1_report_stage

    pop {r0-r3, r12, lr}
    b xm1_native_close
.Lrollback_halt:
    wfi
    b .Lrollback_halt
.size native_rollback_stub, .-native_rollback_stub

.org 0xd0, 0
.global native_post_close
.type native_post_close,%function
native_post_close:
    mov r0, #3
    mov r1, #0
    bl xm1_report_stage
.Lpost_close_halt:
    wfi
    b .Lpost_close_halt
.size native_post_close, .-native_post_close
