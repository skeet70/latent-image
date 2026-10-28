/* Foreign calls retain stock addresses; host helpers occupy a separate span. */
ENTRY(rbm_open)
SECTIONS {
  . = 0x0061e110;
  .text.getter : { *(.text.rbm_get) }
  . = 0x00624694;
  .text.setter : { *(.text.rbm_set) }
  . = 0x006ffff4;
  .text.lowclose : { *(.text.rbm_low_close) }
  . = 0x0071be74;
  .text.read : { *(.text.rbm_read) }
  . = 0x0071bfe0;
  .text.write : { *(.text.rbm_write) }
  . = 0x0071c134;
  .text.close : { *(.text.rbm_close) }
  . = 0x0071c2b0;
  .text.open : { *(.text.rbm_open) }
  . = 0x0071ce84;
  .text.stat : { *(.text.rbm_stat) }
  . = 0x0071e9ec;
  .text.indexfree : { *(.text.rbm_index_free) }
  . = 0x0071eaac;
  .text.mapping : { *(.text.rbm_mapping_delete) }
  . = 0x0071eb28;
  .text.resolve : { *(.text.rbm_resolve) }
  . = 0x0071eff4;
  .text.tagrelease : { *(.text.rbm_tag_release) }
  . = 0x00730000;
  .text.helpers : { *(.text*) }
  . = 0x00910340;
  .data : { *(.data*) *(.bss*) *(COMMON) *(.rodata*) }
  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) }
}
ASSERT(rbm_get == 0x0061e110, "getter at stock address")
ASSERT(rbm_set == 0x00624694, "setter at stock address")
ASSERT(rbm_read == 0x0071be74, "read base at stock address")
ASSERT(rbm_write == 0x0071bfe0, "write base at stock address")
ASSERT(rbm_close == 0x0071c134, "close base at stock address")
ASSERT(rbm_open == 0x0071c2b0, "open base at stock address")
ASSERT(rbm_stat == 0x0071ce84, "stat base at stock address")
ASSERT(rbm_low_close == 0x006ffff4, "low close at stock address")
ASSERT(rbm_index_free == 0x0071e9ec, "index free at stock address")
ASSERT(rbm_mapping_delete == 0x0071eaac, "mapping delete at stock address")
ASSERT(rbm_resolve == 0x0071eb28, "resolve at stock address")
ASSERT(rbm_tag_release == 0x0071eff4, "tag release at stock address")
ASSERT(ADDR(.data) + SIZEOF(.data) <= 0x00911340, "model data fits the media-state tail")
