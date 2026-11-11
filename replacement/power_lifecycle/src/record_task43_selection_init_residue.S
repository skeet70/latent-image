.syntax unified
.arm
.section .text.xm1_power_record_helper_f16ffc,"ax",%progbits
.global xm1_power_record_helper_f16ffc
.type xm1_power_record_helper_f16ffc,%function
xm1_power_record_helper_f16ffc:
    sub ip, sp, #356
    ldrh r1, [ip, #-2]
    b xm1_power_record_selection_initialize
.size xm1_power_record_helper_f16ffc, .-xm1_power_record_helper_f16ffc
