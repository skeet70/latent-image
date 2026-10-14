.syntax unified
.arch armv7-r
.arm
.section .text,"ax",%progbits
.global _start
.type _start,%function
_start:
    cpsid if
#ifdef STARTUP_RESTORE
    movw r6, #0x8084
    movt r6, #0xfff8
    mov r7, #1
    str r7, [r6]
    dsb sy
#endif
    movw r0, #0x93c0
    movt r0, #0xfff8
#ifdef LED_SETUP
    movw r5, #0x0001
    movt r5, #0x0005
    str r5, [r0, #4]
    dsb sy
#endif
    movw r1, #0x0001
    movt r1, #0x0001
    movw r2, #0x0000
    movt r2, #0x0001
cycle:
    mov r3, #3
pulse:
    str r1, [r0]
    dsb sy
    mov r4, #0x01000000
on_delay:
    subs r4, r4, #1
    bne on_delay
    str r2, [r0]
    dsb sy
    mov r4, #0x01000000
off_delay:
    subs r4, r4, #1
    bne off_delay
    subs r3, r3, #1
    bne pulse
    mov r4, #0x08000000
gap_delay:
    subs r4, r4, #1
    bne gap_delay
    b cycle
.size _start, .-_start
.org 512, 0
