@ Callback glue for rb_modal_dispatch. Each wrapper returns the callee's r0/r1
@ as an AAPCS 8-byte result.

.syntax unified
.arm
.section .text,"ax",%progbits

@ r0-r3 pass through to the retained prehook 0x00d0cc68.
.p2align 2
.global rb_modal_call_prehook
.type rb_modal_call_prehook,%function
rb_modal_call_prehook:
    push    {r4, lr}
    movw    r12, #0xcc68
    movt    r12, #0x00d0
    blx     r12
    pop     {r4, pc}
.size rb_modal_call_prehook, .-rb_modal_call_prehook

@ Entered with r0 = table target and r1-r3 = the callee's r0-r2; the callee
@ sees the target in r3, as at the original BLX r3 sites.
.p2align 2
.global rb_modal_call
.type rb_modal_call,%function
rb_modal_call:
    push    {r4, lr}
    mov     r12, r0
    mov     r0, r1
    mov     r1, r2
    mov     r2, r3
    mov     r3, r12
    blx     r12
    pop     {r4, pc}
.size rb_modal_call, .-rb_modal_call
