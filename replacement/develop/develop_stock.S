.syntax unified
.arm
.section .text,"ax",%progbits

.macro DEVELOP_SERVICE name, address
.global \name
.type \name,%function
\name:
    movw r12, #(\address & 0xffff)
    movt r12, #((\address >> 16) & 0xffff)
    bx r12
.size \name, .-\name
.endm

DEVELOP_SERVICE develop_trace_event, 0x00eb0c24
DEVELOP_SERVICE develop_numbering, 0x007101e4
DEVELOP_SERVICE develop_post_service, 0x00e45084
DEVELOP_SERVICE develop_post_error, 0x00e919b0
.global develop_finish_trace
.type develop_finish_trace,%function
develop_finish_trace:
    b xm1_develop_finish_trace
.size develop_finish_trace, .-develop_finish_trace
.global develop_diagnostic
.type develop_diagnostic,%function
develop_diagnostic:
    b xm1_develop_diagnostic
.size develop_diagnostic, .-develop_diagnostic
.macro DEVELOP_OWNED name, target
.global \name
.type \name,%function
\name:
    b \target
.size \name, .-\name
.endm
DEVELOP_OWNED rb_capture_stock_check, xm1_develop_transition
DEVELOP_OWNED rb_capture_stock_advance, xm1_develop_store
DEVELOP_OWNED rb_capture_stock_log, xm1_develop_diagnostic
DEVELOP_OWNED rb_capture_stock_release, xm1_pipeline_image_signal_current_zero
.global rb_capture_stock_clear
.type rb_capture_stock_clear,%function
rb_capture_stock_clear:
    mov r2, #0
    mov r3, #0
    b xm1_develop_callback
.size rb_capture_stock_clear, .-rb_capture_stock_clear

.global rb_capture_stock_cancel_signals
.type rb_capture_stock_cancel_signals,%function
rb_capture_stock_cancel_signals:
    ldr r0, [r0, #0x4c]
    bx lr
.size rb_capture_stock_cancel_signals, .-rb_capture_stock_cancel_signals

.global rb_capture_stock_wait
.type rb_capture_stock_wait,%function
rb_capture_stock_wait:
    push {r4, lr}
    sub sp, sp, #8
    mov r1, sp
    mvn r2, #0
    bl xm1_pipeline_image_wait_all
    add sp, sp, #8
    pop {r4, pc}
.size rb_capture_stock_wait, .-rb_capture_stock_wait

.global rb_capture_invalid_descriptor
.type rb_capture_invalid_descriptor,%function
rb_capture_invalid_descriptor:
    b rb_capture_invalid_descriptor
.size rb_capture_invalid_descriptor, .-rb_capture_invalid_descriptor

