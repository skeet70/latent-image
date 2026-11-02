.syntax unified
.arm
.section .text.xm1_root_task,"ax",%progbits
.balign 4
.global xm1_root_task
.type xm1_root_task,%function
xm1_root_task:
    push {r4, lr}
    bl root_platform_init
    bl root_runtime_init
    mov r0, #0
    movt r0, #0x101
    bl root_mark
    movw r0, #0x835c
    movt r0, #0x72
    bl root_install_table
    mov r4, #1
.Lresource:
    mov r0, r4
    mov r1, #1
    bl root_create_resource
    add r4, r4, #1
    cmp r4, #98
    bne .Lresource
    bl root_create_workers
    bl root_init_676054
    bl root_init_662e48
    bl root_init_665224
    bl root_init_649070
    mov r0, #1
    bl root_init_6490dc
    bl root_checked_init_a
    cmp r0, #0
    bne .Lhalt
    bl root_checked_init_b
    cmp r0, #0
    bne .Lhalt
    bl root_init_69f204
    bl root_init_656788
    bl root_init_7262cc
    movw r0, #0x6478
    movt r0, #0x72
    bl root_install_descriptor
    bl root_init_6a5634
    mov r0, #34
    mov r1, #0
    mov r2, #0
    bl root_config_write
    bl root_mode
    cmp r0, #0
    beq .Lzero
    bl root_submit_later
    mov r0, #30
    mov r1, #0
    movw r2, #:lower16:root_periodic_callback
    movt r2, #:upper16:root_periodic_callback
    movw r3, #:lower16:root_periodic_context
    movt r3, #:upper16:root_periodic_context
    bl root_register_periodic
    mov r0, #0x11000000
    add r0, r0, #0x10000
    bl root_mark
    mov r0, #0
    bl root_cleanup
    pop {r4, pc}
.Lzero:
    mov r0, #34
    bl root_config_query
    cmp r0, #0
    beq .Lzero_tail
    mov r0, #34
    mov r1, #0
    mov r2, #0
    bl root_config_write
    mov r0, #31
    mov r1, #60
    mov r2, #0
    bl root_config_write
    mov r0, #32
    mov r1, #0
    mov r2, #0
    bl root_config_write
    mov r0, #34
    mov r1, #1
    mov r2, #0
    bl root_config_write
.Lzero_tail:
    bl root_init_6a564c
    bl root_init_6a5c08
    bl root_init_6568ec
.Lhalt:
    b .Lhalt
.size xm1_root_task, . - xm1_root_task
.section .note.GNU-stack,"",%progbits
