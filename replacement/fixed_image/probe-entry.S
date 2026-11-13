.syntax unified
.arm
.section .text.entry,"ax",%progbits
.global xm1_fixed_test_entry
.type xm1_fixed_test_entry,%function
xm1_fixed_test_entry:
    push {r4, r5, r11, lr}
    push {r0-r3, r12, lr}
    bl xm1_fixed_test_observe
    pop {r0-r3, r12, lr}
    b xm1_fixed_test_resume
.size xm1_fixed_test_entry, .-xm1_fixed_test_entry
