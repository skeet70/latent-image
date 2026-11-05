@ Owned arm stubs, entered from the stock getter and setter jump tables inside the stock frame,
@ and the accessors: the only stores the owned service makes outside the stack, and its reads of
@ the stock settings images, defaults and configuration bytes.
    .syntax unified
    .arm

@ The getter prologue pushed {r4, r5, r11, lr}; r12 holds the property id and r1, r2 and r3 the
@ bank, defaults flag and slot.
    .section .text.xm1_getter_stub,"ax",%progbits
    .global xm1_getter_stub
    .type xm1_getter_stub,%function
    .p2align 2
xm1_getter_stub:
    mov r0, r12
    bl xm1_property_get
    pop {r4, r5, r11, pc}
    .size xm1_getter_stub, . - xm1_getter_stub

@ The setter prologue pushed {r3-r8, r11, lr} and holds the property, value, bank and slot in r4,
@ r5, r6 and r8, and r7 is free. For a bracketed set the stub calls the stock open 0x61da3c and
@ close 0x61d944 with r0 = 0x96cc64 at the stock frame's sp, as the stock arms do; across them it
@ keeps live only r4-r7 (open) and r4-r6 (close), which the stock arms keep live across the same calls.
    .section .text.xm1_setter_stub,"ax",%progbits
    .global xm1_setter_stub
    .type xm1_setter_stub,%function
    .p2align 2
xm1_setter_stub:
    mov r0, r4
    mov r1, r6
    bl xm1_set_bracketed
    mov r7, r0
    cmp r7, #0
    beq 1f
    movw r0, #0xcc64
    movt r0, #0x96
    bl xm1_stock_61da3c
1:  mov r0, r4
    mov r1, r5
    mov r2, r6
    mov r3, r8
    bl xm1_property_set
    cmp r7, #0
    beq 2f
    movw r0, #0xcc64
    movt r0, #0x96
    bl xm1_stock_61d944
    mov r0, r4
    mov r1, r5
    mov r2, r6
    bl xm1_property_set_after
2:  pop {r3, r4, r5, r6, r7, r8, r11, pc}
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
    accessor xm1_slot_defaults_load8, 0xb804, 0x7c, "ldrb r0, [r12, r0]"
    accessor xm1_slot_defaults_load16, 0xb804, 0x7c, "ldrh r0, [r12, r0]"
    accessor xm1_tail_defaults_load8, 0xb074, 0x81, "ldrb r0, [r12, r0]"
    accessor xm1_tail_defaults_load16, 0xb074, 0x81, "ldrh r0, [r12, r0]"
    accessor xm1_config_load8, 0xb400, 0x96, "ldrb r0, [r12, r0]"
