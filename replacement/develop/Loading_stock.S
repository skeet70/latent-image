    .syntax unified
    .arm

    .macro loading_seam name, address
    .global \name
    .set \name, \address
    .endm

    /* X-M1 1.50 dispatch and core-0 clock leaves retained by the typed loader. */
    loading_seam xm1_lower_read_clock_mask,      0x00042560
    loading_seam xm1_lower_read_clock_restore,   0x00042568
    loading_seam xm1_loading_current_task,       0x00723cb8
    loading_seam xm1_loading_wait_reserved,      0x00724b68
    loading_seam xm1_loading_signal_reserved,    0x00724c00
    loading_seam xm1_loading_create_channel,     0x00724cdc
    loading_seam xm1_loading_send_message,       0x0072503c
    loading_seam xm1_loading_receive_message,    0x00725318
    loading_seam xm1_loading_create_task,        0x00725dd8

    .section .note.GNU-stack, "", %progbits
