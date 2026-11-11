.syntax unified
.arm
.section .text.xm1_power_serial_write,"ax",%progbits
.global xm1_power_serial_write
.type xm1_power_serial_write,%function
xm1_power_serial_write:
    ldr r2, [sp, #-20]
    b xm1_power_serial_write_with_residue
.size xm1_power_serial_write, .-xm1_power_serial_write
