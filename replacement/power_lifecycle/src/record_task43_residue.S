.syntax unified
.arm
.text
.align 2
.global xm1_power_record_helper_f46c20
.type xm1_power_record_helper_f46c20, %function
xm1_power_record_helper_f46c20:
    push {r11, lr}
    add r11, sp, #4
    movw r0, #0x738c
    movt r0, #0xf4
    bl .Lrecord_task43_log
    pop {r11, pc}
.size xm1_power_record_helper_f46c20, .-xm1_power_record_helper_f46c20

.type .Lrecord_task43_log, %function
.Lrecord_task43_log:
    push {r4, r11, lr}
    add r11, sp, #8
    sub sp, sp, #84
    mov r4, r0
    movw r3, #0x91b
    str r3, [r11, #-88]
    mov r3, #0
    str r3, [r11, #-84]
    sub r0, r11, #88
    mov r1, r4
    bl xm1_power_record_task43_log_backed
    sub sp, r11, #8
    pop {r4, r11, pc}
.size .Lrecord_task43_log, .-.Lrecord_task43_log

.align 2
.global xm1_power_record_helper_f14824
.type xm1_power_record_helper_f14824, %function
xm1_power_record_helper_f14824:
    push {r4, r5, r11, lr}
    add r11, sp, #12
    sub sp, sp, #48
    mov r3, #1
    str r3, [r11, #-20]
    mov r3, #0
    strh r3, [r11, #-16]
    sub r1, r11, #20
    sub r2, r11, #60
    sub r3, r11, #44
    bl xm1_power_record_task43_initialize_backed
    sub sp, r11, #12
    pop {r4, r5, r11, pc}
.size xm1_power_record_helper_f14824, .-xm1_power_record_helper_f14824
