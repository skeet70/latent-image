ENTRY(rb_capture_develop_wait)

SECTIONS
{
  . = 0x009c0000;
  .text :
  {
    KEEP(*(.text))
    KEEP(*(.text.rb_capture_step_wait))
    *(.text*)
    *(.rodata*)
  }

  .data : { *(.data*) }
  .bss : { *(.bss*) *(COMMON) }

  /DISCARD/ :
  {
    *(.ARM.exidx*)
    *(.ARM.extab*)
    *(.comment)
    *(.note*)
    *(.eh_frame*)
  }
}

ASSERT(ADDR(.text) == 0x009c0000, "capture text must start at its case load base")
ASSERT(SIZEOF(.bss) == 0, "capture must not carry bss")
