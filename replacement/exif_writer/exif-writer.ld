ENTRY(exif_entry_write_exif_header)
INCLUDE imports.ld
SECTIONS {
  .compiler_builtins 0 : { *compiler_builtins*(.text* .rodata* .data* .bss*) }
  .entry_initialize 0x11c14f0 : { KEEP(*(.text.exif_entry_initialize)) }
  .gap_c14f4 0x11c14f4 : { *(.text.*writer10initialize*) *(.text.*writer21transfer_saved_parser*) *(.text.*writer12patch_offset*) *(.text.*writer13swap_halfword*) }
  .entry_write_primary 0x11c170c : { KEEP(*(.text.exif_entry_write_primary)) }
  .gap_c1710 0x11c1710 : { *(.text.*writer9write_gps*) KEEP(*(.text.xm1_exif_write_gps)) KEEP(*(.text.xm1_exif_write_filtered_directory)) KEEP(*(.text.xm1_exif_select_table)) }
  .entry_write_exif 0x11c17cc : { KEEP(*(.text.exif_entry_write_exif)) }
  .gap_c17d0 0x11c17d0 : { *(.text.*writer10write_exif*) *(.text.*writer13write_primary*) *(.text.*writer21saved_directory_slots*) *(.text.*writer22saved_directory_values*) *(.text.*RangeInclusive*) }
  .entry_write_makernote 0x11c19c8 : { KEEP(*(.text.exif_entry_write_makernote)) }
  .gap_c19cc 0x11c19cc : { KEEP(*(.text.xm1_exif_cb_copyright)) KEEP(*(.text.xm1_exif_finish)) KEEP(*(.text.xm1_exif_cb_source_word)) KEEP(*(.text.xm1_exif_cb_packed_halves)) }
  .entry_write_gps 0x11c1b30 : { KEEP(*(.text.exif_entry_write_gps)) }
  .gap_c1b34 0x11c1b34 : { *(.text.*writer22write_interoperability*) *(.text.*callback12source_bytes*) KEEP(*(.text.xm1_exif_cb_staged_block)) KEEP(*(.text.xm1_exif_cb_image_records)) }
  .gap_c1e00 0x11c1e00 : { KEEP(*(.text.xm1_exif_initialize)) KEEP(*(.text.xm1_exif_write_exif)) KEEP(*(.text.xm1_exif_write_primary)) }
  .entry_patch_thumbnail_length 0x11c1e4c : { KEEP(*(.text.exif_entry_patch_thumbnail_length)) }
  .gap_c1e50 0x11c1e50 : { KEEP(*(.text.xm1_exif_write_makernote)) KEEP(*(.text.xm1_exif_patch_thumbnail_length)) }
  .entry_transfer_saved_parser 0x11c2154 : { KEEP(*(.text.exif_entry_transfer_saved_parser)) }
  .gap_c2158 0x11c2158 : { *(.text.*writer18write_short_record*) KEEP(*(.text.xm1_exif_cb_source_bytes_122)) KEEP(*(.text.xm1_exif_cb_source_bytes_256)) }
  .entry_swap_halfword 0x11c2b50 : { KEEP(*(.text.exif_entry_swap_halfword)) }
  .gap_c2b54 0x11c2b54 : { KEEP(*(.text.xm1_exif_swap_halfword)) }
  .entry_copy_value 0x11c2c40 : { KEEP(*(.text.exif_entry_copy_value)) }
  .gap_c2c44 0x11c2c44 : { KEEP(*(.text.xm1_exif_copy_value)) KEEP(*(.text.xm1_exif_type_size)) KEEP(*(.text.xm1_exif_table_count)) }
  .entry_type_size 0x11c2c98 : { KEEP(*(.text.exif_entry_type_size)) }
  .gap_c2c9c 0x11c2c9c : { KEEP(*(.text.xm1_exif_cb_one)) }
  .entry_saved_directory_slots 0x11c35f8 : { KEEP(*(.text.exif_entry_saved_directory_slots)) }
  .gap_c35fc 0x11c35fc : { KEEP(*(.text.xm1_exif_saved_directory_slots)) KEEP(*(.text.xm1_exif_saved_directory_values)) KEEP(*(.text.xm1_exif_transfer_saved_parser)) }
  .entry_saved_directory_values 0x11c3678 : { KEEP(*(.text.exif_entry_saved_directory_values)) }
  .gap_c367c 0x11c367c : { *(.text.*writer22patch_thumbnail_length*) KEEP(*(.text.xm1_exif_cb_exif_pointer)) KEEP(*(.text.xm1_exif_cb_interoperability_pointer)) }
  .gap_cda48 0x11cda48 : { *(.text.*writer24write_filtered_directory*) *(.text.*writer18maker_capture_tags*) *(.text.*writer8capacity*) *(.rodata*) }
  .entry_cb_thumbnail_offset 0x11cecb8 : { KEEP(*(.text.exif_entry_cb_thumbnail_offset)) }
  .gap_cecbc 0x11cecbc : { KEEP(*(.text.xm1_exif_cb_thumbnail_offset)) }
  .entry_cb_thumbnail_length 0x11cece8 : { KEEP(*(.text.exif_entry_cb_thumbnail_length)) }
  .gap_cecec 0x11cecec : { KEEP(*(.text.xm1_exif_cb_thumbnail_length)) }
  .entry_cb_exif_pointer 0x11ced18 : { KEEP(*(.text.exif_entry_cb_exif_pointer)) }
  .entry_cb_gps_pointer 0x11ced48 : { KEEP(*(.text.exif_entry_cb_gps_pointer)) }
  .gap_ced4c 0x11ced4c : { KEEP(*(.text.xm1_exif_cb_gps_pointer)) }
  .entry_cb_one 0x11ced80 : { KEEP(*(.text.exif_entry_cb_one)) }
  .entry_cb_makernote 0x11ceda0 : { KEEP(*(.text.exif_entry_cb_makernote)) }
  .gap_ceda4 0x11ceda4 : { KEEP(*(.text.xm1_exif_cb_makernote)) }
  .entry_cb_interoperability_pointer 0x11cede0 : { KEEP(*(.text.exif_entry_cb_interoperability_pointer)) }
  .entry_cb_packed_halves 0x11cee10 : { KEEP(*(.text.exif_entry_cb_packed_halves)) }
  .entry_cb_source_word 0x11cee4c : { KEEP(*(.text.exif_entry_cb_source_word)) }
  .gap_cee50 0x11cee50 : { *(.text.*writer15write_makernote*) }
  .entry_cb_staged_block 0x11cef60 : { KEEP(*(.text.exif_entry_cb_staged_block)) }
  .entry_cb_source_bytes_256 0x11cefa8 : { KEEP(*(.text.exif_entry_cb_source_bytes_256)) }
  .entry_cb_source_bytes_122 0x11ceff0 : { KEEP(*(.text.exif_entry_cb_source_bytes_122)) }
  .entry_cb_default_text 0x11cf038 : { KEEP(*(.text.exif_entry_cb_default_text)) }
  .gap_cf03c 0x11cf03c : { KEEP(*(.text.xm1_exif_cb_default_text)) *(.text.*writer16copy_typed_value*) *(.text.*writer17write_value_entry*) }
  .entry_cb_copyright 0x11cf3cc : { KEEP(*(.text.exif_entry_cb_copyright)) }
  .gap_cf3d0 0x11cf3d0 : { *(.text.*writer20write_callback_entry*) KEEP(*(.text.xm1_exif_cb_printim)) *(.text.*writer19trailing_maker_tags*) *(.text.*writer19saved_tag_available*) *(.text.*writer15write_directory*) }
  .entry_cb_printim 0x11cf82c : { KEEP(*(.text.exif_entry_cb_printim)) }
  .entry_select_table 0x11cf854 : { KEEP(*(.text.exif_entry_select_table)) }
  .gap_cf858 0x11cf858 : { *(.text.*writer15trace_directory*) }
  .entry_table_count 0x11cf908 : { KEEP(*(.text.exif_entry_table_count)) }
  .gap_cf90c 0x11cf90c : { *(.text.*writer12select_table*) *(.text.*writer11table_count*) }
  .entry_write_filtered_directory 0x11cf990 : { KEEP(*(.text.exif_entry_write_filtered_directory)) }
  .gap_cf994 0x11cf994 : { KEEP(*(.text.xm1_exif_write_exif_header)) }
  .entry_cb_image_records 0x11cfd58 : { KEEP(*(.text.exif_entry_cb_image_records)) }
  .gap_cfd5c 0x11cfd5c : { *(.text.*writer21write_saved_directory*) *(.text.*writer13copy_identity*) *(.text._R*) }
  .entry_write_exif_header 0x11db098 : { KEEP(*(.text.exif_entry_write_exif_header)) }
  .entry_finish 0x11db188 : { KEEP(*(.text.exif_entry_finish)) }
  .data : { *(.data*) *(.sdata*) *(.got*) }
  .bss : { *(.bss*) *(.sbss*) *(COMMON) }
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.eh_frame*) *(.ARM.attributes) *(.note*) *(.comment*) *(.debug*) *(.llvm_addrsig) }
}
ASSERT(ADDR(.gap_c14f4) + SIZEOF(.gap_c14f4) <= 0x11c170c, "gap 0x11c14f4 overflows")
ASSERT(ADDR(.gap_c1710) + SIZEOF(.gap_c1710) <= 0x11c17cc, "gap 0x11c1710 overflows")
ASSERT(ADDR(.gap_c17d0) + SIZEOF(.gap_c17d0) <= 0x11c19c8, "gap 0x11c17d0 overflows")
ASSERT(ADDR(.gap_c19cc) + SIZEOF(.gap_c19cc) <= 0x11c1b30, "gap 0x11c19cc overflows")
ASSERT(ADDR(.gap_c1b34) + SIZEOF(.gap_c1b34) <= 0x11c1d18, "gap 0x11c1b34 overflows")
ASSERT(ADDR(.gap_c1e00) + SIZEOF(.gap_c1e00) <= 0x11c1e4c, "gap 0x11c1e00 overflows")
ASSERT(ADDR(.gap_c1e50) + SIZEOF(.gap_c1e50) <= 0x11c1e7c, "gap 0x11c1e50 overflows")
ASSERT(ADDR(.gap_c2158) + SIZEOF(.gap_c2158) <= 0x11c2208, "gap 0x11c2158 overflows")
ASSERT(ADDR(.gap_c2b54) + SIZEOF(.gap_c2b54) <= 0x11c2b80, "gap 0x11c2b54 overflows")
ASSERT(ADDR(.gap_c2c44) + SIZEOF(.gap_c2c44) <= 0x11c2c98, "gap 0x11c2c44 overflows")
ASSERT(ADDR(.gap_c2c9c) + SIZEOF(.gap_c2c9c) <= 0x11c2cb8, "gap 0x11c2c9c overflows")
ASSERT(ADDR(.gap_c35fc) + SIZEOF(.gap_c35fc) <= 0x11c3678, "gap 0x11c35fc overflows")
ASSERT(ADDR(.gap_c367c) + SIZEOF(.gap_c367c) <= 0x11c36f8, "gap 0x11c367c overflows")
ASSERT(ADDR(.gap_cda48) + SIZEOF(.gap_cda48) <= 0x11cecb8, "gap 0x11cda48 overflows")
ASSERT(ADDR(.gap_cecbc) + SIZEOF(.gap_cecbc) <= 0x11cece8, "gap 0x11cecbc overflows")
ASSERT(ADDR(.gap_cecec) + SIZEOF(.gap_cecec) <= 0x11ced18, "gap 0x11cecec overflows")
ASSERT(ADDR(.gap_ced4c) + SIZEOF(.gap_ced4c) <= 0x11ced80, "gap 0x11ced4c overflows")
ASSERT(ADDR(.gap_ceda4) + SIZEOF(.gap_ceda4) <= 0x11cede0, "gap 0x11ceda4 overflows")
ASSERT(ADDR(.gap_cee50) + SIZEOF(.gap_cee50) <= 0x11cef60, "gap 0x11cee50 overflows")
ASSERT(ADDR(.gap_cf03c) + SIZEOF(.gap_cf03c) <= 0x11cf3cc, "gap 0x11cf03c overflows")
ASSERT(ADDR(.gap_cf3d0) + SIZEOF(.gap_cf3d0) <= 0x11cf82c, "gap 0x11cf3d0 overflows")
ASSERT(ADDR(.gap_cf858) + SIZEOF(.gap_cf858) <= 0x11cf908, "gap 0x11cf858 overflows")
ASSERT(ADDR(.gap_cf90c) + SIZEOF(.gap_cf90c) <= 0x11cf990, "gap 0x11cf90c overflows")
ASSERT(ADDR(.gap_cf994) + SIZEOF(.gap_cf994) <= 0x11cfd58, "gap 0x11cf994 overflows")
ASSERT(ADDR(.gap_cfd5c) + SIZEOF(.gap_cfd5c) <= 0x11d0404, "gap 0x11cfd5c overflows")
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "the exif writer has writable statics")
ASSERT(SIZEOF(.compiler_builtins) == 0, "the exif writer links compiler-builtins")
