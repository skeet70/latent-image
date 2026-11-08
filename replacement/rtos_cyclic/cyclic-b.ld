ENTRY(xm1_rtos_cyclic_start)

MEMORY {
  configure_entry (rx) : ORIGIN = 0xdbdfc, LENGTH = 4
  configure_body (rx) : ORIGIN = 0xdbe00, LENGTH = 0x44
  schedule_entry (rx) : ORIGIN = 0xdbe44, LENGTH = 4
  schedule_body (rx) : ORIGIN = 0xdbe48, LENGTH = 0x28
  expire_entry (rx) : ORIGIN = 0xdbe70, LENGTH = 4
  expire_body (rx) : ORIGIN = 0xdbe74, LENGTH = 0x88
  initialize_entry (rx) : ORIGIN = 0xdbefc, LENGTH = 4
  initialize_body (rx) : ORIGIN = 0xdbf00, LENGTH = 0x8c
  start_entry (rx) : ORIGIN = 0xdbf8c, LENGTH = 4
  start_body (rx) : ORIGIN = 0xdbf90, LENGTH = 0x16c
  stop_entry (rx) : ORIGIN = 0xdc0fc, LENGTH = 4
  stop_body (rx) : ORIGIN = 0xdc100, LENGTH = 0x14c
  create_entry (rx) : ORIGIN = 0xdd7dc, LENGTH = 4
  create_body (rx) : ORIGIN = 0xdd7e0, LENGTH = 0x8c
  delete_entry (rx) : ORIGIN = 0xdd86c, LENGTH = 4
  delete_body (rx) : ORIGIN = 0xdd870, LENGTH = 0x50
  cyclic_core (rx) : ORIGIN = 0xdc24c, LENGTH = 0x800
}

SECTIONS {
  .configure_entry : { KEEP(*(.text.cyclic_configure)) } > configure_entry
  .configure_body : {
    KEEP(*(.text.cyclic_body_configure))
    __configure_body_end = .;
    . = ORIGIN(configure_body) + LENGTH(configure_body);
  } > configure_body =0xf000f0e7
  .schedule_entry : { KEEP(*(.text.cyclic_schedule)) } > schedule_entry
  .schedule_body : {
    KEEP(*(.text.cyclic_body_schedule))
    __schedule_body_end = .;
    . = ORIGIN(schedule_body) + LENGTH(schedule_body);
  } > schedule_body =0xf000f0e7
  .expire_entry : { KEEP(*(.text.cyclic_expire)) } > expire_entry
  .expire_body : {
    KEEP(*(.text.cyclic_body_expire))
    __expire_body_end = .;
    . = ORIGIN(expire_body) + LENGTH(expire_body);
  } > expire_body =0xf000f0e7
  .initialize_entry : { KEEP(*(.text.cyclic_initialize)) } > initialize_entry
  .initialize_body : {
    KEEP(*(.text.cyclic_body_initialize))
    __initialize_body_end = .;
    . = ORIGIN(initialize_body) + LENGTH(initialize_body);
  } > initialize_body =0xf000f0e7
  .start_entry : { KEEP(*(.text.cyclic_start)) } > start_entry
  .start_body : {
    KEEP(*(.text.cyclic_body_start))
    __start_body_end = .;
    . = ORIGIN(start_body) + LENGTH(start_body);
  } > start_body =0xf000f0e7
  .stop_entry : { KEEP(*(.text.cyclic_stop)) } > stop_entry
  .stop_body : {
    KEEP(*(.text.cyclic_body_stop))
    __stop_body_end = .;
    . = ORIGIN(stop_body) + LENGTH(stop_body);
  } > stop_body =0xf000f0e7
  .create_entry : { KEEP(*(.text.cyclic_create)) } > create_entry
  .create_body : {
    KEEP(*(.text.cyclic_body_create))
    __create_body_end = .;
    . = ORIGIN(create_body) + LENGTH(create_body);
  } > create_body =0xf000f0e7
  .delete_entry : { KEEP(*(.text.cyclic_delete)) } > delete_entry
  .delete_body : {
    KEEP(*(.text.cyclic_body_delete))
    __delete_body_end = .;
    . = ORIGIN(delete_body) + LENGTH(delete_body);
  } > delete_body =0xf000f0e7
  .cyclic_core : {
    *(.text.cyclic_configure_core)
    *(.text.cyclic_schedule_core)
    *(.text.cyclic_expire_core)
    *(.text.cyclic_initialize_core)
    *(.text.cyclic_start_core)
    *(.text.cyclic_stop_core)
    *(.text.cyclic_create_core)
    *(.text.cyclic_delete_core)
    *(.text.*rust_begin_unwind)
    *(.rodata .rodata.*)
    __cyclic_core_end = .;
    . = ORIGIN(cyclic_core) + LENGTH(cyclic_core);
  } > cyclic_core =0xf000f0e7
  .forbidden (NOLOAD) : { *(.text.* .rodata .rodata.* .data .data.* .bss .bss.* COMMON) } > cyclic_core
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}

ASSERT(xm1_rtos_cyclic_configure == 0xdbdfc, "configure entry moved")
ASSERT(xm1_rtos_cyclic_schedule == 0xdbe44, "schedule entry moved")
ASSERT(xm1_rtos_cyclic_expire == 0xdbe70, "expire entry moved")
ASSERT(xm1_rtos_cyclic_initialize == 0xdbefc, "initialize entry moved")
ASSERT(xm1_rtos_cyclic_start == 0xdbf8c, "start entry moved")
ASSERT(xm1_rtos_cyclic_stop == 0xdc0fc, "stop entry moved")
ASSERT(xm1_rtos_cyclic_create == 0xdd7dc, "create entry moved")
ASSERT(xm1_rtos_cyclic_delete == 0xdd86c, "delete entry moved")
ASSERT(SIZEOF(.configure_entry) == 4 && SIZEOF(.schedule_entry) == 4 && SIZEOF(.expire_entry) == 4 && SIZEOF(.initialize_entry) == 4 && SIZEOF(.start_entry) == 4 && SIZEOF(.stop_entry) == 4 && SIZEOF(.create_entry) == 4 && SIZEOF(.delete_entry) == 4, "entry veneer size differs")
ASSERT(__configure_body_end <= ORIGIN(configure_body) + LENGTH(configure_body), "configure body exceeds carrier")
ASSERT(__schedule_body_end <= ORIGIN(schedule_body) + LENGTH(schedule_body), "schedule body exceeds carrier")
ASSERT(__expire_body_end <= ORIGIN(expire_body) + LENGTH(expire_body), "expire body exceeds carrier")
ASSERT(__initialize_body_end <= ORIGIN(initialize_body) + LENGTH(initialize_body), "initialize body exceeds carrier")
ASSERT(__start_body_end <= ORIGIN(start_body) + LENGTH(start_body), "start body exceeds carrier")
ASSERT(__stop_body_end <= ORIGIN(stop_body) + LENGTH(stop_body), "stop body exceeds carrier")
ASSERT(__create_body_end <= ORIGIN(create_body) + LENGTH(create_body), "create body exceeds carrier")
ASSERT(__delete_body_end <= ORIGIN(delete_body) + LENGTH(delete_body), "delete body exceeds carrier")
ASSERT(__cyclic_core_end <= ORIGIN(cyclic_core) + LENGTH(cyclic_core), "core exceeds native scratch")
ASSERT(ORIGIN(cyclic_core) + LENGTH(cyclic_core) <= 0xdd7dc, "native scratch reaches create")
ASSERT(SIZEOF(.forbidden) == 0, "unplaced code or writable data")
