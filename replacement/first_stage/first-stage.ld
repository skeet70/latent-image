/* Image load offset 0x6000; the retained stock first-record body occupies
   0x1800-0x5280 and provides these entry symbols. */
ENTRY(xm1_first_stage_entry)
SECTIONS
{
  . = 0x6000;
  __first_stage_start = .;
  .text : {
    KEEP(*(.text.entry))
    *(.text*)
  }
  .rodata : { *(.rodata*) }
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
xm1_retained_body_start = 0x1800;
xm1_retained_body_end = 0x4300;
xm1_retained_data_start = 0x4300;
xm1_retained_data_end = 0x5280;
xm1_retained_core_init = 0x25d4;
xm1_retained_storage_init = 0x2034;
xm1_retained_skip_scan = 0x2058;
xm1_retained_segment_table = 0x1dc4;
xm1_retained_trailer_check = 0x1cc0;
xm1_retained_stage_images = 0x1d08;
xm1_retained_storage_fini = 0x2044;
