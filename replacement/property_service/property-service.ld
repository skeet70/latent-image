ENTRY(xm1_property_get_entry)

/* The stock leaf functions whose results are defaults. */
xm1_stock_61df44 = 0x61df44;
xm1_stock_61d8a4 = 0x61d8a4;
xm1_stock_61d59c = 0x61d59c;
xm1_stock_61d618 = 0x61d618;
xm1_stock_61d5f0 = 0x61d5f0;
xm1_stock_61d65c = 0x61d65c;

xm1_backup_context723cb8 = 0x723cb8;
xm1_backup_acquire724b68 = 0x724b68;
xm1_backup_release724c00 = 0x724c00;
xm1_backup_diagnostic726c74 = 0x726c74;
xm1_backup_write6a59b8 = 0x6a59b8;
xm1_backup_report633594 = 0x633594;
PROVIDE(xm1_bank_record_append = 0x6ac7d4);

/* The hash-bound stage-3 carrier. */
MEMORY {
  CARRIER (rx) : ORIGIN = 0x62c120, LENGTH = 0x62fbfc - 0x62c120
}

SECTIONS
{
  .text : {
    __property_service_start = .;
    KEEP(*(.text.xm1_property_get_entry))
    KEEP(*(.text.xm1_property_set_entry))
    *(.text*)
    KEEP(*(.rodata.property_mapping))
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
ASSERT(xm1_property_get_entry == 0x62c120, "getter entry moved")
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "property service has writable statics")
