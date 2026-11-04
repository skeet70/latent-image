@ Owned reset entry for the staged first record (ROM handoff SVC, SP=0xfff00f00).
@ Stock 0x1800 also clobbers r0/r1; the core flag comes from DBGDSAR in Rust.
    .syntax unified
    .arm
    .section .text.entry, "ax", %progbits
    .global xm1_first_stage_entry
    .type xm1_first_stage_entry, %function
xm1_first_stage_entry:
    @ Current flags and IF, A clear, System on return, for every exception mode (stock 0x1854-0x18b0).
    mrs r0, cpsr
    orr r0, r0, #0xc0
    bic r0, r0, #0x1f
    bic r0, r0, #0x100
    mov r1, r0
    orr r0, r0, #0x1f
    mov r4, r1

    orr r2, r4, #0x1b
    msr cpsr_fsxc, r2
    msr spsr_fsxc, r0
    orr r2, r4, #0x17
    msr cpsr_fsxc, r2
    msr spsr_fsxc, r0
    orr r2, r4, #0x11
    msr cpsr_fsxc, r2
    msr spsr_fsxc, r0
    orr r2, r4, #0x12
    msr cpsr_fsxc, r2
    msr spsr_fsxc, r0
    orr r2, r4, #0x13
    msr cpsr_fsxc, r2
    msr spsr_fsxc, r0

    @ ATCM 0x70000000, 8 KiB, CP15 c9,c1,1; cleared eight words per store as stock 0x18e0-0x1904.
    ldr r0, =0x70000011
    mcr p15, 0, r0, c9, c1, 1
    ldr r1, =0x70000000
    mov r2, #0x100
    mov r3, #0
    mov r5, #0
    mov r6, #0
    mov r7, #0
    mov r8, #0
    mov r9, #0
    mov r10, #0
    mov r11, #0
1:
    stm r1!, {r3, r5, r6, r7, r8, r9, r10, r11}
    subs r2, r2, #1
    bne 1b

    mov r2, #0x12
    orr r2, r4, r2
    msr cpsr_c, r2
    msr spsr_c, #0x1f
    ldr sp, =0x70002000
    mov r2, #0x11
    orr r2, r4, r2
    msr cpsr_c, r2
    msr spsr_c, #0x1f
    ldr sp, =0x70001000
    mov r2, #0x1f
    orr r2, r4, r2
    msr cpsr_c, r2
    ldr sp, =0x70000e00
    mov r2, #0x17
    orr r2, r4, r2
    msr cpsr_c, r2
    msr spsr_c, #0x1f
    ldr sp, =0x70000c00
    mov r2, #0x1b
    orr r2, r4, r2
    msr cpsr_c, r2
    msr spsr_c, #0x1f
    ldr sp, =0x70000a00
    mov r2, #0x13
    orr r2, r4, r2
    msr cpsr_c, r2
    msr spsr_c, #0x1f
    ldr sp, =0x70000800

    ldr r0, =0x4300
    ldr r1, =0x5280
    mov r2, #0
2:
    cmp r0, r1
    strlo r2, [r0], #4
    blo 2b

    b xm1_first_stage_main
    .size xm1_first_stage_entry, . - xm1_first_stage_entry

    @ Stock 0x183c handoff: SVC, target at 0x4300, argument at 0x4304.
    .global xm1_first_stage_handoff
    .type xm1_first_stage_handoff, %function
xm1_first_stage_handoff:
    cps #0x13
    ldr r3, =0x4300
    ldr r2, [r3]
    ldr r0, [r3, #4]
    .global xm1_first_stage_handoff_vector
xm1_first_stage_handoff_vector:
    mov pc, r2
    .size xm1_first_stage_handoff, . - xm1_first_stage_handoff

    @ Boot-log timestamp: r0 = slot word address. The read and store are the relation's log site;
    @ .xm1_log_text follows every .text input so no other symbol moves.
    .section .xm1_log_text, "ax", %progbits
    .global xm1_first_stage_log_timestamp
    .type xm1_first_stage_log_timestamp, %function
xm1_first_stage_log_timestamp:
    ldr r1, =0xfff91000
    .global xm1_first_stage_log_read
xm1_first_stage_log_read:
    ldr r2, [r1]
    .global xm1_first_stage_log_store
xm1_first_stage_log_store:
    str r2, [r0]
    bx lr
    .ltorg
    .size xm1_first_stage_log_timestamp, . - xm1_first_stage_log_timestamp

#ifdef XM1_FIRST_STAGE_INTERLEAVED_TAIL_FAULT
    @ Original synthetic store between KernelA's first and second handler stores.
    .global xm1_first_stage_interleaved_tail_fault
    .type xm1_first_stage_interleaved_tail_fault, %function
xm1_first_stage_interleaved_tail_fault:
    mov r3, #0x24
    ldr r4, [pc, #8]
    str r4, [r3, r6]
    mov r3, #0x28
    ldr pc, [pc, #0]
    .word 0xdeadbe00
    .word 0x40044
    .size xm1_first_stage_interleaved_tail_fault, . - xm1_first_stage_interleaved_tail_fault
#endif

    .section .rodata
    .global xm1_first_stage_contract
xm1_first_stage_contract:
    .word xm1_vector_table_start, xm1_vector_before_reset_end
    .word xm1_vector_after_reset_start, xm1_vector_table_end
    .word xm1_retained_body_start, xm1_retained_body_end
    .word xm1_retained_data_start, xm1_retained_data_end

    .section .note.GNU-stack, "", %progbits
