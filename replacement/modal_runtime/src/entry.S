.syntax unified
.arm
.section .text.rb_modal_entry,"ax",%progbits
.balign 4
.global rb_modal_entry
.type rb_modal_entry,%function
rb_modal_entry:
    str r12, [sp, #-4]!
    mrs r12, cpsr
    str r12, [sp, #-4]!
    cmp r0, #75
    beq .Lowned
    ldr r12, [sp], #4
    msr APSR_nzcvq, r12
    ldr r12, [sp], #4
    push {r4-r11, lr}
    b __rb_modal_stock_resume
.Lowned:
    ldr r12, [sp], #4
    msr APSR_nzcvq, r12
    ldr r12, [sp], #4
    b rb_modal_dispatch
.size rb_modal_entry, .-rb_modal_entry
