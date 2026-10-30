/* Record 2 loads at 0x10000. The owned image fills padding at 0x10400, past
   the 0x3e8-byte vector store record 1 copies to 0x3fff0000. */
ENTRY(xm1_secondary_entry)
SECTIONS
{
  . = 0x10400;
  .text : {
    KEEP(*(.text.entry))
    *(.text*)
  }
  .rodata : { *(.rodata*) }
  .data : { *(.data*) }
  .bss : { *(.bss*) *(COMMON) }
  . = ALIGN(4);
  __secondary_stage_end = .;
  /DISCARD/ : {
    *(.ARM.exidx*)
    *(.ARM.extab*)
    *(.note*)
    *(.comment*)
  }
}
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "owned record-2 image has writable statics")
ASSERT(__secondary_stage_end <= 0x11800, "owned record-2 image reaches the retained body")

xm1_record_base = 0x10000;
xm1_vector_table_start = 0x10000;
xm1_vector_before_reset_end = 0x10020;
xm1_vector_after_reset_start = 0x10024;
xm1_vector_table_end = 0x103e8;
xm1_retained_body_start = 0x11800;
xm1_retained_body_end = 0x14698;
xm1_retained_data_start = 0x14698;
xm1_retained_data_end = 0x14c80;
xm1_publication_target = 0x14c80;
xm1_publication_argument = 0x14c84;

xm1_retained_execution_state = 0x11854;
xm1_retained_tcm_stacks = 0x118b8;
xm1_retained_peripheral_init = 0x12204;
xm1_retained_delay = 0x1270c;
xm1_retained_companion_transfer = 0x12ad0;
xm1_retained_peripheral_final = 0x130c8;
xm1_retained_conversion = 0x13124;
xm1_retained_config_record = 0x13244;
xm1_retained_segment_lookup = 0x13574;
xm1_retained_companion_flags = 0x137a8;
xm1_retained_structure_init = 0x13b80;

/* Candidate forbidden ranges: stock code the owned path replaces (fetch) and
   literal pools only that code reads (read). No runtime guard enforces them. */
xm1_forbid_fetch_entry_start = 0x11800;
xm1_forbid_fetch_entry_end = 0x11854;
xm1_forbid_fetch_mpu_discriminator_start = 0x119e4;
xm1_forbid_fetch_mpu_discriminator_end = 0x11abc;
xm1_forbid_fetch_main_start = 0x11cc0;
xm1_forbid_fetch_main_end = 0x11fe4;
xm1_forbid_fetch_stub_a_start = 0x12960;
xm1_forbid_fetch_stub_a_end = 0x12974;
xm1_forbid_fetch_stub_b_start = 0x12abc;
xm1_forbid_fetch_stub_b_end = 0x12ad0;
xm1_forbid_fetch_log_start = 0x13490;
xm1_forbid_fetch_log_end = 0x13574;
xm1_forbid_fetch_selection_start = 0x1386c;
xm1_forbid_fetch_selection_end = 0x138ac;
xm1_forbid_read_entry_pool_start = 0x11ac4;
xm1_forbid_read_entry_pool_end = 0x11acc;
xm1_forbid_read_handoff_pool_start = 0x11ad4;
xm1_forbid_read_handoff_pool_end = 0x11ae8;
xm1_forbid_read_main_pool_start = 0x11fe4;
xm1_forbid_read_main_pool_end = 0x11fec;
