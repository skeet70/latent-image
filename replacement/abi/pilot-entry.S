    .syntax unified
    .arm
    .section .text.xm1_abi_probe,"ax",%progbits
    .global xm1_abi_probe
    .type xm1_abi_probe,%function
xm1_abi_probe:
    push {r4-r11, lr}
    sub sp, sp, #4
    str sp, [sp]
    mov r0, #0x10
    mov r1, #0x20
    mov r2, #0x30
    bl xm1_pilot_preserve_pressure

    movw r1, #0x3000
    mov r0, #0
    mov r3, #0x44
    cmp r4, r3
    orrne r0, r0, #1
    mov r3, #0x55
    cmp r5, r3
    orrne r0, r0, #2
    mov r3, #0x66
    cmp r6, r3
    orrne r0, r0, #4
    mov r3, #0x77
    cmp r7, r3
    orrne r0, r0, #8
    mov r3, #0x88
    cmp r8, r3
    orrne r0, r0, #16
    mov r3, #0x99
    cmp r9, r3
    orrne r0, r0, #32
    mov r3, #0xaa
    cmp r10, r3
    orrne r0, r0, #64
    mov r3, #0xbb
    cmp r11, r3
    orrne r0, r0, #128
    str r0, [r1, #60]

    mov r0, #0
    ldr r3, [sp]
    cmp sp, r3
    orrne r0, r0, #1
    tst sp, #7
    orrne r0, r0, #2
    str r0, [r1, #64]

    bl pilot_run
    movw r1, #0x3000
    ldr r2, [r1, #60]
    orr r0, r0, r2
    ldr r2, [r1, #64]
    orr r0, r0, r2
    str r0, [r1]
    ldr r3, [sp]
    cmp sp, r3
    moveq r2, #0
    movne r2, #4
    orr r0, r0, r2
    str r0, [r1]

    add sp, sp, #4
    pop {r4-r11, lr}
    movw r1, #0x3300
    movw r2, #0x3400
    mov r3, #0
1:
    str r3, [r1]
    add r1, r1, #4
    cmp r1, r2
    bne 1b
    adds r2, r2, #0
    bx lr
    .ltorg
    .size xm1_abi_probe, .-xm1_abi_probe

    .section .text.xm1_panic_loop,"ax",%progbits
    .global xm1_panic_loop
    .type xm1_panic_loop,%function
xm1_panic_loop:
    b xm1_panic_loop
    .size xm1_panic_loop, .-xm1_panic_loop
