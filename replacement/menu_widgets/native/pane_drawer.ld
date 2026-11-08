ENTRY(owned_pane_drawer)
SECTIONS
{
    . = 0x01800000;
    .text : { *(.text.owned_pane_drawer) *(.text*) }
    .rodata : { *(.rodata*) }
    .data : { *(.data*) }
    .bss : { *(.bss*) *(COMMON) }
    /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.comment*) }
}
