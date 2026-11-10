.syntax unified
.arm
.section .text.retained,"ax",%progbits
.macro retained name, address
.global \name
.type \name,%function
\name:
    movw ip, #(\address & 0xffff)
    movt ip, #(\address >> 16)
    bx ip
.size \name, .-\name
.endm
retained retained_primary, 0x006a9e98
retained retained_backup, 0x006a9c70
retained retained_fixed, 0x006a9ac4
retained retained_copy, 0x0074213c
retained retained_wb_convert, 0x00ee5ec0
retained retained_wb_publish, 0x00eea6a8
retained retained_halfword, 0x00eea6f8
retained retained_lock, 0x00ee418c
retained retained_rebuild, 0x00ee364c
