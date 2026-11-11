.syntax unified
.arm
.text
.balign 4
.global xm1_power_record_helper_f17838
.type xm1_power_record_helper_f17838,%function
xm1_power_record_helper_f17838:
    push {r4,r5,r11,lr}
    add r11,sp,#12
    sub sp,sp,#24
    sub r1,r11,#20
    bl xm1_power_record_helper_f17838_backed
    sub sp,r11,#12
    pop {r4,r5,r11,pc}
.size xm1_power_record_helper_f17838,.-xm1_power_record_helper_f17838
.balign 4
.global xm1_power_record_helper_f17984
.type xm1_power_record_helper_f17984,%function
xm1_power_record_helper_f17984:
    push {r4,r5,r6,r11,lr}
    add r11,sp,#16
    sub sp,sp,#28
    sub r2,r11,#28
    bl xm1_power_record_helper_f17984_backed
    sub sp,r11,#16
    pop {r4,r5,r6,r11,pc}
.size xm1_power_record_helper_f17984,.-xm1_power_record_helper_f17984
