ENTRY(rb_mp_command)

SECTIONS
{
  . = 0x00e8d82c;
  .text :
  {
    KEEP(*(.text.rb_mp_command))
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

ASSERT(ADDR(.text) == 0x00e8d82c, "command model address")
