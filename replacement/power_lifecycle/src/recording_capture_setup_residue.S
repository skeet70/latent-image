.syntax unified
.arm
.text
.align 2
.global xm1_power_recording_capture_initialize_profile
.type xm1_power_recording_capture_initialize_profile,%function
xm1_power_recording_capture_initialize_profile:
    push {r4, lr}
    sub sp, sp, #40
    ldr r1, [sp, #8]
    bl xm1_power_recording_capture_initialize_profile_with_residue
    add sp, sp, #40
    pop {r4, pc}
.size xm1_power_recording_capture_initialize_profile, .-xm1_power_recording_capture_initialize_profile

.align 2
.global xm1_power_recording_capture_post_start
.type xm1_power_recording_capture_post_start,%function
xm1_power_recording_capture_post_start:
    push {r4, lr}
    sub sp, sp, #184
    ldr r12, [sp, #192]
    str r12, [sp]
    ldr r12, [sp, #196]
    str r12, [sp, #4]
    ldr r12, [sp, #164]
    str r12, [sp, #8]
    bl xm1_power_recording_capture_post_start_with_residue
    add sp, sp, #184
    pop {r4, pc}
.size xm1_power_recording_capture_post_start, .-xm1_power_recording_capture_post_start
