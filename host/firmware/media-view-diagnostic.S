// X-M1 1.50 read-only comparison of boot-time and application media mappings.
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
.equ XM1_BOOT_BAD_LIST,       0x00004310
.equ XM1_BOOT_SNAPSHOT,       0x00006f00
.equ XM1_DRIVER_READY,        0x00828574
.equ XM1_APP_BAD_LIST_PTR,    0x009552f0
.equ XM1_APP_REMAP_14,        0x0082859c
.equ XM1_READ_DESTINATION,    0x00020000
#ifdef PHYSICAL_WRITE
.equ XM1_REPAIR_SOURCE,       XM1_READ_DESTINATION
#else
.equ XM1_REPAIR_SOURCE,       0x00040000
#endif
.equ XM1_RC0_REFERENCE,       0x00008000
.equ XM1_RC0_MIDDLE,          0x00016000
.equ XM1_RC0_HEAD_BYTES,      0x00010000
.equ XM1_RC0_TAIL,            0x00088000
.equ XM1_BLOCK_PROGRAM,       0x006faf5c
.equ XM1_MAP_REFRESH,         0x006a90ac
.equ XM1_SOURCE_CLEAN,        0x0004263c
.equ XM1_STORAGE_LOCK,        0x006faf00
.equ XM1_STORAGE_UNLOCK,      0x006faf40
.equ XM1_PHYSICAL_READER,     0x006fc138
.equ XM1_LOWER_LOCK,          0x006fbac4
.equ XM1_LOWER_UNLOCK,        0x006fbadc
.equ XM1_CACHE_INVALIDATE,    0x00042634
.equ XM1_RC0_CRC32,           0x92863e8b
.equ XM1_RECORDER_CRC32,      0xd1943bd0
.equ XM1_PHYSICAL_BLOCKS,     0x0000013a
.equ XM1_MAX_BAD_BLOCKS,      20

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
    bl .Lsnapshot_boot_map
    movw r0, #:lower16:XM1_MEDIA_RECEIVE_TAP
    movt r0, #:upper16:XM1_MEDIA_RECEIVE_TAP
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

