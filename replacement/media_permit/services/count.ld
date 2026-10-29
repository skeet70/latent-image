ENTRY(rb_mp_subframe_count)

SECTIONS
{
  . = 0x00e74c68;
  .text :
  {
    KEEP(*(.text.rb_mp_subframe_count))
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

ASSERT(ADDR(.text.rb_mp_subframe_count) == 0x00e74c68, "count model address")
