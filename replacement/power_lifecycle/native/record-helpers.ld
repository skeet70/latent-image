ENTRY(xm1_power_record_helper_ef9bc0)
SECTIONS
{
    . = 0x00100000;
    .text : { *(.text .text.*) }
    .rodata : { *(.rodata .rodata.*) }
    /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.comment) *(.note*) }
}
ASSERT(. <= 0x00180000, "owned record helpers exceed native bank")
