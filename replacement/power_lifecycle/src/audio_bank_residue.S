.syntax unified
.arm
.text
.balign 4
.global xm1_power_audio_bank_complete
.type xm1_power_audio_bank_complete,%function
xm1_power_audio_bank_complete:
    push {r4,r11,lr}
    add r11,sp,#8
    sub sp,sp,#84
    sub r0,r11,#88
    bl xm1_power_audio_bank_complete_backed
    sub sp,r11,#8
    pop {r4,r11,pc}
.size xm1_power_audio_bank_complete,.-xm1_power_audio_bank_complete
.balign 4
.global xm1_power_audio_bank_notify
.type xm1_power_audio_bank_notify,%function
xm1_power_audio_bank_notify:
    push {r11,lr}
    add r11,sp,#4
    sub sp,sp,#80
    mov r1,r0
    sub r0,r11,#80
    bl xm1_power_audio_bank_notify_backed
    sub sp,r11,#4
    pop {r11,pc}
.size xm1_power_audio_bank_notify,.-xm1_power_audio_bank_notify
.balign 4
.global xm1_power_audio_bank_notify_unchanged
.type xm1_power_audio_bank_notify_unchanged,%function
xm1_power_audio_bank_notify_unchanged:
    push {r4,r11,lr}
    add r11,sp,#8
    sub sp,sp,#84
    mov r1,r0
    sub r0,r11,#88
    bl xm1_power_audio_bank_notify_backed
    sub sp,r11,#8
    pop {r4,r11,pc}
.size xm1_power_audio_bank_notify_unchanged,.-xm1_power_audio_bank_notify_unchanged
