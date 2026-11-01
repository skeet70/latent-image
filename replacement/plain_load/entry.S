    .syntax unified
    .arm
    .section .text.entry, "ax", %progbits
    .global xm1_plain_load_entry
    .type xm1_plain_load_entry, %function
xm1_plain_load_entry:
    push {r11, lr}
    bl xm1_plain_load
    movw lr, #0x992c
    movt lr, #0x6a
    pop {r11, pc}
    .size xm1_plain_load_entry, . - xm1_plain_load_entry
    .section .note.GNU-stack, "", %progbits
