ENTRY(raw_native_guard)
xm1_native_reset          = 0x00001800;
xm1_primary_discriminator = 0x00001aa0;
xm1_native_close          = 0x00002044;

SECTIONS {
  .text 0x6000 : { *(.text) }
  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) }
  .rel.plt : { *(.rel.plt) }
  .unexpected : { INPUT_SECTION_FLAGS (SHF_ALLOC) *(*) }
}

ASSERT(raw_native_guard == 0x6000, "raw guard must be at RAM 0x6000")
ASSERT(raw_native_rollback == 0x6040, "raw rollback must be at RAM 0x6040")
ASSERT(SIZEOF(.text) == 0xa8, "raw guard and rollback must occupy 168 bytes")
ASSERT(SIZEOF(.unexpected) == 0, "raw rollback must contain only text")
ASSERT(SIZEOF(.rel.plt) == 0, "raw rollback must not contain runtime relocations")
