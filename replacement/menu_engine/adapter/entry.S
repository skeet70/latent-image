.syntax unified
.arm

@ The published branch at 0x00bf95c8 lands here with the stage event in r0-r3.
@ rb_menu_engine_apply returns 0 for a fallback, else 1 + the result. The
@ fallback recreates the wrapper's push and resumes at 0x00bf95cc, so stock's
@ own call 0x00bf95d0 (lr 0x00bf95d4) is the fallback's entry to 0x00bfa234.
.section .text.rb_menu_engine_entry,"ax",%progbits
.balign 4
.global rb_menu_engine_entry
.type rb_menu_engine_entry,%function
rb_menu_engine_entry:
    push    {r0, r1, r2, r3, r4, lr}
    bl      rb_menu_engine_apply
    cmp     r0, #0
    beq     .Lstock
    sub     r0, r0, #1
    add     sp, sp, #16
    pop     {r4, pc}
.Lstock:
    pop     {r0, r1, r2, r3, r4, lr}
    push    {r3, r4, r11, lr}
    b       __rb_menu_engine_stock_resume
.size rb_menu_engine_entry, .-rb_menu_engine_entry

.macro RB_MENU_ENGINE_CALL name, addr
.section .text.\name,"ax",%progbits
.balign 4
.global \name
.type \name,%function
\name:
    push    {r4, lr}
    movw    r12, #:lower16:\addr
    movt    r12, #:upper16:\addr
    blx     r12
    pop     {r4, pc}
.size \name, .-\name
.endm

RB_MENU_ENGINE_CALL rb_menu_engine_call_availability, 0x00d52684
RB_MENU_ENGINE_CALL rb_menu_engine_call_initial_row, 0x00d588b8
RB_MENU_ENGINE_CALL rb_menu_engine_call_notify, 0x00d5b350
RB_MENU_ENGINE_CALL rb_menu_engine_call_kind_switch, 0x00bfa234
RB_MENU_ENGINE_CALL rb_menu_engine_call_property_write, 0x0061c91c

@ Six arguments: r0-r3 and the two stack words the caller passed, copied to
@ this frame's outgoing area.
.section .text.rb_menu_engine_call_seek,"ax",%progbits
.balign 4
.global rb_menu_engine_call_seek
.type rb_menu_engine_call_seek,%function
rb_menu_engine_call_seek:
    push    {r4, r5, r6, lr}
    ldr     r4, [sp, #16]
    ldr     r5, [sp, #20]
    sub     sp, sp, #8
    str     r4, [sp]
    str     r5, [sp, #4]
    movw    r12, #:lower16:0x00bfad04
    movt    r12, #:upper16:0x00bfad04
    blx     r12
    add     sp, sp, #8
    pop     {r4, r5, r6, pc}
.size rb_menu_engine_call_seek, .-rb_menu_engine_call_seek

@ The AEABI copy and clear helpers the engine's struct moves call, kept in the
@ engine region so the link takes nothing from compiler_builtins.
.section .text.rb_menu_engine_mem,"ax",%progbits
.balign 4
.global __aeabi_memcpy
.global __aeabi_memcpy4
.global __aeabi_memcpy8
.type __aeabi_memcpy,%function
.type __aeabi_memcpy4,%function
.type __aeabi_memcpy8,%function
__aeabi_memcpy:
__aeabi_memcpy4:
__aeabi_memcpy8:
    cmp     r2, #0
    bxeq    lr
1:
    ldrb    r3, [r1], #1
    strb    r3, [r0], #1
    subs    r2, r2, #1
    bne     1b
    bx      lr
.size __aeabi_memcpy, .-__aeabi_memcpy

.global __aeabi_memclr4
.global __aeabi_memclr8
.type __aeabi_memclr4,%function
.type __aeabi_memclr8,%function
__aeabi_memclr4:
__aeabi_memclr8:
    cmp     r1, #0
    bxeq    lr
    mov     r2, #0
1:
    strb    r2, [r0], #1
    subs    r1, r1, #1
    bne     1b
    bx      lr
.size __aeabi_memclr4, .-__aeabi_memclr4

@ The run's checker fails on any entry here; the trace site is the entry nop.
.section .text.rb_menu_engine_guard_failure,"ax",%progbits
.balign 4
.global rb_menu_engine_guard_failure
.type rb_menu_engine_guard_failure,%function
rb_menu_engine_guard_failure:
    nop
1:
    b       1b
.size rb_menu_engine_guard_failure, .-rb_menu_engine_guard_failure
