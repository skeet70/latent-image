ENTRY(rb_mp_handback)

SECTIONS
{
  . = 0x00e87404;
  .text :
  {
    KEEP(*(.text.rb_mp_handback))
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

ASSERT(ADDR(.text) == 0x00e87404, "handback model address")
