.syntax unified
.arm
.macro motion_seam name, address
.global \name
.type \name, %function
\name:
    movw ip, #:lower16:\address
    movt ip, #:upper16:\address
    bx ip
.size \name, .-\name
.endm
motion_seam xm1_motion_config_byte, 0x0069f8fc
motion_seam xm1_motion_config_half, 0x0069fec8
motion_seam xm1_motion_current_task, 0x00723cb8
motion_seam xm1_motion_wait, 0x007243a8
motion_seam xm1_motion_lock, 0x007235d8
motion_seam xm1_motion_unlock, 0x007236b0
motion_seam xm1_motion_release_handle, 0x0064eb20
motion_seam xm1_motion_register_interrupt, 0x0064e918
motion_seam xm1_motion_scheduler, 0x00042634
motion_seam xm1_motion_resource_lock, 0x00724b68
motion_seam xm1_motion_resource_unlock, 0x00724c00
motion_seam xm1_motion_acquire_resource, 0x00649c4c
motion_seam xm1_motion_release_resource, 0x00649ddc
motion_seam xm1_motion_signal, 0x00724558
.global xm1_motion_debug
.type xm1_motion_debug, %function
xm1_motion_debug:
    b xm1_pipeline_image_write_debug_file
.size xm1_motion_debug, .-xm1_motion_debug
