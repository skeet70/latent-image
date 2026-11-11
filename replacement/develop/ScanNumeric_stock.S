.syntax unified
.arm

.section .text.xm1_pipeline_image_scan_normalize_samples_tail,"ax",%progbits
.balign 4
.global xm1_pipeline_image_scan_normalize_samples_tail
.type xm1_pipeline_image_scan_normalize_samples_tail,%function
xm1_pipeline_image_scan_normalize_samples_tail:
    str r1, [sp, #4]
    str r2, [sp, #8]
    str r3, [sp, #12]
    add sp, sp, #16
    b xm1_pipeline_image_scan_normalize_samples
.size xm1_pipeline_image_scan_normalize_samples_tail, .-xm1_pipeline_image_scan_normalize_samples_tail

.section .text.scan_numeric_arithmetic_fault,"ax",%progbits
.balign 4
.global scan_numeric_arithmetic_fault
.type scan_numeric_arithmetic_fault,%function
scan_numeric_arithmetic_fault:
    svc 0x1010
1:
    b 1b
.size scan_numeric_arithmetic_fault, .-scan_numeric_arithmetic_fault
