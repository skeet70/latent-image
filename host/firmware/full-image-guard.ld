ENTRY(xm1_full_image_guard)
SECTIONS
{
  . = 0x6000;
  .text : { *(.text) }
  . = 0x6200;
  .report : { *(.report.text) }
  . = 0x7000;
  .report_data (NOLOAD) : { *(.report.data) }
  .unexpected : { *(.data) *(.bss) *(COMMON) *(.rodata) }
}
ASSERT(xm1_full_image_guard == 0x6000, "full-image guard must be at RAM 0x6000")
ASSERT(xm1_report_stage == 0x6200, "stage reporter must be at RAM 0x6200")
ASSERT(xm1_report_buffer == 0x7000, "stage buffers must be at RAM 0x7000")
ASSERT(SIZEOF(.text) <= 0x200, "guard text must fit below RAM 0x6200")
ASSERT(SIZEOF(.report) <= 0xe00, "stage reporter must fit below RAM 0x7000")
ASSERT(SIZEOF(.report_data) == 0x400, "stage buffers must occupy two sectors")
ASSERT(SIZEOF(.unexpected) == 0, "guard must contain only text and report buffers")
ASSERT(SIZEOF(.rel.plt) == 0, "guard must not contain runtime relocations")
