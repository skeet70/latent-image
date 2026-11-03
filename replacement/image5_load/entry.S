    .syntax unified
    .arm
    .section .text.entry, "ax", %progbits
    .global xm1_image5_load_entry
    .type xm1_image5_load_entry, %function
xm1_image5_load_entry:
    push {r11, lr}
    bl xm1_image5_load
    movw lr, #0x22b8
    movt lr, #0x4
    pop {r11, pc}
    .size xm1_image5_load_entry, . - xm1_image5_load_entry
    .section .note.GNU-stack, "", %progbits
