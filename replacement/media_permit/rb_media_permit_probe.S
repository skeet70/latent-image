@ Test-only transfer into the retired resolver body.

.syntax unified
.arm
.section .text.rb_mp_forbidden_body_probe,"ax",%progbits
.global rb_mp_forbidden_body_probe
.type rb_mp_forbidden_body_probe,%function
rb_mp_forbidden_body_probe:
    movw r12, #0x716c
    movt r12, #0x00e7
    bx r12
.size rb_mp_forbidden_body_probe, .-rb_mp_forbidden_body_probe
