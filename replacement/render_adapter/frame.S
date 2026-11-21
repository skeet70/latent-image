.syntax unified
.arm

@ Published at 0x006d7920. Builds the stock frame (I4:6d7920-6d7928) and takes the lock 0x6d9478 (I4:6d792c);
@ Fallback continues stock at 0x6d7930 in that state. Owned runs the stock-service sequence from this frame; a
@ nonzero commit result unlocks and returns it like I4:6d7ddc-6d7dfc, otherwise it waits like I4:6d7d94-6d7da0
@ and returns 0 like I4:6d7df4-6d7dfc. The FujiMod presented-frame tracker observes rb_render_frame_after_wait
@ (r0 = wait result) and rb_render_frame_finish (r4 = result) with r11 as the frame identity, as at the stock
@ 0x6d7da4 and 0x6d7df4; it checks both instruction words.
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
    movs r4, r0
    beq 2f
    bl rb_render_call_6d9494
    b rb_render_frame_finish
2:
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
RB_RENDER_CALL rb_render_call_723ce8, 0x723ce8, 0
RB_RENDER_CALL rb_render_call_88bf94, 0x88bf94, 1
RB_RENDER_CALL rb_render_call_88d620, 0x88d620, 0
RB_RENDER_CALL rb_render_call_887280, 0x887280, 0
RB_RENDER_CALL rb_render_call_858e60, 0x858e60, 0
RB_RENDER_CALL rb_render_call_858c04, 0x858c04, 0
RB_RENDER_CALL rb_render_call_858b78, 0x858b78, 0
RB_RENDER_CALL rb_render_call_858c90, 0x858c90, 0
RB_RENDER_CALL rb_render_call_858d24, 0x858d24, 0
RB_RENDER_CALL rb_render_call_85a1b0, 0x85a1b0, 0
RB_RENDER_CALL rb_render_call_858748, 0x858748, 0
RB_RENDER_CALL rb_render_call_8587c4, 0x8587c4, 0
RB_RENDER_CALL rb_render_call_8588cc, 0x8588cc, 0
RB_RENDER_CALL rb_render_call_858950, 0x858950, 0
RB_RENDER_CALL rb_render_call_858af4, 0x858af4, 0
RB_RENDER_CALL rb_render_call_8589dc, 0x8589dc, 0
RB_RENDER_CALL rb_render_call_85afbc, 0x85afbc, 0
RB_RENDER_CALL rb_render_call_85ab88, 0x85ab88, 0
RB_RENDER_CALL rb_render_call_85b930, 0x85b930, 0
RB_RENDER_CALL rb_render_call_8586d8, 0x8586d8, 0
