.syntax unified
.arm

.global xm1_playback_retained_dispatch
.type xm1_playback_retained_dispatch, %function
.set xm1_playback_retained_dispatch, 0x011e8bf8
.global xm1_power_output_route_set_requested_level
.type xm1_power_output_route_set_requested_level, %function
.set xm1_power_output_route_set_requested_level, 0x0064982c
.global xm1_scanout_mode_preset
.type xm1_scanout_mode_preset, %function
.set xm1_scanout_mode_preset, 0x006d1294

.global memcpy
.global memmove
.global __aeabi_memcpy
.global __aeabi_memcpy4
.global __aeabi_memcpy8
.global __aeabi_memmove
.global __aeabi_memmove4
.global __aeabi_memmove8
.set memcpy, 0x0089d908
.set memmove, 0x0089d908
.set __aeabi_memcpy, 0x0089d908
.set __aeabi_memcpy4, 0x0089d908
.set __aeabi_memcpy8, 0x0089d908
.set __aeabi_memmove, 0x0089d908
.set __aeabi_memmove4, 0x0089d908
.set __aeabi_memmove8, 0x0089d908
