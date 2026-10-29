@ X-M1 1.50 image 5 develop-step boundary calls. Tail trampolines follow the
@ bank's RB_TRAMPOLINE form; the nested wait loads its target and uses BLX.

.syntax unified
.arm
.section .text,"ax",%progbits

.macro RB_CAPTURE_TRAMPOLINE name, addr
.global \name
.type \name,%function
\name:
    movw r12, #(\addr & 0xffff)
    movt r12, #((\addr >> 16) & 0xffff)
    bx r12
.size \name, .-\name
.endm

@ FUN_00e9e038(context, step): resolve the plan and run the owned wait.
.global rb_capture_develop_wait
.type rb_capture_develop_wait, %function
rb_capture_develop_wait:
    push {r4, lr}
    mov r4, r0
    mov r0, r1
    bl rb_capture_stock_step_plan
    mov r1, r0
    mov r0, r4
    bl rb_capture_step_wait
    pop {r4, pc}
.size rb_capture_develop_wait, .-rb_capture_develop_wait

@ FUN_00e93664(context, event, 0, 0).
.global rb_capture_stock_clear
.type rb_capture_stock_clear, %function
rb_capture_stock_clear:
    mov r2, #0
    mov r3, #0
    movw r12, #0x3664
    movt r12, #0x00e9
    bx r12
.size rb_capture_stock_clear, .-rb_capture_stock_clear

@ The develop context's requested-event word at +0x4c.
.global rb_capture_stock_cancel_signals
.type rb_capture_stock_cancel_signals, %function
rb_capture_stock_cancel_signals:
    ldr r0, [r0, #0x4c]
    bx lr
.size rb_capture_stock_cancel_signals, .-rb_capture_stock_cancel_signals

@ FUN_007243a8(event, &out, infinite).
.global rb_capture_stock_wait
.type rb_capture_stock_wait, %function
rb_capture_stock_wait:
    push {r4, lr}
    sub sp, sp, #8
    mov r1, sp
    mvn r2, #0
    ldr r12, .Lrb_capture_wait_target
    blx r12
    add sp, sp, #8
    pop {r4, pc}
.align 2
.Lrb_capture_wait_target:
    .word 0x007243a8
.size rb_capture_stock_wait, .-rb_capture_stock_wait

.global rb_capture_invalid_descriptor
.type rb_capture_invalid_descriptor, %function
rb_capture_invalid_descriptor:
    udf #0
.size rb_capture_invalid_descriptor, .-rb_capture_invalid_descriptor

RB_CAPTURE_TRAMPOLINE rb_capture_stock_step_plan, 0x00e91810
RB_CAPTURE_TRAMPOLINE rb_capture_stock_check, 0x00e93bdc
RB_CAPTURE_TRAMPOLINE rb_capture_stock_advance, 0x00e9390c
RB_CAPTURE_TRAMPOLINE rb_capture_stock_release, 0x00724638
RB_CAPTURE_TRAMPOLINE rb_capture_stock_log, 0x00e9db80
