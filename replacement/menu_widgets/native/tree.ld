ENTRY(tree_list_entry)
SECTIONS
{
    . = 0x01800000;
    .text : { *(.text.tree_list_entry) *(.text*) }
    .rodata : { *(.rodata*) }
    .data : { *(.data*) }
    .bss : { *(.bss*) *(COMMON) }
    /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.comment*) }
}