.Lsnapshot_boot_map:
    push {r4-r8, lr}
    movw r4, #:lower16:XM1_BOOT_BAD_LIST
    movt r4, #:upper16:XM1_BOOT_BAD_LIST
    mov r0, r4
    bl .Lmap_bad_list
    mov r4, r0
    mov r5, r1
    mov r6, r2
    movw r7, #:lower16:XM1_BOOT_SNAPSHOT
    movt r7, #:upper16:XM1_BOOT_SNAPSHOT
    str r4, [r7]
    str r5, [r7, #4]
    str r6, [r7, #8]
    pop {r4-r8, pc}

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

    movw r4, #:lower16:XM1_BOOT_SNAPSHOT
    movt r4, #:upper16:XM1_BOOT_SNAPSHOT
    ldr r0, [r4]
    cmp r0, #0
    movne r9, #1
    bne .Lemit_status
    ldr r5, [r4, #4]

    movw r0, #:lower16:XM1_DRIVER_READY
    movt r0, #:upper16:XM1_DRIVER_READY
    ldr r0, [r0]
    cmp r0, #1
    movne r9, #2
    bne .Lemit_status

    movw r0, #:lower16:XM1_APP_BAD_LIST_PTR
    movt r0, #:upper16:XM1_APP_BAD_LIST_PTR
    ldr r4, [r0]
    movw r0, #:lower16:XM1_BOOT_BAD_LIST
    movt r0, #:upper16:XM1_BOOT_BAD_LIST
    cmp r4, r0
    movne r9, #3
    bne .Lemit_status
    bl .Lmap_bad_list
    cmp r0, #0
    movne r9, #3
    bne .Lemit_status
    mov r6, r1

    movw r0, #:lower16:XM1_APP_REMAP_14
    movt r0, #:upper16:XM1_APP_REMAP_14
    ldrh r0, [r0]
    cmp r0, r6
    movne r9, #4
    bne .Lemit_status

    sub r10, r5, #13
    subs r11, r6, r5
    rsbmi r11, r11, #0
    add r11, r11, #1

    mov r0, r5
    movw r1, #:lower16:XM1_READ_DESTINATION
    movt r1, #:upper16:XM1_READ_DESTINATION
    bl .Lread_physical_block
    cmp r0, #0
    movne r9, #5
    bne .Lemit_status
#ifdef PHYSICAL_WRITE
    movw r0, #:lower16:XM1_READ_DESTINATION
    movt r0, #:upper16:XM1_READ_DESTINATION
    movw r1, #:lower16:XM1_RC0_REFERENCE
    movt r1, #:upper16:XM1_RC0_REFERENCE
    bl .Lrecorder_equal
    cmp r0, #0
    beq .Lread_app_physical
    movw r0, #:lower16:XM1_READ_DESTINATION
    movt r0, #:upper16:XM1_READ_DESTINATION
    movw r1, #:lower16:XM1_RC0_REFERENCE
    movt r1, #:upper16:XM1_RC0_REFERENCE
    bl .Lblocks_equal
    cmp r0, #0
    moveq r9, #7
    movne r9, #6
#else
    movw r0, #:lower16:XM1_READ_DESTINATION
    movt r0, #:upper16:XM1_READ_DESTINATION
    bl .Lcrc_block
    movw r1, #:lower16:XM1_RECORDER_CRC32
    movt r1, #:upper16:XM1_RECORDER_CRC32
    cmp r0, r1
    beq .Lread_app_physical
    movw r1, #:lower16:XM1_RC0_CRC32
    movt r1, #:upper16:XM1_RC0_CRC32
    cmp r0, r1
    moveq r9, #7
    movne r9, #6
#endif
    b .Lemit_status

.Lread_app_physical:
    mov r0, r6
    movw r1, #:lower16:XM1_READ_DESTINATION
    movt r1, #:upper16:XM1_READ_DESTINATION
    bl .Lread_physical_block
    cmp r0, #0
    movne r9, #8
    bne .Lemit_status
#ifdef PHYSICAL_WRITE
    movw r0, #:lower16:XM1_READ_DESTINATION
    movt r0, #:upper16:XM1_READ_DESTINATION
    movw r1, #:lower16:XM1_RC0_REFERENCE
    movt r1, #:upper16:XM1_RC0_REFERENCE
    bl .Lblocks_equal
    cmp r0, #0
    beq .Lclassify_maps
    movw r0, #:lower16:XM1_READ_DESTINATION
    movt r0, #:upper16:XM1_READ_DESTINATION
    movw r1, #:lower16:XM1_RC0_REFERENCE
    movt r1, #:upper16:XM1_RC0_REFERENCE
    bl .Lrecorder_equal
    cmp r0, #0
    bne .Lapp_other
    cmp r5, #14
    bne .Lapp_recorder_refusal
    cmp r6, #14
    beq .Lrepair_candidate
.Lapp_recorder_refusal:
    mov r9, #13
    b .Lemit_status
.Lapp_other:
    mov r9, #14
    b .Lemit_status
#else
    movw r0, #:lower16:XM1_READ_DESTINATION
    movt r0, #:upper16:XM1_READ_DESTINATION
    bl .Lcrc_block
    movw r1, #:lower16:XM1_RC0_CRC32
    movt r1, #:upper16:XM1_RC0_CRC32
    cmp r0, r1
    beq .Lclassify_maps
    movw r1, #:lower16:XM1_RECORDER_CRC32
    movt r1, #:upper16:XM1_RECORDER_CRC32
    cmp r0, r1
    moveq r9, #13
    movne r9, #14
    b .Lemit_status
#endif

.Lclassify_maps:
    cmp r6, r5
    moveq r9, #12
    beq .Lemit_status
    add r0, r5, #1
    cmp r6, r0
    moveq r9, #9
    movhi r9, #10
    movlo r9, #11

#ifdef PHYSICAL_WRITE
.Lrepair_candidate:
    movw r0, #:lower16:XM1_RC0_REFERENCE
    movt r0, #:upper16:XM1_RC0_REFERENCE
    bl .Lcrc_block
    movw r1, #:lower16:XM1_RC0_CRC32
    movt r1, #:upper16:XM1_RC0_CRC32
    cmp r0, r1
    movne r9, #15
    bne .Lemit_status

    bl .Lled_on
    bl .Lwrite_physical14_guarded
    mov r8, r0
    bl .Lled_off
    movw r0, #0xff00
    movt r0, #0xffff
    cmp r8, r0
    moveq r9, #16
    beq .Lemit_status

    movw r0, #:lower16:XM1_APP_BAD_LIST_PTR
    movt r0, #:upper16:XM1_APP_BAD_LIST_PTR
    ldr r4, [r0]
    movw r0, #:lower16:XM1_BOOT_BAD_LIST
    movt r0, #:upper16:XM1_BOOT_BAD_LIST
    cmp r4, r0
    bne .Lpost_map_invalid
    mov r0, r4
    bl .Lmap_bad_list
    cmp r0, #0
    bne .Lpost_map_invalid
    mov r6, r1
    movw r0, #:lower16:XM1_APP_REMAP_14
    movt r0, #:upper16:XM1_APP_REMAP_14
    ldrh r0, [r0]
    cmp r0, r6
    bne .Lpost_map_invalid
    b .Lpost_map_valid
.Lpost_map_invalid:
    cmp r8, #0
    moveq r9, #18
    movne r9, #24
    b .Lemit_status

.Lpost_map_valid:
    sub r10, r5, #13
    subs r11, r6, r5
    rsbmi r11, r11, #0
    add r11, r11, #1
    mov r0, r6
    movw r1, #:lower16:XM1_READ_DESTINATION
    movt r1, #:upper16:XM1_READ_DESTINATION
    bl .Lread_physical_block
    cmp r0, #0
    bne .Lpost_read_failed
    movw r0, #:lower16:XM1_READ_DESTINATION
    movt r0, #:upper16:XM1_READ_DESTINATION
    movw r1, #:lower16:XM1_RC0_REFERENCE
    movt r1, #:upper16:XM1_RC0_REFERENCE
    bl .Lblocks_equal
    cmp r0, #0
    beq .Lpost_rc0
    movw r0, #:lower16:XM1_READ_DESTINATION
    movt r0, #:upper16:XM1_READ_DESTINATION
    movw r1, #:lower16:XM1_RC0_REFERENCE
    movt r1, #:upper16:XM1_RC0_REFERENCE
    bl .Lrecorder_equal
    cmp r0, #0
    beq .Lpost_recorder
    cmp r8, #0
    moveq r9, #23
    movne r9, #29
    b .Lemit_status
.Lpost_read_failed:
    cmp r8, #0
    moveq r9, #19
    movne r9, #25
    b .Lemit_status
.Lpost_rc0:
    cmp r8, #0
    bne .Lpost_rc0_error
    cmp r6, r5
    moveq r9, #20
    movne r9, #21
    b .Lemit_status
.Lpost_rc0_error:
    cmp r6, r5
    moveq r9, #26
    movne r9, #27
    b .Lemit_status
.Lpost_recorder:
    cmp r8, #0
    moveq r9, #22
    movne r9, #28
#endif


.Lemit_status:
.Lemit_cycle:
    mov r0, #4
    mov r1, r9
    bl .Lpacket
    cmp r9, #8
    blo .Lemit_cycle
#ifdef PHYSICAL_WRITE
    cmp r9, #29
#else
    cmp r9, #14
#endif
    bhi .Lemit_cycle
    mov r0, #8
    mov r1, r10
    bl .Lpacket
    mov r0, #12
    mov r1, r11
    bl .Lpacket
    b .Lemit_cycle

.Lmap_bad_list:
    push {r4-r8, lr}
    mov r4, r0
    ldr r2, [r4]
    cmp r2, #XM1_MAX_BAD_BLOCKS
    bhi .Lmap_invalid
    mov r1, #14
    mov r5, #0
    mov r6, #0
.Lmap_loop:
    cmp r5, r2
    beq .Lmap_valid
    add r7, r4, r5, lsl #2
    ldr r7, [r7, #4]
    movw r8, #XM1_PHYSICAL_BLOCKS
    cmp r7, r8
    bhs .Lmap_invalid
    cmp r5, #0
    beq .Lmap_ordered
    cmp r7, r6
    bls .Lmap_invalid
.Lmap_ordered:
    mov r6, r7
    cmp r7, r1
    addls r1, r1, #1
    add r5, r5, #1
    b .Lmap_loop
.Lmap_valid:
    mov r0, #0
    pop {r4-r8, pc}
.Lmap_invalid:
    mov r0, #1
    mov r1, #0
    pop {r4-r8, pc}

.Lread_physical_block:
    push {r4-r8, lr}
    sub sp, sp, #24
    mov r4, r0
    orr r5, r1, #0x40000000
    mov r6, #0
    str r6, [sp]
    add r7, sp, #8
    str r6, [r7]
    str r6, [r7, #4]
    str r7, [sp, #4]
    str r6, [sp, #16]
    movw r8, #1000
    str r8, [sp, #20]
    add r0, sp, #16
    movw r12, #:lower16:XM1_STORAGE_LOCK
    movt r12, #:upper16:XM1_STORAGE_LOCK
    blx r12
    cmp r0, #0
    bne .Lraw_done
    mov r0, #0
    movw r12, #:lower16:XM1_CACHE_INVALIDATE
    movt r12, #:upper16:XM1_CACHE_INVALIDATE
    blx r12
    movw r12, #:lower16:XM1_LOWER_LOCK
    movt r12, #:upper16:XM1_LOWER_LOCK
    blx r12
    mov r0, r4
    mov r1, r5
    mov r2, #0
    movw r3, #1000
    movw r12, #:lower16:XM1_PHYSICAL_READER
    movt r12, #:upper16:XM1_PHYSICAL_READER
    blx r12
    mov r4, r0
    movw r12, #:lower16:XM1_LOWER_UNLOCK
    movt r12, #:upper16:XM1_LOWER_UNLOCK
    blx r12
    movw r12, #:lower16:XM1_STORAGE_UNLOCK
    movt r12, #:upper16:XM1_STORAGE_UNLOCK
    blx r12
    mov r0, r4
.Lraw_done:
    add sp, sp, #24
    pop {r4-r8, pc}

#ifdef PHYSICAL_WRITE
.Lwrite_physical14_guarded:
    push {r4-r8, lr}
    sub sp, sp, #40
    mov r0, #0
    str r0, [sp, #32]
    movw r0, #1000
    str r0, [sp, #36]
    add r0, sp, #32
    movw r12, #:lower16:XM1_STORAGE_LOCK
    movt r12, #:upper16:XM1_STORAGE_LOCK
    blx r12
    cmp r0, #0
    bne .Lguarded_refusal
    mov r7, #1

    movw r0, #:lower16:XM1_APP_REMAP_14
    movt r0, #:upper16:XM1_APP_REMAP_14
    ldrh r0, [r0]
    cmp r0, #14
    bne .Lguarded_unlock_refusal

    mov r0, #0
    movw r12, #:lower16:XM1_CACHE_INVALIDATE
    movt r12, #:upper16:XM1_CACHE_INVALIDATE
    blx r12
    movw r12, #:lower16:XM1_LOWER_LOCK
    movt r12, #:upper16:XM1_LOWER_LOCK
    blx r12
    mov r0, #14
    movw r1, #:lower16:XM1_READ_DESTINATION
    movt r1, #:upper16:XM1_READ_DESTINATION
    orr r1, r1, #0x40000000
    mov r2, #0
    movw r3, #1000
    mov r4, #0
    str r4, [sp]
    add r5, sp, #32
    str r4, [r5]
    str r4, [r5, #4]
    str r5, [sp, #4]
    movw r12, #:lower16:XM1_PHYSICAL_READER
    movt r12, #:upper16:XM1_PHYSICAL_READER
    blx r12
    mov r4, r0
    movw r12, #:lower16:XM1_LOWER_UNLOCK
    movt r12, #:upper16:XM1_LOWER_UNLOCK
    blx r12
    cmp r4, #0
    bne .Lguarded_unlock_refusal
    movw r0, #:lower16:XM1_READ_DESTINATION
    movt r0, #:upper16:XM1_READ_DESTINATION
    movw r1, #:lower16:XM1_RC0_REFERENCE
    movt r1, #:upper16:XM1_RC0_REFERENCE
    bl .Lrecorder_equal
    cmp r0, #0
    bne .Lguarded_unlock_refusal
    movw r0, #:lower16:XM1_READ_DESTINATION
    movt r0, #:upper16:XM1_READ_DESTINATION
    movw r1, #:lower16:XM1_RC0_REFERENCE
    movt r1, #:upper16:XM1_RC0_REFERENCE
    bl .Lrestore_block_words
    bl .Lblocks_equal
    cmp r0, #0
    bne .Lguarded_unlock_refusal

    movw r0, #:lower16:XM1_REPAIR_SOURCE
    movt r0, #:upper16:XM1_REPAIR_SOURCE
    movw r1, #0
    movt r1, #2
    movw r12, #:lower16:XM1_SOURCE_CLEAN
    movt r12, #:upper16:XM1_SOURCE_CLEAN
    blx r12
    mov r0, #14
    str r0, [sp]
    mov r0, #1
    str r0, [sp, #4]
    movw r0, #0x8c9c
    movt r0, #0x007c
    ldr r0, [r0]
    movw r1, #0x8578
    movt r1, #0x0082
    ldr r1, [r1]
    sub r0, r0, r1
    str r0, [sp, #8]
    movw r0, #:lower16:XM1_REPAIR_SOURCE
    movt r0, #:upper16:XM1_REPAIR_SOURCE
    orr r0, r0, #0x40000000
    str r0, [sp, #12]
    mov r0, #1
    str r0, [sp, #16]
    mov r0, #0
    str r0, [sp, #20]
    str r0, [sp, #24]
    movw r0, #1000
    str r0, [sp, #28]
    mov r0, sp
    movw r12, #:lower16:XM1_BLOCK_PROGRAM
    movt r12, #:upper16:XM1_BLOCK_PROGRAM
    blx r12
    mov r4, r0
    ldr r5, [sp, #20]
    movw r12, #:lower16:XM1_STORAGE_UNLOCK
    movt r12, #:upper16:XM1_STORAGE_UNLOCK
    blx r12
    cmp r4, #0
    bne .Lguarded_done
    cmp r5, #0
    beq .Lguarded_done
    movw r12, #:lower16:XM1_MAP_REFRESH
    movt r12, #:upper16:XM1_MAP_REFRESH
    blx r12
.Lguarded_done:
    mov r0, r4
    add sp, sp, #40
    pop {r4-r8, pc}
.Lguarded_unlock_refusal:
    movw r12, #:lower16:XM1_STORAGE_UNLOCK
    movt r12, #:upper16:XM1_STORAGE_UNLOCK
    blx r12
.Lguarded_refusal:
    movw r0, #0xff00
    movt r0, #0xffff
    add sp, sp, #40
    pop {r4-r8, pc}

.Lrecorder_equal:
    adr r2, .Lblock_delta
    b .Lcompare_blocks
.Lblocks_equal:
    mov r2, #0
.Lcompare_blocks:
    push {r4-r8, lr}
    mov r5, #0
    mov r6, #0x20000
    cmp r2, #0
    ldrne r6, [r2]
.Lblocks_equal_loop:
    ldr r3, [r0], #4
    cmp r1, #0x10000
    movweq r1, #:lower16:XM1_RC0_MIDDLE
    movteq r1, #:upper16:XM1_RC0_MIDDLE
    cmp r1, #0x1e000
    movweq r1, #:lower16:XM1_RC0_TAIL
    movteq r1, #:upper16:XM1_RC0_TAIL
    ldr r4, [r1], #4
    cmp r5, r6
    bne .Lcompare_word
    ldr r7, [r2, #8]
    cmp r4, r7
    bne .Lblocks_differ
    ldr r4, [r2, #4]
    add r2, r2, #12
    ldr r6, [r2]
.Lcompare_word:
    cmp r3, r4
    bne .Lblocks_differ
    add r5, r5, #4
    cmp r5, #0x20000
    bne .Lblocks_equal_loop
    mov r0, #0
    pop {r4-r8, pc}
.Lblocks_differ:
    mov r0, #1
    pop {r4-r8, pc}

.Lrestore_block_words:
    push {r1, r4, r5, lr}
    movw r1, #:lower16:(XM1_RC0_TAIL-XM1_RC0_HEAD_BYTES)
    movt r1, #:upper16:(XM1_RC0_TAIL-XM1_RC0_HEAD_BYTES)
    adr r2, .Lblock_delta
    mov r3, #4
.Lrestore_word:
    ldr r4, [r2], #12
    ldr r5, [r1, r4]
    str r5, [r0, r4]
    subs r3, r3, #1
    bne .Lrestore_word
    pop {r1, r4, r5, pc}

.Lblock_delta:
    .word 0x1f120, 0xa9c90446, 0xffdc0446
    .word 0x1f124, 0x04d8eb34, 0x04d8ebff
    .word 0x1f480, 0x280d4381, 0x28024381
    .word 0x1f484, 0xebffffda, 0x0803c8da
    .word 0x20000
#endif

.Lcrc_block:
    push {r4-r9, lr}
    mov r4, r0
    movw r5, #0
    movt r5, #2
    mvn r6, #0
    movw r9, #0x8320
    movt r9, #0xedb8
.Lcrc_byte:
#ifdef PHYSICAL_WRITE
    cmp r4, #0x10000
    movweq r4, #:lower16:XM1_RC0_MIDDLE
    movteq r4, #:upper16:XM1_RC0_MIDDLE
    cmp r4, #0x1e000
    movweq r4, #:lower16:XM1_RC0_TAIL
    movteq r4, #:upper16:XM1_RC0_TAIL
#endif
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

.Lpacket:
    push {r4-r6, lr}
    mov r4, r0
    mov r5, r1
    bl .Lled_on
.Lpacket_sync_on:
    bl .Llong_delay
    subs r4, r4, #1
    bne .Lpacket_sync_on
    bl .Lled_off
    mov r4, #4
.Lpacket_sync_off:
    bl .Llong_delay
    subs r4, r4, #1
    bne .Lpacket_sync_off
    mov r6, r5
.Lpacket_pulse:
    bl .Lled_on
    bl .Llong_delay
    bl .Lled_off
    bl .Llong_delay
    subs r6, r6, #1
    bne .Lpacket_pulse
    mov r4, #4
.Lpacket_gap:
    bl .Llong_delay
    subs r4, r4, #1
    bne .Lpacket_gap
    pop {r4-r6, pc}

.Lfailed_fast:
    bl .Lled_on
    bl .Lshort_delay
    bl .Lled_off
    bl .Lshort_delay
    b .Lfailed_fast

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
.Lshort_delay:
    mov r0, #0x00800000
.Lshort_delay_loop:
    subs r0, r0, #1
    bne .Lshort_delay_loop
    bx lr
.size persistent_repair_stage, .-persistent_repair_stage

.org 0xf00, 0
.Lboot_snapshot:
    .word 1
    .word 0
    .word 0
