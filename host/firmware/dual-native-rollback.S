// X-M1 1.50 ROM mode 1 returns through 0x15c; mode 2 returns through 0x174.
.syntax unified
.arch armv7-r
.arm
.section .text,"ax",%progbits
.balign 4

.equ XM1_ROM_STACK_TOP,     0xfff00f00
.equ XM1_FAT_RETURN_OFFSET, 0x0000015c
.equ XM1_RAW_RETURN_OFFSET, 0x00000174
.equ XM1_MEDIA_RECEIVE_TAP, 0x0071feb4
.equ XM1_RECORDER_TAP_WORD, 0xeb34a9c9
.equ XM1_STOCK_TAP_WORD,    0xebffffdc

.global dual_native_guard
.type dual_native_guard,%function
dual_native_guard:
    movw r0, #:lower16:XM1_ROM_STACK_TOP
    movt r0, #:upper16:XM1_ROM_STACK_TOP
    cmp sp, r0
    bne .Lguard_halt
    ldr r0, [sp, #-4]
    bic r0, r0, #0x80000000
    cmp r0, #XM1_FAT_RETURN_OFFSET
    beq .Lmark_fat
    cmp r0, #XM1_RAW_RETURN_OFFSET
    bne .Lguard_halt
    push {r0-r8, r12, lr}
    mov r8, #2
    b .Lmark_route
.Lmark_fat:
    push {r0-r8, r12, lr}
    mov r8, #1
.Lmark_route:
    movw r6, #0x8084
    movt r6, #0xfff8
    mov r7, #1
    str r7, [r6]
    dsb sy
    movw r0, #0x93c0
    movt r0, #0xfff8
    movw r5, #0x0001
    movt r5, #0x0005
    str r5, [r0, #4]
    dsb sy
    movw r1, #0x0001
    movt r1, #0x0001
    movw r2, #0x0000
    movt r2, #0x0001
.Lroute_pulse:
    str r1, [r0]
    dsb sy
    mov r4, #0x01000000
.Lroute_on_delay:
    subs r4, r4, #1
    bne .Lroute_on_delay
    str r2, [r0]
    dsb sy
    mov r4, #0x01000000
.Lroute_off_delay:
    subs r4, r4, #1
    bne .Lroute_off_delay
    subs r8, r8, #1
    bne .Lroute_pulse
    pop {r0-r8, r12, lr}
    mov r0, #0
    msr APSR_nzcvq, r0
    b xm1_native_reset

.Lguard_halt:
    wfi
    b .Lguard_halt
.size dual_native_guard, .-dual_native_guard

.org 0xc0, 0
.global dual_native_rollback
.type dual_native_rollback,%function
dual_native_rollback:
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
.size dual_native_rollback, .-dual_native_rollback
