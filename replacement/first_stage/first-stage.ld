/* Image load offset 0x6000; the retained stock first-record body occupies
   0x1800-0x5280 and provides these entry symbols. */
ENTRY(xm1_first_stage_entry)
SECTIONS
{
  . = 0x6000;
  .text : {
    KEEP(*(.text.entry))
    *(.text*)
  }
  .rodata : { *(.rodata*) }
  .data : { *(.data*) }
  . = ALIGN(4);
  __first_stage_end = .;
  /DISCARD/ : {
    *(.ARM.exidx*)
    *(.ARM.extab*)
    *(.note*)
    *(.comment*)
  }
}

xm1_retained_core_init = 0x25d4;
xm1_retained_storage_init = 0x2034;
xm1_retained_skip_scan = 0x2058;
xm1_retained_segment_table = 0x1dc4;
xm1_retained_trailer_check = 0x1cc0;
xm1_retained_stage_images = 0x1d08;
xm1_retained_storage_fini = 0x2044;
