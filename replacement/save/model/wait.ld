ENTRY(svc_wait)

SECTIONS
{
  . = 0x007243a8;
  .svcwait : { KEEP(*(.svcwait)) }

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

ASSERT(SIZEOF(.svcwait) <= 0x100, "wait model exceeds its slot")
