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
.extern rb_stock_present_continue
.extern rb_stock_draw_root_continue

@ Both playback callers save {r4,r5,r11,lr}, with r11 at the saved lr.
.global rb_mode_pair_apply
.type rb_mode_pair_apply,%function
rb_mode_pair_apply:
    push    {r4, lr}
    bl      rb_mode_pair_run
    pop     {r4, lr}
    cmp     r0, #0
    bxeq    lr
    sub     sp, r11, #12
    mvn     r0, #0
    pop     {r4, r5, r11, pc}
.size rb_mode_pair_apply, .-rb_mode_pair_apply

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

@ Guards enter before stock handlers clear the request callback.
.global rb_media_open_guard_entry
.type rb_media_open_guard_entry,%function
rb_media_open_guard_entry:
    b       rb_media_guarded_open
.size rb_media_open_guard_entry, .-rb_media_open_guard_entry

.global rb_media_write_guard_entry
.type rb_media_write_guard_entry,%function
rb_media_write_guard_entry:
    b       rb_media_guarded_write
.size rb_media_write_guard_entry, .-rb_media_write_guard_entry

.global rb_media_close_guard_entry
.type rb_media_close_guard_entry,%function
rb_media_close_guard_entry:
    b       rb_media_guarded_close
.size rb_media_close_guard_entry, .-rb_media_close_guard_entry

.global rb_call_stock_media_open
.type rb_call_stock_media_open,%function
rb_call_stock_media_open:
    push    {r4, r5, r6, r7, r8, r11, lr}
    movw    r12, #0x15a8
    movt    r12, #0x0071
    bx      r12
.size rb_call_stock_media_open, .-rb_call_stock_media_open

.global rb_call_stock_media_write
.type rb_call_stock_media_write,%function
rb_call_stock_media_write:
    push    {r4, r5, r6, r7, r11, lr}
    movw    r12, #0x16e8
    movt    r12, #0x0071
    bx      r12
.size rb_call_stock_media_write, .-rb_call_stock_media_write

.global rb_call_stock_media_close
.type rb_call_stock_media_close,%function
rb_call_stock_media_close:
    push    {r3, r4, r5, r6, r11, lr}
    movw    r12, #0x176c
    movt    r12, #0x0071
    bx      r12
.size rb_call_stock_media_close, .-rb_call_stock_media_close

@ The page-entry cache shim: every page open and transition reaches the walk
@ through FUN_00bfa800 at the three call sites the plan rewrites, so this entry
@ drops the visibility lane's verdict cache and then takes the stock path.
.global rb_bank_page_entry
.type rb_bank_page_entry,%function
rb_bank_page_entry:
    push    {r0, r1, r2, r3, r8, r9, r12, lr}
    bl      rb_ui_probe_visit
    pop     {r0, r1, r2, r3, r8, r9, r12, lr}
    b       rb_stock_page_entry
.size rb_bank_page_entry, .-rb_bank_page_entry

.global rb_call_stock_present
.type rb_call_stock_present,%function
rb_call_stock_present:
    push    {r11, lr}
    b       rb_stock_present_continue
.size rb_call_stock_present, .-rb_call_stock_present

.global rb_call_stock_draw_root
.type rb_call_stock_draw_root,%function
rb_call_stock_draw_root:
    push    {r4, r5, r11, lr}
    b       rb_stock_draw_root_continue
.size rb_call_stock_draw_root, .-rb_call_stock_draw_root

@ Shooting navigation rows have no current-setting label.
.global rb_bank_current_value_label
.type rb_bank_current_value_label,%function
rb_bank_current_value_label:
    cmp     r0, #1
    bne     .Lrb_current_value_stock
    cmp     r3, #0
    bne     .Lrb_current_value_stock
    movw    r12, #0x281
    cmp     r1, r12
    beq     .Lrb_no_current_value_label
    movw    r12, #0x285
    cmp     r1, r12
    bne     .Lrb_current_value_stock
.Lrb_no_current_value_label:
    mov     r0, #0
    bx      lr
.Lrb_current_value_stock:
    movw    r12, #0x93e0
    movt    r12, #0x00bf
    bx      r12
.size rb_bank_current_value_label, .-rb_bank_current_value_label

