/* The one fixed bank: the file service and the display scanout. The file service's retired
   extents hold 4-byte entry branches at its stock entries and traps elsewhere; the scanout's
   entry branches are staged as publications. All bodies are in OWNED_FIXED. */
ENTRY(xm1_file_service_entry_route)
INCLUDE imports.ld
INCLUDE fixed-imports.ld
MEMORY {
  RETIRED_WORKERS (rx) : ORIGIN = 0x7115a4, LENGTH = 0x712890 - 0x7115a4
  RETIRED_CAPACITY (rx) : ORIGIN = 0x715a58, LENGTH = 0x715f0c - 0x715a58
  RETIRED_CATALOG (rx) : ORIGIN = 0x718a8c, LENGTH = 0x71b9e8 - 0x718a8c
  RETIRED (rx) : ORIGIN = 0x71bc2c, LENGTH = 0x721834 - 0x71bc2c
  INCLUDE fixed-memory.ld
}
SECTIONS {
  .open_worker 0x7115a4 : { KEEP(*(.text.file_service_entry_open_worker)) . = 0x7116e4; } > RETIRED_WORKERS =0xf000f0e7
  .write_worker 0x7116e4 : { KEEP(*(.text.file_service_entry_write_worker)) . = 0x711768; } > RETIRED_WORKERS =0xf000f0e7
  .close_worker 0x711768 : { KEEP(*(.text.file_service_entry_close_worker)) . = 0x711840; } > RETIRED_WORKERS =0xf000f0e7
  .close_entry_worker 0x711840 : { KEEP(*(.text.file_service_entry_close_entry_worker)) . = 0x7118fc; } > RETIRED_WORKERS =0xf000f0e7
  .find_first_worker 0x712700 : { KEEP(*(.text.file_service_entry_find_first_worker)) . = 0x7127e0; } > RETIRED_WORKERS =0xf000f0e7
  .find_next_worker 0x7127e0 : { KEEP(*(.text.file_service_entry_find_next_worker)) . = 0x712890; } > RETIRED_WORKERS =0xf000f0e7
  .volume_attach_worker 0x7120d8 : { KEEP(*(.text.file_service_entry_volume_attach_worker)) . = 0x7122dc; } > RETIRED_WORKERS =0xf000f0e7
  .volume_detach_worker 0x7122dc : { KEEP(*(.text.file_service_entry_volume_detach_worker)) . = 0x7123b4; } > RETIRED_WORKERS =0xf000f0e7
  .volume_format_armed_worker 0x7123b4 : { KEEP(*(.text.file_service_entry_volume_format_armed_worker)) . = 0x712580; } > RETIRED_WORKERS =0xf000f0e7
  .volume_format_worker 0x712580 : { KEEP(*(.text.file_service_entry_volume_format_worker)) . = 0x712700; } > RETIRED_WORKERS =0xf000f0e7
  .result_acquire 0x71bc2c : { KEEP(*(.text.file_service_entry_result_acquire)) . = 0x71bc6c; } > RETIRED =0xf000f0e7
  .result_release 0x71bc6c : { KEEP(*(.text.file_service_entry_result_release)) . = 0x71bc90; } > RETIRED =0xf000f0e7
  .result_wait 0x71bc90 : { KEEP(*(.text.file_service_entry_result_wait)) . = 0x71bcc8; } > RETIRED =0xf000f0e7
  .read 0x71be74 : { KEEP(*(.text.file_service_entry_read)) . = 0x71bfe0; } > RETIRED =0xf000f0e7
  .write 0x71bfe0 : { KEEP(*(.text.file_service_entry_write)) . = 0x71c134; } > RETIRED =0xf000f0e7
  .close 0x71c134 : { KEEP(*(.text.file_service_entry_close)) . = 0x71c2b0; } > RETIRED =0xf000f0e7
  .open 0x71c2b0 : { KEEP(*(.text.file_service_entry_open)) . = 0x71c504; } > RETIRED =0xf000f0e7
  .result_done 0x71c634 : { KEEP(*(.text.file_service_entry_result_done)) . = 0x71c648; } > RETIRED =0xf000f0e7
  .open_async 0x71c648 : { KEEP(*(.text.file_service_entry_open_async)) . = 0x71c670; } > RETIRED =0xf000f0e7
  .open_wait 0x71c670 : { KEEP(*(.text.file_service_entry_open_wait)) . = 0x71c6a4; } > RETIRED =0xf000f0e7
  .open_unflagged 0x71c6a4 : { KEEP(*(.text.file_service_entry_open_unflagged)) . = 0x71c6c8; } > RETIRED =0xf000f0e7
  .open_unflagged_wait 0x71c6c8 : { KEEP(*(.text.file_service_entry_open_unflagged_wait)) . = 0x71c6fc; } > RETIRED =0xf000f0e7
  .read_async 0x71c6fc : { KEEP(*(.text.file_service_entry_read_async)) . = 0x71c720; } > RETIRED =0xf000f0e7
  .read_wait 0x71c720 : { KEEP(*(.text.file_service_entry_read_wait)) . = 0x71c75c; } > RETIRED =0xf000f0e7
  .write_async 0x71c75c : { KEEP(*(.text.file_service_entry_write_async)) . = 0x71c780; } > RETIRED =0xf000f0e7
  .write_wait 0x71c780 : { KEEP(*(.text.file_service_entry_write_wait)) . = 0x71c7bc; } > RETIRED =0xf000f0e7
  .close_async 0x71c7bc : { KEEP(*(.text.file_service_entry_close_async)) . = 0x71c7e0; } > RETIRED =0xf000f0e7
  .close_wait 0x71c7e0 : { KEEP(*(.text.file_service_entry_close_wait)) . = 0x71c81c; } > RETIRED =0xf000f0e7
  .translate 0x71d450 : { KEEP(*(.text.file_service_entry_translate)) . = 0x71d66c; } > RETIRED =0xf000f0e7
  .media_ready 0x71ddf0 : { KEEP(*(.text.file_service_entry_media_ready)) . = 0x71ded4; } > RETIRED =0xf000f0e7
  .find_first_entry_worker 0x71e3a4 : { KEEP(*(.text.file_service_entry_find_first_entry_worker)) . = 0x71e550; } > RETIRED =0xf000f0e7
  .find_next_entry_worker 0x71e550 : { KEEP(*(.text.file_service_entry_find_next_entry_worker)) . = 0x71e6f0; } > RETIRED =0xf000f0e7
  .bind 0x71ea34 : { KEEP(*(.text.file_service_entry_map_bind)) . = 0x71eaac; } > RETIRED =0xf000f0e7
  .unbind 0x71eaac : { KEEP(*(.text.file_service_entry_map_unbind)) . = 0x71eb28; } > RETIRED =0xf000f0e7
  .lookup 0x71eb28 : { KEEP(*(.text.file_service_entry_map_lookup)) . = 0x71eba0; } > RETIRED =0xf000f0e7
  .seek_worker 0x71eca4 : { KEEP(*(.text.file_service_entry_seek_worker)) . = 0x71edbc; } > RETIRED =0xf000f0e7
  .allocate 0x71ef18 : { KEEP(*(.text.file_service_entry_tag_allocate)) . = 0x71eff4; } > RETIRED =0xf000f0e7
  .release 0x71eff4 : { KEEP(*(.text.file_service_entry_tag_release)) . = 0x71f078; } > RETIRED =0xf000f0e7
  .release_worker 0x71f078 : { . = 0x71f134; KEEP(*(.text.file_service_entry_release_worker)) . = 0x71f418; } > RETIRED =0xf000f0e7
  .post 0x71fde4 : { KEEP(*(.text.file_service_entry_post)) . = 0x71fe2c; } > RETIRED =0xf000f0e7
  .media_task 0x71fe7c : { KEEP(*(.text.file_service_entry_media_task)) . = 0x71ff3c; } > RETIRED =0xf000f0e7
  .mediamsg 0x721108 : { . = 0x721578; } > RETIRED =0xf000f0e7
  .route 0x721578 : { KEEP(*(.text.file_service_entry_route)) . = 0x721744; } > RETIRED =0xf000f0e7
  .intercept 0x7217f0 : { KEEP(*(.text.file_service_entry_intercept)) . = 0x721834; } > RETIRED =0xf000f0e7
  .file_counts_worker 0x718a8c : { KEEP(*(.text.file_service_entry_file_counts_worker)) . = 0x718de4; } > RETIRED_CATALOG =0xf000f0e7
  .file_position_worker 0x718de4 : { KEEP(*(.text.file_service_entry_file_position_worker)) . = 0x719098; } > RETIRED_CATALOG =0xf000f0e7
  .select_worker 0x719098 : { KEEP(*(.text.file_service_entry_select_worker)) . = 0x719614; } > RETIRED_CATALOG =0xf000f0e7
  .text : { __fixed_body_start = .; *(.text*) } > OWNED_FIXED
  .rodata : { KEEP(*(.rodata.XM1_REQUIRES_LENS_SAFETY_CONTRACT)) *(.rodata*) } > OWNED_FIXED
  .data : { *(.data*) . = ALIGN(4); __fixed_body_end = .; } > OWNED_FIXED
  .bss (NOLOAD) : { __fixed_bss_start = .; *(.bss*) *(COMMON) . = ALIGN(4); __fixed_bss_end = .; } > OWNED_FIXED
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.eh_frame*) *(.ARM.attributes) *(.note*) *(.comment*) *(.debug*) *(.llvm_addrsig) }
}
ASSERT(xm1_file_service_entry_result_acquire == 0x71bc2c && xm1_file_service_entry_result_release == 0x71bc6c && xm1_file_service_entry_result_wait == 0x71bc90 && xm1_file_service_entry_read == 0x71be74 && xm1_file_service_entry_write == 0x71bfe0 && xm1_file_service_entry_close == 0x71c134 && xm1_file_service_entry_open == 0x71c2b0 && xm1_file_service_entry_result_done == 0x71c634 && xm1_file_service_entry_open_async == 0x71c648 && xm1_file_service_entry_open_wait == 0x71c670 && xm1_file_service_entry_open_unflagged == 0x71c6a4 && xm1_file_service_entry_open_unflagged_wait == 0x71c6c8 && xm1_file_service_entry_read_async == 0x71c6fc && xm1_file_service_entry_read_wait == 0x71c720 && xm1_file_service_entry_write_async == 0x71c75c && xm1_file_service_entry_write_wait == 0x71c780 && xm1_file_service_entry_close_async == 0x71c7bc && xm1_file_service_entry_close_wait == 0x71c7e0 && xm1_file_service_entry_translate == 0x71d450 && xm1_file_service_entry_media_ready == 0x71ddf0 && xm1_file_service_entry_map_bind == 0x71ea34 && xm1_file_service_entry_map_unbind == 0x71eaac && xm1_file_service_entry_map_lookup == 0x71eb28 && xm1_file_service_entry_tag_allocate == 0x71ef18 && xm1_file_service_entry_tag_release == 0x71eff4 && xm1_file_service_entry_post == 0x71fde4 && xm1_file_service_entry_media_task == 0x71fe7c && xm1_file_service_entry_route == 0x721578 && xm1_file_service_entry_intercept == 0x7217f0 && xm1_file_service_entry_open_worker == 0x7115a4 && xm1_file_service_entry_write_worker == 0x7116e4 && xm1_file_service_entry_close_worker == 0x711768 && xm1_file_service_entry_close_entry_worker == 0x711840 && xm1_file_service_entry_seek_worker == 0x71eca4 && xm1_file_service_entry_release_worker == 0x71f134, "entry veneer moved")
ASSERT(xm1_file_service_entry_find_first_worker == 0x712700 && xm1_file_service_entry_find_next_worker == 0x7127e0 && xm1_file_service_entry_find_first_entry_worker == 0x71e3a4 && xm1_file_service_entry_find_next_entry_worker == 0x71e550, "search worker veneers are not at their stock entries")
ASSERT(xm1_file_service_entry_volume_attach_worker == 0x7120d8 && xm1_file_service_entry_volume_detach_worker == 0x7122dc && xm1_file_service_entry_volume_format_armed_worker == 0x7123b4 && xm1_file_service_entry_volume_format_worker == 0x712580, "volume worker veneers are not at their stock entries")
ASSERT(xm1_file_service_entry_select_worker == 0x719098, "selection worker veneers are not at their stock entries")
ASSERT(__fixed_body_start == ORIGIN(OWNED_FIXED), "fixed body does not start at the fixed origin")
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "the fixed bank has writable statics")
ASSERT(__fixed_bss_end <= ORIGIN(OWNED_FIXED) + LENGTH(OWNED_FIXED), "the fixed bank exceeds the fixed reservation")
ASSERT(xm1_file_service_entry_file_counts_worker == 0x718a8c && xm1_file_service_entry_file_position_worker == 0x718de4, "counts worker veneers are not at their stock entries")
