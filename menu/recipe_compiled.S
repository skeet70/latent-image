@ The compiled-recipe commit hook of the D3/Q combined prefix
@ (reports/recipe-patch-d3.md section 5). A patched `bl` at 0x00d50a48 enters
@ here: the first instruction of the case 0xfe arm of the menu commit
@ FUN_00d4fc74, whose `mov r0, r4` this hook re-executes on the way out.
@
@ Register contract at entry, the case arm's own state: r5 the values-entry row
@ token, r6 the property 0xfe, r8 the row's col0 and r4 its col2. The push
@ covers every register the hook touches, so the stock arm continues with r4,
@ r8 and r6 intact and with its stack alignment unchanged (ten registers).
@
@ Row token 2 selects the first compiled recipe row and row token 4 the
@ second. Each applies its eight parameter rows by re-entering FUN_00d4fc74
@ once per property, which runs that property's own case arm (its live-cell
@ write, its setter and its property-specific work). The nested calls carry
@ only the parameter properties, so the case 0xfe arm cannot be re-entered.
@ Every other row token returns without a property call.
@
@ rc_row_tables is two 8-byte row tables, one byte per applied property, in
@ rc_props order; rc_props is the eight property halfwords the walk consumes
@ in step. Both addresses come from menu/recipe_compiled.ld at link time
@ (--defsym=RC_BASE=..., --defsym=RC_DATA=...).
.syntax unified
.arm
.section .text,"ax",%progbits

.global rc_hook
.type rc_hook,%function
rc_hook:
    push    {r3, r4, r5, r6, r7, r8, r9, r10, r11, lr}
    cmp     r5, #4
    movw    r4, #:lower16:rc_row_tables
    movt    r4, #:upper16:rc_row_tables
    addeq   r4, r4, #8
    beq     1f
    cmp     r5, #2
    bne     2f
1:
    movw    r6, #:lower16:rc_props
    movt    r6, #:upper16:rc_props
    mov     r7, #8
3:
    ldrh    r0, [r6], #2
    ldrb    r1, [r4], #1
    bl      menu_commit
    subs    r7, r7, #1
    bne     3b
2:
    pop     {r3, r4, r5, r6, r7, r8, r9, r10, r11, lr}
    mov     r0, r4
    bx      lr
.size rc_hook, .-rc_hook
