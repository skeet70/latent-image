.syntax unified
.arm

.macro service name, address
.section .text.\name,"ax",%progbits
.balign 4
.global \name
.type \name,%function
\name:
    movw r12, #(\address & 0xffff)
    movt r12, #((\address >> 16) & 0xffff)
    bx r12
.size \name, .-\name
.endm

service processing_storage_snapshot, 0x007101e4

.section .text.xm1_develop_pipeline_resize_preview,"ax",%progbits
.balign 4
.global xm1_develop_pipeline_resize_preview
.type xm1_develop_pipeline_resize_preview,%function
xm1_develop_pipeline_resize_preview:
    sub r12, sp, #0x50
    push {r4-r11, lr}
    ldm r12, {r1-r7}
    sub sp, sp, #28
    stm sp, {r1-r7}
    mov r1, sp
    bl xm1_develop_pipeline_resize_preview_snapshot
    add sp, sp, #28
    pop {r4-r11, pc}
.size xm1_develop_pipeline_resize_preview, .-xm1_develop_pipeline_resize_preview
