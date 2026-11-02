    .syntax unified
    .arm
    .global xm1_application_prehook
    .set xm1_application_prehook, 0xbfb1b8
    .global xm1_application_normalizer
    .set xm1_application_normalizer, 0xbfb20c
    .global xm1_application_posthook
    .set xm1_application_posthook, 0xbfb1e0
    .section .note.GNU-stack, "", %progbits
