@ Owned record-2 reset entry, named by the vector literal at record +0x20.
@ The retained services return through the SVC-banked LR, as from stock 0x11800.
    .syntax unified
    .arm
    .section .text.entry, "ax", %progbits
    .global xm1_secondary_entry
    .type xm1_secondary_entry, %function
xm1_secondary_entry:
    bl xm1_retained_execution_state
    bl xm1_retained_tcm_stacks
    bl xm1_secondary_main
#ifdef XM1_SECONDARY_FORBIDDEN_FETCH_FAULT
    bl xm1_forbid_fetch_stub_a_start
#endif
    .size xm1_secondary_entry, . - xm1_secondary_entry

    @ Stock 0x1183c-0x11850 handoff, reached by falling through from the entry.
    .global xm1_secondary_handoff
    .type xm1_secondary_handoff, %function
xm1_secondary_handoff:
    cps #0x13
    ldr r3, =xm1_publication_target
    ldr r3, [r3]
    ldr r0, =xm1_publication_argument
    ldr r0, [r0]
#ifdef XM1_SECONDARY_STACK_FAULT
    sub sp, sp, #8
#endif
    .global xm1_secondary_handoff_vector
xm1_secondary_handoff_vector:
    mov pc, r3
    .size xm1_secondary_handoff, . - xm1_secondary_handoff
    .ltorg

    .section .note.GNU-stack, "", %progbits
