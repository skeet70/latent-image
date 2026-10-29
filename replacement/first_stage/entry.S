@ Owned reset entry for the staged first record. The CPU arrives in SVC with
@ SP=0xfff00f00; the container patch points the reset vector here.
    .syntax unified
    .arm
    .section .text.entry, "ax", %progbits
    .global xm1_first_stage_entry
    .type xm1_first_stage_entry, %function
xm1_first_stage_entry:
    @ Current flags and IF, SVC on return, for every exception mode.
    mrs r0, cpsr
    orr r0, r0, #0xc0
    bic r0, r0, #0x1f
    bic r0, r0, #0x100
    mov r1, r0
    orr r0, r0, #0x1f
    mov r4, r1

    orr r2, r4, #0x1b
    msr cpsr_c, r2
    msr spsr_fsxc, r0
    orr r2, r4, #0x17
    msr cpsr_c, r2
    msr spsr_fsxc, r0
    orr r2, r4, #0x11
    msr cpsr_c, r2
    msr spsr_fsxc, r0
    orr r2, r4, #0x12
    msr cpsr_c, r2
    msr spsr_fsxc, r0
    orr r2, r4, #0x13
    msr cpsr_c, r2
    msr spsr_fsxc, r0

    @ ATCM 0x70000000, 8 KiB, CP15 c9,c1,1.
    ldr r0, =0x70000011
    mcr p15, 0, r0, c9, c1, 1
    ldr r1, =0x70000000
    mov r2, #0x2000
    mov r3, #0
1:
    str r3, [r1], #4
    subs r2, r2, #4
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

    .section .note.GNU-stack, "", %progbits
