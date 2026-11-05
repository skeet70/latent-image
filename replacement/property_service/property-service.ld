ENTRY(xm1_getter_stub)

/* The stock leaf functions whose results are defaults. */
xm1_stock_61df44 = 0x61df44;
xm1_stock_61d8a4 = 0x61d8a4;
xm1_stock_61d59c = 0x61d59c;
xm1_stock_61d618 = 0x61d618;
xm1_stock_61d5f0 = 0x61d5f0;
xm1_stock_61d65c = 0x61d65c;

/* The interval of owned setter arms the hull scan chooses; staging derives and checks it. */
MEMORY {
  CARRIER (rx) : ORIGIN = 0x62c120, LENGTH = 0x62fbfc - 0x62c120
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

ASSERT(__property_service_start == 0x62c120, "property service left its carrier start")
ASSERT(xm1_getter_stub == 0x62c120, "getter stub moved")
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "property service has writable statics")
