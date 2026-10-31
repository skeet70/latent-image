@ Loader-A completion delay, entered by the `bl` that replaces `mov r5,#1`
@ at 0x40a00 and returning to 0x40a04 before the original 0x40a08 store.
@ r3 is reloaded at 0x40a04; lr is not read before 0x40a10 overwrites it.
    .syntax unified
    .cpu cortex-r4
    .arm
    .section .text.entry, "ax", %progbits
    .global xm1_completion_delay_entry
    .type xm1_completion_delay_entry, %function
xm1_completion_delay_entry:
    mrs r3, cpsr
    mov r5, #0x10000
delay:
    subs r5, r5, #1
    bne delay
    msr cpsr_f, r3
    .global xm1_completion_delay_release
xm1_completion_delay_release:
    mov r5, #1
    bx lr
    .size xm1_completion_delay_entry, . - xm1_completion_delay_entry
    .global xm1_completion_delay_end
xm1_completion_delay_end:
    .section .note.GNU-stack, "", %progbits
