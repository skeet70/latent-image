ENTRY(record_store_entry_open)
INCLUDE imports.ld
SECTIONS {
  .compiler_builtins 0 : { *compiler_builtins*(.text* .rodata* .data* .bss*) }
  .entry_open 0x6ac2b4 : { KEEP(*(.text.record_store_entry_open)) }
  .run0 0x6ac2b8 : { KEEP(*(.text.record_store_zero)) }
  .entry_register 0x6ac328 : { KEEP(*(.text.record_store_entry_register)) }
  .run1 0x6ac32c : { KEEP(*(.text.record_store_open)) }
  .entry_close 0x6ac478 : { KEEP(*(.text.record_store_entry_close)) }
  .entry_read_latest 0x6ac4c8 : { KEEP(*(.text.record_store_entry_read_latest)) }
  .run3 0x6ac4cc : { KEEP(*(.text.record_store_read_at)) KEEP(*(.text.record_store_close)) }
  .entry_append 0x6ac7d4 : { KEEP(*(.text.record_store_entry_append)) }
  .run4 0x6ac7d8 : { KEEP(*(.text.record_store_register)) }
  .entry_erase_all 0x6ac984 : { KEEP(*(.text.record_store_entry_erase_all)) }
  .run5 0x6ac988 : { KEEP(*(.text.record_store_read_latest)) }
  .entry_collect 0x6acab8 : { KEEP(*(.text.record_store_entry_collect)) }
  .run6 0x6acabc : { KEEP(*(.text.record_store_append)) KEEP(*(.text.record_store_erase_all)) *(.rodata*) }
  .entry_read_at 0x6acda0 : { KEEP(*(.text.record_store_entry_read_at)) }
  .entry_zero 0x6ae0fc : { KEEP(*(.text.record_store_entry_zero)) }
  .run7 0x6acda4 : { KEEP(*(.text.record_store_collect)) *(.text*) }
  .end 0x6ae113 : { BYTE(0) }
  .data : { *(.data*) *(.sdata*) *(.got*) }
  .bss : { *(.bss*) *(.sbss*) *(COMMON) }
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.eh_frame*) *(.ARM.attributes) *(.note*) *(.comment*) *(.debug*) *(.llvm_addrsig) }
}
ASSERT(SIZEOF(.run0) <= 112, "record run0 overflow")
ASSERT(SIZEOF(.run1) <= 332, "record run1 overflow")
ASSERT(SIZEOF(.run3) <= 776, "record run3 overflow")
ASSERT(SIZEOF(.run4) <= 428, "record run4 overflow")
ASSERT(SIZEOF(.run5) <= 304, "record run5 overflow")
ASSERT(SIZEOF(.run6) <= 740, "record run6 overflow")
ASSERT(SIZEOF(.run7) <= 4952, "record run7 overflow")
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "record store has writable statics")
ASSERT(SIZEOF(.compiler_builtins) == 0, "record store links compiler-builtins")
ASSERT(record_store_entry_open == 0x6ac2b4, "open veneer moved")
ASSERT(record_store_entry_register == 0x6ac328, "register veneer moved")
ASSERT(record_store_entry_close == 0x6ac478, "close veneer moved")
ASSERT(record_store_entry_read_latest == 0x6ac4c8, "latest veneer moved")
ASSERT(record_store_entry_append == 0x6ac7d4, "append veneer moved")
ASSERT(record_store_entry_erase_all == 0x6ac984, "erase veneer moved")
ASSERT(record_store_entry_collect == 0x6acab8, "collect veneer moved")
ASSERT(record_store_entry_read_at == 0x6acda0, "read-at veneer moved")
ASSERT(record_store_entry_zero == 0x6ae0fc, "zero veneer moved")
