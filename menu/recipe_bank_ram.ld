ENTRY(rb_probe_entry)
rb_stock_detach_continue = 0x0071faa0;
rb_stock_attach_continue = 0x0071f7e0;
rb_stock_page_entry = 0x00bfa800;
rb_stock_probe_epilogue = 0x00d5868c;
rb_stock_present_continue = 0x00d3d898;
rb_stock_draw_root_continue = 0x01158160;

SECTIONS {
  __ram_push_start = RAM_PUSH_BASE;
  . = RAM_PUSH_BASE + 0x500;
  __rb_image_start = .;
  .text : {
    *(EXCLUDE_FILE (*ram_push_installer.o) .text)
    *(EXCLUDE_FILE (*ram_push_installer.o) .text.*)
    *(EXCLUDE_FILE (*ram_push_installer.o) .rodata)
    *(EXCLUDE_FILE (*ram_push_installer.o) .rodata.*)
  }
  __rb_text_end = .;
  .data : {
    *(EXCLUDE_FILE (*ram_push_installer.o) .data)
    *(EXCLUDE_FILE (*ram_push_installer.o) .data.*)
    __rb_bss_start = .;
    *(EXCLUDE_FILE (*ram_push_installer.o) .bss)
    *(EXCLUDE_FILE (*ram_push_installer.o) .bss.*)
    *(EXCLUDE_FILE (*ram_push_installer.o) COMMON)
    __rb_bss_end = .;
    BYTE(0)
    . = ALIGN(4);
  }
  __rb_image_end = .;

  . = RAM_PUSH_BANK_END;
  . = ALIGN(32);
  __ram_push_installer_start = .;
  .ram_push.text : {
    *ram_push_installer.o(.text)
    *ram_push_installer.o(.text.*)
    *ram_push_installer.o(.rodata)
    *ram_push_installer.o(.rodata.*)
    *(.ram_push.text)
    *(.ram_push.text.*)
    *(.ram_push.rodata)
    *(.ram_push.rodata.*)
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

ASSERT(__rb_image_start == RAM_PUSH_BASE + 0x500, "bank executable offset changed")
ASSERT(__rb_image_end <= RAM_PUSH_BASE + 0xa800, "bank executable overlaps generated data")
ASSERT(__ram_push_installer_start == ALIGN(RAM_PUSH_BANK_END, 32), "installer does not follow the bank image")
ASSERT(__ram_push_publications_end - __ram_push_publications_start == 253 * 16, "publication table ABI changed")
ASSERT(__ram_push_start == RAM_PUSH_BASE, "RAM-push runtime start changed")
ASSERT(__ram_push_mailbox_end - __ram_push_mailbox_start == 104, "mailbox ABI changed")
ASSERT(__ram_push_rendezvous_end - __ram_push_rendezvous_start == 128, "rendezvous ABI changed")
