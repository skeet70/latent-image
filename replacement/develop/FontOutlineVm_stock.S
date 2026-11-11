.syntax unified
.arm
.section .text,"ax",%progbits

.global xm1_pipeline_image_font_render_outline_program
.type xm1_pipeline_image_font_render_outline_program,%function
xm1_pipeline_image_font_render_outline_program:
    push {r4-r11, lr}
    mov r3, r6
    b .Lfont_outline_capture
.global xm1_pipeline_image_font_render_outline_program_child
.type xm1_pipeline_image_font_render_outline_program_child,%function
xm1_pipeline_image_font_render_outline_program_child:
    push {r4-r11, lr}
.Lfont_outline_capture:
    sub sp, sp, #44
    str r3, [sp]
    ldr r12, [sp, #20]
    str r12, [sp, #4]
    ldr r12, [sp, #24]
    str r12, [sp, #8]
    ldr r12, [sp, #28]
    str r12, [sp, #12]
    ldr r12, [sp, #32]
    str r12, [sp, #16]
    ldr r12, [sp, #36]
    str r12, [sp, #20]
    ldr r12, [sp, #40]
    str r12, [sp, #24]
    mov r3, sp
    bl xm1_pipeline_image_font_render_outline_program_state
    add sp, sp, #44
    pop {r4-r11, pc}
.size xm1_pipeline_image_font_render_outline_program, .-xm1_pipeline_image_font_render_outline_program

.macro ORIGINAL name, address
.global \name
.type \name,%function
\name:
    movw r12, #(\address & 0xffff)
    movt r12, #((\address >> 16) & 0xffff)
    bx r12
.size \name, .-\name
.endm

ORIGINAL xm1_font_outline_original_render, 0x01198598
ORIGINAL xm1_font_outline_original_execute, 0x01199868
ORIGINAL xm1_font_outline_original_emit, 0x01199440
