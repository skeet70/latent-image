/* Loader B: the owned global-lock entries and atomic helper closure at their stock extents. */
ENTRY(xm1_rtos_global_lock_try)

MEMORY {
  slot_try (rx) : ORIGIN = 0xd12cc, LENGTH = 0x1c
  slot_release (rx) : ORIGIN = 0xd12e8, LENGTH = 0x1c
  slot_observe (rx) : ORIGIN = 0xd1304, LENGTH = 0x1c
  helper_try (rx) : ORIGIN = 0xd27d4, LENGTH = 0x28
  helper_release (rx) : ORIGIN = 0xd27fc, LENGTH = 0x2c
  helper_observe (rx) : ORIGIN = 0xd2828, LENGTH = 0x8
}

SECTIONS {
  .slot_try : {
    KEEP(*(.text.global_lock_try))
    __slot_try_end = .;
    . = ORIGIN(slot_try) + LENGTH(slot_try);
  } > slot_try =0xf000f0e7
  .slot_release : {
    KEEP(*(.text.global_lock_release))
    __slot_release_end = .;
    . = ORIGIN(slot_release) + LENGTH(slot_release);
  } > slot_release =0xf000f0e7
  .slot_observe : {
    KEEP(*(.text.global_lock_observe))
    __slot_observe_end = .;
    . = ORIGIN(slot_observe) + LENGTH(slot_observe);
  } > slot_observe =0xf000f0e7
  .helper_try : {
    KEEP(*(.text.global_lock_helper_try))
    __helper_try_end = .;
    . = ORIGIN(helper_try) + LENGTH(helper_try);
  } > helper_try =0xf000f0e7
  .helper_release : {
    KEEP(*(.text.global_lock_helper_release))
    __helper_release_end = .;
    . = ORIGIN(helper_release) + LENGTH(helper_release);
  } > helper_release =0xf000f0e7
  .helper_observe : {
    KEEP(*(.text.global_lock_helper_observe))
    __helper_observe_end = .;
    . = ORIGIN(helper_observe) + LENGTH(helper_observe);
  } > helper_observe =0xf000f0e7
  .forbidden (NOLOAD) : { *(.text .text.* .rodata .rodata.* .data .data.* .bss .bss.* COMMON) } > helper_observe
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}

ASSERT(xm1_rtos_global_lock_try == ORIGIN(slot_try), "+0x161c entry is not at its stock address")
ASSERT(__slot_try_end <= ORIGIN(slot_try) + LENGTH(slot_try), "+0x161c entry exceeds its stock extent")
ASSERT(xm1_rtos_global_lock_release == ORIGIN(slot_release), "+0x1620 entry is not at its stock address")
ASSERT(__slot_release_end <= ORIGIN(slot_release) + LENGTH(slot_release), "+0x1620 entry exceeds its stock extent")
ASSERT(xm1_rtos_global_lock_observe == ORIGIN(slot_observe), "+0x1624 entry is not at its stock address")
ASSERT(__slot_observe_end <= ORIGIN(slot_observe) + LENGTH(slot_observe), "+0x1624 entry exceeds its stock extent")
ASSERT(helper_try == ORIGIN(helper_try), "try helper is not at its stock address")
ASSERT(__helper_try_end <= ORIGIN(helper_try) + LENGTH(helper_try), "try helper exceeds its stock extent")
ASSERT(helper_release == ORIGIN(helper_release), "release helper is not at its stock address")
ASSERT(__helper_release_end <= ORIGIN(helper_release) + LENGTH(helper_release), "release helper exceeds its stock extent")
ASSERT(helper_observe == ORIGIN(helper_observe), "observe helper is not at its stock address")
ASSERT(__helper_observe_end <= ORIGIN(helper_observe) + LENGTH(helper_observe), "observe helper exceeds its stock extent")
ASSERT(SIZEOF(.forbidden) == 0, "unplaced code or writable data")
