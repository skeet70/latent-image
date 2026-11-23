ENTRY(dcf_entry_prepare_name)
INCLUDE imports.ld
SECTIONS {
  .compiler_builtins 0 : { *compiler_builtins*(.text* .rodata* .data* .bss*) }
  .entry_set_prefix_mode 0x70f790 : { KEEP(*(.text.dcf_entry_set_prefix_mode)) }
  .gap_70f794 0x70f794 : { KEEP(*(.text.xm1_dcf_set_prefix_mode)) *(.text.*numbering10check_name*) *(.rodata.*stock4DCIM*) }
  .entry_check_name 0x70f840 : { KEEP(*(.text.dcf_entry_check_name)) }
  .gap_70f844 0x70f844 : { *(.text.*numbering12read_profile*) *(.text.*Guest19report_folder_limit*) *(.rodata.cst16) }
  .entry_apply_continuous_numbering 0x70faa0 : { KEEP(*(.text.dcf_entry_apply_continuous_numbering)) }
  .gap_70faa4 0x70faa4 : { KEEP(*(.text.xm1_dcf_apply_continuous_numbering)) KEEP(*(.text.xm1_dcf_last_used)) }
  .entry_read_numbers 0x70fb68 : { KEEP(*(.text.dcf_entry_read_numbers)) }
  .gap_70fb6c 0x70fb6c : { *(.text.*numbering11next_folder*) KEEP(*(.text.xm1_dcf_create_folder)) }
  .entry_prepare_name 0x70fbfc : { KEEP(*(.text.dcf_entry_prepare_name)) }
  .gap_70fc00 0x70fc00 : { KEEP(*(.text.xm1_dcf_prepare_name)) *(.text.*numbering8allocate*) *(.text.*numbering12read_numbers*) *(.text.*numbering11block_start*) *(.text.*numbering13write_profile*) }
  .entry_note_written 0x71141c : { KEEP(*(.text.dcf_entry_note_written)) }
  .gap_711420 0x711420 : { KEEP(*(.text.xm1_dcf_note_written)) KEEP(*(.text.xm1_dcf_check_name)) }
  .entry_read_profile 0x7159f8 : { KEEP(*(.text.dcf_entry_read_profile)) }
  .gap_7159fc 0x7159fc : { KEEP(*(.text.xm1_dcf_read_profile)) }
  .entry_write_profile 0x715a2c : { KEEP(*(.text.dcf_entry_write_profile)) }
  .entry_create_folder 0x716184 : { KEEP(*(.text.dcf_entry_create_folder)) }
  .gap_716188 0x716188 : { *(.text.*numbering9last_used*) KEEP(*(.text.xm1_dcf_write_profile)) }
  .entry_last_used 0x716324 : { KEEP(*(.text.dcf_entry_last_used)) }
  .gap_716328 0x716328 : { *(.text.*numbering13create_folder*) KEEP(*(.text.xm1_dcf_read_numbers)) *(.text.*numbering9block_end*) *(.text*) *(.rodata*) }
  .data : { *(.data*) *(.sdata*) *(.got*) }
  .bss : { *(.bss*) *(.sbss*) *(COMMON) }
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.eh_frame*) *(.ARM.attributes) *(.note*) *(.comment*) *(.debug*) *(.llvm_addrsig) }
}
ASSERT(ADDR(.gap_70f794) + SIZEOF(.gap_70f794) <= 0x70f840, "gap 0x70f794 overflows")
ASSERT(ADDR(.gap_70f844) + SIZEOF(.gap_70f844) <= 0x70f8b8, "gap 0x70f844 overflows")
ASSERT(ADDR(.gap_70faa4) + SIZEOF(.gap_70faa4) <= 0x70fb68, "gap 0x70faa4 overflows")
ASSERT(ADDR(.gap_70fb6c) + SIZEOF(.gap_70fb6c) <= 0x70fbfc, "gap 0x70fb6c overflows")
ASSERT(ADDR(.gap_70fc00) + SIZEOF(.gap_70fc00) <= 0x7101e4, "gap 0x70fc00 overflows")
ASSERT(ADDR(.gap_711420) + SIZEOF(.gap_711420) <= 0x7115a4, "gap 0x711420 overflows")
ASSERT(ADDR(.gap_7159fc) + SIZEOF(.gap_7159fc) <= 0x715a2c, "gap 0x7159fc overflows")
ASSERT(ADDR(.gap_716188) + SIZEOF(.gap_716188) <= 0x716324, "gap 0x716188 overflows")
ASSERT(ADDR(.gap_716328) + SIZEOF(.gap_716328) <= 0x71657c, "gap 0x716328 overflows")
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "dcf numbering has writable statics")
ASSERT(SIZEOF(.compiler_builtins) == 0, "dcf numbering links compiler-builtins")
