ENTRY(xm1_rtos_timer_insert)

MEMORY {
  timer_initialize (rx) : ORIGIN = 0x44a50, LENGTH = 4
  initialize_fill (rx) : ORIGIN = 0x44a54, LENGTH = 0x20c
  timer_insert (rx) : ORIGIN = 0x44c60, LENGTH = 4
  insert_fill (rx) : ORIGIN = 0x44c64, LENGTH = 0x44
  timer_remove (rx) : ORIGIN = 0x44ca8, LENGTH = 4
  remove_fill (rx) : ORIGIN = 0x44cac, LENGTH = 0xe0
  timer_tick (rx) : ORIGIN = 0x44d8c, LENGTH = 4
  tick_fill (rx) : ORIGIN = 0x44d90, LENGTH = 0x19c
  INCLUDE fixed-memory.ld
}

SECTIONS {
  .timer_initialize : { KEEP(*(.text.timer_initialize)) } > timer_initialize
  .initialize_fill : { . = ORIGIN(initialize_fill) + LENGTH(initialize_fill); } > initialize_fill =0xf000f0e7
  .timer_insert : { KEEP(*(.text.timer_insert)) } > timer_insert
  .insert_fill : { . = ORIGIN(insert_fill) + LENGTH(insert_fill); } > insert_fill =0xf000f0e7
  .timer_remove : { KEEP(*(.text.timer_remove)) } > timer_remove
  .remove_fill : { . = ORIGIN(remove_fill) + LENGTH(remove_fill); } > remove_fill =0xf000f0e7
  .timer_tick : { KEEP(*(.text.timer_tick)) } > timer_tick
  .tick_fill : {
    . = ORIGIN(tick_fill) + LENGTH(tick_fill);
    __timer_carrier_end = .;
  } > tick_fill =0xf000f0e7
  .bank : {
    __timer_bank_start = .;
    KEEP(*(.text.timer_body_*))
    *(.text .text.*)
    __timer_bank_end = .;
  } > OWNED_FIXED
  .forbidden (NOLOAD) : { *(.rodata .rodata.* .data .data.* .bss .bss.* COMMON) } > OWNED_FIXED
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}

ASSERT(xm1_rtos_timer_initialize == 0x44a50, "initializer entry moved")
ASSERT(xm1_rtos_timer_insert == 0x44c60, "insert entry moved")
ASSERT(xm1_rtos_timer_remove == 0x44ca8, "remove entry moved")
ASSERT(xm1_rtos_timer_tick == 0x44d8c, "tick entry moved")
ASSERT(SIZEOF(.timer_initialize) == 4 && SIZEOF(.timer_insert) == 4 && SIZEOF(.timer_remove) == 4 && SIZEOF(.timer_tick) == 4, "entry veneer size differs")
ASSERT(__timer_carrier_end == 0x44f2c, "timer carrier end moved")
ASSERT(__timer_bank_start == ORIGIN(OWNED_FIXED), "bank body moved")
ASSERT(SIZEOF(.forbidden) == 0, "read-only or writable data")
