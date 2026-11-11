.syntax unified
.arm
.section .text.xm1_font_division_fault,"ax",%progbits
.balign 4
.global xm1_font_division_fault
.type xm1_font_division_fault,%function
xm1_font_division_fault:
    movw r12, #0x3604
    movt r12, #0x4
    bx r12
.size xm1_font_division_fault, .-xm1_font_division_fault
