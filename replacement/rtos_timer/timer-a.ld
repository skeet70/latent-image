/* Loader A: timer entries at their stock addresses; bodies after the stock block. */
ENTRY(xm1_rtos_timer_insert)

MEMORY {
  timer_insert (rx) : ORIGIN = 0x44c60, LENGTH = 0x48
  timer_remove (rx) : ORIGIN = 0x44ca8, LENGTH = 0xe4
  timer_tick (rx) : ORIGIN = 0x44d8c, LENGTH = 0x1a0
  timer_bodies (rx) : ORIGIN = 0x50000, LENGTH = 0x4000
}

SECTIONS {
  .timer_insert : {
    KEEP(*(.text.timer_insert))
    __timer_insert_end = .;
    . = ORIGIN(timer_insert) + LENGTH(timer_insert);
  } > timer_insert =0xf000f0e7
  .timer_remove : {
    KEEP(*(.text.timer_remove))
    __timer_remove_end = .;
    . = ORIGIN(timer_remove) + LENGTH(timer_remove);
  } > timer_remove =0xf000f0e7
  .timer_tick : {
    KEEP(*(.text.timer_tick))
    __timer_tick_end = .;
    . = ORIGIN(timer_tick) + LENGTH(timer_tick);
  } > timer_tick =0xf000f0e7
  .timer_bodies : {
    KEEP(*(.text.timer_body_*))
    *(.text.*)
    *(.rodata .rodata.*)
    __timer_bodies_end = .;
    . = ORIGIN(timer_bodies) + LENGTH(timer_bodies);
  } > timer_bodies =0xf000f0e7
  .forbidden (NOLOAD) : { *(.data .data.* .bss .bss.* COMMON) } > timer_bodies
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}

ASSERT(xm1_rtos_timer_insert == ORIGIN(timer_insert), "insert entry is not at its stock address")
ASSERT(__timer_insert_end <= ORIGIN(timer_insert) + LENGTH(timer_insert), "insert entry exceeds its stock extent")
ASSERT(xm1_rtos_timer_remove == ORIGIN(timer_remove), "remove entry is not at its stock address")
ASSERT(__timer_remove_end <= ORIGIN(timer_remove) + LENGTH(timer_remove), "remove entry exceeds its stock extent")
ASSERT(xm1_rtos_timer_tick == ORIGIN(timer_tick), "tick entry is not at its stock address")
ASSERT(__timer_tick_end <= ORIGIN(timer_tick) + LENGTH(timer_tick), "tick entry exceeds its stock extent")
ASSERT(__timer_bodies_end <= ORIGIN(timer_bodies) + LENGTH(timer_bodies), "timer bodies exceed their region")
ASSERT(SIZEOF(.forbidden) == 0, "data or bss present")
