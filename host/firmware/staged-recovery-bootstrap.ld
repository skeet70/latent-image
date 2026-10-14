ENTRY(staged_recovery_bootstrap)

SECTIONS {
  .text 0x00000000 : { *(.text) }
  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) }
  .rel.plt : { *(.rel.plt) }
  .unexpected : { INPUT_SECTION_FLAGS (SHF_ALLOC) *(*) }
}

ASSERT(staged_recovery_bootstrap == 0, "bootstrap entry must be RAM zero")
ASSERT(SIZEOF(.text) == 0x200, "bootstrap must occupy one sector")
ASSERT(SIZEOF(.unexpected) == 0, "bootstrap must contain only text and padding")
ASSERT(SIZEOF(.rel.plt) == 0, "bootstrap must not contain runtime relocations")
