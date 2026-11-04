@ Owned arm stubs, entered from the stock getter and setter jump tables inside the stock frame,
@ and the only stores the owned service makes outside the stack.
    .syntax unified
    .arm

@ The getter prologue saved r4 and r5, so r4 carries the property id across the call; the shared
@ exit 0x624400 compares r12 with the properties it adjusts.
    .section .text.xm1_getter_stub,"ax",%progbits
    .global xm1_getter_stub
    .type xm1_getter_stub,%function
    .p2align 2
xm1_getter_stub:
    mov r4, r12
    mov r0, r12
    bl xm1_property_get
    mov r12, r4
    b xm1_getter_exit
    .size xm1_getter_stub, . - xm1_getter_stub

    .section .text.xm1_setter_stub,"ax",%progbits
    .global xm1_setter_stub
    .type xm1_setter_stub,%function
    .p2align 2
xm1_setter_stub:
    mov r0, r4
    mov r1, r5
    mov r2, r6
    mov r3, r7
    bl xm1_property_set
    b xm1_setter_exit
    .size xm1_setter_stub, . - xm1_setter_stub

    .macro accessor name, base_low, base_high, op
    .section .text.\name,"ax",%progbits
    .global \name
    .type \name,%function
    .p2align 2
\name:
    movw r12, #\base_low
    movt r12, #\base_high
    \op
    bx lr
    .size \name, . - \name
    .endm

    accessor xm1_bank_load8, 0xcc64, 0x96, "ldrb r0, [r12, r0]"
    accessor xm1_bank_load16, 0xcc64, 0x96, "ldrh r0, [r12, r0]"
    accessor xm1_bank_store8, 0xcc64, 0x96, "strb r1, [r12, r0]"
    accessor xm1_bank_store16, 0xcc64, 0x96, "strh r1, [r12, r0]"
    accessor xm1_save_load8, 0xec00, 0x89, "ldrb r0, [r12, r0]"
    accessor xm1_save_load16, 0xec00, 0x89, "ldrh r0, [r12, r0]"
    accessor xm1_save_store8, 0xec00, 0x89, "strb r1, [r12, r0]"
    accessor xm1_save_store16, 0xec00, 0x89, "strh r1, [r12, r0]"
