ENTRY(lens_codec_command)
PROVIDE(lens_publication_edge = 0x006bf218);
SECTIONS
{
    . = 0x00100000;
    .text : { *(.text .text.*) }
    .rodata : { *(.rodata .rodata.*) }
    /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.comment) *(.note*) }
}
ASSERT(. <= 0x00180000, "owned codec exceeds native bank")
