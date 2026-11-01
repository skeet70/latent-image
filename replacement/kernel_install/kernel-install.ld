ENTRY(xm1_kernel_install_entry)
xm1_kernel_cache = 0x42634;
SECTIONS
{
  . = 0x725a04;
  __kernel_install_start = .;
  .text : { KEEP(*(.text.entry)) *(.text*) }
  .rodata : { *(.rodata*) }
  .data : { *(.data*) }
  .bss : { *(.bss*) *(COMMON) }
  . = ALIGN(4);
  __kernel_install_end = .;
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.note*) *(.comment*) }
}
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "kernel installation has writable statics")
ASSERT(__kernel_install_end <= 0x725b54, "kernel installation exceeds its complete 336-byte carrier")
