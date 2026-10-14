// X-M1 1.50 block-14 recovery with exact pre-read and post-write classification.
.syntax unified
.arch armv7-r
.arm
.section .text,"ax",%progbits
.balign 4

.equ XM1_ROM_STACK_TOP,       0xfff00f00
.equ XM1_FAT_RETURN_OFFSET,   0x0000015c
.equ XM1_RAW_RETURN_OFFSET,   0x00000174
.equ XM1_MEDIA_RECEIVE_TAP,   0x0071feb4
.equ XM1_RECORDER_TAP_WORD,   0xeb34a9c9
.equ XM1_STOCK_TAP_WORD,      0xebffffdc
.equ XM1_COMPLETION_SITE,     0x0073b564
.equ XM1_COMPLETION_WORD,     0xebffa12a
.equ XM1_STAGE_BRANCH_WORD,   0xebe32b25
.equ XM1_DRIVER_READY,        0x00828574
.equ XM1_READ_DESTINATION,    0x00020000
.equ XM1_REPAIR_SOURCE,       0x00040000
.equ XM1_BLOCK_READER,        0x006a8ff0
.equ XM1_BLOCK_WRITER,        0x006a9268
.equ XM1_RC0_CRC32,           0x92863e8b
.equ XM1_RECORDER_CRC32,      0xd1943bd0

