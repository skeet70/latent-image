ENTRY(area3_validate)

SECTIONS
{
  . = 0x00800000;
  .text :
  {
    KEEP(*(.text.area3_validate))
    *(.text*)
    *(.rodata*)
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

ASSERT(ADDR(.text) == 0x00800000, "validator text must start at the harness load base")
ASSERT(SIZEOF(.bss) == 0, "validator must not carry bss")
