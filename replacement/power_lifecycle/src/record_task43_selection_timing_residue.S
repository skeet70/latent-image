.syntax unified
.arm
.text
.align 2
.global xm1_power_record_helper_f19634
.type xm1_power_record_helper_f19634,%function
xm1_power_record_helper_f19634:
    push {r4-r11, lr}
    add r11, sp, #32
    sub sp, sp, #756
    sub r0, r11, #296
    bl xm1_power_record_timing_publish_cpu
    sub sp, r11, #32
    pop {r4-r11, pc}
.size xm1_power_record_helper_f19634, .-xm1_power_record_helper_f19634
