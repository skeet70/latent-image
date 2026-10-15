.syntax unified
.arch armv7-r
.arm
.section .text,"ax",%progbits
.balign 4

.equ ROM_STACK, 0xfff00f00
.equ FAT_RETURN, 0x15c
.equ RAW_RETURN, 0x174
.equ RELOC, 0xc0000
.equ FAT_OPEN, 0x80001dc4
.equ FAT_READ, 0x80001e88
.equ RAW_READ, 0x8000148c

.global ram_rescue_bootstrap
.type ram_rescue_bootstrap,%function
ram_rescue_bootstrap:
    movw r0, #:lower16:ROM_STACK
    movt r0, #:upper16:ROM_STACK
    cmp sp, r0
    bne .Lrefuse
    // SD transfers require the data cache disabled.
    mrc p15, 0, r0, c1, c0, 0
    tst r0, #4
    bne .Lrefuse
    adr r0, ram_rescue_bootstrap
    cmp r0, #0
    cmpne r0, #0x40000000
    bne .Lrefuse
    ldr r0, [sp, #-4]
    bic r0, r0, #0x80000000
    cmp r0, #FAT_RETURN
    cmpne r0, #RAW_RETURN
    bne .Lrefuse
    ldr r0, [sp, #-4]
    push {r0-r12, lr}
    bl .Lled_setup
    bl .Lled_on
    mov r0, #0
    mov r1, #RELOC
    mov r2, #4096
.Lrelocate:
    ldr r3, [r0], #4
    str r3, [r1], #4
    subs r2, r2, #4
    bne .Lrelocate
    mov r0, #RELOC
    mov r1, #4096
    bl .Lclean
    mov r0, #0
    mcr p15, 0, r0, c7, c5, 0
    dsb sy
    isb sy
    adr r0, .Lrelocated
    add r0, r0, #RELOC
    bx r0

.Lrelocated:
    mov r8, #RELOC
    add r8, r8, #0x800
    ldr r0, [r8]
    movw r1, #0x4d58
    movt r1, #0x3152
    cmp r0, r1
    bne .Lrefuse
    ldr r0, [r8, #4]
    cmp r0, #1
    bne .Lrefuse
    ldr r9, [r8, #8]
    cmp r9, #1
    blo .Lrefuse
    cmp r9, #16
    bhi .Lrefuse
    ldr r0, [r8, #24]
    ldr r1, [r8, #28]
    orrs r0, r0, r1
    bne .Lrefuse
    ldr r10, [r8, #12]
    mov r0, #0
    str r0, [r8, #12]
    mov r0, r8
    mov r1, #512
    mvn r2, #0
    bl .Lcrc
    str r10, [r8, #12]
    mvn r2, r2
    cmp r2, r10
    bne .Lrefuse
    ldr r10, [r8, #16]
    ldr r0, [r8, #20]
    cmp r0, #0
    beq .Lrefuse
    movw r1, #511
    tst r0, r1
    bne .Lrefuse
    adds r11, r10, r0, lsr #9
    bcs .Lrefuse
    add r8, r8, #32
    mov r7, r9

.Lpreflight:
    ldm r8, {r0-r4}
    cmp r0, #1
    cmpne r0, #2
    bne .Lrefuse
    cmp r2, #0
    beq .Lrefuse
    tst r1, #3
    bne .Lrefuse
    tst r2, #3
    bne .Lrefuse
    cmp r1, #0x40000000
    blo .Lrefuse
    adds r5, r1, r2
    bcs .Lrefuse
    cmp r5, #0x42000000
    bhi .Lrefuse
    movw r6, #0x2000
    movt r6, #0x400c
    cmp r1, r6
    bhs .Lprotected_ok
    sub r6, r6, #0x2000
    cmp r5, r6
    bhi .Lrefuse
.Lprotected_ok:
    cmp r0, #2
    beq .Lzero_preflight
    movw r6, #511
    tst r2, r6
    bne .Lrefuse
    tst r1, r6
    bne .Lrefuse
    cmp r3, r10
    bne .Lrefuse
    adds r10, r10, r2, lsr #9
    bcs .Lrefuse
    cmp r10, r11
    bhi .Lrefuse
    b .Loverlap_preflight
.Lzero_preflight:
    orrs r3, r3, r4
    bne .Lrefuse
.Loverlap_preflight:
    mov r6, #RELOC
    add r6, r6, #0x800
    add r6, r6, #32
.Loverlap_loop:
    cmp r6, r8
    beq .Lpreflight_next
    ldr r3, [r6, #4]
    ldr r4, [r6, #8]
    add r4, r3, r4
    cmp r1, r4
    bhs .Loverlap_next
    cmp r3, r5
    blo .Lrefuse
.Loverlap_next:
    add r6, r6, #20
    b .Loverlap_loop
.Lpreflight_next:
    add r8, r8, #20
    subs r7, r7, #1
    bne .Lpreflight
    cmp r10, r11
    bne .Lrefuse

    ldr r0, [sp]
    bic r0, r0, #0x80000000
    cmp r0, #FAT_RETURN
    bne .Lbegin_regions
    adr r0, .Lname
    movw r3, #:lower16:FAT_OPEN
    movt r3, #:upper16:FAT_OPEN
    blx r3
    cmp r0, #0
    bne .Lrefuse
.Lbegin_regions:
    mov r8, #RELOC
    add r8, r8, #0x800
    add r8, r8, #32
.Lregion:
    ldr r0, [r8]
    ldr r4, [r8, #4]
    ldr r6, [r8, #8]
    cmp r0, #2
    beq .Lzero_region
    ldr r5, [r8, #12]
.Lcopy_sector:
    ldr r0, [sp]
    bic r0, r0, #0x80000000
    cmp r0, #FAT_RETURN
    beq .Lfat_sector
    mov r0, #1
    mov r1, r4
    mov r2, r5
    movw r3, #:lower16:RAW_READ
    movt r3, #:upper16:RAW_READ
    blx r3
    cmp r0, #0
    bne .Lrefuse
    b .Lsector_loaded
.Lfat_sector:
    sub sp, sp, #8
    mov r0, #0
    str r0, [sp]
    mov r0, r4
    mov r1, #512
    mov r2, sp
    movw r3, #:lower16:FAT_READ
    movt r3, #:upper16:FAT_READ
    blx r3
    ldrh r1, [sp]
    add sp, sp, #8
    cmp r0, #0
    bne .Lrefuse
    cmp r1, #512
    bne .Lrefuse
.Lsector_loaded:
    mrc p15, 0, r0, c1, c0, 0
    tst r0, #4
    bne .Lrefuse
    add r4, r4, #512
    add r5, r5, #1
    subs r6, r6, #512
    bne .Lcopy_sector
    ldr r0, [r8, #4]
    ldr r1, [r8, #8]
    mvn r2, #0
    bl .Lcrc
    mvn r2, r2
    ldr r3, [r8, #16]
    cmp r2, r3
    bne .Lrefuse
    b .Lregion_clean
.Lzero_region:
    mov r0, #0
.Lzero_loop:
    str r0, [r4], #4
    subs r6, r6, #4
    bne .Lzero_loop
.Lregion_clean:
    ldr r0, [r8, #4]
    ldr r1, [r8, #8]
    bl .Lclean
    add r8, r8, #20
    subs r9, r9, #1
    bne .Lregion
    mov r0, #0
    mcr p15, 0, r0, c7, c5, 0
    dsb sy
    isb sy
    bl .Lled_off
    pop {r0-r12, lr}
    str r0, [sp, #-4]
    mov r0, #0
    msr APSR_nzcvq, r0
    mov pc, #0x1800

.Lcrc:
    adr r12, .Lcrc_table
.Lcrc_byte:
    ldrb r3, [r0], #1
    eor r2, r2, r3
    and r3, r2, #15
    ldr r3, [r12, r3, lsl #2]
    eor r2, r3, r2, lsr #4
    and r3, r2, #15
    ldr r3, [r12, r3, lsl #2]
    eor r2, r3, r2, lsr #4
    subs r1, r1, #1
    bne .Lcrc_byte
    bx lr

.Lclean:
    add r1, r0, r1
    bic r0, r0, #31
.Lclean_line:
    mcr p15, 0, r0, c7, c10, 1
    add r0, r0, #32
    cmp r0, r1
    blo .Lclean_line
    dsb sy
    bx lr

.Lrefuse:
    bl .Lled_setup
    mov r4, #4
.Lsignal:
    bl .Lled_on
    bl .Ldelay
    bl .Lled_off
    bl .Ldelay
    subs r4, r4, #1
    bne .Lsignal
.Lhalt:
    wfi
    b .Lhalt
.Ldelay:
    mov r0, #0x4000
.Ldelay_loop:
    subs r0, r0, #1
    bne .Ldelay_loop
    bx lr
.Lled_setup:
    movw r0, #0x8084
    movt r0, #0xfff8
    mov r1, #1
    str r1, [r0]
    dsb sy
    movw r0, #0x93c0
    movt r0, #0xfff8
    movw r1, #1
    movt r1, #5
    str r1, [r0, #4]
    dsb sy
    bx lr
.Lled_on:
    movw r1, #1
    b .Lled_value
.Lled_off:
    movw r1, #0
.Lled_value:
    movt r1, #1
    movw r0, #0x93c0
    movt r0, #0xfff8
    str r1, [r0]
    dsb sy
    bx lr

.Lname:
    .asciz "ffrecov.bin"
.balign 4
.Lcrc_table:
    .word 0x00000000, 0x1db71064, 0x3b6e20c8, 0x26d930ac
    .word 0x76dc4190, 0x6b6b51f4, 0x4db26158, 0x5005713c
    .word 0xedb88320, 0xf00f9344, 0xd6d6a3e8, 0xcb61b38c
    .word 0x9b64c2b0, 0x86d3d2d4, 0xa00ae278, 0xbdbdf21c
.size ram_rescue_bootstrap, .-ram_rescue_bootstrap

.section .parameters,"aw",%progbits
.global ram_rescue_parameters
ram_rescue_parameters:
    .space 512, 0
.section .padding,"a",%progbits
    .space 1536, 0
