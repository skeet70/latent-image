ENTRY(dual_native_guard)
xm1_native_reset          = 0x00001800;
xm1_primary_discriminator = 0x00001aa0;
xm1_native_close          = 0x00002044;

SECTIONS {
  .text 0x6000 : { *(.text) }
  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) }
  .rel.plt : { *(.rel.plt) }
  .unexpected : { INPUT_SECTION_FLAGS (SHF_ALLOC) *(*) }
}

ASSERT(dual_native_guard == 0x6000, "dual guard must be at RAM 0x6000")
ASSERT(dual_native_rollback == 0x60c0, "dual rollback must be at RAM 0x60c0")
ASSERT(SIZEOF(.text) <= 0x180, "dual guard and rollback exceed reserved staging space")
ASSERT(SIZEOF(.unexpected) == 0, "dual rollback must contain only text")
ASSERT(SIZEOF(.rel.plt) == 0, "dual rollback must not contain runtime relocations")
