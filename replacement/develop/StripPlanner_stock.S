.syntax unified
.arm
.section .text,"ax",%progbits

.macro STRIP_DEVICE name, address
.global \name
.type \name,%function
\name:
    movw r12, #(\address & 0xffff)
    movt r12, #((\address >> 16) & 0xffff)
    bx r12
.size \name, .-\name
.endm

STRIP_DEVICE strip_device_paired, 0x00666a0c
STRIP_DEVICE strip_device_extended, 0x00666cb4
STRIP_DEVICE strip_device_primary, 0x00666f98
STRIP_DEVICE strip_device_secondary, 0x00667128
STRIP_DEVICE strip_device_simple, 0x00667498
STRIP_DEVICE strip_device_profile, 0x006677c8
STRIP_DEVICE strip_device_alternate, 0x00668144
STRIP_DEVICE strip_device_reset, 0x006689d4
STRIP_DEVICE strip_cache_sync, 0x00042634
