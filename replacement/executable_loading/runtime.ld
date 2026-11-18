ENTRY(xm1_loading_startup)
EXTERN(xm1_loading_startup)
EXTERN(xm1_loading_legacy_request)
EXTERN(xm1_loading_legacy_cancel)
EXTERN(xm1_decoder_init)
EXTERN(xm1_decoder_feed)
EXTERN(xm1_decoder_progress)
EXTERN(xm1_decoder_complete)
EXTERN(xm1_decoder_finish)

xm1_loading_create_channel = 0x724cdc;
xm1_loading_create_task = 0x725dd8;
xm1_loading_send_message = 0x72503c;
xm1_loading_receive_message = 0x725318;
xm1_loading_current_task = 0x723cb8;
xm1_loading_wait_reserved = 0x724b68;
xm1_loading_signal_reserved = 0x724c00;

xm1_loading_segment = 0x657114;
xm1_loading_extents = 0x6571a0;
xm1_loading_mode = 0x657184;
xm1_loading_plain_progress = 0x73b924;
xm1_loading_take = 0x724b68;
xm1_loading_give = 0x724c00;
xm1_loading_delay = 0x723ce8;
xm1_loading_cache = 0x42634;
xm1_loading_action1 = 0x6aa5b4;
xm1_loading_action2 = 0x6aa5e4;
xm1_loading_section3 = 0x6aa6f8;
xm1_loading_selected = 0x6aa63c;
xm1_loading_region = 0x6aa750;
xm1_loading_initialize_storage = 0x69f920;
xm1_loading_register_completion = 0x6c1444;
xm1_loading_auxiliary = 0x6aa838;

xm1_power_semaphore_wait = 0x724b68;
xm1_power_semaphore_signal = 0x724c00;
xm1_power_resource_route_request = 0x6499d0;
xm1_lower_read_clean_invalidate = 0x42634;
xm1_lower_read_clock_mask = 0x42560;
xm1_lower_read_clock_restore = 0x42568;

xm1_decoder_current_task = 0x723cb8;
xm1_decoder_take = 0x724b68;
xm1_decoder_give = 0x724c00;
xm1_decoder_route = 0x6499d0;
xm1_decoder_clock_take = 0x649c4c;
xm1_decoder_clock_give = 0x649ddc;
xm1_decoder_irq_attach = 0x64e918;
xm1_decoder_irq_detach = 0x64eb20;
xm1_decoder_marker = 0x42188;
xm1_decoder_progress_entry = 0x6ae1dc;
xm1_decoder_complete_entry = 0x6ae310;
SECTIONS
