.syntax unified
.arm
.section .text.crop_process_retained,"ax",%progbits
.balign 4
.global xm1_pipeline_image_copy_reset
.type xm1_pipeline_image_copy_reset,%function
xm1_pipeline_image_copy_reset:
    ldr pc, 1f
1:  .word 0x0068d954
.size xm1_pipeline_image_copy_reset, .-xm1_pipeline_image_copy_reset
.global xm1_pipeline_image_copy_cache_flush
.type xm1_pipeline_image_copy_cache_flush,%function
xm1_pipeline_image_copy_cache_flush:
    ldr pc, 2f
2:  .word 0x00042634
.size xm1_pipeline_image_copy_cache_flush, .-xm1_pipeline_image_copy_cache_flush
.section .note.GNU-stack,"",%progbits
