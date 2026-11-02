.syntax unified
.arm

.section .text.entry,"ax",%progbits
.balign 4
.global xm1_task_submit_entry
.type xm1_task_submit_entry,%function
xm1_task_submit_entry:
    b xm1_task_submit
.size xm1_task_submit_entry, . - xm1_task_submit_entry

.section .task_release_entry,"ax",%progbits
.balign 4
.global xm1_task_release_entry
.type xm1_task_release_entry,%function
xm1_task_release_entry:
    push {r3, lr}
    bl xm1_task_release
    pop {r3, pc}
.size xm1_task_release_entry, . - xm1_task_release_entry

.section .task_release_literal,"a",%progbits
.balign 4
.global xm1_task_release_lock_literal
.type xm1_task_release_lock_literal,%object
xm1_task_release_lock_literal:
    .word xm1_task_release_lock
.size xm1_task_release_lock_literal, 4

.section .note.GNU-stack,"",%progbits
