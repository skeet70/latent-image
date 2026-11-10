ENTRY(xm1_liveview_publish)
SECTIONS {
  . = 0x02000000;
  .text : {
    KEEP(*(.text.xm1_calibration_*))
    KEEP(*(.text.xm1_liveview_publish))
    KEEP(*(.text.xm1_liveview_service))
    *(.text .text.*)
  }
  .rodata : { *(.rodata .rodata.*) }
  .data : { *(.data*) *(.got .got.* .igot .igot.*) }
  .bss : { *(.bss*) *(COMMON) }
  .rel : { *(.rel .rel.*) }
  .rela : { *(.rela .rela.*) }
  .dynamic : { *(.dynamic) }
  ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "unexpected writable calibration/liveview state")
  ASSERT(SIZEOF(.rel) == 0 && SIZEOF(.rela) == 0 && SIZEOF(.dynamic) == 0, "unexpected relocations")
  /DISCARD/ : {
    *(.ARM.exidx* .ARM.extab* .comment .note* .eh_frame*)
    INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.ARM.attributes .llvm_addrsig)
  }
  .unexpected : { INPUT_SECTION_FLAGS (SHF_ALLOC) *(*) }
  ASSERT(. <= 0x02010000, "calibration/liveview artifact exceeds 64 KiB")
  .symtab 0 : { INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.symtab) }
  .symtab_shndx 0 : { INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.symtab_shndx) }
  .strtab 0 : { INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.strtab) }
  .shstrtab 0 : { INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.shstrtab) }
  ASSERT(SIZEOF(.unexpected) == 0, "unexpected allocated calibration/liveview data")
}
