    .syntax unified
    .arm
    .text
    .balign 4

    .macro window low, high, next, store
    movw r12, #:lower16:\low
    movt r12, #:upper16:\low
    cmp r0, r12
    blo \next
    movw r12, #:lower16:\high
    movt r12, #:upper16:\high
    cmp r0, r12
    blo \store
    .endm

    .macro checked_store name, width, instruction
    .global \name
    .type \name, %function
\name:
    window 0x0096cc64, (0x0096d1aa - \width + 1), 1f, 5f
1:
    window 0x0089ec00, (0x0089ede0 - \width + 1), 2f, 5f
2:
    window 0x409c281c, (0x409c29b6 - \width + 1), 3f, 5f
3:
    window 0x409c29b8, (0x409c29c8 - \width + 1), 4f, 5f
4:
    bx lr
5:
    \instruction r1, [r0]
    bx lr
    .size \name, . - \name
    .endm

    checked_store xm1_storage_store8, 1, strb
    checked_store xm1_storage_store16, 2, strh

    .macro retained_call name, target
    .global \name
    .type \name, %function
\name:
    push {r11, lr}
    mov r11, sp
    bl \target
    pop {r11, pc}
    .size \name, . - \name
    .endm

    retained_call xm1_storage_context, xm1_backup_context723cb8
    retained_call xm1_storage_acquire, xm1_backup_acquire724b68
    retained_call xm1_storage_release, xm1_backup_release724c00
    retained_call xm1_storage_diagnostic, xm1_backup_diagnostic726c74
    retained_call xm1_storage_write, xm1_backup_write6a59b8
    retained_call xm1_storage_report, xm1_backup_report633594

    .global xm1_storage_output8
    .type xm1_storage_output8, %function
xm1_storage_output8:
    movw r12, #0x19a
    cmp r1, r12
    bxhi lr
    adds r12, r0, r1
    bxcs lr
    cmp r2, r1
    bxhs lr
    strb r3, [r0, r2]
    bx lr
    .size xm1_storage_output8, . - xm1_storage_output8

    .global xm1_storage_output16
    .type xm1_storage_output16, %function
xm1_storage_output16:
    movw r12, #0x19a
    cmp r1, r12
    bxhi lr
    adds r12, r0, r1
    bxcs lr
    adds r12, r2, #2
    bxcs lr
    cmp r12, r1
    bxhi lr
    add r12, r0, r2
    tst r12, #1
    bxne lr
    strh r3, [r12]
    bx lr
    .size xm1_storage_output16, . - xm1_storage_output16

    .global xm1_storage_status32
    .type xm1_storage_status32, %function
xm1_storage_status32:
    movw r12, #0xedc4
    movt r12, #0x0089
    str r0, [r12]
    bx lr
    .size xm1_storage_status32, . - xm1_storage_status32
