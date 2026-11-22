/* The file service's retired image-4 extents, as SECTIONS commands the bank script INCLUDEs before
   its .text: a 4-byte A32 entry branch at each stock entry and traps elsewhere. RETIRED and
   RETIRED_WORKERS come from retired-memory.ld. */
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
  .resolve_index_worker 0x719614 : { KEEP(*(.text.file_service_entry_resolve_index_worker)) . = 0x71985c; } > RETIRED_CATALOG =0xf000f0e7
  .select_groups_worker 0x71a834 : { KEEP(*(.text.file_service_entry_select_groups_worker)) . = 0x71a8a0; } > RETIRED_CATALOG =0xf000f0e7
  .rebuild_catalog_worker 0x71a8a0 : { KEEP(*(.text.file_service_entry_rebuild_catalog_worker)) . = 0x71a93c; } > RETIRED_CATALOG =0xf000f0e7
  .find_entry_worker 0x71b8c0 : { KEEP(*(.text.file_service_entry_find_entry_worker)) . = 0x71b9b0; } > RETIRED_CATALOG =0xf000f0e7
  .complete_request_worker 0x71b9b0 : { KEEP(*(.text.file_service_entry_complete_request_worker)) . = 0x71b9e8; } > RETIRED_CATALOG =0xf000f0e7
  .file_delete_worker 0x7118fc : { KEEP(*(.text.file_service_entry_file_delete_worker)) . = 0x7119b0; } > RETIRED_WORKERS =0xf000f0e7
  .filename_set_worker 0x7119b0 : { KEEP(*(.text.file_service_entry_filename_set_worker)) . = 0x711b50; } > RETIRED_WORKERS =0xf000f0e7
  .directory_delete_worker 0x711b50 : { KEEP(*(.text.file_service_entry_directory_delete_worker)) . = 0x711bc8; } > RETIRED_WORKERS =0xf000f0e7
  .image_remove_worker 0x711bc8 : { KEEP(*(.text.file_service_entry_image_remove_worker)) . = 0x711ea4; } > RETIRED_WORKERS =0xf000f0e7
  .readonly_worker 0x711ea4 : { KEEP(*(.text.file_service_entry_readonly_worker)) . = 0x711f58; } > RETIRED_WORKERS =0xf000f0e7
  .attribute_set_worker 0x711f58 : { KEEP(*(.text.file_service_entry_attribute_set_worker)) . = 0x7120d8; } > RETIRED_WORKERS =0xf000f0e7
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
  .capacity_worker 0x715a58 : { KEEP(*(.text.file_service_entry_capacity_worker)) . = 0x715b34; } > RETIRED_CAPACITY =0xf000f0e7
  .pattern_worker 0x715de4 : { KEEP(*(.text.file_service_entry_pattern_worker)) . = 0x715f0c; } > RETIRED_CAPACITY =0xf000f0e7
  .query_worker 0x71ded4 : { KEEP(*(.text.file_service_entry_query_worker)) . = 0x71df20; } > RETIRED =0xf000f0e7
  .read_pages_worker 0x71e828 : { KEEP(*(.text.file_service_entry_read_pages_worker)) . = 0x71e8c4; } > RETIRED =0xf000f0e7
  .write_pages_worker 0x71e8c4 : { KEEP(*(.text.file_service_entry_write_pages_worker)) . = 0x71e960; } > RETIRED =0xf000f0e7
  .read_worker 0x71edbc : { KEEP(*(.text.file_service_entry_read_worker)) . = 0x71ef18; } > RETIRED =0xf000f0e7
