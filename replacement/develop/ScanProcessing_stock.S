.syntax unified
.arm
.section .text.xm1_pipeline_image_scan_divide_fault,"ax",%progbits
.balign 4
.global xm1_pipeline_image_scan_divide_fault
.type xm1_pipeline_image_scan_divide_fault,%function
xm1_pipeline_image_scan_divide_fault:
    movw r12, #0x3614
    movt r12, #0x0004
    bx r12
.size xm1_pipeline_image_scan_divide_fault, .-xm1_pipeline_image_scan_divide_fault
