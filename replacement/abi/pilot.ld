ENTRY(xm1_abi_probe)

SECTIONS
{
  . = 0x00001000;
  .text :
  {
    KEEP(*(.text.xm1_abi_probe))
    *(.text*)
    *(.rodata*)
    . = 0x00001f00;
  }

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

ASSERT(SIZEOF(.text) == 0xf00, "pilot text does not fill the declared 0xf00-byte function span")
ASSERT(SIZEOF(.data) == 0, "pilot must not carry mutable data")
ASSERT(SIZEOF(.bss) == 0, "pilot must not carry bss")
