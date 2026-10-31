    .syntax unified
    .arm
    .section .text.interface_initializer, "ax", %progbits
    .global xm1_interface_initializer_entry
    .type xm1_interface_initializer_entry, %function
xm1_interface_initializer_entry:
    push {r4, lr}
    mrs r4, cpsr
    bl xm1_interface_initializer_publish
    bic r4, r4, #0xf0000000
    orr r4, r4, #0x60000000
    msr cpsr_f, r4
    pop {r4, lr}
    movw r0, #0xbe6c
    movt r0, #0x400e
    movw r1, #0xd120
    movt r1, #0x400e
    movw r2, #0x18f0
    movt r2, #0x400f
    movw r3, #0xef40
    movt r3, #0x400d
    mov r12, #120
    bx lr
    .size xm1_interface_initializer_entry, . - xm1_interface_initializer_entry
    .section .note.GNU-stack, "", %progbits
