.syntax unified
.arm

@ The dispatcher's jump-table word for property 0xf6 lands here with the row
@ token in r5 and the property in r6. A 0 return resumes the stock arm; a 1
@ return goes to the dispatcher's shared tail.
.section .text.rb_menu_value_size_arm,"ax",%progbits
.balign 4
.global rb_menu_value_size_arm
.type rb_menu_value_size_arm,%function
rb_menu_value_size_arm:
    push    {r4, lr}
    mov     r0, r6
    mov     r1, r5
    bl      rb_menu_value_size_apply
    cmp     r0, #0
    pop     {r4, lr}
    beq     .Lstock_arm
    b       __rb_menu_value_shared_tail
.Lstock_arm:
    b       __rb_menu_value_stock_arm
.size rb_menu_value_size_arm, .-rb_menu_value_size_arm

.macro RB_MENU_VALUE_CALL name, addr
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

RB_MENU_VALUE_CALL rb_menu_value_call_size_cell, 0x00633a0c
RB_MENU_VALUE_CALL rb_menu_value_call_aspect_cell, 0x00633b0c
RB_MENU_VALUE_CALL rb_menu_value_call_current_property, 0x0063c51c
RB_MENU_VALUE_CALL rb_menu_value_call_display_update, 0x00641438
RB_MENU_VALUE_CALL rb_menu_value_call_change_marker, 0x0063db24
RB_MENU_VALUE_CALL rb_menu_value_call_snapshot_cells, 0x00637e5c
RB_MENU_VALUE_CALL rb_menu_value_call_snapshot_state, 0x0063220c
RB_MENU_VALUE_CALL rb_menu_value_call_restore_aux, 0x0063647c
