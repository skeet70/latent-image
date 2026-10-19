/* Classic Chrome stubs for X-M1 1.50 (reports/classic-chrome-patch.md
 * sections 4.3 to 4.5 and 6.2). Two blobs replace two image-4 converters in
 * place; each starts with the converter's new body. The stage stubs are
 * entered by bl from the five image-5 sites the patch rewrites, with the
 * develop record in the register named per stub. A record whose +0x110 word
 * is 13 is a Classic Chrome record (setter FUN_00d7851c rewrite); any other
 * record tail-branches to the stock reader with r0-r2 and lr unchanged.
 * Registers used: r0-r3, r12, flags. No stack. */
        .syntax unified
        .arm

        .section .text.capture, "ax", %progbits

/* FUN_00738fe8 replacement: canonical film value to capture +0x508.
 * 0..13 pass, anything else is 0. */
        .global cc_convert_capture
cc_convert_capture:
        cmp     r0, #13
        movhi   r0, #0
        bx      lr

/* Site 0x00d67284 in FUN_00d671b4. r0 = destination of nine u64, r1 = id-1,
 * r5 = record. */
        .global cc_colour
cc_colour:
        ldr     r12, [r5, #0x110]
        cmp     r12, #13
        bne     stock_colour_read
        movw    r1, #:lower16:cc_matrix
        movt    r1, #:upper16:cc_matrix
        mov     r12, #9
        b       cc_copy

/* Sites 0x00d60f60 in FUN_00d60ea8 and 0x00d64ef4 in FUN_00d64e2c.
 * r0 = destination of four u64, r1 = id-1, r4 = record. */
        .global cc_chroma
cc_chroma:
        ldr     r12, [r4, #0x110]
        cmp     r12, #13
        bne     stock_chroma_read
        movw    r1, #:lower16:cc_chroma_fields
        movt    r1, #:upper16:cc_chroma_fields
        mov     r12, #4

/* r1 = u16 source, r12 = count, r0 = u64 destination. */
cc_copy:
        mov     r2, #0
1:      ldrh    r3, [r1], #2
        str     r3, [r0]
        str     r2, [r0, #4]
        add     r0, r0, #8
        subs    r12, r12, #1
        bne     1b
        bx      lr

        .section .text.settings, "ax", %progbits

/* FUN_00736a74 replacement: live film value to settings-object byte +0x3c.
 * 0..13 pass, anything else is 0. */
        .global cc_convert_settings
cc_convert_settings:
        cmp     r0, #13
        movhi   r0, #0
        bx      lr

/* Sites 0x00d677ac and 0x00d677bc in FUN_00d675d8. r0 = curve id,
 * r1 = nine-byte head destination, r2 = 28-halfword curve destination,
 * r7 = record. */
        .global cc_tone
cc_tone:
        ldr     r12, [r7, #0x110]
        cmp     r12, #13
        bne     stock_tone_read
        movw    r3, #:lower16:cc_tone_head
        movt    r3, #:upper16:cc_tone_head
        mov     r12, #0
1:      ldrb    r0, [r3, r12]
        strb    r0, [r1, r12]
        add     r12, r12, #1
        cmp     r12, #9
        bne     1b
        add     r3, r3, #10
        mov     r12, #28
2:      ldrh    r0, [r3], #2
        strh    r0, [r2], #2
        subs    r12, r12, #1
        bne     2b
        bx      lr
