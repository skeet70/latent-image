ENTRY(xm1_heap_malloc_entry)
__heap_retained_lock = 0x72b164;
__heap_retained_unlock = 0x72b1cc;
/* Retained stock memmove; it returns the destination and supports overlap. */
memcpy = 0x89d908;
memmove = 0x89d908;
__aeabi_memcpy = 0x89d908;
__aeabi_memcpy4 = 0x89d908;
__aeabi_memcpy8 = 0x89d908;
__aeabi_memmove = 0x89d908;
__aeabi_memmove4 = 0x89d908;
__aeabi_memmove8 = 0x89d908;
MEMORY {
  MALLOC_ENTRY (rx) : ORIGIN = 0x89cda4, LENGTH = 0x20
  FREE_ENTRY (rx) : ORIGIN = 0x89cdc4, LENGTH = 0x20
  MALLOC_CARRIER (rx) : ORIGIN = 0x89cde4, LENGTH = 0x89d4f8 - 0x89cde4
  SENTINEL (rw) : ORIGIN = 0x89d4f8, LENGTH = 4
  BIN_CARRIER (rx) : ORIGIN = 0x89d4fc, LENGTH = 0x89d908 - 0x89d4fc
  REALLOC_ENTRY (rx) : ORIGIN = 0x89da3c, LENGTH = 0x28
  REALLOC_CARRIER (rx) : ORIGIN = 0x89da64, LENGTH = 0x89dfc0 - 0x89da64
  TRIM_CARRIER (rx) : ORIGIN = 0x89e464, LENGTH = 0x89e560 - 0x89e464
  FREE_CARRIER (rx) : ORIGIN = 0x89e560, LENGTH = 0x89e7c4 - 0x89e560
  CALLOC_ENTRY (rx) : ORIGIN = 0xbf1c1c, LENGTH = 0x28
  CALLOC_CARRIER (rx) : ORIGIN = 0xbf1c44, LENGTH = 0xbf1cd4 - 0xbf1c44
}
/* The first matching description in script order receives an input section; unpinned .text/.rodata spill from the malloc carrier through bin, realloc, trim and free (--enable-non-contiguous-regions). */
SECTIONS {
  .malloc_entry : { KEEP(*(.heap.entry.malloc)) } > MALLOC_ENTRY
  .free_entry : { KEEP(*(.heap.entry.free)) } > FREE_ENTRY
  .realloc_entry : { KEEP(*(.heap.entry.realloc)) } > REALLOC_ENTRY
  .calloc_entry : { KEEP(*(.heap.entry.calloc)) } > CALLOC_ENTRY
  .realloc_body : {
    KEEP(*(.heap.entry.realloc_r)) *(.heap.realloc)
    *(.text.*4rlsf*10reallocate*) *(.text.*4rlsf*E8allocate*) *(.text.*4Heap12resize_stock*)
  } > REALLOC_CARRIER
  .free_body : {
    KEEP(*(.heap.entry.free_r)) *(.heap.free)
    *(.text.*4Heap3new*) *(.text.*4rlsf*21insert_free_block_ptr*) *(.text.*4rlsf*29insert_free_block_ptr_aligned*)
  } > FREE_CARRIER
  .trim_body : { KEEP(*(.heap.entry.trim_r)) *(.heap.trim) } > TRIM_CARRIER
  .malloc_body : { KEEP(*(.heap.entry.malloc_r)) *(.heap.malloc) *(.text*) *(.rodata*) . = ALIGN(4); } > MALLOC_CARRIER
  .sentinel : { KEEP(*(.heap.sentinel)) } > SENTINEL
  .bin_metadata : { KEEP(*(.heap.metadata)) } > BIN_CARRIER
  .bin_body : { KEEP(*(.heap.support)) *(.heap.calloc) *(.text*) *(.rodata*) . = ALIGN(4); } > BIN_CARRIER
  .realloc_spill : { *(.text*) *(.rodata*) . = ALIGN(4); } > REALLOC_CARRIER
  .trim_spill : { *(.text*) *(.rodata*) . = ALIGN(4); } > TRIM_CARRIER
  .free_spill : { *(.text*) *(.rodata*) . = ALIGN(4); } > FREE_CARRIER
  .calloc_body : { KEEP(*(.heap.entry.calloc_r)) } > CALLOC_CARRIER
  __heap_link_end = .;
  .data : { *(.data*) *(.sdata*) *(.got*) } > CALLOC_CARRIER
  .bss : { *(.bss*) *(.sbss*) *(COMMON) } > CALLOC_CARRIER
  .symtab 0 : { *(.symtab) }
  .strtab 0 : { *(.strtab) }
  .shstrtab 0 : { *(.shstrtab) }
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.eh_frame*) *(.ARM.attributes) *(.note*) *(.comment*) *(.debug*) *(.llvm_addrsig) }
}
ASSERT(ADDR(.malloc_body) == 0x89cde4 && ADDR(.realloc_body) == 0x89da64 && ADDR(.trim_body) == 0x89e464 && ADDR(.free_body) == 0x89e560 && ADDR(.calloc_body) == 0xbf1c44, "core entries left their stock addresses")
ASSERT(SIZEOF(.sentinel) == 4, "heap sentinel must be one word")
ASSERT(SIZEOF(.calloc_body) == 4, "image5 carries only the calloc core thunk")
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "heap has writable statics outside charged arena")
