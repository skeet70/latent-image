@ X-M1 1.50 image 5 develop-step boundary calls.
@ AAPCS; each wrapper preserves the Rust caller's r4-r11 and sp.

.syntax unified
.arm
.text

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

@ FUN_00e91810(0x80, step): the stock descriptor lookup.
.global rb_capture_stock_step_plan
.type rb_capture_stock_step_plan, %function
rb_capture_stock_step_plan:
    mov r1, r0
    mov r0, #0x80
    b 0x00e91810
.size rb_capture_stock_step_plan, .-rb_capture_stock_step_plan

@ FUN_00e93bdc(context, event).
.global rb_capture_stock_check
.type rb_capture_stock_check, %function
rb_capture_stock_check:
    b 0x00e93bdc
.size rb_capture_stock_check, .-rb_capture_stock_check

@ FUN_00e93664(context, event, 0, 0).
.global rb_capture_stock_clear
.type rb_capture_stock_clear, %function
rb_capture_stock_clear:
    mov r2, #0
    mov r3, #0
    b 0x00e93664
.size rb_capture_stock_clear, .-rb_capture_stock_clear

@ FUN_00e9390c(context, state).
.global rb_capture_stock_advance
.type rb_capture_stock_advance, %function
rb_capture_stock_advance:
    b 0x00e9390c
.size rb_capture_stock_advance, .-rb_capture_stock_advance

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
    bl 0x007243a8
    add sp, sp, #8
    pop {r4, pc}
.size rb_capture_stock_wait, .-rb_capture_stock_wait

@ FUN_00724638().
.global rb_capture_stock_release
.type rb_capture_stock_release, %function
rb_capture_stock_release:
    b 0x00724638
.size rb_capture_stock_release, .-rb_capture_stock_release

@ FUN_00e9db80(context, phase).
.global rb_capture_stock_log
.type rb_capture_stock_log, %function
rb_capture_stock_log:
    b 0x00e9db80
.size rb_capture_stock_log, .-rb_capture_stock_log
