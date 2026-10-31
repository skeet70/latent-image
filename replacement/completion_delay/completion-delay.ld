/* Record-1 RAM [0x180,0x19c): seven 0xffffffff padding words in the original
   record, after the #135 delayed-release veneer [0x100,0x168). */
ENTRY(xm1_completion_delay_entry)
SECTIONS
{
    . = 0x180;
    .text : { KEEP(*(.text.entry)) *(.text .text.*) }
    .rodata : { *(.rodata .rodata.*) }
    .data : { *(.data .data.*) }
    .bss (NOLOAD) : { *(.bss .bss.*) *(COMMON) }
    /DISCARD/ : { *(.ARM.exidx* .ARM.extab* .comment .note* .eh_frame*) }
}
ASSERT(ADDR(.text) == 0x180, "completion delay entry address changed")
ASSERT(SIZEOF(.text) == 28, "completion delay veneer must be exactly 28 bytes")
ASSERT(SIZEOF(.rodata) + SIZEOF(.data) + SIZEOF(.bss) == 0, "completion delay veneer has extra data")
ASSERT(xm1_completion_delay_release == 0x194, "completion delay release moved")
ASSERT(xm1_completion_delay_end == 0x19c, "completion delay end moved")
