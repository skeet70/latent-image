.syntax unified
.arm

.macro owned name, target
.section .text.\name,"ax",%progbits
.balign 4
.global \name
.type \name,%function
\name:
    b \target
.size \name, .-\name
.endm

owned output_jpeg_mpf_scatter, xm1_pipeline_image_build_mpf_scatter
owned output_jpeg_build_raf, xm1_pipeline_image_build_raf_header
owned output_jpeg_populate_metadata, xm1_pipeline_image_populate_output_metadata
owned output_jpeg_special_metadata, xm1_pipeline_image_populate_special_metadata
owned output_jpeg_prepare_raf, xm1_pipeline_image_prepare_raf_metadata
owned output_jpeg_begin_raf, xm1_pipeline_image_begin_raf_header
owned output_jpeg_prepare_image, xm1_pipeline_image_prepare_jpeg_output
