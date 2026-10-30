ENTRY(rb_media_permit_case)

SECTIONS
{
  . = 0x00900000;
  .text :
  {
    KEEP(*(.text.rb_media_permit_case))
    . = 0x00900400;
    KEEP(*(.text.rb_media_permit_resolve))
    . = 0x00900800;
    KEEP(*(.text.rb_media_permit_apply))
    . = 0x00900c00;
    KEEP(*(.text.rb_mp_forbidden_body_probe))
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
ASSERT(SIZEOF(.text) == 0x7f00, "candidate text does not fill the declared span")
