ENTRY(sfm_entry_begin)
INCLUDE imports.ld
SECTIONS {
  .compiler_builtins 0 : { *compiler_builtins*(.text* .rodata* .data* .bss*) }
  .gap_d5090 0x11d5090 : { *(.text.*machine11write_entry*) }
  .entry_on_deleted 0x11d50d0 : { KEEP(*(.text.sfm_entry_on_deleted)) }
  .gap_d50d4 0x11d50d4 : { *(.text.*stock14cancel_closing) KEEP(*(.text.xm1_sfm_copy_name)) }
  .entry_on_closed 0x11d5100 : { KEEP(*(.text.sfm_entry_on_closed)) }
  .gap_d5104 0x11d5104 : { *(.text.*stock12notify_owner) }
  .entry_on_written 0x11d5130 : { KEEP(*(.text.sfm_entry_on_written)) }
  .gap_d5134 0x11d5134 : { *(.text.*stock14delete_written) }
  .entry_on_positioned 0x11d5164 : { KEEP(*(.text.sfm_entry_on_positioned)) }
  .gap_d5168 0x11d5168 : { KEEP(*(.text.xm1_sfm_on_closed)) }
  .entry_on_opened 0x11d5198 : { KEEP(*(.text.sfm_entry_on_opened)) }
  .gap_d519c 0x11d519c : { *(.text.*stock4open) *(.text.*stock6closed) *(.text.*stock7written) *(.text.*Guest7request) }
  .entry_begin 0x11d5600 : { KEEP(*(.text.sfm_entry_begin)) }
  .gap_d5604 0x11d5604 : { *(.text.*machine4fail*) KEEP(*(.text.xm1_sfm_on_deleted)) }
  .entry_begin_append 0x11d5688 : { KEEP(*(.text.sfm_entry_begin_append)) }
  .gap_d568c 0x11d568c : { *(.text.*machine5begin*) *(.text.*stock19closed_after_cancel) }
  .entry_request_cancel 0x11d5714 : { KEEP(*(.text.sfm_entry_request_cancel)) }
  .gap_d5718 0x11d5718 : { *(.text.*stock11cancel_idle) *(.text.*stock11cancel_open) }
  .entry_append_buffer 0x11d57ac : { KEEP(*(.text.sfm_entry_append_buffer)) }
  .gap_d57b0 0x11d57b0 : { *(.text.*Guest8register) *(.text.*stock12deleted_idle) }
  .entry_set_output_mode 0x11d5850 : { KEEP(*(.text.sfm_entry_set_output_mode)) }
  .gap_d5854 0x11d5854 : { *(.text.*stock20deleted_after_cancel) KEEP(*(.text.xm1_sfm_begin)) KEEP(*(.text.xm1_sfm_begin_append)) }
  .entry_copy_name 0x11d591c : { KEEP(*(.text.sfm_entry_copy_name)) }
  .entry_set_name 0x11d5934 : { KEEP(*(.text.sfm_entry_set_name)) }
  .tables 0x11d5d44 : { KEEP(*(.rodata.sfm_tables)) }
  .gap_d5938 0x11d5938 : { KEEP(*(.text.xm1_sfm_*)) *(.text.*) *(.rodata.*) }
  .data : { *(.data*) *(.sdata*) *(.got*) }
  .bss : { *(.bss*) *(.sbss*) *(COMMON) }
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.eh_frame*) *(.ARM.attributes) *(.note*) *(.comment*) *(.debug*) *(.llvm_addrsig) }
}
ASSERT(ADDR(.gap_d5090) + SIZEOF(.gap_d5090) <= 0x11d50d0, "gap 0x11d5090 overflows")
ASSERT(ADDR(.gap_d50d4) + SIZEOF(.gap_d50d4) <= 0x11d5100, "gap 0x11d50d4 overflows")
ASSERT(ADDR(.gap_d5104) + SIZEOF(.gap_d5104) <= 0x11d5130, "gap 0x11d5104 overflows")
ASSERT(ADDR(.gap_d5134) + SIZEOF(.gap_d5134) <= 0x11d5164, "gap 0x11d5134 overflows")
ASSERT(ADDR(.gap_d5168) + SIZEOF(.gap_d5168) <= 0x11d5198, "gap 0x11d5168 overflows")
ASSERT(ADDR(.gap_d519c) + SIZEOF(.gap_d519c) <= 0x11d5600, "gap 0x11d519c overflows")
ASSERT(ADDR(.gap_d5604) + SIZEOF(.gap_d5604) <= 0x11d5688, "gap 0x11d5604 overflows")
ASSERT(ADDR(.gap_d568c) + SIZEOF(.gap_d568c) <= 0x11d5714, "gap 0x11d568c overflows")
ASSERT(ADDR(.gap_d5718) + SIZEOF(.gap_d5718) <= 0x11d57ac, "gap 0x11d5718 overflows")
ASSERT(ADDR(.gap_d57b0) + SIZEOF(.gap_d57b0) <= 0x11d5850, "gap 0x11d57b0 overflows")
ASSERT(ADDR(.gap_d5854) + SIZEOF(.gap_d5854) <= 0x11d591c, "gap 0x11d5854 overflows")
ASSERT(ADDR(.gap_d5938) + SIZEOF(.gap_d5938) <= 0x11d5d44, "gap 0x11d5938 overflows")
ASSERT(SIZEOF(.tables) == 0x7c, "the dispatcher tables changed size")
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "the save file machine has writable statics")
ASSERT(SIZEOF(.compiler_builtins) == 0, "the save file machine links compiler-builtins")
