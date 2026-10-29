ENTRY(rb_mp_fatal2)

SECTIONS
{
  . = 0x00e87944;
  .text :
  {
    KEEP(*(.text.rb_mp_fatal2))
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

ASSERT(ADDR(.text.rb_mp_fatal2) == 0x00e87944, "fatal2 model address")
