.syntax unified
.arm
.section .text.xm1_pipeline_image_output_arithmetic_fault,"ax",%progbits
.balign 4
.global xm1_pipeline_image_output_arithmetic_fault
.type xm1_pipeline_image_output_arithmetic_fault,%function
xm1_pipeline_image_output_arithmetic_fault:
    movw r12, #0x3614
    movt r12, #0x0004
    bx r12
.size xm1_pipeline_image_output_arithmetic_fault, .-xm1_pipeline_image_output_arithmetic_fault

.section .text.xm1_pipeline_image_overlay_trace,"ax",%progbits
.balign 4
.global xm1_pipeline_image_overlay_trace
.type xm1_pipeline_image_overlay_trace,%function
xm1_pipeline_image_overlay_trace:
    movw r12, #0x2188
    movt r12, #0x0004
    bx r12
.size xm1_pipeline_image_overlay_trace, .-xm1_pipeline_image_overlay_trace
