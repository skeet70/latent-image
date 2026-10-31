    .syntax unified
    .arm
    .section .text.critical_pair, "ax", %progbits
    .global xm1_critical_enter_adapter
    .type xm1_critical_enter_adapter, %function
xm1_critical_enter_adapter:
    push {r4, r5, r6, r7, r8, r10, r11, lr}
    add r11, sp, #28
    movw r5, #0xbd20
    movt r5, #0x96
    ldr r2, [r5]
    movw r3, #0x1618
    ldr r3, [r2, r3]
    blx r3
    mov r4, r0
    ldr r3, [r5, r0, lsl #2]
    ldr r3, [r3, #0x164]
    blx r3
    mov r10, r0
    sub sp, sp, #32
    stmia sp, {r0-r3, r12}
    str lr, [sp, #20]
    mrs r2, cpsr
    str r2, [sp, #24]
    mov r0, sp
    mov r1, r4
    mov r2, r10
    bl xm1_critical_enter_continue
    ldr r2, [sp, #24]
    msr cpsr_fs, r2
    ldmia sp, {r0-r3, r12}
    ldr lr, [sp, #20]
    add sp, sp, #32
    pop {r4, r5, r6, r7, r8, r10, r11, pc}
    .size xm1_critical_enter_adapter, . - xm1_critical_enter_adapter

    .global xm1_critical_exit_adapter
    .type xm1_critical_exit_adapter, %function
xm1_critical_exit_adapter:
    push {r3, r4, r5, r6, r11, lr}
    add r11, sp, #20
    sub sp, sp, #32
    stmia sp, {r0-r3, r12}
    str lr, [sp, #20]
    mrs r2, cpsr
    str r2, [sp, #24]
    mov r0, sp
    bl xm1_critical_exit
    ldr r2, [sp, #24]
    msr cpsr_fs, r2
    ldmia sp, {r0-r3, r12}
    ldr lr, [sp, #20]
    add sp, sp, #32
    pop {r3, r4, r5, r6, r11, pc}
    .size xm1_critical_exit_adapter, . - xm1_critical_exit_adapter

    .global xm1_critical_retained_call
    .type xm1_critical_retained_call, %function
xm1_critical_retained_call:
    push {r4, r5, r6, lr}
    mov r4, r0
    mov r5, r1
    ldr lr, [r4, #24]
    msr cpsr_fs, lr
    ldmia r4, {r0-r3, r12}
    blx r5
    stmia r4, {r0-r3, r12}
    mrs r0, cpsr
    str r0, [r4, #24]
    pop {r4, r5, r6, pc}
    .size xm1_critical_retained_call, . - xm1_critical_retained_call
    .section .note.GNU-stack, "", %progbits
