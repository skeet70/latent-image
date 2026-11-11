.syntax unified
.arm
.section .text,"ax",%progbits

.macro SENSOR_MMIO name, address
.global \name
.type \name,%function
\name:
    movw r12, #(\address & 0xffff)
    movt r12, #((\address >> 16) & 0xffff)
    bx r12
.size \name, .-\name
.endm

SENSOR_MMIO xm1_pipeline_sensor_program_mode, 0x006748a8
SENSOR_MMIO xm1_pipeline_sensor_program_offsets, 0x00674978
SENSOR_MMIO xm1_pipeline_sensor_program_bounds, 0x00674a4c
SENSOR_MMIO xm1_pipeline_sensor_program_addresses, 0x00674d8c
SENSOR_MMIO xm1_pipeline_sensor_program_color, 0x006750c4
SENSOR_MMIO xm1_pipeline_sensor_program_extra, 0x00675cbc
SENSOR_MMIO xm1_pipeline_sensor_program_filters, 0x006757c8
SENSOR_MMIO xm1_pipeline_sensor_program_padding, 0x00675cf0
SENSOR_MMIO xm1_pipeline_sensor_filter_primary_enable, 0x00688f58
SENSOR_MMIO xm1_pipeline_sensor_filter_primary_parameters, 0x00689298
SENSOR_MMIO xm1_pipeline_sensor_filter_second_enable, 0x006890f8
SENSOR_MMIO xm1_pipeline_sensor_filter_second_parameters, 0x006893f4
SENSOR_MMIO xm1_pipeline_sensor_filter_third_enable, 0x006890a0
SENSOR_MMIO xm1_pipeline_sensor_filter_third_parameters, 0x0068949c
SENSOR_MMIO xm1_pipeline_sensor_filter_fourth_enable, 0x00689048
SENSOR_MMIO xm1_pipeline_sensor_filter_fourth_parameters, 0x00689604
SENSOR_MMIO xm1_pipeline_sensor_filter_fifth_enable, 0x00688ff0
SENSOR_MMIO xm1_pipeline_sensor_filter_fifth_parameters, 0x006899fc
SENSOR_MMIO xm1_pipeline_sensor_filter_lookup_enable, 0x00689150
SENSOR_MMIO xm1_pipeline_sensor_filter_lookup_parameters, 0x00689fe8
SENSOR_MMIO xm1_pipeline_sensor_filter_large_enable, 0x006891c8
SENSOR_MMIO xm1_pipeline_sensor_filter_large_parameters, 0x0068ae24
SENSOR_MMIO xm1_pipeline_sensor_filter_routing, 0x00689240
SENSOR_MMIO xm1_pipeline_sensor_runtime_first_enable, 0x0068b750
SENSOR_MMIO xm1_pipeline_sensor_runtime_first_parameters, 0x0068b858
SENSOR_MMIO xm1_pipeline_sensor_runtime_second_enable, 0x0068b7a8
SENSOR_MMIO xm1_pipeline_sensor_runtime_second_parameters, 0x0068bab0
SENSOR_MMIO xm1_pipeline_sensor_runtime_third_enable, 0x0068b800
SENSOR_MMIO xm1_pipeline_sensor_runtime_third_parameters, 0x0068bd4c
SENSOR_MMIO xm1_pipeline_sensor_runtime_commit, 0x0068be84
SENSOR_MMIO xm1_pipeline_sensor_device_center, 0x0068f5b0
SENSOR_MMIO xm1_pipeline_sensor_device_secondary_center, 0x0068ecf8

.global xm1_pipeline_sensor_clear_primary_table_gate
.type xm1_pipeline_sensor_clear_primary_table_gate,%function
xm1_pipeline_sensor_clear_primary_table_gate:
    sub sp, sp, #4
    ldr r0, [sp]
    bic r0, r0, #1
    movw r1, #0x4800
    movt r1, #0xfff4
    str r0, [r1]
    add sp, sp, #4
    bx lr
.size xm1_pipeline_sensor_clear_primary_table_gate, .-xm1_pipeline_sensor_clear_primary_table_gate
