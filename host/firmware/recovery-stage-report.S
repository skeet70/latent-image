.syntax unified
.arch armv7-r
.arm

.equ XM1_SD_BASE,          0xfff50000
.equ XM1_SD_STATE,         0xfff00434
.equ XM1_MODE_WORD,        0xfff98000
.equ XM1_REPORT_BASE_LBA,  0x00020800
.equ XM1_REPORT_BUFFER,    0x00007000
.equ XM1_READBACK_BUFFER,  0x00007200
.equ XM1_WAIT_LIMIT,       0x00010000

#ifndef XM1_REPORT_ATTEMPT
#define XM1_REPORT_ATTEMPT 2
#endif

.section .report.text,"ax",%progbits
.balign 4
.global xm1_report_stage
.type xm1_report_stage,%function
// r0 = stage 0..3, r1 = stage-specific evidence word.
// Returns zero only after one-sector write, readback and exact comparison.
xm1_report_stage:
    push {r4-r11, lr}
    mov r8, r0
    mov r9, r1
    movw r4, #:lower16:XM1_SD_BASE
    movt r4, #:upper16:XM1_SD_BASE
    movw r10, #:lower16:XM1_SD_STATE
    movt r10, #:upper16:XM1_SD_STATE
    movw r6, #:lower16:XM1_REPORT_BUFFER
    movt r6, #:upper16:XM1_REPORT_BUFFER

    mov r0, #0
    mov r1, #128
