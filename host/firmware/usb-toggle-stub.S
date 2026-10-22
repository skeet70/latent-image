/* USB personality toggle for X-M1 1.50 (reports/usb-personality-toggle.md
 * sections 3.1 and 5). Entered by bl from 0x00ecf900 in connect handler A,
 * which tests the returned r0 as cache[0xf7]. On entry r0 = 0xf7 and r1 is
 * the new attach bit. On an attach edge with KEY_Q's debounced driver state
 * set and system mode neither 5 nor 7, cache[0xf7] is flipped (0 to 1,
 * nonzero to 0) in RAM and the new value returned; otherwise the stored
 * value is returned unchanged. AAPCS: r4-r11 and sp preserved. */
        .syntax unified
        .arm

        .section .text.toggle, "ax", %progbits

        .global usb_toggle
usb_toggle:
        push    {r4, r5, r6, lr}
        mov     r4, r1
        bl      stock_cache_byte
        mov     r5, r0
        cmp     r4, #1
        bne     1f
        mov     r0, #0x32
        bl      stock_key_state
        cmp     r0, #0
        beq     1f
        bl      stock_system_mode
        cmp     r0, #5
        cmpne   r0, #7
        beq     1f
        cmp     r5, #0
        moveq   r5, #1
        movne   r5, #0
        mov     r0, #0xf7
        mov     r1, r5
        bl      stock_cache_store
1:      mov     r0, r5
        pop     {r4, r5, r6, pc}
