.syntax unified
.arm
.section .text.ram_push_bootstrap,"ax",%progbits
.global ram_push_bootstrap
.type ram_push_bootstrap,%function
ram_push_bootstrap:
    push    {r1, r4, r5, lr}
    mov     r5, r1
    mrc     p14, 0, r4, c2, c0, 0
    tst     r4, #0x1000
    bleq    ram_push_malloc
    stm     r5, {r0, r4}
    mov     r0, #2
    pop     {r1, r4, r5, pc}
.size ram_push_bootstrap, .-ram_push_bootstrap
