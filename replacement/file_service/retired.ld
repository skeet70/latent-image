/* The file service's retired image-4 extents, as SECTIONS commands the bank script INCLUDEs before
   its .text: a 4-byte A32 entry branch at each stock entry and traps elsewhere. RETIRED comes from
   retired-memory.ld. */
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
  .bind 0x71ea34 : { KEEP(*(.text.file_service_entry_map_bind)) . = 0x71eaac; } > RETIRED =0xf000f0e7
  .unbind 0x71eaac : { KEEP(*(.text.file_service_entry_map_unbind)) . = 0x71eb28; } > RETIRED =0xf000f0e7
  .lookup 0x71eb28 : { KEEP(*(.text.file_service_entry_map_lookup)) . = 0x71eba0; } > RETIRED =0xf000f0e7
  .allocate 0x71ef18 : { KEEP(*(.text.file_service_entry_tag_allocate)) . = 0x71eff4; } > RETIRED =0xf000f0e7
  .release 0x71eff4 : { KEEP(*(.text.file_service_entry_tag_release)) . = 0x71f078; } > RETIRED =0xf000f0e7
  .post 0x71fde4 : { KEEP(*(.text.file_service_entry_post)) . = 0x71fe2c; } > RETIRED =0xf000f0e7
  .media_task 0x71fe7c : { KEEP(*(.text.file_service_entry_media_task)) . = 0x71ff3c; } > RETIRED =0xf000f0e7
  .mediamsg 0x721108 : { . = 0x721578; } > RETIRED =0xf000f0e7
  .route 0x721578 : { KEEP(*(.text.file_service_entry_route)) . = 0x721744; } > RETIRED =0xf000f0e7
  .intercept 0x7217f0 : { KEEP(*(.text.file_service_entry_intercept)) . = 0x721834; } > RETIRED =0xf000f0e7
ASSERT(xm1_file_service_entry_result_acquire == 0x71bc2c && xm1_file_service_entry_result_release == 0x71bc6c && xm1_file_service_entry_result_wait == 0x71bc90 && xm1_file_service_entry_read == 0x71be74 && xm1_file_service_entry_write == 0x71bfe0 && xm1_file_service_entry_close == 0x71c134 && xm1_file_service_entry_open == 0x71c2b0 && xm1_file_service_entry_result_done == 0x71c634 && xm1_file_service_entry_open_async == 0x71c648 && xm1_file_service_entry_open_wait == 0x71c670 && xm1_file_service_entry_open_unflagged == 0x71c6a4 && xm1_file_service_entry_open_unflagged_wait == 0x71c6c8 && xm1_file_service_entry_read_async == 0x71c6fc && xm1_file_service_entry_read_wait == 0x71c720 && xm1_file_service_entry_write_async == 0x71c75c && xm1_file_service_entry_write_wait == 0x71c780 && xm1_file_service_entry_close_async == 0x71c7bc && xm1_file_service_entry_close_wait == 0x71c7e0 && xm1_file_service_entry_translate == 0x71d450 && xm1_file_service_entry_media_ready == 0x71ddf0 && xm1_file_service_entry_map_bind == 0x71ea34 && xm1_file_service_entry_map_unbind == 0x71eaac && xm1_file_service_entry_map_lookup == 0x71eb28 && xm1_file_service_entry_tag_allocate == 0x71ef18 && xm1_file_service_entry_tag_release == 0x71eff4 && xm1_file_service_entry_post == 0x71fde4 && xm1_file_service_entry_media_task == 0x71fe7c && xm1_file_service_entry_route == 0x721578 && xm1_file_service_entry_intercept == 0x7217f0, "entry veneer moved")
