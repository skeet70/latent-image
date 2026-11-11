.syntax unified
.arm

.section .text.xm1_pipeline_image_utf16_load,"ax",%progbits
.balign 4
.global xm1_pipeline_image_utf16_load
.type xm1_pipeline_image_utf16_load,%function
xm1_pipeline_image_utf16_load:
    ldrh r0, [r0]
    bx lr
.size xm1_pipeline_image_utf16_load, .-xm1_pipeline_image_utf16_load

.section .text.xm1_pipeline_image_utf16_store,"ax",%progbits
.balign 4
.global xm1_pipeline_image_utf16_store
.type xm1_pipeline_image_utf16_store,%function
xm1_pipeline_image_utf16_store:
    strh r1, [r0]
    bx lr
.size xm1_pipeline_image_utf16_store, .-xm1_pipeline_image_utf16_store

.section .text.xm1_pipeline_image_date_dispatch_trace,"ax",%progbits
.balign 4
.global xm1_pipeline_image_date_dispatch_trace
.type xm1_pipeline_image_date_dispatch_trace,%function
xm1_pipeline_image_date_dispatch_trace:
    movw r12, #0x1a34
    movt r12, #0x0004
    bx r12
.size xm1_pipeline_image_date_dispatch_trace, .-xm1_pipeline_image_date_dispatch_trace

.section .text.xm1_pipeline_image_font_notification_trace,"ax",%progbits
.balign 4
.global xm1_pipeline_image_font_notification_trace
.type xm1_pipeline_image_font_notification_trace,%function
xm1_pipeline_image_font_notification_trace:
    movw r12, #0x2188
    movt r12, #0x0004
    bx r12
.size xm1_pipeline_image_font_notification_trace, .-xm1_pipeline_image_font_notification_trace

.section .text.xm1_pipeline_image_ui_font_notification,"ax",%progbits
.balign 4
.global xm1_pipeline_image_ui_font_notification
.type xm1_pipeline_image_ui_font_notification,%function
xm1_pipeline_image_ui_font_notification:
    push {r4, lr}
    sub r3, sp, #76
    sub sp, sp, #160
    mov r2, sp
    mov r4, #17
1:
    ldr r12, [r3], #4
    str r12, [r2], #4
    subs r4, r4, #1
    bne 1b
    mov r2, sp
    bl xm1_pipeline_image_ui_font_notification_from_entry
    add sp, sp, #160
    pop {r4, pc}
.size xm1_pipeline_image_ui_font_notification, .-xm1_pipeline_image_ui_font_notification
