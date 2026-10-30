ENTRY(rb_mp_cancel)

SECTIONS
{
  . = 0x00e6e368;
  .text :
  {
    KEEP(*(.text.rb_mp_cancel))
    *(.text*)
    *(.rodata*)
  }

  /DISCARD/ :
  {
    *(.ARM.exidx*)
    *(.ARM.extab*)
    *(.comment)
    *(.note*)
    *(.eh_frame*)
    *(.data*)
    *(.bss*)
    *(COMMON)
  }
}

ASSERT(ADDR(.text) == 0x00e6e368, "cancel model address")
