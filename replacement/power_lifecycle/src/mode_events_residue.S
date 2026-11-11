.syntax unified
.arm
.text
.align 2
.global xm1_power_application_post
.type xm1_power_application_post, %function
xm1_power_application_post:
    push {r4, lr}
    sub sp, sp, #160
    mov r2, sp
    add r3, sp, #80
    bl xm1_power_application_post_backed
    add sp, sp, #160
    pop {r4, pc}
.size xm1_power_application_post, .-xm1_power_application_post
