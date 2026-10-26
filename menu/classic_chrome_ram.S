/* Classic Chrome RAM-push code for X-M1 1.50 on the RC0 body
 * (reports/classic-chrome-patch.md section 8.2). Linked into the RAM-push
 * runtime allocation. Each stock site the patch changes keeps its stock bytes
 * except one aligned word, a branch or bl into this file; a task already
 * inside a changed stock range continues on stock bytes.
 *
 * Entry trampolines (stock word -> b): the two converters, the +0x110
 * setter, the FilmMode and Saturation default arms, the RAW commit and
 * highlight arms and the plain RAW request block. Hooks (stock bl -> bl): the
 * colour, chroma and tone data reads. A develop record whose +0x110 word is
 * 13 is a Classic Chrome record.
 *
 * Stage stubs use r0-r3, r12 and flags only and no stack. */
        .syntax unified
        .arm
        .text
        .balign 4

/* 0x00738fe8 (FUN_00738fe8, canonical film to capture +0x508) and
 * 0x00736a74 (FUN_00736a74, live film to settings object +0x3c):
 * 0..13 pass, anything else is 0. */
        .global cc_ram_convert
        .type cc_ram_convert,%function
cc_ram_convert:
        cmp     r0, #13
        movhi   r0, #0
        bx      lr
        .size cc_ram_convert, .-cc_ram_convert

/* 0x00d7851c (FUN_00d7851c): r0 = &record+0x110, r1 = film value. */
        .global cc_ram_marker
        .type cc_ram_marker,%function
cc_ram_marker:
        cmp     r1, #13
        moveq   r3, #13
        movne   r3, #0
        str     r3, [r0]
        bx      lr
        .size cc_ram_marker, .-cc_ram_marker

/* 0x00eab2e0, FilmMode FUN_00eab258 default arm: r3 = col2 - 1. */
        .global cc_ram_filmmode
        .type cc_ram_filmmode,%function
cc_ram_filmmode:
        cmp     r3, #12
        moveq   r0, #0x600
        beq     cc_stock_filmmode_return
        b       cc_stock_filmmode_zero
        .size cc_ram_filmmode, .-cc_ram_filmmode

/* 0x00eaac88, Saturation FUN_00eaac74 default arm: r3 = col2. */
        .global cc_ram_saturation
        .type cc_ram_saturation,%function
cc_ram_saturation:
        cmp     r3, #13
        beq     cc_stock_saturation_mode_arm
        b       cc_stock_saturation_zero
        .size cc_ram_saturation, .-cc_ram_saturation

/* 0x00d461ec in FUN_00d46010's property-0x22b arm: r2 = 0, r3 = 0x00954d78,
 * r6 = row token. Row 10 stores colour 8. */
        .global cc_ram_raw_commit
        .type cc_ram_raw_commit,%function
cc_ram_raw_commit:
        cmp     r6, #10
        moveq   r2, #8
        str     r2, [r3, #8]
        b       cc_stock_raw_commit_resume
        .size cc_ram_raw_commit, .-cc_ram_raw_commit

/* 0x00d5ada4 in FUN_00d588b8's property-0x22b arm: r3 = colour field.
 * Colour 8 highlights row 10. */
        .global cc_ram_raw_highlight
        .type cc_ram_raw_highlight,%function
cc_ram_raw_highlight:
        cmp     r3, #8
        moveq   r3, #10
        streq   r3, [r11, #-0x18]
        beq     cc_stock_raw_highlight_done
        cmp     r3, #0
        b       cc_stock_raw_highlight_resume
        .size cc_ram_raw_highlight, .-cc_ram_raw_highlight

/* 0x012548a4 in plain FUN_012546a8: r3 = settings-record film field.
 * Field 8 sets request word 2 to 13. */
        .global cc_ram_raw_request
        .type cc_ram_raw_request,%function
cc_ram_raw_request:
        cmp     r3, #8
        subne   r3, r3, #3
        bne     cc_stock_raw_request_resume
        movw    r3, #0x3a7c
        movt    r3, #0x151
        mov     r2, #13
        str     r2, [r3, #0x7a4]
        b       cc_stock_raw_request_done
        .size cc_ram_raw_request, .-cc_ram_raw_request

/* bl site 0x00d67284 in FUN_00d671b4: r0 = nine u64, r1 = id-1,
 * r5 = record. */
        .global cc_ram_colour
        .type cc_ram_colour,%function
cc_ram_colour:
        ldr     r12, [r5, #0x110]
        cmp     r12, #13
        bne     cc_stock_colour_read
        movw    r1, #:lower16:cc_ram_matrix
        movt    r1, #:upper16:cc_ram_matrix
        mov     r12, #9
        b       cc_ram_copy
        .size cc_ram_colour, .-cc_ram_colour

/* bl sites 0x00d60f60 in FUN_00d60ea8 and 0x00d64ef4 in FUN_00d64e2c:
 * r0 = four u64, r1 = id-1, r4 = record. */
        .global cc_ram_chroma
        .type cc_ram_chroma,%function
cc_ram_chroma:
        ldr     r12, [r4, #0x110]
        cmp     r12, #13
        bne     cc_stock_chroma_read
        movw    r1, #:lower16:cc_ram_chroma_fields
        movt    r1, #:upper16:cc_ram_chroma_fields
        mov     r12, #4
        .size cc_ram_chroma, .-cc_ram_chroma

/* r1 = u16 source, r12 = count, r0 = u64 destination. */
        .type cc_ram_copy,%function
cc_ram_copy:
        mov     r2, #0
1:      ldrh    r3, [r1], #2
        str     r3, [r0]
        str     r2, [r0, #4]
        add     r0, r0, #8
        subs    r12, r12, #1
        bne     1b
        bx      lr
        .size cc_ram_copy, .-cc_ram_copy

/* bl sites 0x00d677ac and 0x00d677bc in FUN_00d675d8: r0 = curve id,
 * r1 = nine-byte head, r2 = 28 halfwords, r7 = record. */
        .global cc_ram_tone
        .type cc_ram_tone,%function
cc_ram_tone:
        ldr     r12, [r7, #0x110]
        cmp     r12, #13
        bne     cc_stock_tone_read
        movw    r3, #:lower16:cc_ram_tone_head
        movt    r3, #:upper16:cc_ram_tone_head
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
        .size cc_ram_tone, .-cc_ram_tone

/* The 100-byte data block of section 7; the builder writes the parameter
 * set's block over these bytes. */
        .data
        .balign 4
        .global cc_ram_data
cc_ram_data:
        .ascii  "CCD1"
        .global cc_ram_matrix
cc_ram_matrix:
        .space  18
        .global cc_ram_chroma_fields
cc_ram_chroma_fields:
        .space  8
        .space  2
        .global cc_ram_tone_head
cc_ram_tone_head:
        .space  9
        .space  1
        .space  56
        .space  2
        .global cc_ram_data_end
cc_ram_data_end:
