ENTRY(ram_push_install)
SECTIONS {
  . = RAM_PUSH_BASE;
  __ram_push_start = .;
  .text : {
    *af_ring_hooks.o(.text .text.*)
    *af_ring_payload.o(.text .text.* .rodata .rodata.*)
  }
  __af_code_end = .;
  . = RAM_PUSH_BASE + 0x1000;
  . = ALIGN(32);
  __ram_push_installer_start = .;
  .ram_push.text : {
    *ram_push_installer.o(.text .text.* .rodata .rodata.*)
  }
  . = ALIGN(4);
  .ram_push.publications : {
    __ram_push_publications_start = .;
    KEEP(*(.ram_push.publications))
    __ram_push_publications_end = .;
  }
  . = ALIGN(32);
  .ram_push.rendezvous : { KEEP(*(.ram_push.rendezvous)) }
  . = ALIGN(32);
  .ram_push.mailbox : {
    KEEP(*(.ram_push.mailbox))
    BYTE(0)
    . = ALIGN(32);
  }
  __af_installer_end = .;
  . = RAM_PUSH_BASE + 0x4000;
  .af_ring.storage : { KEEP(*(.af_ring.storage)) }
  __ram_push_end = .;
  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) }
}
ASSERT(__af_code_end <= RAM_PUSH_BASE + 0x1000, "AF code exceeds its window")
ASSERT(__af_installer_end <= RAM_PUSH_BASE + 0x4000, "installer overlaps AF ring")
ASSERT(af_ring_storage == RAM_PUSH_BASE + 0x4000, "AF ring offset changed")
ASSERT(__ram_push_end == RAM_PUSH_BASE + 0x6440, "AF ring extent changed")
ASSERT(__ram_push_publications_end - __ram_push_publications_start == 96, "AF publication count changed")
