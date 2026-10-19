/* The Classic Chrome RAM-push runtime (reports/classic-chrome-patch.md
 * section 8.2): menu/classic_chrome_ram.S at the allocation base, then the
 * generic installer, its publication table, rendezvous and mailbox. Links
 * no recipe-bank object. RAM_PUSH_BASE and RAM_PUSH_PUBLICATION_COUNT are
 * --defsym inputs. */
ENTRY(ram_push_install)

SECTIONS {
  __ram_push_start = RAM_PUSH_BASE;
  . = RAM_PUSH_BASE;
  __cc_image_start = .;
  .text : {
    *(EXCLUDE_FILE (*ram_push_installer.o) .text)
    *(EXCLUDE_FILE (*ram_push_installer.o) .text.*)
    *(EXCLUDE_FILE (*ram_push_installer.o) .rodata)
    *(EXCLUDE_FILE (*ram_push_installer.o) .rodata.*)
  }
  .data : {
    *(EXCLUDE_FILE (*ram_push_installer.o) .data)
    *(EXCLUDE_FILE (*ram_push_installer.o) .data.*)
    *(EXCLUDE_FILE (*ram_push_installer.o) .bss)
    *(EXCLUDE_FILE (*ram_push_installer.o) .bss.*)
    *(EXCLUDE_FILE (*ram_push_installer.o) COMMON)
    . = ALIGN(4);
  }
  __cc_image_end = .;

  . = ALIGN(32);
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

ASSERT(__cc_image_start == RAM_PUSH_BASE, "Classic Chrome code does not start at the base")
ASSERT(__ram_push_publications_end - __ram_push_publications_start == RAM_PUSH_PUBLICATION_COUNT * 16, "publication table ABI changed")
ASSERT(__ram_push_start == RAM_PUSH_BASE, "RAM-push runtime start changed")
ASSERT(__ram_push_mailbox_end - __ram_push_mailbox_start == 104, "mailbox ABI changed")
ASSERT(__ram_push_rendezvous_end - __ram_push_rendezvous_start == 128, "rendezvous ABI changed")
