ENTRY(rb_media_permit_case)

SECTIONS
{
  . = 0x00900000;
  .text :
  {
    KEEP(*(.text.rb_media_permit_case))
    *(.text*)
    *(.rodata*)
    . = 0x00907f00;
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

ASSERT(ADDR(.text) == 0x00900000, "candidate text base")
ASSERT(ADDR(.text.rb_media_permit_case) == 0x00900000, "candidate entry")
ASSERT(SIZEOF(.text) == 0x7f00, "candidate text does not fill the declared span")
