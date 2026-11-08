@ Owned X-M1 1.50 exception entries. The vector, dispatcher and wrapper bodies
@ stay at their stock addresses; the Rust decisions are linked in the ack/EOI
@ extent by exception-{a,b}.ld.
    .syntax unified
    .arm

#ifdef DOMAIN_A
    .equ FIQ_CELL, 0x4004f110
    .equ RESUME_GATE, 0x4004f10c
#else
    .equ FIQ_CELL, 0x400df0d0
    .equ RESUME_GATE, 0x400df0cc
#endif

    .section .text.exception_undefined, "ax", %progbits
    .global xm1_exception_undefined
    .type xm1_exception_undefined, %function
xm1_exception_undefined:
    sub lr, lr, #4
    push {r12, lr}
    mrs lr, spsr
    push {r0-r11, r12, lr}
    mov r0, #1
    mov r1, sp
    bl xm1_exception_vfp_owner
    pop {r0-r11, r12, lr}
    pop {r12, lr}
    movs pc, lr
    .size xm1_exception_undefined, . - xm1_exception_undefined

    .section .text.exception_prefetch, "ax", %progbits
    .global xm1_exception_prefetch
    .type xm1_exception_prefetch, %function
xm1_exception_prefetch:
    sub lr, lr, #4
    push {r12, lr}
    mrs lr, spsr
    push {r0-r11, r12, lr}
    ldr r2, [pc, #0x2d0]
    mov r0, #3
    mov r1, sp
    bx r2
    .size xm1_exception_prefetch, . - xm1_exception_prefetch

    .section .text.exception_data, "ax", %progbits
    .global xm1_exception_data
    .type xm1_exception_data, %function
xm1_exception_data:
    sub lr, lr, #8
    push {r12, lr}
    mrs lr, spsr
    push {r0-r11, r12, lr}
    ldr r2, [pc, #0x2b0]
    mov r0, #4
    mov r1, sp
    bx r2
    .size xm1_exception_data, . - xm1_exception_data

    .section .text.exception_reserved, "ax", %progbits
    .global xm1_exception_reserved
    .type xm1_exception_reserved, %function
xm1_exception_reserved:
    push {r12, lr}
    mrs lr, spsr
    push {r0-r11, r12, lr}
    ldr r2, [pc, #0x294]
    mov r0, #5
    mov r1, sp
    bx r2
    .size xm1_exception_reserved, . - xm1_exception_reserved

    .section .text.exception_fiq, "ax", %progbits
    .global xm1_exception_fiq
    .type xm1_exception_fiq, %function
xm1_exception_fiq:
    sub lr, lr, #4
    push {r12, lr}
    mrs lr, spsr
    push {r0-r11, r12, lr}
    ldr r2, [pc, #0x278]
    mov r0, #7
    mov r1, sp
    blx r2
    pop {r0-r11}
    pop {r12, lr}
    msr SPSR_fsxc, lr
    ldm sp!, {r12, pc}^
    .size xm1_exception_fiq, . - xm1_exception_fiq

    .section .text.exception_svc, "ax", %progbits
    .global xm1_exception_svc
    .type xm1_exception_svc, %function
xm1_exception_svc:
    push {r12, lr}
    mrs lr, spsr
    push {r12, lr}
    tst lr, #0x20
    ldr lr, [sp, #0xc]
    ldrhne lr, [lr, #-2]
    bicne lr, lr, #0xff00
    ldreq lr, [lr, #-4]
    biceq lr, lr, #0xff000000
    teq lr, #0
    bne xm1_exception_svc_diag
    bx r0
    .size xm1_exception_svc, . - xm1_exception_svc

    .section .text.exception_svc_diag, "ax", %progbits
    .global xm1_exception_svc_diag
    .type xm1_exception_svc_diag, %function
xm1_exception_svc_diag:
    push {r0-r11}
    ldr r2, [pc, #0x220]
    mov r0, #2
    mov r1, sp
    blx r2
    pop {r12, lr}
    msr SPSR_fsxc, lr
    ldm sp!, {r12, pc}^
    .size xm1_exception_svc_diag, . - xm1_exception_svc_diag

    .section .text.exception_irq, "ax", %progbits
    .global xm1_exception_irq
    .type xm1_exception_irq, %function
xm1_exception_irq:
    mov sp, #0xd3
    msr CPSR_fc, sp
    push {r0, r1, r2, r3, r12, lr, pc}
    mov r0, #0xd2
    msr CPSR_fc, r0
    sub r0, lr, #4
    mrs r1, spsr
    and r2, r1, #0x40
    orr r2, r2, #0x93
    msr CPSR_fc, r2
    str r0, [sp, #0x18]
    push {r1}
    ldr r2, [pc, #0x1dc]
    ldr r3, [r2]
    add r0, r3, #1
    str r0, [r2]
    cmp r3, #0
    bne 1f
    mov r2, sp
    ldr sp, [pc, #0x118]
    push {r2}
1:
    .org 0x54
    bl xm1_exception_lock_acquire
    .rept 25
    nop
    .endr
    .org 0xbc
    bl xm1_exception_ack_wrapper
    mrs r2, apsr
    bic r2, r2, #0x80
    msr CPSR_fc, r2
    cmp r0, #0
    movne lr, pc
    movne pc, r0
    mrs r2, apsr
    and r2, r2, #0x40
    orr r2, r2, #0x93
    msr CPSR_fc, r2
    bl xm1_exception_eoi_wrapper
    .org 0xf4
    bl xm1_exception_exit_tail
    cmp r0, #0
    beq 2f
    ldm sp!, {r1}
    mov sp, r1
    cmp r0, #1
    beq 3f
    b 2f
3:
    b xm1_exception_scheduler_return
2:
    .org 0x148
    ldm sp!, {r1}
    mrs r2, apsr
    and r2, r2, #0x40
    bic r1, r1, #0x40
    orr r1, r1, r2
    msr SPSR_fc, r1
    ldm sp!, {r0, r1, r2, r3, r12, lr, pc}^
    .size xm1_exception_irq, . - xm1_exception_irq

    .section .text.exception_dispatcher, "ax", %progbits
    .global xm1_exception_dispatcher
    .type xm1_exception_dispatcher, %function
xm1_exception_dispatcher:
    push {r4, r5, r11, lr}
    add r11, sp, #12
    mov r5, r0
    mov r4, r1
    bl xm1_exception_fault_record
    cmp r0, #0
    beq 2f
    mov r1, r4
    ldr r1, [r1, #0x3c]
    mov r2, r0
    mov r0, r5
    blx r2
2:
    movw r3, #:lower16:RESUME_GATE
    movt r3, #:upper16:RESUME_GATE
    ldr r3, [r3]
    cmp r3, #0
    beq 1f
    mov sp, r4
    pop {r0-r11, r12, lr}
    pop {r12, lr}
    movs pc, lr
1:
    bl xm1_exception_shutdown
    cmp r0, #0
    beq 1b
    bl xm1_exception_halt
    b .
    .size xm1_exception_dispatcher, . - xm1_exception_dispatcher

    .section .text.exception_ack_wrapper, "ax", %progbits
    .global xm1_exception_ack_wrapper
    .type xm1_exception_ack_wrapper, %function
xm1_exception_ack_wrapper:
    push {r11, lr}
    add r11, sp, #4
    bl xm1_exception_ack
    pop {r11, pc}
    .size xm1_exception_ack_wrapper, . - xm1_exception_ack_wrapper

    .section .text.exception_eoi_wrapper, "ax", %progbits
    .global xm1_exception_eoi_wrapper
    .type xm1_exception_eoi_wrapper, %function
xm1_exception_eoi_wrapper:
    push {r11, lr}
    add r11, sp, #4
    bl xm1_exception_eoi
    pop {r11, pc}
    .size xm1_exception_eoi_wrapper, . - xm1_exception_eoi_wrapper

    .section .text.exception_fiq_hook, "ax", %progbits
    .global xm1_exception_fiq_hook
    .type xm1_exception_fiq_hook, %function
xm1_exception_fiq_hook:
    push {r11, lr}
    add r11, sp, #4
    movw r3, #:lower16:FIQ_CELL
    movt r3, #:upper16:FIQ_CELL
    ldr r3, [r3]
    cmp r3, #0
    popeq {r11, pc}
    ldr r0, [r1, #0x3c]
    blx r3
    pop {r11, pc}
    .size xm1_exception_fiq_hook, . - xm1_exception_fiq_hook

    .section .text.exception_vector_slot, "ax", %progbits
    .global xm1_exception_vector_slot
    .type xm1_exception_vector_slot, %function
xm1_exception_vector_slot:
    bx lr
    .size xm1_exception_vector_slot, . - xm1_exception_vector_slot

    .section .note.GNU-stack, "", %progbits
