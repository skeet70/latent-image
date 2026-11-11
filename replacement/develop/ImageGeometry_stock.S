.syntax unified
.arm

.macro endpoint name, address
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

endpoint xm1_pipeline_geometry_cache_whole, 0x00042634
endpoint xm1_pipeline_geometry_divide_fault, 0x00043614
endpoint xm1_pipeline_geometry_crop_reset_wait, 0x00683958
