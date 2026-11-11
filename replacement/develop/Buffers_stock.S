.syntax unified
.arm
.section .text.xm1_buffers_cache_endpoint,"ax",%progbits
.balign 4
.global xm1_buffers_cache_endpoint
.type xm1_buffers_cache_endpoint,%function
xm1_buffers_cache_endpoint:
    movw r12, #0x2634
    movt r12, #0x0004
    bx r12
.size xm1_buffers_cache_endpoint, .-xm1_buffers_cache_endpoint

.section .text.xm1_develop_trace_event,"ax",%progbits
.balign 4
.global xm1_develop_trace_event
.type xm1_develop_trace_event,%function
xm1_develop_trace_event:
    movw r12, #0x2188
    movt r12, #0x0004
    bx r12
.size xm1_develop_trace_event, .-xm1_develop_trace_event
