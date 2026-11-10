.syntax unified
.arm
.section .text.af_select,"ax",%progbits
.global xm1_af_select
.type xm1_af_select,%function
xm1_af_select:
    b xm1_af_select_adapter
.size xm1_af_select, . - xm1_af_select
