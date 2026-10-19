.syntax unified
.arm
.section .text.ram_push_free,"ax",%progbits
.global ram_push_free
.type ram_push_free,%function
ram_push_free:
    push    {r4, r5, r6, lr}
    mrc     p14, 0, r4, c2, c0, 0
    movw    r12, #0x4003
    cmp     r4, r12
    bleq    ram_push_stock_free
    moveq   r0, #2
    movne   r0, #2
    pop     {r4, r5, r6, pc}
.size ram_push_free, .-ram_push_free
