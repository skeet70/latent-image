.syntax unified
.arm
.section .text.ram_push_peer_prepare,"ax",%progbits
.global ram_push_peer_prepare
.type ram_push_peer_prepare,%function
ram_push_peer_prepare:
    push {r4, lr}
    mov r4, r1
    ldr r2, [r2]
    ldr r1, [r1]
    bl ram_push_core1_range
    str r0, [r4]
    orr r0, r0, #2
    pop {r4, pc}
.size ram_push_peer_prepare, . - ram_push_peer_prepare
