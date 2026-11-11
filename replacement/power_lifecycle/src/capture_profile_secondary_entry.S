.syntax unified
.arm
.text
.balign 4
.global xm1_power_record_helper_738800
.type xm1_power_record_helper_738800, %function
xm1_power_record_helper_738800:
    sub sp, sp, #64
    str lr, [sp, #60]
    add r3, sp, #12
    mov r12, #0
    str r12, [r3]
    str r12, [r3, #4]
    str r12, [r3, #8]
    str r12, [r3, #12]
    bl xm1_power_capture_profile_secondary_with_residue
    ldr lr, [sp, #60]
    add sp, sp, #64
    bx lr
.size xm1_power_record_helper_738800, .-xm1_power_record_helper_738800
.section .note.GNU-stack,"",%progbits
