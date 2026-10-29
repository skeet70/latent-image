ENTRY(xm1_save_probe)

SECTIONS
{
  . = 0x00100000;
  .probe1 : { KEEP(*(.text.xm1_save_probe)) }
  . = 0x00100100;
  .probe2 : { KEEP(*(.text.xm1_save_reentry_probe)) }
  . = 0x00100200;
  .probe3 : { KEEP(*(.text.xm1_save_lifecycle_probe)) }
  . = 0x00100300;
  .text : { *(.text*) *(.rodata*) }
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

ASSERT(SIZEOF(.probe1) <= 0x100, "save probe exceeds its function extent")
ASSERT(SIZEOF(.probe2) <= 0x100, "save reentry probe exceeds its function extent")
ASSERT(SIZEOF(.probe3) <= 0x100, "save lifecycle probe exceeds its function extent")
