.syntax unified
.arm
.section .text.entry,"ax",%progbits
.balign 4
.global xm1_task_storage_entry
.type xm1_task_storage_entry,%function
xm1_task_storage_entry:
    mov r0, r4
    mov r1, r7
    mov r2, r8
    bl xm1_task_storage_transition
    mov r4, r0
    b xm1_task_storage_resume
.size xm1_task_storage_entry, . - xm1_task_storage_entry
