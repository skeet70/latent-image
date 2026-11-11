.syntax unified
.arm
.section .text,"ax",%progbits

.macro preparation_service name, address
.global \name
.type \name,%function
\name:
    movw r12, #(\address & 0xffff)
    movt r12, #((\address >> 16) & 0xffff)
    bx r12
.size \name, .-\name
.endm

preparation_service xm1_preparation_configuration_byte, 0x0069f8fc
preparation_service xm1_preparation_configuration_half, 0x0069fec8
preparation_service xm1_preparation_signed_divide, 0x0089aa90
preparation_service xm1_preparation_unsigned_divide, 0x0004d978
