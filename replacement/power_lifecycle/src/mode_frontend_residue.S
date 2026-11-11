.syntax unified
.arm
.text
.align 2
.global rb_power_save_action
.type rb_power_save_action, %function
rb_power_save_action:
    push {r4, lr}
    sub sp, sp, #16
    mov r0, sp
    bl rb_power_save_action_backed
    add sp, sp, #16
    pop {r4, pc}
.size rb_power_save_action, .-rb_power_save_action

.global rb_power_terminal_action
.type rb_power_terminal_action, %function
rb_power_terminal_action:
    push {r4, lr}
    sub sp, sp, #16
    mov r0, sp
    bl rb_power_terminal_action_backed
    add sp, sp, #16
    pop {r4, pc}
.size rb_power_terminal_action, .-rb_power_terminal_action
