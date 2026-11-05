ENTRY(xm1_getter_stub)

xm1_getter_exit = 0x624400;
xm1_setter_exit = 0x630200;

/* An owned run of setter arms that staging admits; admission derives the candidates. */
MEMORY {
  CARRIER (rx) : ORIGIN = 0x62c4b0, LENGTH = 0x62d674 - 0x62c4b0
}

SECTIONS
{
  .text : {
    __property_service_start = .;
    KEEP(*(.text.xm1_getter_stub))
    KEEP(*(.text.xm1_setter_stub))
    *(.text*)
    *(.rodata*)
    . = ALIGN(4);
    __property_service_end = .;
  } > CARRIER
  .data : { *(.data*) *(.sdata*) *(.got*) } > CARRIER
  .bss : { *(.bss*) *(.sbss*) *(COMMON) } > CARRIER
  /DISCARD/ : {
    *(.ARM.exidx*)
    *(.ARM.extab*)
    *(.eh_frame*)
    *(.ARM.attributes)
    *(.note*)
    *(.comment*)
    *(.debug*)
    *(.llvm_addrsig)
  }
}

ASSERT(__property_service_start == 0x62c4b0, "property service left its carrier start")
ASSERT(xm1_getter_stub == 0x62c4b0, "getter stub moved")
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "property service has writable statics")
