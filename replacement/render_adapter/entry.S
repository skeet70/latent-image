.syntax unified
.arm

.macro RB_RENDER_ENTRY name, apply, resume, report=1
.section .text.\name,"ax",%progbits
.balign 4
.global \name
.type \name,%function
\name:
    push {r0-r12, lr}
    mov r0, sp
    add r1, sp, #56
    mov r2, lr
.if \report
    ldr r3, [sp, #80]
.else
    mov r3, #0
.endif
    bl \apply
    cmp r0, #0
    pop {r0-r12, lr}
    bxne lr
    push {r4-r11, lr}
    b \resume
.size \name, .-\name
.endm

RB_RENDER_ENTRY rb_render_picker_entry, rb_render_picker_apply, __rb_render_picker_stock_resume
RB_RENDER_ENTRY rb_render_pane_entry, rb_render_pane_apply, __rb_render_pane_stock_resume, 0

.macro RB_RENDER_CALL name, addr, stacked=0
.section .text.\name,"ax",%progbits
.balign 4
.global \name
.type \name,%function
\name:
    push {r4, lr}
.if \stacked
    sub sp, sp, #16
    ldr r4, [sp, #24]
    str r4, [sp]
.if \stacked > 1
    ldr r4, [sp, #28]
    str r4, [sp, #4]
.endif
.if \stacked > 2
    ldr r4, [sp, #32]
    str r4, [sp, #8]
.endif
.endif
    movw r12, #:lower16:\addr
    movt r12, #:upper16:\addr
    blx r12
.if \stacked
    add sp, sp, #16
.endif
    pop {r4, pc}
.size \name, .-\name
.endm

RB_RENDER_CALL rb_render_call_43550, 0x43550, 0
RB_RENDER_CALL rb_render_call_7221c8, 0x7221c8, 0
RB_RENDER_CALL rb_render_call_61ce1c, 0x61ce1c, 0
RB_RENDER_CALL rb_render_call_61cd14, 0x61cd14, 0
RB_RENDER_CALL rb_render_call_bf8f4c, 0xbf8f4c, 0
RB_RENDER_CALL rb_render_call_bf90f0, 0xbf90f0, 0
RB_RENDER_CALL rb_render_call_bf93e0, 0xbf93e0, 0
RB_RENDER_CALL rb_render_call_1112a6c, 0x1112a6c, 0
RB_RENDER_CALL rb_render_call_11391a0, 0x11391a0, 0
RB_RENDER_CALL rb_render_call_4d978, 0x4d978, 0
RB_RENDER_CALL rb_render_call_1112b2c, 0x1112b2c, 0
RB_RENDER_CALL rb_render_call_1137cf8, 0x1137cf8, 0
RB_RENDER_CALL rb_render_call_1137d2c, 0x1137d2c, 0
RB_RENDER_CALL rb_render_call_116f948, 0x116f948, 0
RB_RENDER_CALL rb_render_call_1112e78, 0x1112e78, 0
RB_RENDER_CALL rb_render_call_1112e38, 0x1112e38, 0
RB_RENDER_CALL rb_render_call_1112e58, 0x1112e58, 0
RB_RENDER_CALL rb_render_call_1138a00, 0x1138a00, 0
RB_RENDER_CALL rb_render_call_111236c, 0x111236c, 0
RB_RENDER_CALL rb_render_call_1138060, 0x1138060, 2
RB_RENDER_CALL rb_render_call_1113514, 0x1113514, 3
RB_RENDER_CALL rb_render_call_11130e0, 0x11130e0, 3
RB_RENDER_CALL rb_render_call_1177e10, 0x1177e10, 0
RB_RENDER_CALL rb_render_call_1130e68, 0x1130e68, 0
RB_RENDER_CALL rb_render_call_111258c, 0x111258c, 0
RB_RENDER_CALL rb_render_call_111299c, 0x111299c, 0
RB_RENDER_CALL rb_render_call_bf96b0, 0xbf96b0, 0
RB_RENDER_CALL rb_render_call_1139a68, 0x1139a68, 3
RB_RENDER_CALL rb_render_call_1159168, 0x1159168, 0

RB_RENDER_ENTRY rb_render_tree_entry, rb_render_tree_apply, __rb_render_tree_stock_resume
RB_RENDER_CALL rb_render_call_61ca2c, 0x61ca2c, 0
RB_RENDER_CALL rb_render_call_bf8e64, 0xbf8e64, 0
RB_RENDER_CALL rb_render_call_11730dc, 0x11730dc, 1
RB_RENDER_CALL rb_render_call_bf8d20, 0xbf8d20, 0
RB_RENDER_CALL rb_render_call_bf9360, 0xbf9360, 0
RB_RENDER_CALL rb_render_call_1163e04, 0x1163e04, 0
RB_RENDER_CALL rb_render_call_1112da4, 0x1112da4, 0
RB_RENDER_CALL rb_render_call_1112d84, 0x1112d84, 0
RB_RENDER_CALL rb_render_call_116dce0, 0x116dce0, 0
RB_RENDER_CALL rb_render_call_1163c1c, 0x1163c1c, 0
RB_RENDER_CALL rb_render_call_116cf50, 0x116cf50, 0
RB_RENDER_CALL rb_render_call_1112ce4, 0x1112ce4, 0
RB_RENDER_CALL rb_render_call_116e264, 0x116e264, 0
RB_RENDER_CALL rb_render_call_1165aa0, 0x1165aa0, 0
RB_RENDER_CALL rb_render_call_111278c, 0x111278c, 3
RB_RENDER_CALL rb_render_call_11121d4, 0x11121d4, 0
RB_RENDER_CALL rb_render_call_116d4e4, 0x116d4e4, 0
RB_RENDER_CALL rb_render_call_1112d64, 0x1112d64, 0
RB_RENDER_CALL rb_render_call_1135db0, 0x1135db0, 0
RB_RENDER_CALL rb_render_call_113f1e8, 0x113f1e8, 0
RB_RENDER_CALL rb_render_call_bf9614, 0xbf9614, 0
RB_RENDER_CALL rb_render_call_bf963c, 0xbf963c, 0
RB_RENDER_CALL rb_render_call_bf9660, 0xbf9660, 0
RB_RENDER_CALL rb_render_call_bf9688, 0xbf9688, 0
RB_RENDER_CALL rb_render_call_112eb38, 0x112eb38, 0

.section .text.rb_render_remainder,"ax",%progbits
.balign 4
.global rb_render_remainder
.type rb_render_remainder,%function
rb_render_remainder:
    push {r4, lr}
    movw r12, #0xda6c
    movt r12, #4
    blx r12
    mov r0, r1
    pop {r4, pc}
.size rb_render_remainder, .-rb_render_remainder

