@ The editable recipe bank's target code: the render-probe entry, the page-entry
@ cache shim, the media-change shims and the trampolines that reach stock
@ functions by absolute address.
@
@ Commit dispatch lives in the actions module: the modal interposer
@ rb_bank_interpose calls rb_bank_action, which decodes a bank token and calls
@ the storage kernel directly, so this file carries no commit arms.
@
.syntax unified
.arm
.section .text,"ax",%progbits

.extern rb_stock_detach_continue
.extern rb_stock_attach_continue
.extern rb_stock_page_entry
.extern rb_stock_probe_epilogue

@ The media-change shims: the detach and attach workers are the two producers of
@ a device transition, so each entry bumps the epoch and then replays the push
@ the branch displaced. Neither is on the file-request path.
.global rb_media_epoch_entry_a
.type rb_media_epoch_entry_a,%function
rb_media_epoch_entry_a:
    push    {r0, r1, r2, r3, r12, lr}
    bl      rb_media_epoch_bump
    pop     {r0, r1, r2, r3, r12, lr}
    push    {r4, r5, r11, lr}
    b       rb_stock_detach_continue
.size rb_media_epoch_entry_a, .-rb_media_epoch_entry_a

.global rb_media_epoch_entry_b
.type rb_media_epoch_entry_b,%function
rb_media_epoch_entry_b:
    push    {r0, r1, r2, r3, r12, lr}
    bl      rb_media_epoch_bump
    pop     {r0, r1, r2, r3, r12, lr}
    push    {r4, r5, r6, r7, r8, r9, r10, r11, lr}
    b       rb_stock_attach_continue
.size rb_media_epoch_entry_b, .-rb_media_epoch_entry_b

@ The page-entry cache shim: every page open and transition reaches the walk
@ through FUN_00bfa800 at the three call sites the plan rewrites, so this entry
@ drops the visibility lane's verdict cache and then takes the stock path.
.global rb_bank_page_entry
.type rb_bank_page_entry,%function
rb_bank_page_entry:
    push    {r0, r1, r2, r3, r8, r9, r12, lr}
    bl      rb_probe_invalidate
    pop     {r0, r1, r2, r3, r8, r9, r12, lr}
    b       rb_stock_page_entry
.size rb_bank_page_entry, .-rb_bank_page_entry

@ ---------------------------------------------------------------- trampolines
@ Each trampoline is the twelve-byte tail form: the callee returns straight to
@ the kernel caller.
.macro RB_TRAMPOLINE name, addr
.global \name
.type \name,%function
\name:
    movw r12, #(\addr & 0xffff)
    movt r12, #((\addr >> 16) & 0xffff)
    bx      r12
.size \name, .-\name
.endm

RB_TRAMPOLINE rb_call_stage_open,  0x00d3d858
RB_TRAMPOLINE rb_call_transition,  0x00c12aac
RB_TRAMPOLINE rb_call_redraw,      0x00d38d98
RB_TRAMPOLINE rb_call_set_dispatch, 0x00c72e10
RB_TRAMPOLINE rb_call_editor_confirm, 0x00c75ed4
RB_TRAMPOLINE rb_call_postlude, 0x00d38f34
RB_TRAMPOLINE rb_call_leave_edge, 0x00c76650

@ The render probe's interposed entry: entered with r5 = property, r6 = row
@ token, returns through the stock probe epilogue 0x00d5868c with the state in
@ r4. The final transfer is a direct branch at a link-time constant target
@ inside the branch range, and the eight-register push keeps sp 8-byte aligned
@ at the bl.
.global rb_probe_entry
.type rb_probe_entry,%function
rb_probe_entry:
    push    {r0, r1, r2, r3, r8, r9, r12, lr}
    mov     r0, r5
    mov     r1, r6
    bl      rb_probe_state
    mov     r4, r0
    pop     {r0, r1, r2, r3, r8, r9, r12, lr}
    b       rb_stock_probe_epilogue
.size rb_probe_entry, .-rb_probe_entry
