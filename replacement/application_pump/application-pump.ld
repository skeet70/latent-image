ENTRY(xm1_application_pump)
SECTIONS
{
  . = 0xbfbd20;
  __application_pump_start = .;
  .text : { KEEP(*(.text.application_pump)) *(.text*) }
  .rodata : { *(.rodata*) }
  .data : { *(.data*) *(.got .got.* .igot .igot.*) }
  .bss : { *(.bss*) *(COMMON) }
  .rel : { *(.rel .rel.*) }
  .rela : { *(.rela .rela.*) }
  .dynamic : { *(.dynamic) }
  . = ALIGN(4);
  __application_pump_end = .;
  /DISCARD/ : {
    *(.ARM.exidx*) *(.ARM.extab*) *(.eh_frame*)
    INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.ARM.attributes .note* .comment* .llvm_addrsig)
  }
  .unexpected : { INPUT_SECTION_FLAGS (SHF_ALLOC) *(*) }
  .symtab 0 : { INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.symtab) }
  .symtab_shndx 0 : { INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.symtab_shndx) }
  .strtab 0 : { INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.strtab) }
  .shstrtab 0 : { INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.shstrtab) }
}
ASSERT(xm1_application_pump == 0xbfbd20, "application pump entry moved")
ASSERT(SIZEOF(.rodata) == 0 && SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "application pump has data")
ASSERT(SIZEOF(.rel) == 0 && SIZEOF(.rela) == 0 && SIZEOF(.dynamic) == 0, "application pump has residual relocation or dynamic content")
ASSERT(__application_pump_end <= 0xbfbd40, "application pump exceeds its complete 32-byte carrier")

ASSERT(SIZEOF(.unexpected) == 0, "application pump has an unexpected allocated section")