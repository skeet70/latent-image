.syntax unified
.arm

@ Published at 0x006d7920. Builds the stock frame (I4:6d7920-6d7928) and takes the lock 0x6d9478 (I4:6d792c);
@ Fallback continues stock at 0x6d7930 in that state. Owned runs the stock-service sequence from this frame, waits
@ like I4:6d7d94-6d7da0 and returns 0 like I4:6d7df4-6d7dfc. The FujiMod presented-frame tracker observes
@ rb_render_frame_after_wait (r0 = wait result) and rb_render_frame_finish (r4 = result) with r11 as the frame
@ identity, as at the stock 0x6d7da4 and 0x6d7df4; it checks both instruction words.
.section .text.rb_render_frame_entry,"ax",%progbits
.balign 4
.global rb_render_frame_entry
.type rb_render_frame_entry,%function
rb_render_frame_entry:
    push {r4, r5, r6, r7, r8, r11, lr}
    add r11, sp, #24
    sub sp, sp, #20
    bl rb_render_call_6d9478
    bl rb_render_frame_apply
    cmp r0, #0
    bne 1f
    b __rb_render_frame_stock_resume
1:
    bl rb_render_frame_commit
    movw r0, #0xfefe
    mov r1, sp
    mov r2, #100
    bl rb_render_call_7243a8
.global rb_render_frame_after_wait
rb_render_frame_after_wait:
    mov r4, r0
    bl rb_render_frame_complete
    mov r4, #0
.global rb_render_frame_finish
rb_render_frame_finish:
    mov r0, r4
    sub sp, r11, #24
    pop {r4, r5, r6, r7, r8, r11, pc}
.size rb_render_frame_entry, .-rb_render_frame_entry

RB_RENDER_CALL rb_render_call_6d9478, 0x6d9478, 0
RB_RENDER_CALL rb_render_call_6d9494, 0x6d9494, 0
RB_RENDER_CALL rb_render_call_6499d0, 0x6499d0, 0
RB_RENDER_CALL rb_render_call_6d76ec, 0x6d76ec, 0
RB_RENDER_CALL rb_render_call_6cfc68, 0x6cfc68, 0
RB_RENDER_CALL rb_render_call_7243a8, 0x7243a8, 0
