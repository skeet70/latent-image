.syntax unified
.arm
.section .text.xm1_pipeline_image_motion,"ax",%progbits
.global xm1_pipeline_image_motion
.type xm1_pipeline_image_motion,%function
xm1_pipeline_image_motion:
    push {r4, lr}
    ldr r4, [sp, #-120]
    sub sp, sp, #48
    ldr ip, [sp, #56]
    str ip, [sp]
    ldr ip, [sp, #60]
    str ip, [sp, #4]
    ldr ip, [sp, #64]
    str ip, [sp, #8]
    ldr ip, [sp, #68]
    str ip, [sp, #12]
    ldr ip, [sp, #72]
    str ip, [sp, #16]
    ldr ip, [sp, #76]
    str ip, [sp, #20]
    ldr ip, [sp, #80]
    str ip, [sp, #24]
    ldr ip, [sp, #84]
    str ip, [sp, #28]
    ldr ip, [sp, #88]
    str ip, [sp, #32]
    ldr ip, [sp, #92]
    str ip, [sp, #36]
    ldr ip, [sp, #96]
    str ip, [sp, #40]
    str r4, [sp, #44]
    bl xm1_pipeline_image_motion_owned
    add sp, sp, #48
    pop {r4, pc}
.size xm1_pipeline_image_motion, .-xm1_pipeline_image_motion
