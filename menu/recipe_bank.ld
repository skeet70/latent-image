/* Linker script for the bank hook blob. The runtime worker passes the plan's
 * placement with --defsym=RB_BASE=<address>, so the blob's absolute references
 * and its container placement cannot disagree. BLOB_BYTES is the region the
 * plan declares for the blob. The storage kernel's zero data follows the code
 * in the same carried span, so the emitted binary covers it. */
ENTRY(rb_probe_entry)
rb_stock_detach_continue = 0x0071faa0;
rb_stock_attach_continue = 0x0071f7e0;
rb_stock_page_entry = 0x00bfa800;
rb_stock_probe_epilogue = 0x00d5868c;
SECTIONS {
  . = RB_BASE;
  .text : { *(.text) *(.text.*) *(.rodata) *(.rodata.*) }
  .data : { *(.data) *(.data.*) *(.bss) *(.bss.*) *(COMMON) }
  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) }
}
ASSERT(rb_probe_entry >= RB_BASE, "hook blob starts at RB_BASE")
