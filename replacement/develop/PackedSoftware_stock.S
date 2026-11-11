.syntax unified
.arm

.macro cache name, address
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

cache xm1_pipeline_packed_cache_read, 0x00042634
cache xm1_pipeline_packed_cache_write, 0x0004263c
