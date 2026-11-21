.syntax unified
.arm

.macro retained name, address
.global \name
.type \name, %function
.set \name, \address
.endm

retained xm1_playback_retained_dispatch, 0x011e8bf8
retained xm1_power_output_route_set_requested_level, 0x0064982c
retained xm1_scanout_mode_preset, 0x006d1294
retained xm1_jpeg_wait, 0x00724b68
retained xm1_jpeg_signal, 0x00724c00
retained xm1_jpeg_resource_acquire, 0x0065a3cc
retained xm1_jpeg_resource_release, 0x0065a424
retained xm1_jpeg_publish_restart, 0x0065aa44
retained xm1_jpeg_interrupt_acquire, 0x0064e918
retained xm1_jpeg_interrupt_release, 0x0064eb20
retained xm1_jpeg_cache, 0x00042634
retained xm1_jpeg_critical_enter, 0x007235d8
retained xm1_jpeg_critical_leave, 0x007236b0
retained xm1_jpeg_timer_start, 0x0069b328
retained xm1_jpeg_timer_cancel, 0x0069b7a4
retained xm1_jpeg_task_notify, 0x00724558
retained xm1_jpeg_current_wait, 0x007243a8
retained xm1_jpeg_current_retain, 0x007246f0
retained memcpy, 0x0089d908
retained memmove, 0x0089d908
retained __aeabi_memcpy, 0x0089d908
retained __aeabi_memcpy4, 0x0089d908
retained __aeabi_memcpy8, 0x0089d908
retained __aeabi_memmove, 0x0089d908
retained __aeabi_memmove4, 0x0089d908
retained __aeabi_memmove8, 0x0089d908