.global persistent_repair_guard
.type persistent_repair_guard,%function
persistent_repair_guard:
    movw r0, #:lower16:XM1_ROM_STACK_TOP
    movt r0, #:upper16:XM1_ROM_STACK_TOP
    cmp sp, r0
    bne .Lfailed_fast
    ldr r0, [sp, #-4]
    bic r0, r0, #0x80000000
    cmp r0, #XM1_FAT_RETURN_OFFSET
    beq .Lguard_ok
    cmp r0, #XM1_RAW_RETURN_OFFSET
    bne .Lfailed_fast
.Lguard_ok:
    mov r0, #0
    msr APSR_nzcvq, r0
    b xm1_native_reset
.size persistent_repair_guard, .-persistent_repair_guard

.org 0xc0, 0
.global persistent_repair_callback
.type persistent_repair_callback,%function
persistent_repair_callback:
    push {r0-r3, r12, lr}
    bl xm1_primary_discriminator
    cmp r0, #0
    bne .Lfailed_fast
    movw r0, #:lower16:XM1_MEDIA_RECEIVE_TAP
    movt r0, #:upper16:XM1_MEDIA_RECEIVE_TAP
    ldr r1, [r0]
    movw r2, #:lower16:XM1_RECORDER_TAP_WORD
    movt r2, #:upper16:XM1_RECORDER_TAP_WORD
    cmp r1, r2
    bne .Lfailed_fast
    movw r2, #:lower16:XM1_STOCK_TAP_WORD
    movt r2, #:upper16:XM1_STOCK_TAP_WORD
    str r2, [r0]
    bl .Lcache_word
    movw r0, #:lower16:XM1_COMPLETION_SITE
    movt r0, #:upper16:XM1_COMPLETION_SITE
    ldr r1, [r0]
    movw r2, #:lower16:XM1_COMPLETION_WORD
    movt r2, #:upper16:XM1_COMPLETION_WORD
    cmp r1, r2
    bne .Lfailed_fast
    movw r2, #:lower16:XM1_STAGE_BRANCH_WORD
    movt r2, #:upper16:XM1_STAGE_BRANCH_WORD
    str r2, [r0]
    bl .Lcache_word
    pop {r0-r3, r12, lr}
    b xm1_native_close
.size persistent_repair_callback, .-persistent_repair_callback

.Lcache_word:
    mcr p15, 0, r0, c7, c10, 1
    dsb sy
    mcr p15, 0, r0, c7, c5, 1
    dsb sy
    isb sy
    bx lr

.org 0x200, 0
.global persistent_repair_stage
.type persistent_repair_stage,%function
persistent_repair_stage:
    push {r0-r12, lr}

    movw r0, #:lower16:XM1_COMPLETION_SITE
    movt r0, #:upper16:XM1_COMPLETION_SITE
    movw r1, #:lower16:XM1_COMPLETION_WORD
    movt r1, #:upper16:XM1_COMPLETION_WORD
    str r1, [r0]
    bl .Lcache_word

    movw r0, #:lower16:XM1_DRIVER_READY
    movt r0, #:upper16:XM1_DRIVER_READY
    ldr r0, [r0]
    cmp r0, #1
#ifdef CODED_OUTCOMES
    movne r10, #1
    bne .Lcoded_result
#else
    bne .Lfailed_fast
#endif

    bl .Lread_block
    cmp r0, #0
#ifdef CODED_OUTCOMES
    movne r10, #2
    bne .Lcoded_result
#else
    bne .Lfailed_slow
#endif
    movw r0, #:lower16:XM1_READ_DESTINATION
    movt r0, #:upper16:XM1_READ_DESTINATION
    bl .Lcrc_block
    movw r1, #:lower16:XM1_RC0_CRC32
    movt r1, #:upper16:XM1_RC0_CRC32
    cmp r0, r1
#ifdef CODED_OUTCOMES
    moveq r10, #4
    beq .Lcoded_result
#else
    moveq r10, #3
    beq .Lfinite_result
#endif
    movw r1, #:lower16:XM1_RECORDER_CRC32
    movt r1, #:upper16:XM1_RECORDER_CRC32
    cmp r0, r1
#ifdef CODED_OUTCOMES
    movne r10, #3
    bne .Lcoded_result
#else
    movne r10, #2
    bne .Lfinite_result
#endif

    movw r0, #:lower16:XM1_REPAIR_SOURCE
    movt r0, #:upper16:XM1_REPAIR_SOURCE
    bl .Lcrc_block
    movw r1, #:lower16:XM1_RC0_CRC32
    movt r1, #:upper16:XM1_RC0_CRC32
    cmp r0, r1
#ifdef CODED_OUTCOMES
    movne r10, #5
    bne .Lcoded_result
#else
    bne .Lfailed_fast
#endif

    bl .Lled_on
    mov r0, #14
    mov r1, #1
    movw r2, #:lower16:XM1_REPAIR_SOURCE
    movt r2, #:upper16:XM1_REPAIR_SOURCE
    movw r3, #:lower16:XM1_BLOCK_WRITER
    movt r3, #:upper16:XM1_BLOCK_WRITER
    blx r3
    mov r11, r0
    bl .Lled_off

    bl .Lread_block
    cmp r0, #0
#ifdef CODED_OUTCOMES
    movne r10, #6
    bne .Lcoded_result
#else
    bne .Lfailed_slow
#endif
    movt r0, #:upper16:XM1_READ_DESTINATION
    bl .Lcrc_block
    movw r1, #:lower16:XM1_RC0_CRC32
    movt r1, #:upper16:XM1_RC0_CRC32
    cmp r0, r1
#ifdef CODED_OUTCOMES
    moveq r10, #7
    beq .Lcoded_result
#else
    moveq r10, #3
    beq .Lfinite_result
#endif
    movw r1, #:lower16:XM1_RECORDER_CRC32
    movt r1, #:upper16:XM1_RECORDER_CRC32
    cmp r0, r1
#ifdef CODED_OUTCOMES
    moveq r10, #8
    movne r10, #9
    b .Lcoded_result
#else
    moveq r10, #1
    beq .Lfinite_result
    b .Lfailed_slow
#endif

#ifdef CODED_OUTCOMES
.Lcoded_result:
    mov r9, r10
.Lcoded_packet:
    bl .Lled_on
    bl .Lsync_delay
    bl .Lled_off
    bl .Lsync_delay
    mov r10, r9
.Lcoded_pulse:
    bl .Lled_on
    bl .Llong_delay
    bl .Lled_off
    bl .Llong_delay
    subs r10, r10, #1
    bne .Lcoded_pulse
    bl .Lsync_delay
    b .Lcoded_packet
#endif
.Lfinite_result:
    bl .Lled_off
    bl .Llong_delay
.Lresult_pulse:
    bl .Lled_on
    bl .Llong_delay
    bl .Lled_off
    bl .Llong_delay
    subs r10, r10, #1
    bne .Lresult_pulse
    pop {r0-r12, lr}
    b xm1_completion_helper

.Lread_block:
    mov r0, #14
    mov r1, #1
    movw r2, #:lower16:XM1_READ_DESTINATION
    movt r2, #:upper16:XM1_READ_DESTINATION
    mov r3, #1
    movw r12, #:lower16:XM1_BLOCK_READER
    movt r12, #:upper16:XM1_BLOCK_READER
    bx r12

.Lcrc_block:
    push {r4-r9, lr}
    mov r4, r0
    movw r5, #0
    movt r5, #2
    mvn r6, #0
    movw r9, #0x8320
    movt r9, #0xedb8
.Lcrc_byte:
    ldrb r7, [r4], #1
    eor r6, r6, r7
    mov r8, #8
.Lcrc_bit:
    lsrs r6, r6, #1
    eorcs r6, r6, r9
    subs r8, r8, #1
    bne .Lcrc_bit
    subs r5, r5, #1
    bne .Lcrc_byte
    mvn r0, r6
    pop {r4-r9, pc}

.Lfailed_fast:
    bl .Lled_on
    bl .Lshort_delay
    bl .Lled_off
    bl .Lshort_delay
    b .Lfailed_fast

.Lfailed_slow:
    bl .Lled_on
    bl .Llong_delay
    bl .Lled_off
    bl .Llong_delay
    b .Lfailed_slow

.Lled_on:
    movw r0, #0x8084
    movt r0, #0xfff8
    mov r1, #1
    str r1, [r0]
    movw r0, #0x93c0
    movt r0, #0xfff8
    movw r1, #0x0001
    movt r1, #0x0005
    str r1, [r0, #4]
    movw r1, #0x0001
    movt r1, #0x0001
    str r1, [r0]
    dsb sy
    bx lr
.Lled_off:
    movw r0, #0x93c0
    movt r0, #0xfff8
    movw r1, #0x0000
    movt r1, #0x0001
    str r1, [r0]
    dsb sy
    bx lr
.Llong_delay:
    mov r0, #0x02000000
.Llong_delay_loop:
    subs r0, r0, #1
    bne .Llong_delay_loop
    bx lr
#ifdef CODED_OUTCOMES
.Lsync_delay:
    mov r0, #0x08000000
.Lsync_delay_loop:
    subs r0, r0, #1
    bne .Lsync_delay_loop
    bx lr
#endif
.Lshort_delay:
    mov r0, #0x00800000
.Lshort_delay_loop:
    subs r0, r0, #1
    bne .Lshort_delay_loop
    bx lr
.size persistent_repair_stage, .-persistent_repair_stage
