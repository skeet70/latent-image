.syntax unified
.arm

.macro service name, address
.section .text.\name,"ax",%progbits
.balign 4
.global \name
.type \name,%function
\name:
    movw r12, #(\address & 0xffff)
    movt r12, #((\address >> 16) & 0xffff)
    bx r12
.size \name, .-\name
.endm

service xm1_warp_barrier, 0x00042634
service xm1_warp_save_irq, 0x00042574
service xm1_warp_restore_irq, 0x0004257c
service xm1_warp_kernel_diagnostic, 0x000422e0
