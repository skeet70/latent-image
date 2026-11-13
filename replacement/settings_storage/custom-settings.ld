ENTRY(settings_custom_boot_entry)
EXTERN(TAIL_BYTES SLOT_BYTES)
INCLUDE imports.ld
xm1_stock_61df44 = 0x61df44;
xm1_stock_61d8a4 = 0x61d8a4;
xm1_stock_61d59c = 0x61d59c;
xm1_stock_61d618 = 0x61d618;
xm1_stock_61d5f0 = 0x61d5f0;
xm1_stock_61d65c = 0x61d65c;
xm1_stock_custom_config_read = 0x69f8fc;
xm1_stock_custom_reset_modes = 0x6a6fd8;
xm1_bank_property_set = 0x624694;
xm1_bank_current_selection = 0x6439e0;
xm1_bank_short_read = 0x72b3a4;
xm1_bank_custom_refresh = 0x6a86c4;
xm1_kernel_console9 = 0x41a34;
xm1_kernel_critical_enter = 0x7235d8;
xm1_kernel_critical_exit = 0x7236b0;
xm1_backup_diagnostic726c74 = 0x726c74;
xm1_backup_write6a59b8 = 0x6a59b8;
xm1_backup_report633594 = 0x633594;
SECTIONS {
  .text 0x5d0000 : {
    *custom-settings-rust*.a:*(.text* .rodata*)
    *(.text .text.memcpy .text.memset .text.__aeabi_memset .text.memmove)
    *(.text.settings_custom_config_read .text.settings_custom_reset_modes .text.settings_custom_store*)
    . = ALIGN(4);
    __custom_text_end = .;
  }
  .bank_write_body 0x61d8dc : { KEEP(*(.text.settings_bank_write_body_entry)) }
  .bank_write_active_body 0x61d92c : { KEEP(*(.text.settings_bank_write_active_body_entry)) }
  .bank_write_header 0x61d944 : { KEEP(*(.text.settings_bank_write_header_entry)) }
  .bank_read_custom 0x61d994 : { KEEP(*(.text.settings_bank_read_custom_entry)) }
  .bank_read_body 0x61d9ec : { KEEP(*(.text.settings_bank_read_body_entry)) }
  .bank_read_header 0x61da3c : { KEEP(*(.text.settings_bank_read_header_entry)) }
  .bank_exchange 0x61daa8 : { KEEP(*(.text.settings_bank_exchange_entry)) }
  .bank_copy 0x61db04 : { KEEP(*(.text.settings_bank_copy_entry)) }
  .bank_restore 0x61db18 : { KEEP(*(.text.settings_bank_restore_entry)) }
  .bank_capture 0x61db3c : { KEEP(*(.text.settings_bank_capture_entry)) }
  .bank_apply 0x6309c4 : { KEEP(*(.text.settings_bank_apply_entry)) }
  .bank_initialize 0x630a34 : { KEEP(*(.text.settings_bank_initialize_entry)) }
  .custom_prepare 0x6a8484 : { KEEP(*(.text.settings_custom_prepare_entry)) }
  .custom_check 0x6a84d4 : { KEEP(*(.text.settings_custom_check_entry)) }
  .custom_read 0x6a865c : { KEEP(*(.text.settings_custom_read_entry)) }
  .custom_refresh 0x6a86c4 : { KEEP(*(.text.settings_custom_refresh_entry)) }
  .custom_defaults 0x6a877c : { KEEP(*(.text.settings_custom_defaults_entry)) }
  .custom_boot 0x6a8abc : { KEEP(*(.text.settings_custom_boot_entry)) }
  .record_open 0x6ac2b4 : { KEEP(*(.text.record_store_entry_open)) }
  .record_register 0x6ac328 : { KEEP(*(.text.record_store_entry_register)) }
  .record_close 0x6ac478 : { KEEP(*(.text.record_store_entry_close)) }
  .record_latest 0x6ac4c8 : { KEEP(*(.text.record_store_entry_read_latest)) }
  .record_append 0x6ac7d4 : { KEEP(*(.text.record_store_entry_append)) }
  .record_erase 0x6ac984 : { KEEP(*(.text.record_store_entry_erase_all)) }
  .record_collect 0x6acab8 : { KEEP(*(.text.record_store_entry_collect)) }
  .record_read_at 0x6acda0 : { KEEP(*(.text.record_store_entry_read_at)) }
  .record_zero 0x6ae0fc : { KEEP(*(.text.record_store_entry_zero)) }
  .data : { *(.data*) *(.sdata*) *(.got*) }
  .bss : { *(.bss*) *(.sbss*) *(COMMON) }
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.eh_frame*) *(.ARM.attributes) *(.note*) *(.comment*) *(.debug*) *(.llvm_addrsig) }
}
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "custom settings has writable statics")
ASSERT(SIZEOF(.text) <= 0x10000, "custom settings carrier overflow")
ASSERT(settings_custom_prepare_entry == 0x6a8484, "custom prepare moved")
ASSERT(settings_custom_check_entry == 0x6a84d4, "custom check moved")
ASSERT(settings_custom_read_entry == 0x6a865c, "custom read moved")
ASSERT(settings_custom_refresh_entry == 0x6a86c4, "custom refresh moved")
ASSERT(settings_custom_defaults_entry == 0x6a877c, "custom defaults moved")
ASSERT(settings_custom_boot_entry == 0x6a8abc, "custom boot moved")
ASSERT(record_store_wrappers + 5 * 12 <= 0x89f8f0 && record_store_state == 0x89f8f8, "record store service layout moved")
ASSERT(record_store_block == 0x8a0080 && record_store_second_block == 0x8c0080, "record store buffers moved")
ASSERT(record_store_block + 0x20000 == record_store_second_block && record_store_second_block + 0x20000 == record_store_cache_tags, "record store buffers overlap")
ASSERT(record_store_cache_tags == 0x8e0080 && record_store_cache_timestamp == 0x8e0088, "record store cache metadata moved")
ASSERT(record_store_cache_tags + 8 == record_store_cache_timestamp && record_store_cache_timestamp + 4 <= record_store_staging_1, "record store cache metadata overlaps staging")
ASSERT(record_store_cache_ages == 0x89fa60 && record_store_cache_ages + 8 <= record_store_staging_3, "record store cache ages overlap staging")
ASSERT(!DEFINED(record_store_cache_tag), "obsolete single cache tag remains")
