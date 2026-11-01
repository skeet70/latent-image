ENTRY(xm1_startup_dispatch_entry)
xm1_startup_mode = 0x657184;
xm1_startup_clear = 0x7221a0;
xm1_startup_install = 0x725a04;
xm1_startup_submit = 0x725dd8;
SECTIONS
{
  . = 0x69cdf8;
  __startup_dispatch_start = .;
  .text : { KEEP(*(.text.entry)) *(.text*) }
  .rodata : { *(.rodata*) }
  .data : { *(.data*) }
  .bss : { *(.bss*) *(COMMON) }
  . = ALIGN(4);
  __startup_dispatch_end = .;
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.note*) *(.comment*) }
}
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "startup dispatcher has writable statics")
ASSERT(__startup_dispatch_end <= 0x69cf84, "startup dispatcher exceeds its complete 396-byte carrier")
