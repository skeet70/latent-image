.syntax unified
.arch armv7-r
.arm
.section .text,"ax",%progbits
.global _start
.type _start,%function
_start:
    cpsid if
    movw sp, #0x0200
    movw r0, #0x0100
    movw r1, #0x4d58
    movt r1, #0x4431
    movw r2, #0x4f42
    movt r2, #0x544f
    movw r3, #0x0150
    movt r3, #0x0000
    movw r4, #0x0001
    movt r4, #0x0000
    stmia r0, {r1-r4}
    dsb sy
    wfi
halt:
    b halt
.size _start, .-_start
.org 512, 0
