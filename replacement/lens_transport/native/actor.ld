ENTRY(lens_actor_request)
SECTIONS
{
    . = 0x00100000;
    .text : { *(.text .text.*) }
    .rodata : { *(.rodata .rodata.*) }
    .data : { *(.data .data.*) }
    .bss : { *(.bss .bss.*) *(COMMON) }
    /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.comment) *(.note*) }
}
ASSERT(. <= 0x00180000, "owned actor exceeds native bank")
