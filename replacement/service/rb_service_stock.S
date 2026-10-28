@ FUN_00d52684(field, row); the bank ELF allowlist resolves the target from
@ the constant this sequence builds.
.syntax unified
.arm
.section .text,"ax",%progbits

.global rb_call_stock_probe
.type rb_call_stock_probe,%function
rb_call_stock_probe:
    @ The 8-byte frame holds the caller's r12 and lr; r4-r11 are not part of it.
    push    {r12, lr}
    movw    r12, #0x2684
    movt    r12, #0x00d5
    blx     r12
    pop     {r12, pc}
.size rb_call_stock_probe, .-rb_call_stock_probe
