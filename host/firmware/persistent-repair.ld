ENTRY(persistent_repair_guard)
xm1_native_reset          = 0x00001800;
xm1_primary_discriminator = 0x00001aa0;
xm1_native_close          = 0x00002044;
xm1_completion_helper     = 0x00723a14;

SECTIONS {
  .text 0x6000 : { *(.text) }
  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) }
  .rel.plt : { *(.rel.plt) }
  .unexpected : { INPUT_SECTION_FLAGS (SHF_ALLOC) *(*) }
}

ASSERT(persistent_repair_guard == 0x6000, "persistent guard must be at RAM 0x6000")
ASSERT(persistent_repair_callback == 0x60c0, "persistent callback must be at RAM 0x60c0")
ASSERT(persistent_repair_stage == 0x6200, "persistent stage must be at RAM 0x6200")
ASSERT(SIZEOF(.text) <= 0x1000, "persistent repair exceeds native-record gap")
ASSERT(SIZEOF(.unexpected) == 0, "persistent repair must contain only text")
ASSERT(SIZEOF(.rel.plt) == 0, "persistent repair must not contain runtime relocations")
