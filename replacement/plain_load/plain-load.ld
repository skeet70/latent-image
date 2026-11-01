ENTRY(xm1_plain_load_entry)
xm1_plain_segment = 0x657114;
xm1_plain_extents = 0x6571a0;
xm1_plain_mode = 0x657184;
xm1_plain_read = 0x6a8f1c;
xm1_plain_progress = 0x73b924;
xm1_plain_take = 0x724b68;
xm1_plain_give = 0x724c00;
xm1_plain_delay = 0x69b9c4;
SECTIONS
{
  . = 0x6a9844;
  __plain_load_start = .;
  .text : { KEEP(*(.text.entry)) *(.text*) }
  .rodata : { *(.rodata*) }
  .data : { *(.data*) }
  .bss : { *(.bss*) *(COMMON) }
  . = ALIGN(4);
  __plain_load_end = .;
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.note*) *(.comment*) }
}
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "plain load has writable statics")
ASSERT(__plain_load_start == 0x6a9844 && xm1_plain_load_entry == __plain_load_start, "plain load entry moved")
ASSERT(__plain_load_end <= 0x6a9938, "plain load exceeds its complete 244-byte carrier")
