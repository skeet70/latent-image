ENTRY(xm1_primary_initializer_entry)

MEMORY
{
  primary (rx) : ORIGIN = 0x40430, LENGTH = 0x57c
}

SECTIONS
{
  .image ORIGIN(primary) : {
    __primary_initializer_image_start = .;
    __primary_initializer_text_start = .;
    KEEP(*(.text.primary_initializer))
    *(.text*)
    __primary_initializer_text_end = .;
    . = ALIGN(4);
    __primary_initializer_rodata_start = .;
    *(.rodata*)
    __primary_initializer_rodata_end = .;
    . = ALIGN(4);
    __primary_initializer_payload_end = .;
    FILL(0x00000000)
    . = ORIGIN(primary) + LENGTH(primary);
    __primary_initializer_image_end = .;
  } > primary

  .data : { *(.data*) *(.sdata*) } > primary
  .bss : { *(.bss*) *(.sbss*) *(COMMON) } > primary
  .rel : { *(.rel .rel.*) } > primary
  .rela : { *(.rela .rela.*) } > primary
  .dynamic : { *(.dynamic) } > primary

  /DISCARD/ : {
    /* The toolchain uses panic=abort. */
    *(.ARM.exidx*)
    *(.ARM.extab*)
    *(.eh_frame*)
    INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.ARM.attributes .note* .comment* .llvm_addrsig)
  }

  .unexpected : { INPUT_SECTION_FLAGS (SHF_ALLOC) *(*) } > primary

  .symtab 0 : { INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.symtab) }
  .symtab_shndx 0 : { INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.symtab_shndx) }
  .strtab 0 : { INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.strtab) }
  .shstrtab 0 : { INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.shstrtab) }
}

__primary_initializer_capacity = LENGTH(primary);

ASSERT(xm1_primary_initializer_entry == 0x40430, "primary initializer entry moved")
ASSERT(__primary_initializer_text_start == 0x40430, "primary initializer text moved")
ASSERT(xm1_primary_initializer_publish >= __primary_initializer_text_start && xm1_primary_initializer_publish < __primary_initializer_text_end, "primary initializer Rust publisher is outside text")
ASSERT(__primary_initializer_payload_end <= 0x409ac, "primary initializer exceeds its complete A leaf")
ASSERT(__primary_initializer_image_start == 0x40430 && __primary_initializer_image_end == 0x409ac, "primary initializer image extent differs")
ASSERT(SIZEOF(.image) == 0x57c, "primary initializer image capacity differs")
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "primary initializer has writable statics")
ASSERT(SIZEOF(.rel) == 0 && SIZEOF(.rela) == 0 && SIZEOF(.dynamic) == 0, "primary initializer has residual relocation or dynamic content")
ASSERT(SIZEOF(.unexpected) == 0, "primary initializer has an unexpected input section")
