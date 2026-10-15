ENTRY(ram_rescue_bootstrap)
SECTIONS {
  .text 0 : { *(.text) }
  .parameters 0x800 : { *(.parameters) }
  .padding 0xa00 : { *(.padding) }
  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) }
  .rel.plt : { *(.rel.plt) }
  .unexpected : { INPUT_SECTION_FLAGS (SHF_ALLOC) *(*) }
}
ASSERT(ram_rescue_bootstrap == 0, "bootstrap entry must be RAM zero")
ASSERT(SIZEOF(.text) <= 0x800, "bootstrap text and tables must fit below parameters")
ASSERT(ram_rescue_parameters == 0x800, "parameter sector must begin at 0x800")
ASSERT(SIZEOF(.parameters) == 512, "parameters must occupy one sector")
ASSERT(ADDR(.padding) + SIZEOF(.padding) == 4096, "bootstrap must occupy 4096 bytes")
ASSERT(SIZEOF(.unexpected) == 0, "bootstrap must contain only text, parameters and padding")
ASSERT(SIZEOF(.rel.plt) == 0, "bootstrap must not contain runtime relocations")
