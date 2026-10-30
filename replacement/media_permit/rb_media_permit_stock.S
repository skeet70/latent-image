@ X-M1 1.50 image 5 retained services called by the owned permit module.

.syntax unified
.arm
.section .text,"ax",%progbits

.macro RB_MP_TRAMPOLINE name, addr
.global \name
.type \name,%function
\name:
    movw r12, #(\addr & 0xffff)
    movt r12, #((\addr >> 16) & 0xffff)
    bx r12
.size \name, .-\name
.endm

RB_MP_TRAMPOLINE rb_mp_stock_mirror_read, 0x00e88e1c
RB_MP_TRAMPOLINE rb_mp_stock_frame_kind, 0x00e75490
RB_MP_TRAMPOLINE rb_mp_stock_bit_set, 0x00eb0ea0
RB_MP_TRAMPOLINE rb_mp_stock_context, 0x00e87ad4
RB_MP_TRAMPOLINE rb_mp_stock_subframe_count, 0x00e74c68
RB_MP_TRAMPOLINE rb_mp_stock_subframe, 0x00e748c8
RB_MP_TRAMPOLINE rb_mp_stock_map_subframe, 0x00e886ec
RB_MP_TRAMPOLINE rb_mp_stock_count_kind, 0x00e87114
RB_MP_TRAMPOLINE rb_mp_stock_frame_of_kind, 0x00e88804
RB_MP_TRAMPOLINE rb_mp_stock_command, 0x00e8d82c
RB_MP_TRAMPOLINE rb_mp_stock_cancel, 0x00e6e368
RB_MP_TRAMPOLINE rb_mp_stock_fatal, 0x00e881e8
