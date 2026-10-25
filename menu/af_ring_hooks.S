.syntax unified
.arm
.text

.macro AF_HOOK name, kind, first, second, continuation
.global \name
.type \name,%function
\name:
    push {r0-r12, lr}
    mrs r12, cpsr
    push {r12, lr}
    mov r0, #\kind
    add r1, sp, #8
    bl af_ring_record
    pop {r12, lr}
    msr cpsr_f, r12
    pop {r0-r12, lr}
    .word \first
    .word \second
    ldr pc, [pc, #-4]
    .word \continuation
.size \name, .-\name
.endm

AF_HOOK af_ring_direction, 1, 0xe92d48f0, 0xe28db014, 0x00f41c50
AF_HOOK af_ring_peak, 2, 0xe92d4870, 0xe28db010, 0x00f42ac4
AF_HOOK af_ring_accepted, 3, 0xe92d49f0, 0xe28db018, 0x00f44ff4
