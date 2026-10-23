/* Zero BSS is carried in .data for the bootstrap copy. */
ENTRY(rb_probe_entry)
rb_stock_detach_continue = 0x0071faa0;
rb_stock_attach_continue = 0x0071f7e0;
rb_stock_page_entry = 0x00bfa800;
rb_stock_probe_epilogue = 0x00d5868c;
rb_stock_present_continue = 0x00d3d898;
rb_stock_draw_root_continue = 0x01158160;
SECTIONS {
  . = RB_BASE;
  __rb_image_start = .;
  .text : { *(.text) *(.text.*) *(.rodata) *(.rodata.*) }
  __rb_text_end = .;
  .data : {
    *(.data) *(.data.*)
    __rb_bss_start = .;
    *(.bss) *(.bss.*) *(COMMON)
    __rb_bss_end = .;
    BYTE(0)
    . = ALIGN(4);
  }
  __rb_image_end = .;
  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) }
}
ASSERT(rb_probe_entry >= RB_BASE, "hook blob starts at RB_BASE")
ASSERT(!DEFINED(RB_CAPACITY) || __rb_image_end - RB_BASE <= RB_CAPACITY, "executable plus BSS exceeds carve capacity")
