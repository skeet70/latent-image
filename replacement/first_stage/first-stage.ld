/* Owned code is staged at 0x6000. Fixed-address configuration and workspace
   are separate from the retired executable body. */
ENTRY(xm1_first_stage_entry)
SECTIONS
{
  . = 0x6000;
  __first_stage_start = .;
  .text : {
    KEEP(*(.text.entry))
    *(.text*)
    KEEP(*(.xm1_log_text))
  }
  .rodata : { KEEP(*(.rodata.xm1_first_stage_contract)) *(.rodata*) }
  .data : { *(.data*) }
  .bss : { *(.bss*) *(COMMON) }
  . = ALIGN(4);
  __first_stage_end = .;
  /DISCARD/ : {
    *(.ARM.exidx*)
    *(.ARM.extab*)
    *(.note*)
    *(.comment*)
  }
}
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "owned first-stage image has writable statics")
ASSERT(__first_stage_end <= 0x10000, "owned first-stage image exceeds the first record")

xm1_vector_table_start = 0x0;
xm1_vector_before_reset_end = 0x20;
xm1_vector_after_reset_start = 0x24;
xm1_vector_table_end = 0x40;
xm1_retired_body_start = 0x1800;
xm1_retired_body_end = 0x4228;
xm1_config_data_start = 0x4228;
xm1_config_data_end = 0x4300;
xm1_retained_data_start = 0x4300;
xm1_retained_data_end = 0x5280;
