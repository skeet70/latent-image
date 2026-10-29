@ Tail veneers from the owned save facade to the retained image-5 services.

.syntax unified
.arm
.section .text,"ax",%progbits

.macro SAVE_TAIL name, lo, hi
.global \name
.type \name,%function
\name:
    movw    r12, #\lo
    movt    r12, #\hi
    bx      r12
.size \name, .-\name
.endm

SAVE_TAIL rb_save_service_present,       0xabec, 0x0073
SAVE_TAIL rb_save_service_profile,       0xac60, 0x0073
SAVE_TAIL rb_save_service_byte,          0xf8fc, 0x0069
SAVE_TAIL rb_save_prepare_metadata,      0xe0b4, 0x00e4
SAVE_TAIL rb_save_extent_preset,         0x5850, 0x011d
SAVE_TAIL rb_save_extent_name,           0x5934, 0x011d
SAVE_TAIL rb_save_extent_copy,           0x591c, 0x011d
SAVE_TAIL rb_save_sample_cached_state,   0x01e4, 0x0071
SAVE_TAIL rb_save_storage_direct,        0x5600, 0x011d
SAVE_TAIL rb_save_storage_kind_three,    0x5688, 0x011d
SAVE_TAIL rb_save_register_release,      0x5714, 0x011d
SAVE_TAIL rb_save_current_task,          0x3cb8, 0x0072
SAVE_TAIL rb_save_wait,                  0x43a8, 0x0072
SAVE_TAIL rb_save_signal,                0x4558, 0x0072
SAVE_TAIL rb_save_stage,                 0xae80, 0x006f
