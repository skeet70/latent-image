ENTRY(xm1_rtos_fixed_pool_poll_unchecked)
MEMORY {
  fp_unchecked (rx) : ORIGIN = 0x411cc, LENGTH = 0xf0
  fp_get (rx) : ORIGIN = 0x4b480, LENGTH = 0x250
  fp_poll (rx) : ORIGIN = 0x4b6d0, LENGTH = 0x144
  fp_timed (rx) : ORIGIN = 0x4b814, LENGTH = 0x2a4
  fp_release (rx) : ORIGIN = 0x4bab8, LENGTH = 0x294
  fp_create (rx) : ORIGIN = 0x4d6c4, LENGTH = 0xcc
  fp_delete (rx) : ORIGIN = 0x4d790, LENGTH = 0x88
  fp_bodies (rx) : ORIGIN = 0x50000, LENGTH = 0x3000
}
xm1_fixed_pool_stock_acquire = 0x439d8;
xm1_fixed_pool_stock_release = 0x43a14;
xm1_fixed_pool_stock_notify = 0x44018;
xm1_fixed_pool_stock_dispatch = 0x43640;
xm1_fixed_pool_stock_mask = 0x42560;
xm1_fixed_pool_stock_restore = 0x42568;
xm1_fixed_pool_stock_save_irq = 0x42574;
xm1_fixed_pool_stock_restore_irq = 0x4257c;
xm1_fixed_pool_stock_wait_release = 0x44568;
xm1_fixed_pool_stock_wait_cancel = 0x44744;
xm1_fixed_pool_stock_wait_enqueue = 0x44800;
xm1_fixed_pool_stock_wait_enqueue_timed = 0x448e4;
xm1_fixed_pool_stock_uidivmod = 0x4da6c;
SECTIONS {
  .fp_entry_unchecked : {
    KEEP(*(.text.fp_entry_unchecked))
    __fp_entry_unchecked_end = .;
    . = ORIGIN(fp_unchecked) + LENGTH(fp_unchecked);
  } > fp_unchecked =0xf000f0e7
  .fp_entry_get : {
    KEEP(*(.text.fp_entry_get))
    __fp_entry_get_end = .;
    . = ORIGIN(fp_get) + LENGTH(fp_get);
  } > fp_get =0xf000f0e7
  .fp_entry_poll : {
    KEEP(*(.text.fp_entry_poll))
    __fp_entry_poll_end = .;
    . = ORIGIN(fp_poll) + LENGTH(fp_poll);
  } > fp_poll =0xf000f0e7
  .fp_entry_timed : {
    KEEP(*(.text.fp_entry_timed))
    __fp_entry_timed_end = .;
    . = ORIGIN(fp_timed) + LENGTH(fp_timed);
  } > fp_timed =0xf000f0e7
  .fp_entry_release : {
    KEEP(*(.text.fp_entry_release))
    __fp_entry_release_end = .;
    . = ORIGIN(fp_release) + LENGTH(fp_release);
  } > fp_release =0xf000f0e7
  .fp_entry_create : {
    KEEP(*(.text.fp_entry_create))
    __fp_entry_create_end = .;
    . = ORIGIN(fp_create) + LENGTH(fp_create);
  } > fp_create =0xf000f0e7
  .fp_entry_delete : {
    KEEP(*(.text.fp_entry_delete))
    __fp_entry_delete_end = .;
    . = ORIGIN(fp_delete) + LENGTH(fp_delete);
  } > fp_delete =0xf000f0e7
  .fp_bodies : {
    KEEP(*(.text.fp_bodies))
    *(.text .text.*)
    *(.rodata .rodata.*)
    __fp_bodies_end = .;
    . = ORIGIN(fp_bodies) + LENGTH(fp_bodies);
  } > fp_bodies =0xf000f0e7
  .forbidden (NOLOAD) : { *(.data .data.* .bss .bss.* COMMON) } > fp_bodies
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}
ASSERT(xm1_rtos_fixed_pool_poll_unchecked == ORIGIN(fp_unchecked), "unchecked entry is not at its stock address")
ASSERT(__fp_entry_unchecked_end <= ORIGIN(fp_unchecked) + LENGTH(fp_unchecked), "unchecked stub exceeds its stock extent")
ASSERT(xm1_rtos_fixed_pool_get == ORIGIN(fp_get), "get entry is not at its stock address")
ASSERT(__fp_entry_get_end <= ORIGIN(fp_get) + LENGTH(fp_get), "get stub exceeds its stock extent")
ASSERT(xm1_rtos_fixed_pool_poll == ORIGIN(fp_poll), "poll entry is not at its stock address")
ASSERT(__fp_entry_poll_end <= ORIGIN(fp_poll) + LENGTH(fp_poll), "poll stub exceeds its stock extent")
ASSERT(xm1_rtos_fixed_pool_timed_get == ORIGIN(fp_timed), "timed get entry is not at its stock address")
ASSERT(__fp_entry_timed_end <= ORIGIN(fp_timed) + LENGTH(fp_timed), "timed get stub exceeds its stock extent")
ASSERT(xm1_rtos_fixed_pool_release == ORIGIN(fp_release), "release entry is not at its stock address")
ASSERT(__fp_entry_release_end <= ORIGIN(fp_release) + LENGTH(fp_release), "release stub exceeds its stock extent")
ASSERT(xm1_rtos_fixed_pool_create == ORIGIN(fp_create), "create entry is not at its stock address")
ASSERT(__fp_entry_create_end <= ORIGIN(fp_create) + LENGTH(fp_create), "create stub exceeds its stock extent")
ASSERT(xm1_rtos_fixed_pool_delete == ORIGIN(fp_delete), "delete entry is not at its stock address")
ASSERT(__fp_entry_delete_end <= ORIGIN(fp_delete) + LENGTH(fp_delete), "delete stub exceeds its stock extent")
ASSERT(__fp_bodies_end <= ORIGIN(fp_bodies) + LENGTH(fp_bodies), "body region exceeds its native-only extent")
ASSERT(SIZEOF(.forbidden) == 0, "unplaced text or data remains")
