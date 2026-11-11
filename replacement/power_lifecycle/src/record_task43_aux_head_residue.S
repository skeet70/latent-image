.syntax unified
.arm
.text
.align 2
.global xm1_power_record_helper_f33a18
.type xm1_power_record_helper_f33a18,%function
xm1_power_record_helper_f33a18:
    push {r4, lr}
    sub sp, sp, #280
    add r0, sp, #284
    bl xm1_power_record_aux_head_f33a18_frame
    add sp, sp, #280
    pop {r4, pc}
.size xm1_power_record_helper_f33a18, .-xm1_power_record_helper_f33a18

.align 2
.global xm1_power_record_helper_f33ef4
.type xm1_power_record_helper_f33ef4,%function
xm1_power_record_helper_f33ef4:
    push {r4, lr}
    sub sp, sp, #440
    add r0, sp, #444
    bl xm1_power_record_aux_head_f33ef4_frame
    add sp, sp, #440
    pop {r4, pc}
.size xm1_power_record_helper_f33ef4, .-xm1_power_record_helper_f33ef4

.align 2
.global xm1_power_record_helper_f34710
.type xm1_power_record_helper_f34710,%function
xm1_power_record_helper_f34710:
    push {r4, lr}
    sub sp, sp, #1568
    sub sp, sp, #8
    add r0, sp, #1568
    add r0, r0, #12
    bl xm1_power_record_aux_head_f34710_frame
    add sp, sp, #1568
    add sp, sp, #8
    pop {r4, pc}
.size xm1_power_record_helper_f34710, .-xm1_power_record_helper_f34710

.align 2
.global xm1_power_record_helper_f3382c
.type xm1_power_record_helper_f3382c,%function
xm1_power_record_helper_f3382c:
    push {r4, lr}
    sub sp, sp, #152
    add r1, sp, #156
    bl xm1_power_record_aux_head_f3382c_frame
    add sp, sp, #152
    pop {r4, pc}
.size xm1_power_record_helper_f3382c, .-xm1_power_record_helper_f3382c
