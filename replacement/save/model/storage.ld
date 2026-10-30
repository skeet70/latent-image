ENTRY(svc_5600)

SECTIONS
{
  . = 0x011d5600;
  .svc5600 : { KEEP(*(.svc5600)) }
  . = 0x011d5688;
  .svc5688 : { KEEP(*(.svc5688)) }
  . = 0x011d5714;
  .svc5714 : { KEEP(*(.svc5714)) }
  . = 0x011d5a00;
  .svc5a00 : { KEEP(*(.svc5a00)) }

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

ASSERT(SIZEOF(.svc5600) <= 0x88, "storage model entry exceeds its slot")
ASSERT(SIZEOF(.svc5688) <= 0x8c, "kind-three storage model exceeds its slot")
ASSERT(SIZEOF(.svc5714) <= 0x80, "release model exceeds its slot")
ASSERT(SIZEOF(.svc5a00) <= 0x200, "shared storage model exceeds its slot")
