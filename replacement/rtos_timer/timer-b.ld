ENTRY(xm1_rtos_timer_insert)

MEMORY {
  timer_initialize (rx) : ORIGIN = 0xd4a10, LENGTH = 4
  tick_and_initialize (rx) : ORIGIN = 0xd4a14, LENGTH = 0x20c
  timer_insert (rx) : ORIGIN = 0xd4c20, LENGTH = 4
  finish_body (rx) : ORIGIN = 0xd4c24, LENGTH = 0x44
  timer_remove (rx) : ORIGIN = 0xd4c68, LENGTH = 4
  insert_body (rx) : ORIGIN = 0xd4c6c, LENGTH = 0x84
  sift_up_body (rx) : ORIGIN = 0xd4cf0, LENGTH = 0x5c
  timer_tick (rx) : ORIGIN = 0xd4d4c, LENGTH = 4
  remove_body (rx) : ORIGIN = 0xd4d50, LENGTH = 0xfc
  sift_down_body (rx) : ORIGIN = 0xd4e4c, LENGTH = 0xa0
}

SECTIONS {
  .timer_initialize : {
    KEEP(*(.text.timer_initialize))
  } > timer_initialize
  .tick_and_initialize : {
    KEEP(*(.text.timer_body_tick))
    KEEP(*(.text.timer_body_initialize))
    *(.text.timer_lock_acquire)
    __tick_and_initialize_end = .;
    . = ORIGIN(tick_and_initialize) + LENGTH(tick_and_initialize);
  } > tick_and_initialize =0xf000f0e7
  .timer_insert : {
    KEEP(*(.text.timer_insert))
  } > timer_insert
  .timer_finish_body : {
    KEEP(*(.text.timer_finish))
    __timer_finish_end = .;
    . = ORIGIN(finish_body) + LENGTH(finish_body);
  } > finish_body =0xf000f0e7
  .timer_remove : {
    KEEP(*(.text.timer_remove))
  } > timer_remove
  .insert_body : {
    KEEP(*(.text.timer_body_insert))
    __insert_body_end = .;
    . = ORIGIN(insert_body) + LENGTH(insert_body);
  } > insert_body =0xf000f0e7
  .sift_up_body : {
    *(.text.timer_sift_up)
    __sift_up_body_end = .;
    . = ORIGIN(sift_up_body) + LENGTH(sift_up_body);
  } > sift_up_body =0xf000f0e7
  .timer_tick : {
    KEEP(*(.text.timer_tick))
  } > timer_tick
  .remove_body : {
    KEEP(*(.text.timer_body_remove))
    __remove_body_end = .;
    . = ORIGIN(remove_body) + LENGTH(remove_body);
  } > remove_body =0xf000f0e7
  .sift_down_body : {
    *(.text.timer_sift_down)
    __sift_down_body_end = .;
    . = ORIGIN(sift_down_body) + LENGTH(sift_down_body);
    __timer_module_end = .;
  } > sift_down_body =0xf000f0e7
  .forbidden (NOLOAD) : { *(.text.* .rodata .rodata.* .data .data.* .bss .bss.* COMMON) } > sift_down_body
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}

ASSERT(xm1_rtos_timer_initialize == 0xd4a10, "initializer entry moved")
ASSERT(xm1_rtos_timer_insert == 0xd4c20, "insert entry moved")
ASSERT(xm1_rtos_timer_remove == 0xd4c68, "remove entry moved")
ASSERT(xm1_rtos_timer_tick == 0xd4d4c, "tick entry moved")
ASSERT(SIZEOF(.timer_initialize) == 4 && SIZEOF(.timer_insert) == 4 && SIZEOF(.timer_remove) == 4 && SIZEOF(.timer_tick) == 4, "entry veneer size differs")
ASSERT(__tick_and_initialize_end <= ORIGIN(tick_and_initialize) + LENGTH(tick_and_initialize), "tick and initializer exceed carrier")
ASSERT(__timer_finish_end <= ORIGIN(finish_body) + LENGTH(finish_body), "finish exceeds carrier")
ASSERT(__insert_body_end <= ORIGIN(insert_body) + LENGTH(insert_body), "insert exceeds carrier")
ASSERT(__sift_up_body_end <= ORIGIN(sift_up_body) + LENGTH(sift_up_body), "sift-up exceeds carrier")
ASSERT(__remove_body_end <= ORIGIN(remove_body) + LENGTH(remove_body), "remove exceeds carrier")
ASSERT(__sift_down_body_end <= ORIGIN(sift_down_body) + LENGTH(sift_down_body), "sift-down exceeds carrier")
ASSERT(__timer_module_end == 0xd4eec, "timer module end moved")
ASSERT(SIZEOF(.forbidden) == 0, "unplaced code or writable data")
