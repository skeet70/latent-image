#![no_std]

use core::arch::global_asm;
use core::panic::PanicInfo;

#[cfg(feature = "domain-a")]
global_asm!(
    ".equ S2_RUNNING, 0x40054760",
    ".equ S2_CONTEXT, 0x400545a4",
    ".equ S2_SELECTED, 0x40054768",
    ".equ S2_ENABLED, 0x40054764",
    ".equ S2_RESCHEDULE, 0x400546d8",
    ".equ S2_IDLE, 0xa0000",
    ".equ S2_IRQ_TAG, 0x11",
);
#[cfg(feature = "domain-b")]
global_asm!(
    ".equ S2_RUNNING, 0x400e4720",
    ".equ S2_CONTEXT, 0x400e4564",
    ".equ S2_SELECTED, 0x400e4728",
    ".equ S2_ENABLED, 0x400e4724",
    ".equ S2_RESCHEDULE, 0x400e4698",
    ".equ S2_IDLE, 0x130000",
    ".equ S2_IRQ_TAG, 0x12",
);

global_asm!(
    r#"
    .arm
    .macro entry name
    .global xm1_rtos_scheduler_\name
    .type xm1_rtos_scheduler_\name, %function
xm1_rtos_scheduler_\name:
    .endm
    .macro endentry name
    .size xm1_rtos_scheduler_\name, . - xm1_rtos_scheduler_\name
    .endm

    .section .text.s2_save, "ax", %progbits
    entry save
    push {{r4-r11, lr}}
    ldr r0, xm1_rtos_scheduler_pool
    ldr r1, [r0]
    .global xm1_rtos_scheduler_save_discarded_read
xm1_rtos_scheduler_save_discarded_read:
    ldr r3, [r1, #0x30]
    cmp r3, #1
    beq xm1_rtos_scheduler_select
    str sp, [r1, #0x18]
    add r2, pc, #4
    str r2, [r1, #0x1c]
    b xm1_rtos_scheduler_select
    endentry save

    .section .text.s2_resume, "ax", %progbits
    entry resume
    pop {{r4-r11, lr}}
    ldrb r0, [r1, #0xe]
    tst r0, #4
    beq 1f
    ldr r0, [r1, #0x10]
    tst r0, r0
    bne xm1_rtos_scheduler_s2_deliver_exception
1:  mov pc, lr
    endentry resume

    .section .text.s2_select, "ax", %progbits
    entry no_save
    ldr r6, xm1_rtos_scheduler_pool + 4
    mov r3, #0
    str r3, [r6]
    mrs r0, cpsr
    and r0, r0, #0x40
    orr r0, r0, #0x93
    msr cpsr_fc, r0
    endentry no_save
    entry select
    ldr r0, xm1_rtos_scheduler_pool + 8
    .global xm1_rtos_scheduler_selected_read
xm1_rtos_scheduler_selected_read:
    ldr r1, [r0]
    str r1, [r0, #-8]
    cmp r1, #0
    beq 2f
    ldr r2, [r1, #8]
    ldr r2, [r2, #0x20]
    mcr p15, 0, r2, c13, c0, 2
    mrc p15, 0, r3, c1, c0, 2
    tst r3, #0xf00000
    beq 5f
    mov r3, #0
    vmsr fpexc, r3
5:  ldr sp, [r1, #0x18]
    ldr pc, [r1, #0x1c]
2:  ldr r6, xm1_rtos_scheduler_pool + 4
    mov r3, #1
    str r3, [r6]
    mov sp, #S2_IDLE
    cpsie i
    .global xm1_rtos_scheduler_null_window
xm1_rtos_scheduler_null_window:
    nop
    cpsid i
    movw r0, #0x804
    movt r0, #0x400c
    ldr r1, [r0]
    cmp r1, #S2_IRQ_TAG
    bne 3f
    dmb sy
    bl xm1_rtos_scheduler_s2_global_unlock
3:  mov r3, #0
    str r3, [r6]
    b xm1_rtos_scheduler_select
    endentry select

    .section .text.s2_first_entry, "ax", %progbits
    entry first_entry
    mov r1, #0x53
    mrs r2, cpsr
    and r2, r2, #0x40
    orr r1, r1, r2
    msr cpsr_fc, r1
    ldr lr, xm1_rtos_scheduler_pool + 12
    ldr r0, xm1_rtos_scheduler_pool
    ldr r1, [r0]
    mov r2, #0
    str r2, [r1, #0x30]
    ldr r2, [r1, #8]
    ldr r0, [r2, #4]
    ldr pc, [r2, #8]
    endentry first_entry

    .section .text.s2_irq_return, "ax", %progbits
    entry irq_return
    ldr r2, xm1_rtos_scheduler_pool
    ldr r1, [r2]
    ldr r2, xm1_rtos_scheduler_pool + 16
    ldr r0, [r2]
    cmp r0, #0
    beq xm1_rtos_scheduler_irq_tail
    .global xm1_rtos_scheduler_irq_discarded_read
xm1_rtos_scheduler_irq_discarded_read:
    ldr r0, [r1, #0x30]
    cmp r0, #1
    beq xm1_rtos_scheduler_select
    ldr r2, xm1_rtos_scheduler_pool + 8
    .global xm1_rtos_scheduler_irq_selected_read
xm1_rtos_scheduler_irq_selected_read:
    ldr r0, [r2]
    cmp r0, r1
    beq xm1_rtos_scheduler_irq_tail
    push {{r4-r11}}
    str sp, [r1, #0x18]
    add r0, pc, #4
    str r0, [r1, #0x1c]
    b xm1_rtos_scheduler_select
    endentry irq_return

    .section .text.s2_irq_resume, "ax", %progbits
    entry irq_resume
    pop {{r4-r11}}
    endentry irq_resume
    entry irq_tail
    ldrb r0, [r1, #0xe]
    tst r0, #4
    beq 4f
    ldr r0, [r1, #0x10]
    tst r0, r0
    blne xm1_rtos_scheduler_s2_deliver_exception
4:  pop {{r0}}
    mrs r2, cpsr
    and r2, r2, #0x40
    bic r0, r0, #0x40
    orr r0, r0, r2
    .global xm1_rtos_scheduler_exception_return_stop
xm1_rtos_scheduler_exception_return_stop:
    msr spsr_fc, r0
    ldm sp!, {{r0-r3, r12, lr, pc}}^
    endentry irq_tail

    .section .rodata.s2_pool, "a", %progbits
    .global xm1_rtos_scheduler_pool
xm1_rtos_scheduler_pool:
    .word S2_RUNNING, S2_CONTEXT, S2_SELECTED
    .word xm1_rtos_scheduler_s2_exit_task, S2_ENABLED

    .section .text.s2_doorbell, "ax", %progbits
    entry doorbell
    push {{r11, lr}}
    add r11, sp, #4
    mov r0, #120
    bl xm1_rtos_scheduler_s2_clear_irq
    movw r3, #:lower16:S2_RESCHEDULE
    movt r3, #:upper16:S2_RESCHEDULE
    mov r2, #1
    str r2, [r3]
    pop {{r11, pc}}
    endentry doorbell
"#
);

#[cfg(not(feature = "component"))]
#[panic_handler]
fn panic(_: &PanicInfo<'_>) -> ! {
    loop {}
}
