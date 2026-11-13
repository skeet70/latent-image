ENTRY(xm1_getter_owned)
xm1_stock_61df44 = 0x61df44;
xm1_stock_61d8a4 = 0x61d8a4;
xm1_stock_61d59c = 0x61d59c;
xm1_stock_61d618 = 0x61d618;
xm1_stock_61d5f0 = 0x61d5f0;
xm1_stock_61d65c = 0x61d65c;
xm1_bank_property_set = 0x624694;
xm1_bank_current_selection = 0x6439e0;
xm1_bank_short_read = 0x72b3a4;
xm1_bank_custom_refresh = 0x6a86c4;
PROVIDE(xm1_bank_record_append = 0x6ac7d4);
xm1_kernel_console9 = 0x41a34;
xm1_kernel_critical_enter = 0x7235d8;
xm1_kernel_critical_exit = 0x7236b0;
xm1_backup_diagnostic726c74 = 0x726c74;
xm1_backup_write6a59b8 = 0x6a59b8;
xm1_backup_report633594 = 0x633594;
SECTIONS {
  . = 0x005c0000;
  .text : {
    KEEP(*(.text.xm1_getter_owned))
    KEEP(*(.text.xm1_setter_owned))
    KEEP(*(.text.xm1_storage_*))
    *(.text*)
    *(.rodata*)
    . = ALIGN(4);
  }
  .data : { *(.data*) *(.sdata*) *(.got*) }
  .bss : { *(.bss*) *(.sbss*) *(COMMON) }
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.eh_frame*) *(.ARM.attributes) *(.note*) *(.comment*) *(.debug*) *(.llvm_addrsig) }
}
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "settings storage has writable statics")
