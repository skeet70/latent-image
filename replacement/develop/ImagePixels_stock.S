.syntax unified
.arm
.section .text.image_pixels_endpoints,"ax",%progbits
.macro endpoint name, address
.global \name
.type \name,%function
\name:
    ldr pc, =\address
.size \name, .-\name
.endm
endpoint image_pixels_cache, 0x00eb0d04
endpoint image_pixels_arithmetic_fault, 0x00043614
