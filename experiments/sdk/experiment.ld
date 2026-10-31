/* An experiment's 0x8000-byte allocation: its code and data in the first
   0x5000 bytes, then the shared RAM-push installer. */
ENTRY(ram_push_install)
SECTIONS {
  . = RAM_PUSH_BASE;
  __ram_push_start = .;
  .text : {
    *(.text.experiment_trampolines)
    *(EXCLUDE_FILE(*ram_push_installer.o) .text .text.*)
  }
  .rodata : { *(EXCLUDE_FILE(*ram_push_installer.o) .rodata .rodata.*) }
  .data : { *(EXCLUDE_FILE(*ram_push_installer.o) .data .data.*) }
  .bss : { *(EXCLUDE_FILE(*ram_push_installer.o) .bss .bss.* COMMON) }
  __experiment_code_end = .;
  . = RAM_PUSH_BASE + 0x5000;
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
  __experiment_installer_end = .;
  __ram_push_end = .;
  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) *(.ARM.extab*) }
}
ASSERT(__experiment_code_end <= RAM_PUSH_BASE + 0x5000, "experiment code and data exceed the 0x5000-byte window")
ASSERT(__experiment_installer_end <= RAM_PUSH_BASE + 0x8000, "installer runs past the allocation")
