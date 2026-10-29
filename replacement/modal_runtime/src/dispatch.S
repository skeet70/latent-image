@ Callback ABI: preserve r4-r11 and sp, call one retained stock service, and
@ return its r0/r1 as an AAPCS 8-byte result. r3 is the target at entry, as at
@ the original BLX r3 sites. The four-byte slot aligns sp at the call.

.syntax unified
.arm
.section .text,"ax",%progbits

.p2align 2
.global rb_modal_call_prehook
.type rb_modal_call_prehook,%function
rb_modal_call_prehook:
    push    {r4-r11, lr}
    sub     sp, sp, #4
    movw    r12, #0xcc68
    movt    r12, #0x00d0
    blx     r12
    add     sp, sp, #4
    pop     {r4-r11, lr}
    bx      lr
.size rb_modal_call_prehook, .-rb_modal_call_prehook

.p2align 2
.global rb_modal_call_action
.type rb_modal_call_action,%function
rb_modal_call_action:
    push    {r4-r11, lr}
    sub     sp, sp, #4
    mov     r12, r0
    mov     r0, r1
    mov     r1, r2
    mov     r2, r3
    mov     r3, r12
    blx     r12
    add     sp, sp, #4
    pop     {r4-r11, lr}
    bx      lr
.size rb_modal_call_action, .-rb_modal_call_action

.p2align 2
.global rb_modal_call_transition
.type rb_modal_call_transition,%function
rb_modal_call_transition:
    push    {r4-r11, lr}
    sub     sp, sp, #4
    mov     r12, r0
    mov     r0, r1
    mov     r1, r2
    mov     r2, r3
    mov     r3, r12
    blx     r12
    add     sp, sp, #4
    pop     {r4-r11, lr}
    bx      lr
.size rb_modal_call_transition, .-rb_modal_call_transition
