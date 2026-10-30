@ Direct and repeated-entry probes for the owned facade.

.syntax unified
.arm
.section .text.xm1_save_probe,"ax",%progbits
.global xm1_save_probe
.type xm1_save_probe,%function
xm1_save_probe:
    b       xm1_save_facade
.size xm1_save_probe, .-xm1_save_probe

.section .text.xm1_save_reentry_probe,"ax",%progbits
.global xm1_save_reentry_probe
.type xm1_save_reentry_probe,%function
xm1_save_reentry_probe:
    push    {r4, r5, r6, lr}
    mov     r4, r0
    mov     r5, r1
    mov     r6, r2
    bl      xm1_save_facade
    str     r0, [r6, #4]
    mov     r0, r4
    mov     r1, r5
    mov     r2, r6
    bl      xm1_save_facade
    str     r0, [r6, #8]
    pop     {r4, r5, r6, lr}
    bx      lr
.size xm1_save_reentry_probe, .-xm1_save_reentry_probe

.section .text.xm1_save_lifecycle_probe,"ax",%progbits
.global xm1_save_lifecycle_probe
.type xm1_save_lifecycle_probe,%function
xm1_save_lifecycle_probe:
    push    {r4, r5, r6, lr}
    bl      xm1_save_facade
    mov     r4, r0
    movw    r5, #0x5048
    movt    r5, #0x0021
    ldr     r12, [r5]
    cmp     r12, #0
    beq     1f
    mvn     r0, #3
    blx     r12
1:
    ldr     r12, [r5, #4]
    cmp     r12, #0
    beq     2f
    blx     r12
2:
    mov     r0, r4
    pop     {r4, r5, r6, lr}
    bx      lr
.size xm1_save_lifecycle_probe, .-xm1_save_lifecycle_probe