ASSERT(xm1_file_service_entry_result_acquire == 0x71bc2c && xm1_file_service_entry_result_release == 0x71bc6c && xm1_file_service_entry_result_wait == 0x71bc90 && xm1_file_service_entry_read == 0x71be74 && xm1_file_service_entry_write == 0x71bfe0 && xm1_file_service_entry_close == 0x71c134 && xm1_file_service_entry_open == 0x71c2b0 && xm1_file_service_entry_result_done == 0x71c634 && xm1_file_service_entry_open_async == 0x71c648 && xm1_file_service_entry_open_wait == 0x71c670 && xm1_file_service_entry_open_unflagged == 0x71c6a4 && xm1_file_service_entry_open_unflagged_wait == 0x71c6c8 && xm1_file_service_entry_read_async == 0x71c6fc && xm1_file_service_entry_read_wait == 0x71c720 && xm1_file_service_entry_write_async == 0x71c75c && xm1_file_service_entry_write_wait == 0x71c780 && xm1_file_service_entry_close_async == 0x71c7bc && xm1_file_service_entry_close_wait == 0x71c7e0 && xm1_file_service_entry_translate == 0x71d450 && xm1_file_service_entry_media_ready == 0x71ddf0 && xm1_file_service_entry_map_bind == 0x71ea34 && xm1_file_service_entry_map_unbind == 0x71eaac && xm1_file_service_entry_map_lookup == 0x71eb28 && xm1_file_service_entry_tag_allocate == 0x71ef18 && xm1_file_service_entry_tag_release == 0x71eff4 && xm1_file_service_entry_post == 0x71fde4 && xm1_file_service_entry_media_task == 0x71fe7c && xm1_file_service_entry_route == 0x721578 && xm1_file_service_entry_intercept == 0x7217f0 && xm1_file_service_entry_open_worker == 0x7115a4 && xm1_file_service_entry_write_worker == 0x7116e4 && xm1_file_service_entry_close_worker == 0x711768 && xm1_file_service_entry_close_entry_worker == 0x711840 && xm1_file_service_entry_seek_worker == 0x71eca4 && xm1_file_service_entry_release_worker == 0x71f134, "entry veneer moved")
ASSERT(xm1_file_service_entry_find_first_worker == 0x712700 && xm1_file_service_entry_find_next_worker == 0x7127e0 && xm1_file_service_entry_find_first_entry_worker == 0x71e3a4 && xm1_file_service_entry_find_next_entry_worker == 0x71e550, "search worker veneers are not at their stock entries")
ASSERT(xm1_file_service_entry_volume_attach_worker == 0x7120d8 && xm1_file_service_entry_volume_detach_worker == 0x7122dc && xm1_file_service_entry_volume_format_armed_worker == 0x7123b4 && xm1_file_service_entry_volume_format_worker == 0x712580, "volume worker veneers are not at their stock entries")
ASSERT(xm1_file_service_entry_select_worker == 0x719098, "selection worker veneers are not at their stock entries")
ASSERT(xm1_file_service_entry_file_delete_worker == 0x7118fc && xm1_file_service_entry_filename_set_worker == 0x7119b0 && xm1_file_service_entry_directory_delete_worker == 0x711b50 && xm1_file_service_entry_image_remove_worker == 0x711bc8 && xm1_file_service_entry_readonly_worker == 0x711ea4 && xm1_file_service_entry_attribute_set_worker == 0x711f58, "paths worker veneers are not at their stock entries")
ASSERT(xm1_file_service_entry_capacity_worker == 0x715a58 && xm1_file_service_entry_pattern_worker == 0x715de4 && xm1_file_service_entry_query_worker == 0x71ded4 && xm1_file_service_entry_read_pages_worker == 0x71e828 && xm1_file_service_entry_write_pages_worker == 0x71e8c4 && xm1_file_service_entry_read_worker == 0x71edbc, "media_io worker veneers are not at their stock entries")
ASSERT(xm1_file_service_entry_file_counts_worker == 0x718a8c && xm1_file_service_entry_file_position_worker == 0x718de4, "counts worker veneers are not at their stock entries")
ASSERT(xm1_file_service_entry_resolve_index_worker == 0x719614 && xm1_file_service_entry_select_groups_worker == 0x71a834 && xm1_file_service_entry_rebuild_catalog_worker == 0x71a8a0 && xm1_file_service_entry_find_entry_worker == 0x71b8c0 && xm1_file_service_entry_complete_request_worker == 0x71b9b0, "catalog worker veneers are not at their stock entries")
