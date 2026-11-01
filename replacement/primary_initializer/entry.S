    .syntax unified
    .arm
    .section .text.primary_initializer, "ax", %progbits
    .global xm1_primary_initializer_entry
    .type xm1_primary_initializer_entry, %function
xm1_primary_initializer_entry:
    push {r4, lr}
    mrs r4, cpsr
    bl xm1_primary_initializer_publish
    bic r4, r4, #0xf0000000
    orr r4, r4, #0x60000000
    msr cpsr_f, r4
    pop {r4, lr}
    movw r0, #0xbeac
    movt r0, #0x4005
    movw r1, #0xd160
    movt r1, #0x4005
    movw r2, #0x1930
    movt r2, #0x4006
    movw r3, #0xef80
    movt r3, #0x4004
    mov r12, #120
    bx lr
    .size xm1_primary_initializer_entry, . - xm1_primary_initializer_entry
    .section .note.GNU-stack, "", %progbits
