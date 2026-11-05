ENTRY(xm1_image5_init)
SECTIONS
{
  . = 0xd9856c;
  __image5_init_start = .;
  .text : { FILL(0x00f020e3); . = . + 0x38; KEEP(*(.text.image5_init)) *(.text*) }
  .rodata : { *(.rodata*) }
  .data : { *(.data*) *(.got .got.* .igot .igot.*) }
  .bss : { *(.bss*) *(COMMON) }
  .rel : { *(.rel .rel.*) }
  .rela : { *(.rela .rela.*) }
  .dynamic : { *(.dynamic) }
  . = ALIGN(4);
  __image5_init_end = .;
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
ASSERT(xm1_image5_init == 0xd985a4, "image5 initializer entry moved")
ASSERT(SIZEOF(.rodata) == 0 && SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "image5 initializer has data")
ASSERT(SIZEOF(.rel) == 0 && SIZEOF(.rela) == 0 && SIZEOF(.dynamic) == 0, "image5 initializer has residual relocation or dynamic content")
ASSERT(__image5_init_end <= 0xd986b0, "image5 initializer exceeds its complete 324-byte carrier")

ASSERT(SIZEOF(.unexpected) == 0, "image5 initializer has an unexpected allocated section")