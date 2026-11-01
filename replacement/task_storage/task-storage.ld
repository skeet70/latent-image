ENTRY(xm1_task_storage_entry)
xm1_task_storage_resume = 0x725da8;
xm1_task_storage_pool_base_literal = 0x725dd4;
SECTIONS
{
  . = 0x725cc4;
  __task_storage_start = .;
  .text : {
    KEEP(*(.text.entry))
    *(.text*)
  }
  .rodata : { *(.rodata*) }
  .data : { *(.data*) }
  .bss : { *(.bss*) *(COMMON) }
  . = ALIGN(4);
  __task_storage_end = .;
  /DISCARD/ : {
    *(.ARM.exidx*)
    *(.ARM.extab*)
    *(.note*)
    *(.comment*)
  }
}
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "task storage image has writable statics")
ASSERT(__task_storage_end <= 0x725da8, "task storage transition exceeds its retained helper window")
