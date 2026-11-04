@ Owned loader-B entry, published as segment id 1's header word +0xc and
@ reached from the record-2 handoff with r0 = the image-4 argument. It exits
@ to loader-B startup 0xd09ac with the stock 0xd00c4 register state.
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
    @ Synthetic stack fault: move the loader-B stack top down two words.
    .global xm1_loader_prefix_stack_fault
xm1_loader_prefix_stack_fault:
    ldr r4, =xm1_loader_b_stack_top - 8
#else
    ldr r4, =xm1_loader_b_stack_top
#endif
    mov sp, r4
    bl xm1_loader_prefix_clear_workspace
    mov r0, r5
    @ Stock 0xd09ac pushes this lr as its frame's last word.
#ifdef XM1_LOADER_PREFIX_FRAME_FAULT
    ldr lr, =xm1_loader_b_frame_return + 4
#else
    ldr lr, =xm1_loader_b_frame_return
#endif
    .global xm1_loader_prefix_exit
xm1_loader_prefix_exit:
    b xm1_loader_b_startup
    .size xm1_loader_prefix_entry, . - xm1_loader_prefix_entry
    .ltorg

@ Zeroes [r0, r1), both word aligned, eight words per store as stock 0xd34f0.
    .section .text.loader_prefix_clear, "ax", %progbits
    .global xm1_loader_prefix_clear
    .type xm1_loader_prefix_clear, %function
xm1_loader_prefix_clear:
    push {r4, r5, r6, r7, r8, r9, r10, r11}
    mov r4, #0
    mov r5, #0
    mov r6, #0
    mov r7, #0
    mov r8, #0
    mov r9, #0
    mov r10, #0
    mov r11, #0
    sub r2, r1, r0
    lsrs r3, r2, #5
    beq 2f
1:
    stm r0!, {r4, r5, r6, r7, r8, r9, r10, r11}
    subs r3, r3, #1
    bne 1b
2:
    ands r2, r2, #31
    beq 4f
3:
    str r4, [r0], #4
    subs r2, r2, #4
    bne 3b
4:
    pop {r4, r5, r6, r7, r8, r9, r10, r11}
    bx lr
    .size xm1_loader_prefix_clear, . - xm1_loader_prefix_clear

    .section .note.GNU-stack, "", %progbits
