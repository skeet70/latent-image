.syntax unified
.arm
.macro entry name
.section .text.dcf_entry_\name,"ax",%progbits
.global dcf_entry_\name
.type dcf_entry_\name,%function
dcf_entry_\name:
    b xm1_dcf_\name
.size dcf_entry_\name, .-dcf_entry_\name
.endm
entry set_prefix_mode
entry check_name
entry apply_continuous_numbering
entry read_numbers
entry prepare_name
entry note_written
entry read_profile
entry write_profile
entry create_folder
entry last_used
