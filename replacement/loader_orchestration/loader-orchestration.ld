ENTRY(xm1_loader_orchestration_entry)
SECTIONS {
  . = xm1_loader_base + 0x9ac;
  __loader_orchestration_start = .;
  .text : {
    KEEP(*(.text.entry))
    *(.text.xm1_loader_orchestration)
    *(.text*)
    *(.rodata*)
    . = ALIGN(4);
  }
  __loader_orchestration_end = .;
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.comment*) *(.note*) }
  ASSERT(__loader_orchestration_end <= xm1_loader_base + 0xb00, "loader orchestration exceeds 340-byte carrier")
}
xm1_loader_identity = xm1_loader_base + 0x27b8;
xm1_loader_clear = xm1_loader_base + 0x34f0;
xm1_loader_private_initialize = xm1_loader_base + 0x430;
xm1_loader_log = xm1_loader_base + 0x1354;
xm1_loader_prepare1 = xm1_loader_base + 0x3834;
xm1_loader_prepare3 = xm1_loader_base + 0x24f8;
xm1_loader_prepare4 = xm1_loader_base + 0xd98;
xm1_loader_primary_continue = xm1_loader_base + 0x386c;
xm1_loader_peer_continue = xm1_loader_base + 0x38a8;
xm1_loader_continue5 = xm1_loader_base + 0x3934;
xm1_loader_continue7 = xm1_loader_base + 0xb5c;
xm1_loader_publish_callback = xm1_loader_base + 0xc6c;
xm1_loader_schedule = xm1_loader_base + 0x3688;
