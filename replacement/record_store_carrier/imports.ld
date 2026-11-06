record_raw_region_read = 0x6aad94;
record_raw_write_0 = 0x6aab1c;
record_raw_write_1 = 0x6aa94c;
record_raw_write_2 = 0x6ab07c;
record_raw_write_3 = 0x6ab2f4;
record_raw_write_4 = 0x6aacb8;
record_raw_erase_0 = 0x6aabd8;
record_raw_erase_1 = 0x6aaa08;
record_raw_erase_2 = 0x6ab1e0;
record_raw_erase_3 = 0x6ab458;
record_raw_erase_4 = 0x6aad40;
record_semaphore_take = 0x724b68;
record_semaphore_give = 0x724c00;
record_store_initialized = 0x89f8b0;
record_store_wrappers = 0x89f8b4;
record_store_state = 0x89f8f8;
record_store_block = 0x8a0080;
record_store_second_block = 0x8c0080;
record_store_cache_tags = 0x8e0080;
record_store_cache_timestamp = 0x8e0088;
record_store_cache_ages = 0x89fa60;
record_store_staging_0 = 0x8e16a0;
record_store_staging_1 = 0x8e00a0;
record_store_staging_2 = 0x8e1700;
record_store_staging_3 = 0x89fa80;
record_store_staging_4 = 0x8e10a0;
ASSERT(record_store_wrappers + 5 * 12 == 0x89f8f0 && record_store_state == 0x89f8f8, "record store service layout moved")
ASSERT(record_store_block + 0x20000 == record_store_second_block && record_store_second_block + 0x20000 == record_store_cache_tags, "record store buffers overlap")
ASSERT(record_store_cache_tags + 8 == record_store_cache_timestamp && record_store_cache_timestamp + 4 <= record_store_staging_1, "record store metadata overlaps staging")
ASSERT(record_store_cache_ages == 0x89fa60 && record_store_cache_ages + 8 <= record_store_staging_3, "record store ages overlap staging")
