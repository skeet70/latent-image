    .syntax unified
    .arm

    .section .text.xm1_saclac_dispatch_entry,"ax",%progbits
    .global xm1_saclac_dispatch_entry
    .type xm1_saclac_dispatch_entry,%function
    .p2align 2
xm1_saclac_dispatch_entry:
    b xm1_saclac_dispatch
    .size xm1_saclac_dispatch_entry, . - xm1_saclac_dispatch_entry

    .section .text.xm1_saclac_router_entry,"ax",%progbits
    .global xm1_saclac_router_entry
    .type xm1_saclac_router_entry,%function
    .p2align 2
xm1_saclac_router_entry:
    b xm1_saclac_router
    .size xm1_saclac_router_entry, . - xm1_saclac_router_entry
