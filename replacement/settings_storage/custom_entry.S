.syntax unified
.arm
.macro custom_entry name, destination
.section .text.\name, "ax", %progbits
.balign 4
.global \name
.type \name, %function
\name:
    b \destination
.size \name, .-\name
.endm
custom_entry settings_custom_boot_entry, settings_custom_boot
custom_entry settings_custom_read_entry, settings_custom_read
custom_entry settings_custom_refresh_entry, settings_custom_refresh
custom_entry settings_custom_defaults_entry, settings_custom_defaults
custom_entry settings_custom_prepare_entry, settings_custom_prepare
custom_entry settings_custom_check_entry, settings_custom_check
custom_entry settings_custom_config_read, xm1_stock_custom_config_read
custom_entry settings_custom_reset_modes, xm1_stock_custom_reset_modes
.section .text.settings_custom_store8, "ax", %progbits
.balign 4
.global settings_custom_store8
.type settings_custom_store8, %function
settings_custom_store8:
    movw r2, #0xf834
    movt r2, #0x0089
    sub r2, r0, r2
    cmp r2, #0x3c
    blo 1f
    movw r2, #0xf874
    movt r2, #0x0089
    sub r2, r0, r2
    cmp r2, #0x34
    blo 1f
    movw r2, #0x295c
    movt r2, #0x409c
    sub r2, r0, r2
    cmp r2, #0x28
    blo 1f
    udf #0
1:
    strb r1, [r0]
    bx lr
.size settings_custom_store8, .-settings_custom_store8

.section .text.settings_custom_store16, "ax", %progbits
.balign 4
.global settings_custom_store16
.type settings_custom_store16, %function
settings_custom_store16:
    tst r0, #1
    bne 2f
    movw r2, #0xf838
    movt r2, #0x0089
    sub r2, r0, r2
    cmp r2, #0x33
    blo 1f
    movw r2, #0xf874
    movt r2, #0x0089
    sub r2, r0, r2
    cmp r2, #0x33
    blo 1f
    movw r2, #0x295c
    movt r2, #0x409c
    sub r2, r0, r2
    cmp r2, #0x27
    blo 1f
2:
    udf #0
1:
    strh r1, [r0]
    bx lr
.size settings_custom_store16, .-settings_custom_store16

.section .text.settings_custom_store32, "ax", %progbits
.balign 4
.global settings_custom_store32
.type settings_custom_store32, %function
settings_custom_store32:
    tst r0, #3
    bne 2f
    movw r2, #0xf834
    movt r2, #0x0089
    sub r2, r0, r2
    cmp r2, #0x39
    blo 1f
2:
    udf #0
1:
    str r1, [r0]
    bx lr
.size settings_custom_store32, .-settings_custom_store32
