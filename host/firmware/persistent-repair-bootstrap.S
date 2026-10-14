.syntax unified
.arch armv7-r
.arm
.section .text,"ax",%progbits
.balign 4

.equ XM1_ROM_STACK_TOP, 0xfff00f00
.equ XM1_FAT_RETURN, 0x0000015c
.equ XM1_RAW_RETURN, 0x00000174
#ifdef PHYSICAL_WRITE
.equ XM1_RELOC_BASE, 0x000c0000
#else
.equ XM1_RELOC_BASE, 0x00070000
#endif
.equ XM1_FAT_OPEN, 0x80001dc4
.equ XM1_FAT_READ, 0x80001e88
.equ XM1_RAW_READ, 0x8000148c

.global persistent_repair_bootstrap
.type persistent_repair_bootstrap,%function
persistent_repair_bootstrap:
    movw r0, #:lower16:XM1_ROM_STACK_TOP
    movt r0, #:upper16:XM1_ROM_STACK_TOP
    cmp sp, r0
    bne .Lrejected
    ldr r0, [sp, #-4]
    bic r0, r0, #0x80000000
    cmp r0, #XM1_FAT_RETURN
    beq .Laccepted
    cmp r0, #XM1_RAW_RETURN
    bne .Lrejected
.Laccepted:
    push {r1-r12, lr}
    push {r0}
    bl .Lled_on

    mov r0, #0
    movw r1, #:lower16:XM1_RELOC_BASE
    movt r1, #:upper16:XM1_RELOC_BASE
    mov r2, #128
.Lcopy:
    ldr r3, [r0], #4
    str r3, [r1], #4
    subs r2, r2, #1
    bne .Lcopy
    dsb sy
    isb sy
    adr r0, .Lrelocated
    movw r1, #:lower16:XM1_RELOC_BASE
    movt r1, #:upper16:XM1_RELOC_BASE
    add r0, r0, r1
    bx r0

.Lrelocated:
    ldr r0, [sp]
    cmp r0, #XM1_FAT_RETURN
    beq .Lload_fat

    movw r4, #0
    movt r4, #0x4000
    mov r5, #3
#ifdef PHYSICAL_WRITE
    movw r6, #1280
#else
    movw r6, #768
#endif
.Lraw_loop:
    mov r0, #1
    mov r1, r4
    mov r2, r5
    movw r3, #:lower16:XM1_RAW_READ
    movt r3, #:upper16:XM1_RAW_READ
    blx r3
    cmp r0, #0
    bne .Lload_failed
    add r4, r4, #512
    add r5, r5, #1
    subs r6, r6, #1
    bne .Lraw_loop
    b .Lloaded

.Lload_fat:
    adr r0, .Lrecovery_name
    movw r3, #:lower16:XM1_FAT_OPEN
    movt r3, #:upper16:XM1_FAT_OPEN
    blx r3
    cmp r0, #0
    bne .Lload_failed
    movw r4, #0
    movt r4, #0x4000
#ifdef PHYSICAL_WRITE
    mov r5, #20
#else
    mov r5, #12
#endif
.Lfat_loop:
    sub sp, sp, #8
    mov r0, r4
    movw r1, #0x8000
    mov r2, sp
    movw r3, #:lower16:XM1_FAT_READ
    movt r3, #:upper16:XM1_FAT_READ
    blx r3
    ldrh r6, [sp]
    add sp, sp, #8
    cmp r0, #0
    bne .Lload_failed
    cmp r6, #0x8000
    bne .Lload_failed
    add r4, r4, #0x8000
    subs r5, r5, #1
    bne .Lfat_loop

.Lloaded:
    mov r0, #0
    mcr p15, 0, r0, c7, c5, 0
    dsb sy
    isb sy
    bl .Lled_off
    pop {r0}
    pop {r1-r12, lr}
    str r0, [sp, #-4]
    mov r0, #0
    msr APSR_nzcvq, r0
    mov pc, r0

.Lrejected:
    bl .Lled_setup
    b .Lfailure_blink
.Lload_failed:
    bl .Lled_off
.Lfailure_blink:
    bl .Lled_on_value
    bl .Ldelay
    bl .Lled_off_value
    bl .Ldelay
    b .Lfailure_blink

.Lled_on:
    mov r12, lr
    bl .Lled_setup
    mov lr, r12
.Lled_on_value:
    movw r0, #0x93c0
    movt r0, #0xfff8
    movw r1, #0x0001
    movt r1, #0x0001
    str r1, [r0]
    dsb sy
    bx lr
.Lled_off:
.Lled_off_value:
    movw r0, #0x93c0
    movt r0, #0xfff8
    movw r1, #0x0000
    movt r1, #0x0001
    str r1, [r0]
    dsb sy
    bx lr
.Lled_setup:
    movw r0, #0x8084
    movt r0, #0xfff8
    mov r1, #1
    str r1, [r0]
    dsb sy
    movw r0, #0x93c0
    movt r0, #0xfff8
    movw r1, #0x0001
    movt r1, #0x0005
    str r1, [r0, #4]
    dsb sy
    bx lr
.Ldelay:
    mov r0, #0x01000000
.Ldelay_loop:
    subs r0, r0, #1
    bne .Ldelay_loop
    bx lr

.Lrecovery_name:
    .asciz "ffrecov.bin"
.balign 4
.size persistent_repair_bootstrap, .-persistent_repair_bootstrap
.org 512, 0
