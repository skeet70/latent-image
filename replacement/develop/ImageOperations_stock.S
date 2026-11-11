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

endpoint image_conversion_device_acquire, 0x00681478
endpoint image_conversion_device_release, 0x0068150c
endpoint image_conversion_device_reset, 0x0068157c
endpoint image_conversion_device_begin, 0x006815b8
endpoint image_conversion_device_primary, 0x00681f44
endpoint image_conversion_device_secondary, 0x00682544
endpoint image_conversion_device_window_primary, 0x0068303c
endpoint image_conversion_device_window_secondary, 0x00683408
endpoint image_conversion_device_enable, 0x006837d4
endpoint image_conversion_register_interrupt, 0x0064e918
endpoint image_conversion_release_interrupt, 0x0064eb20
endpoint image_conversion_prepare_clock, 0x00042634
endpoint xm1_pipeline_image_trace, 0x00042188
endpoint image_dma_acquire, 0x00665304
endpoint image_dma_release, 0x00665424
endpoint image_dma_configure, 0x00665ac4
endpoint image_dma_begin, 0x00665540
endpoint image_operation_divide_fault, 0x00043614
