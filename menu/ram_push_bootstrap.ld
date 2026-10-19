ram_push_malloc = 0x0089cda4;
ram_push_stock_free = 0x0089cdc4;
ram_push_core1_range = 0x006b0c3c;
SECTIONS {
  . = 0x01469b20;
  .text : { *(.text.ram_push_bootstrap) *(.text.ram_push_free) *(.text.ram_push_peer_prepare) }
  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) }
}
ASSERT(SIZEOF(.text) == 32, "RAM-push bootstrap must be exactly 32 bytes")
