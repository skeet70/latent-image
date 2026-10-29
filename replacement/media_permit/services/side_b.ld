ENTRY(rb_mp_fatal)

SECTIONS
{
  . = 0x00e881e8;
  .text :
  {
    KEEP(*(.text.rb_mp_fatal))
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

ASSERT(ADDR(.text.rb_mp_fatal) == 0x00e881e8, "fatal model address")
