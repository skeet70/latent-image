.syntax unified
.arm
.text
.balign 4
.global xm1_power_record_helper_73964c
.type xm1_power_record_helper_73964c, %function
xm1_power_record_helper_73964c:
    sub sp, sp, #80
    str r4, [sp, #72]
    str lr, [sp, #76]
    ldr r12, [sp, #32]
    str r12, [sp, #40]
    ldr r12, [sp, #36]
    str r12, [sp, #44]
    ldr r12, [sp, #80]
    str r12, [sp]
    ldr r12, [sp, #84]
    str r12, [sp, #4]
    ldr r12, [sp, #88]
    str r12, [sp, #8]
    ldr r12, [sp, #92]
    str r12, [sp, #12]
    ldr r12, [sp, #96]
    str r12, [sp, #16]
    ldr r12, [sp, #100]
    str r12, [sp, #20]
    add r12, sp, #40
    str r12, [sp, #24]
    bl xm1_power_capture_profile_primary_with_residue
    ldr r4, [sp, #72]
    ldr lr, [sp, #76]
    add sp, sp, #80
    bx lr
.size xm1_power_record_helper_73964c, .-xm1_power_record_helper_73964c
.section .note.GNU-stack,"",%progbits
