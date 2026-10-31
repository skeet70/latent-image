@ Owned loader-B entry, published as segment id 1's header word +0xc and
@ reached from the record-2 handoff with r0 = the image-4 argument.
    .syntax unified
    .arm
    .section .text.loader_prefix, "ax", %progbits
    .global xm1_loader_prefix_entry
    .type xm1_loader_prefix_entry, %function
xm1_loader_prefix_entry:
    mov r5, r0
#ifdef XM1_LOADER_PREFIX_RETIRED_FETCH_FAULT
    ldr r12, =xm1_loader_b_stock_entry
    .global xm1_loader_prefix_retired_fetch_fault
xm1_loader_prefix_retired_fetch_fault:
    bx r12
#endif
#ifdef XM1_LOADER_PREFIX_WRITE_CONFINEMENT_FAULT
    ldr r12, =xm1_loader_b_image_start
    .global xm1_loader_prefix_write_confinement_fault
xm1_loader_prefix_write_confinement_fault:
    str r5, [r12]
#endif
    bl xm1_loader_prefix_install
    mov r6, r0
#ifdef XM1_LOADER_PREFIX_STACK_FAULT
    ldr r4, =xm1_loader_b_stack_top - 8
#else
    ldr r4, =xm1_loader_b_stack_top
#endif
    mov sp, r4
    bl xm1_loader_prefix_clear_workspace
    @ Stock 0xd09ac frame {0x400f3420, argument, entry r11, 0xd00c0}.
#ifdef XM1_LOADER_PREFIX_FRAME_FAULT
    ldr lr, =xm1_loader_b_frame_return + 4
#else
    ldr lr, =xm1_loader_b_frame_return
#endif
    push {r4, r5, r11, lr}
    add r11, sp, #12
    mov r4, r5
    bl xm1_loader_prefix_dispatch
    .global xm1_loader_prefix_exit
xm1_loader_prefix_exit:
    bx r0
    .size xm1_loader_prefix_entry, . - xm1_loader_prefix_entry
    .ltorg

    .section .note.GNU-stack, "", %progbits
