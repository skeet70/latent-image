ENTRY(xm1_power_record_helper_ef9bc0)
SECTIONS
{
    . = 0x00100000;
    __power_cpu_start = .;
    .text : { *(.text .text.*) }
    .rodata : { *(.rodata .rodata.*) }
    .data : { *(.data .data.*) }
    .bss : { *(.bss .bss.*) *(COMMON) }
    __power_cpu_end = .;
    /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.comment) *(.note*) }
}
ASSERT(. <= 0x00180000, "owned record helpers exceed native bank")
