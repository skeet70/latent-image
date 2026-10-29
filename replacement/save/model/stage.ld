ENTRY(svc_stage)

SECTIONS
{
  . = 0x006fae80;
  .svcstage : { KEEP(*(.svcstage)) }

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

ASSERT(SIZEOF(.svcstage) <= 0x100, "stage model exceeds its slot")