.Lzero_record:
    str r0, [r6], #4
    subs r1, r1, #1
    bne .Lzero_record
    sub r6, r6, #512

    movw r0, #0x4d58
    movt r0, #0x5331
    str r0, [r6, #0]
    movw r0, #0x4154
    movt r0, #0x4547
    str r0, [r6, #4]
    mov r0, #1
    str r0, [r6, #8]
    str r8, [r6, #12]
    mvn r0, r8
    str r0, [r6, #16]
    mov r0, #XM1_REPORT_ATTEMPT
    str r0, [r6, #20]
    str r9, [r6, #24]
    movw r0, #:lower16:XM1_MODE_WORD
    movt r0, #:upper16:XM1_MODE_WORD
    ldr r0, [r0]
    str r0, [r6, #28]
    ldr r0, [r10, #0x10]
    str r0, [r6, #32]
    ldr r1, [r10, #0x0c]
    str r1, [r6, #36]
    ldr r2, [r4, #0x24]
    str r2, [r6, #40]
    ldrh r3, [r4, #0x2c]
    str r3, [r6, #44]
    ldrh r5, [r4, #0x30]
    str r5, [r6, #48]
    ldrh r5, [r4, #0x32]
    str r5, [r6, #52]
    movw r5, #0x4f43
    movt r5, #0x504d
    str r5, [r6, #504]

    mov r5, #0
    mov r7, #127
    mov r11, r6
.Lchecksum:
    ldr r3, [r11], #4
    add r5, r5, r3
    subs r7, r7, #1
    bne .Lchecksum
    rsb r5, r5, #0
    str r5, [r6, #508]
    ldr r0, [r6, #32]
    ldr r9, [r6, #36]
    ldr r2, [r6, #40]
    cmp r0, #1
    bne .Lfail_precondition
    cmp r9, #1
    bhi .Lfail_precondition
    and r3, r2, #0x30000
    movw r5, #0
    movt r5, #3
    cmp r3, r5
    bne .Lfail_precondition
    tst r2, #3
    bne .Lfail_precondition
    ldrh r3, [r4, #0x2c]
    and r3, r3, #7
    cmp r3, #7
    bne .Lfail_precondition
    ldrb r3, [r4, #0x28]
    tst r3, #2
    beq .Lfail_precondition

    bl .Lcard_transfer_state
    cmp r0, #0
    bne .Lfail_cmd13_before

    movw r7, #0x0800
    movt r7, #0x0002
    add r7, r7, r8
    cmp r9, #0
    lsleq r7, r7, #9
    mov r0, #0x33
    strh r0, [r4, #0x30]
    movw r0, #0xffff
    strh r0, [r4, #0x32]
    movw r0, #0x0200
    strh r0, [r4, #0x04]
    mov r0, #1
    strh r0, [r4, #0x06]
    str r7, [r4, #0x08]
    mov r0, #0x26
    strh r0, [r4, #0x0c]
    movw r0, #0x193a
    strh r0, [r4, #0x0e]
    mov r0, #1
    bl .Lwait_status
    cmp r0, #0
    bne .Lfail_cmd25
    bl .Lr1_error
    cmp r0, #0
    bne .Lfail_cmd25
    mov r0, #1
    strh r0, [r4, #0x30]
    mov r0, #0x10
    bl .Lwait_status
    cmp r0, #0
    bne .Lfail_write_ready
    mov r0, #0x10
    strh r0, [r4, #0x30]
    mov r11, r6
    mov r5, #128
.Lwrite_fifo:
    ldr r0, [r11], #4
    str r0, [r4, #0x20]
    subs r5, r5, #1
    bne .Lwrite_fifo
    mov r0, #2
    bl .Lwait_status
    cmp r0, #0
    bne .Lfail_write_complete
    mov r0, #2
    strh r0, [r4, #0x30]

    bl .Lcard_transfer_state
    cmp r0, #0
    bne .Lfail_cmd13_after

    mov r0, #0x33
    strh r0, [r4, #0x30]
    movw r0, #0xffff
    strh r0, [r4, #0x32]
    movw r0, #0x0200
    strh r0, [r4, #0x04]
    mov r0, #1
    strh r0, [r4, #0x06]
    str r7, [r4, #0x08]
    mov r0, #0x10
    strh r0, [r4, #0x0c]
    movw r0, #0x113a
    strh r0, [r4, #0x0e]
    mov r0, #1
    bl .Lwait_status
    cmp r0, #0
    bne .Lfail_cmd17
    bl .Lr1_error
    cmp r0, #0
    bne .Lfail_cmd17
    mov r0, #1
    strh r0, [r4, #0x30]
    mov r0, #0x20
    bl .Lwait_status
    cmp r0, #0
    bne .Lfail_read_ready
    mov r0, #0x20
    strh r0, [r4, #0x30]
    movw r11, #:lower16:XM1_READBACK_BUFFER
    movt r11, #:upper16:XM1_READBACK_BUFFER
    mov r5, #128
.Lread_fifo:
    ldr r0, [r4, #0x20]
    str r0, [r11], #4
    subs r5, r5, #1
    bne .Lread_fifo
    mov r0, #2
    bl .Lwait_status
    cmp r0, #0
    bne .Lfail_read_complete
    mov r0, #2
    strh r0, [r4, #0x30]

    movw r11, #:lower16:XM1_READBACK_BUFFER
    movt r11, #:upper16:XM1_READBACK_BUFFER
    mov r3, r6
    mov r5, #128
.Lcompare:
    ldr r0, [r3], #4
    ldr r1, [r11], #4
    cmp r0, r1
    bne .Lfail_compare
    subs r5, r5, #1
    bne .Lcompare
    mov r0, #0
    pop {r4-r11, pc}

.Lfail_precondition:   mov r0, #1;  b .Lreturn
.Lfail_cmd13_before:   mov r0, #2;  b .Lreturn
.Lfail_cmd25:          mov r0, #3;  b .Lreturn
.Lfail_write_ready:    mov r0, #4;  b .Lreturn
.Lfail_write_complete: mov r0, #5;  b .Lreturn
.Lfail_cmd13_after:    mov r0, #6;  b .Lreturn
.Lfail_cmd17:          mov r0, #7;  b .Lreturn
.Lfail_read_ready:     mov r0, #8;  b .Lreturn
.Lfail_read_complete:  mov r0, #9;  b .Lreturn
.Lfail_compare:        mov r0, #10
.Lreturn:
    pop {r4-r11, pc}

// r0 = normal-status mask; returns 0 on status, 1 on error, 2 on budget.
.Lwait_status:
    movw r2, #0
    movt r2, #1
.Lwait_status_loop:
    ldrh r3, [r4, #0x32]
    cmp r3, #0
    bne .Lwait_status_error
    ldrh r3, [r4, #0x30]
    tst r3, r0
    bne .Lwait_status_ok
    subs r2, r2, #1
    bne .Lwait_status_loop
    mov r0, #2
    bx lr
.Lwait_status_error:
    mov r0, #1
    bx lr
.Lwait_status_ok:
    mov r0, #0
    bx lr

// Checks R1 READY_FOR_DATA and CURRENT_STATE=TRANSFER.
.Lcard_transfer_state:
    push {lr}
    mov r0, #0x33
    strh r0, [r4, #0x30]
    movw r0, #0xffff
    strh r0, [r4, #0x32]
    ldr r0, [r10, #0x00]
    str r0, [r4, #0x08]
    mov r0, #0
    strh r0, [r4, #0x0c]
    movw r0, #0x0d1a
    strh r0, [r4, #0x0e]
    mov r0, #1
    bl .Lwait_status
    cmp r0, #0
    popne {pc}
    mov r0, #1
    strh r0, [r4, #0x30]
    ldr r0, [r4, #0x10]
    movw r1, #0xe008
    movt r1, #0xfdff
    tst r0, r1
    movne r0, #1
    popne {pc}
    movw r1, #0x1f00
    and r0, r0, r1
    cmp r0, #0x0900
    movne r0, #1
    moveq r0, #0
    pop {pc}

.Lr1_error:
    ldr r0, [r4, #0x10]
    movw r1, #0xe008
    movt r1, #0xfdff
    tst r0, r1
    movne r0, #1
    moveq r0, #0
    bx lr

.section .report.data,"aw",%progbits
.balign 512
.global xm1_report_buffer
xm1_report_buffer:
    .space 1024, 0
