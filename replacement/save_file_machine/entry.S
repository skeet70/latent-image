.syntax unified
.arm
.macro entry name
.section .text.sfm_entry_\name,"ax",%progbits
.global sfm_entry_\name
.type sfm_entry_\name,%function
sfm_entry_\name:
    b xm1_sfm_\name
.size sfm_entry_\name, .-sfm_entry_\name
.endm
entry on_deleted
entry on_closed
entry on_written
entry on_positioned
entry on_opened
entry begin
entry begin_append
entry request_cancel
entry append_buffer
entry set_output_mode
entry copy_name
entry set_name
