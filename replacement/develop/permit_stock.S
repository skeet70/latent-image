.syntax unified
.arm
.section .text,"ax",%progbits

.macro XM1_DP_STOCK name, address
.global \name
.type \name,%function
\name:
    movw r12, #(\address & 0xffff)
    movt r12, #((\address >> 16) & 0xffff)
    bx r12
.size \name, .-\name
.endm

XM1_DP_STOCK xm1_dp_trace_task, 0x00e86d30
XM1_DP_STOCK xm1_dp_trace_text, 0x00726658
XM1_DP_STOCK xm1_dp_receive, 0x00e8659c
XM1_DP_STOCK xm1_dp_dispatch, 0x00e87404
XM1_DP_STOCK xm1_dp_mirror, 0x00e88e1c
XM1_DP_STOCK xm1_dp_selected, 0x00e871c4
XM1_DP_STOCK xm1_dp_capture_frame, 0x00e88bc0
XM1_DP_STOCK xm1_dp_processing_kind, 0x00e75490
XM1_DP_STOCK xm1_dp_subframe, 0x00e74d40
XM1_DP_STOCK xm1_dp_ready, 0x00eb0f38
XM1_DP_STOCK xm1_dp_first_of_kind, 0x00e87058
XM1_DP_STOCK xm1_dp_last_capture, 0x00e748c8
XM1_DP_STOCK xm1_dp_sequence_frame, 0x00e7edd0
XM1_DP_STOCK xm1_dp_multi_exposure, 0x00e7c1e0
XM1_DP_STOCK xm1_dp_kind_group, 0x00e73a74
XM1_DP_STOCK xm1_dp_remap, 0x00e74a34
