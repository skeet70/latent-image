    .syntax unified
    .cpu cortex-r4
    .arm
    .section .text.entry, "ax", %progbits
    .global xm1_peer_wait_control_entry
    .type xm1_peer_wait_control_entry, %function
xm1_peer_wait_control_entry:
    push {r0-r3, r12, lr}
    mrs r0, cpsr
    movw r1, #8192
    movt r3, #0x4002
    movw r12, #0x22b
    .global xm1_peer_wait_control_poll
xm1_peer_wait_control_poll:
    ldr r2, [r3, #4]
    cmp r2, #0
    beq xm1_peer_wait_control_acknowledged
    cmp r2, r12
    bne control_failure_restore
    mov r2, #100
poll_backoff:
    nop
    subs r2, r2, #1
    bne poll_backoff
    subs r1, r1, #1
    bne xm1_peer_wait_control_poll
control_failure_restore:
    msr cpsr_f, r0
    pop {r0-r3, r12, lr}
    .global xm1_peer_wait_control_failure
xm1_peer_wait_control_failure:
    b xm1_peer_wait_control_failure
    .global xm1_peer_wait_control_acknowledged
xm1_peer_wait_control_acknowledged:
    movw r1, #4096
release_delay:
    subs r1, r1, #1
    bne release_delay
    msr cpsr_f, r0
    pop {r0-r3, r12, lr}
    movt r3, #0x4002
    b xm1_peer_wait_original_clear
    .size xm1_peer_wait_control_entry, . - xm1_peer_wait_control_entry
    .global xm1_peer_wait_control_end
xm1_peer_wait_control_end:
    .section .note.GNU-stack, "", %progbits
