    .syntax unified
    .arm
    .section .text.entry, "ax", %progbits
    .global xm1_loader_orchestration_entry
    .type xm1_loader_orchestration_entry, %function
xm1_loader_orchestration_entry:
    push {r4, r5, r11, lr}
    add r11, sp, #12
    b xm1_loader_orchestration + 8
    .size xm1_loader_orchestration_entry, . - xm1_loader_orchestration_entry
    .section .note.GNU-stack, "", %progbits
