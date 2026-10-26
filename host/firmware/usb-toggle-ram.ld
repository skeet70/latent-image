ENTRY(ram_push_install)

SECTIONS {
  __ram_push_start = RAM_PUSH_BASE;
  . = ALIGN(RAM_PUSH_BASE, 32);
  __ram_push_installer_start = .;
  .ram_push.text : {
    *ram_push_installer.o(.text)
    *ram_push_installer.o(.text.*)
    *ram_push_installer.o(.rodata)
    *ram_push_installer.o(.rodata.*)
  }
  . = ALIGN(4);
  .ram_push.publications : {
    __ram_push_publications_start = .;
    KEEP(*(.ram_push.publications))
    __ram_push_publications_end = .;
  }
  . = ALIGN(32);
  .ram_push.rendezvous : {
    __ram_push_rendezvous_start = .;
    KEEP(*(.ram_push.rendezvous))
    __ram_push_rendezvous_end = .;
  }
  . = ALIGN(32);
  .ram_push.mailbox : {
    __ram_push_mailbox_start = .;
    KEEP(*(.ram_push.mailbox))
    __ram_push_mailbox_end = .;
    BYTE(0)
    . = ALIGN(32);
  }
  __ram_push_end = .;
  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) }
}

ASSERT(__ram_push_publications_end - __ram_push_publications_start == RAM_PUSH_PUBLICATION_COUNT * 16, "publication table ABI changed")
ASSERT(__ram_push_mailbox_end - __ram_push_mailbox_start == 104, "mailbox ABI changed")
ASSERT(__ram_push_rendezvous_end - __ram_push_rendezvous_start == 128, "rendezvous ABI changed")
ASSERT(__ram_push_end <= RAM_PUSH_BASE + 0xd20, "USB toggle installer exceeds allocation")
