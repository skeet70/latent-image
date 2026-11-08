/* Native-only scratch: the stock extents 0xdd054/0xdd168 cannot hold the typed bodies. */
ENTRY(xm1_rtos_task_create)

MEMORY {
  create (rx) : ORIGIN = 0x50000, LENGTH = 0x200
  delete (rx) : ORIGIN = 0x50200, LENGTH = 0x200
}

SECTIONS {
  .create : {
    KEEP(*(.text.task_create_entry))
    KEEP(*(.text.task_create_body))
    *(.text.task_reset_fields)
    __create_end = .;
    . = ORIGIN(create) + LENGTH(create);
  } > create =0xf000f0e7
  .delete : {
    KEEP(*(.text.task_delete_entry))
    KEEP(*(.text.task_delete_body))
    __delete_end = .;
    . = ORIGIN(delete) + LENGTH(delete);
  } > delete =0xf000f0e7
  .forbidden (NOLOAD) : { *(.text.* .rodata .rodata.* .data .data.* .bss .bss.* COMMON) } > create
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}

xm1_task_mask_save = 0xd2560;
xm1_task_mask_restore = 0xd2568;

ASSERT(xm1_rtos_task_create == 0x50000, "create entry moved")
ASSERT(xm1_rtos_task_delete == 0x50200, "delete entry moved")
ASSERT(__create_end <= ORIGIN(create) + LENGTH(create), "create body exceeds its scratch window")
ASSERT(__delete_end <= ORIGIN(delete) + LENGTH(delete), "delete body exceeds its scratch window")
ASSERT(SIZEOF(.forbidden) == 0, "unplaced code or writable data")
