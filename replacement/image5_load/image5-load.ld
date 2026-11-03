ENTRY(xm1_image5_load_entry)
xm1_image5_segment = 0x657114;
xm1_image5_marker = 0x42188;
xm1_image5_fill_request = 0x66609c;
xm1_image5_fill_done = 0x6a8bec;
xm1_image5_fill_flag = 0x89f8ac;
xm1_image5_mode = 0x657184;
xm1_image5_decoder_init = 0x6ae360;
xm1_image5_read = 0x6a8f1c;
xm1_image5_feed = 0x6ae500;
xm1_image5_decoder_finish = 0x6ae3f4;
xm1_image5_cpu_fill = 0x7221c8;
xm1_image5_cache = 0x42634;
SECTIONS
{
  . = 0x6a96c0;
  __image5_load_start = .;
  .text : { KEEP(*(.text.entry)) *(.text*) }
  .rodata : { *(.rodata*) }
  .data : { *(.data*) }
  .bss : { *(.bss*) *(COMMON) }
  . = ALIGN(4);
  __image5_load_end = .;
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.note*) *(.comment*) }
}
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "image5 load has writable statics")
ASSERT(__image5_load_start == 0x6a96c0 && xm1_image5_load_entry == __image5_load_start, "image5 load entry moved")
ASSERT(__image5_load_end <= 0x6a9844, "image5 load exceeds its complete 388-byte carrier")