@ The selected-drawer call supplies its callback in r3 and packet in r6.
.global rb_bank_draw_selected
.type rb_bank_draw_selected,%function
rb_bank_draw_selected:
    push    {r4, r5, r6, lr}
    ldr     r12, .Lrb_draw_view_revision
    ldr     r4, [r12]
    cmp     r6, #0x47
    bne     .Lrb_draw_stock_callback
    ldr     r12, .Lrb_draw_editor_frame
    ldr     r12, [r12]
    cmp     r12, #0
    beq     .Lrb_draw_stock_callback
    ldr     r12, [r12]
    cmp     r12, #0x6a
    bne     .Lrb_draw_stock_callback
    ldr     r12, .Lrb_draw_editor_frame
    ldr     r12, [r12]
    ldr     r12, [r12, #8]
    movw    lr, #0x131
    cmp     r12, lr
    bne     .Lrb_draw_stock_callback
    bl      rb_bank_draw_name
    b       .Lrb_draw_callback_done
.Lrb_draw_stock_callback:
    blx     r3
.Lrb_draw_callback_done:
    mov     r5, r0
    mov     r0, r6
    mov     r1, r4
    bl      rb_bank_emit_failure_from_stock
    mov     r0, r5
    pop     {r4, r5, r6, pc}
.size rb_bank_draw_selected, .-rb_bank_draw_selected
.Lrb_draw_view_revision:
    .word   rb_view_revision
.Lrb_draw_editor_frame:
    .word   0x009571d0


.global rb_call_preview_clear
.type rb_call_preview_clear,%function
rb_call_preview_clear:
    mov     r0, #0
    movw    r12, #0x5944
    movt    r12, #0x0063
    bx      r12
.size rb_call_preview_clear, .-rb_call_preview_clear

@ Mode 0 reads the live menu; argument 3 retains the requested root.
.global rb_call_draw_action_list
.type rb_call_draw_action_list,%function
rb_call_draw_action_list:
    mov     r1, #0
    mov     r2, #1
    movw    r12, #0x768c
    movt    r12, #0x0116
    bx      r12
.size rb_call_draw_action_list, .-rb_call_draw_action_list

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
RB_TRAMPOLINE rb_call_snapshot_store, 0x0061ceac
RB_TRAMPOLINE rb_call_redraw,      0x00d38d98
RB_TRAMPOLINE rb_call_stock_redraw_dispatch, 0x00d38e1c
RB_TRAMPOLINE rb_call_set_dispatch, 0x00c72e10
RB_TRAMPOLINE rb_call_editor_confirm, 0x00c75ed4
RB_TRAMPOLINE rb_call_postlude, 0x00d38f34
RB_TRAMPOLINE rb_call_leave_edge, 0x00c76650
RB_TRAMPOLINE rb_call_draw_root_prepare, 0x01166200
RB_TRAMPOLINE rb_call_stock_key_right, 0x00c70528
RB_TRAMPOLINE rb_call_stock_key_left, 0x00c6f14c
RB_TRAMPOLINE rb_call_modal_query, 0x0063a0f8
RB_TRAMPOLINE rb_call_name_style, 0x01130e68
RB_TRAMPOLINE rb_call_draw_mode, 0x01112b2c
RB_TRAMPOLINE rb_call_name_background, 0x0112db04
RB_TRAMPOLINE rb_call_name_title, 0x011397a4
RB_TRAMPOLINE rb_call_name_rect, 0x01113514
RB_TRAMPOLINE rb_call_name_shell, 0x01158294
RB_TRAMPOLINE rb_call_name_label, 0x0111236c
RB_TRAMPOLINE rb_call_name_material, 0x01137cf8
RB_TRAMPOLINE rb_call_name_utf16, 0x011302f8
RB_TRAMPOLINE rb_call_name_stretch, 0x01138060
RB_TRAMPOLINE rb_call_name_ascii, 0x011301ac
RB_TRAMPOLINE rb_call_name_footer, 0x0112e7fc
RB_TRAMPOLINE rb_call_name_label_position, 0x0111299c
RB_TRAMPOLINE rb_call_backdrop_suppressed, 0x01135db0

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
    bl      rb_ui_probe_state
    mov     r4, r0
    pop     {r0, r1, r2, r3, r8, r9, r12, lr}
    b       rb_stock_probe_epilogue
.size rb_probe_entry, .-rb_probe_entry

@ FUN_0061e110 dispatches with {r4, r5, r11, lr} saved.
.global rb_stage_edit_get_arm
.type rb_stage_edit_get_arm,%function
rb_stage_edit_get_arm:
    bl      rb_stage_edit_get
    pop     {r4, r5, r11, pc}
.size rb_stage_edit_get_arm, .-rb_stage_edit_get_arm

@ FUN_00624694 dispatches with {r3, r4, r5, r6, r7, r8, r11, lr} saved; the
@ zero return writes nothing and skips FUN_006309c4's store-back.
.global rb_stage_edit_set_arm
.type rb_stage_edit_set_arm,%function
rb_stage_edit_set_arm:
    mov     r0, #0
    pop     {r3, r4, r5, r6, r7, r8, r11, pc}
.size rb_stage_edit_set_arm, .-rb_stage_edit_set_arm

@ After rb_probe_entry and the edit arms, whose addresses a built bank plan names.
RB_TRAMPOLINE rb_call_draw_pane, 0x011662a0
