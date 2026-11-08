/* Loader A: owned global-lock entries at their stock extents and the bodies in native scratch. */
ENTRY(xm1_rtos_global_lock_try)

MEMORY {
  try_entry (rx) : ORIGIN = 0x412cc, LENGTH = 0x1c
  release_entry (rx) : ORIGIN = 0x412e8, LENGTH = 0x1c
  observe_entry (rx) : ORIGIN = 0x41304, LENGTH = 0x1c
  bodies (rx) : ORIGIN = 0x50000, LENGTH = 0xc4
}

SECTIONS {
  .try_entry : {
    KEEP(*(.text.global_lock_try_entry))
    __try_entry_end = .;
    . = ORIGIN(try_entry) + LENGTH(try_entry);
  } > try_entry =0xf000f0e7
  .release_entry : {
    KEEP(*(.text.global_lock_release_entry))
    __release_entry_end = .;
    . = ORIGIN(release_entry) + LENGTH(release_entry);
  } > release_entry =0xf000f0e7
  .observe_entry : {
    KEEP(*(.text.global_lock_observe_entry))
    __observe_entry_end = .;
    . = ORIGIN(observe_entry) + LENGTH(observe_entry);
  } > observe_entry =0xf000f0e7
  .bodies : {
    KEEP(*(.text.global_lock_try_body))
    __try_body_end = .;
    KEEP(*(.text.global_lock_release_body))
    __release_body_end = .;
    KEEP(*(.text.global_lock_observe_body))
    __observe_body_end = .;
    . = ORIGIN(bodies) + LENGTH(bodies);
  } > bodies =0xf000f0e7
  .forbidden (NOLOAD) : { *(.text .text.* .rodata .rodata.* .data .data.* .bss .bss.* COMMON) } > bodies
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}

ASSERT(xm1_rtos_global_lock_try == ORIGIN(try_entry), "+0x161c entry is not at its stock address")
ASSERT(__try_entry_end == ORIGIN(try_entry) + 4, "+0x161c veneer size differs")
ASSERT(xm1_rtos_global_lock_release == ORIGIN(release_entry), "+0x1620 entry is not at its stock address")
ASSERT(__release_entry_end == ORIGIN(release_entry) + 4, "+0x1620 veneer size differs")
ASSERT(xm1_rtos_global_lock_observe == ORIGIN(observe_entry), "+0x1624 entry is not at its stock address")
ASSERT(__observe_entry_end == ORIGIN(observe_entry) + 4, "+0x1624 veneer size differs")
ASSERT(xm1_rtos_global_lock_try_body == ORIGIN(bodies), "try body is not at the scratch origin")
ASSERT(__try_body_end <= __release_body_end && __release_body_end <= __observe_body_end, "body order differs")
ASSERT(__observe_body_end <= ORIGIN(bodies) + LENGTH(bodies), "bodies exceed the native scratch region")
ASSERT(SIZEOF(.forbidden) == 0, "unplaced code or writable data")
