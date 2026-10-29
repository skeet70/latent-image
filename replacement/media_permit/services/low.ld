ENTRY(rb_mp_memcpy)

SECTIONS
{
  . = 0x00722054;
  .text :
  {
    KEEP(*(.text.rb_mp_memcpy))
    . = 0x00726658;
    KEEP(*(.text.rb_mp_log))
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

ASSERT(ADDR(.text.rb_mp_memcpy) == 0x00722054, "memcpy model address")
ASSERT(ADDR(.text.rb_mp_log) == 0x00726658, "log model address")
