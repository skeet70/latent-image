    .syntax unified
    .arm
    .section .text.entry, "ax", %progbits
    .global xm1_startup_dispatch_entry
    .type xm1_startup_dispatch_entry, %function
xm1_startup_dispatch_entry:
    push {r4, r5, r11, lr}
    add r11, sp, #12
    sub sp, sp, #32
    add r1, sp, #4
    bl xm1_startup_dispatch
    sub sp, r11, #12
    pop {r4, r5, r11, pc}
    .size xm1_startup_dispatch_entry, . - xm1_startup_dispatch_entry
    .section .note.GNU-stack, "", %progbits
