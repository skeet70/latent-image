ENTRY(xm1_app_register_callback)

MEMORY {
  dispatch_entry (rx) : ORIGIN = 0x72337c, LENGTH = 0x1c
  register_entry (rx) : ORIGIN = 0x725424, LENGTH = 0x2a0
  unregister_entry (rx) : ORIGIN = 0x7256c4, LENGTH = 0xe4
}

SECTIONS {
  .dispatch_entry : {
    KEEP(*(.text.app_dispatch))
    __dispatch_end = .;
    . = ORIGIN(dispatch_entry) + LENGTH(dispatch_entry);
  } > dispatch_entry =0xf000f0e7
  .register_entry : {
    KEEP(*(.text.app_register))
    KEEP(*(.text.app_register_core))
    KEEP(*(.text.*rust_begin_unwind))
    __register_end = .;
    . = ORIGIN(register_entry) + LENGTH(register_entry);
  } > register_entry =0xf000f0e7
  .unregister_entry : {
    KEEP(*(.text.app_unregister))
    KEEP(*(.text.app_unregister_core))
    __unregister_end = .;
    . = ORIGIN(unregister_entry) + LENGTH(unregister_entry);
  } > unregister_entry =0xf000f0e7
  .forbidden (NOLOAD) : {
    *(.text.* .rodata .rodata.* .data .data.* .bss .bss.* COMMON)
  } > register_entry
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}

ASSERT(xm1_app_dispatch_callback == 0x72337c, "app dispatcher entry moved")
ASSERT(__dispatch_end <= ORIGIN(dispatch_entry) + LENGTH(dispatch_entry), "app dispatcher exceeds its stock extent")
ASSERT(xm1_app_register_callback == 0x725424, "app register entry moved")
ASSERT(xm1_app_unregister_callback == 0x7256c4, "app unregister entry moved")
ASSERT(__register_end <= ORIGIN(register_entry) + LENGTH(register_entry), "app register exceeds its stock extent")
ASSERT(__unregister_end <= ORIGIN(unregister_entry) + LENGTH(unregister_entry), "app unregister exceeds its stock extent")
ASSERT(SIZEOF(.forbidden) == 0, "unplaced app callback code or writable data")
