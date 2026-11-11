.syntax unified
.arm
.text
.balign 4
.global xm1_power_settings_reconstruct
.type xm1_power_settings_reconstruct,%function
xm1_power_settings_reconstruct:
    push {r4,r5,r6,r11,lr}
    add r11,sp,#16
    sub sp,sp,#52
    sub r0,r11,#28
    bl xm1_power_settings_reconstruct_backed
    sub sp,r11,#16
    pop {r4,r5,r6,r11,pc}
.size xm1_power_settings_reconstruct,.-xm1_power_settings_reconstruct
.balign 4
.global xm1_power_settings_output
.type xm1_power_settings_output,%function
xm1_power_settings_output:
    push {r4,r11,lr}
    add r11,sp,#8
    sub sp,sp,#84
    mov r4,r0
    bl xm1_power_settings_output_copy
    movw r3,#0x832
    str r3,[r11,#-88]
    mov r3,#0
    str r3,[r11,#-84]
    sub r0,r11,#88
    bl xm1_power_settings_output_post
    sub sp,r11,#8
    pop {r4,r11,pc}
.size xm1_power_settings_output,.-xm1_power_settings_output
