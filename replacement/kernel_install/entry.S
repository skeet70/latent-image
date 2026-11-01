    .syntax unified
    .arm
    .section .text.entry, "ax", %progbits
    .global xm1_kernel_install_entry
    .type xm1_kernel_install_entry, %function
xm1_kernel_install_entry:
    push {r11, lr}
    push {r4, r12}
    bl xm1_kernel_install
    pop {r4, r12}
    movw r1, #0xfc2c
    movt r1, #0x40a0
    mov r3, r1
    mov r2, #0
    mov r0, #0
    cmp r0, #0
    movw lr, #0x5a40
    movt lr, #0x72
    pop {r11, pc}
    .size xm1_kernel_install_entry, . - xm1_kernel_install_entry
    .section .note.GNU-stack, "", %progbits
