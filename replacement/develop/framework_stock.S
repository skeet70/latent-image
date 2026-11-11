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

service xm1_develop_stock_trace, 0x00042188
service xm1_develop_stock_finish0, 0x00e90628
service xm1_develop_stock_finish1, 0x00e90508
service xm1_develop_stock_finish3, 0x00e8fbe4
service xm1_develop_stock_finish4, 0x00e8fee4

.section .text.xm1_develop_invoke_callback,"ax",%progbits
.balign 4
.global xm1_develop_invoke_callback
.type xm1_develop_invoke_callback,%function
xm1_develop_invoke_callback:
    push {r4, lr}
    blx r3
    pop {r4, pc}
.size xm1_develop_invoke_callback, .-xm1_develop_invoke_callback
