ENTRY(capture_wait)

SECTIONS
{
  .text.capture_wait 0x007243a8 : { KEEP(*(.text.capture_wait)) }
  .text.capture_release 0x00724638 : { KEEP(*(.text.capture_release)) }

  /DISCARD/ :
  {
    *(.ARM.exidx*)
    *(.ARM.extab*)
    *(.comment)
    *(.note*)
    *(.eh_frame*)
    *(.data*)
    *(.bss*)
  }
}

ASSERT(ADDR(.text.capture_wait) == 0x007243a8, "wait address")
ASSERT(ADDR(.text.capture_release) == 0x00724638, "release address")
