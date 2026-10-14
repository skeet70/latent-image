ENTRY(native_rollback_guard)

/* X-M1 1.50 primary boot record, decode/boot-40048.dis. */
xm1_native_reset          = 0x00001800;
xm1_primary_discriminator = 0x00001aa0;
xm1_native_close          = 0x00002044;

SECTIONS {
  .text 0x6000 : { *(.text) }
  .report 0x6200 : { *(.report.text) }
  .report_data 0x7000 : { *(.report.data) }
  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) }
  .rel.plt : { *(.rel.plt) }
  .unexpected : { INPUT_SECTION_FLAGS (SHF_ALLOC) *(*) }
}

ASSERT(ADDR(.text) == 0x6000, "rollback must be linked at RAM 0x6000")
ASSERT(native_rollback_guard == 0x6000, "entry guard must be at RAM 0x6000")
ASSERT(native_rollback_stub == 0x6048, "rollback stub must be at RAM 0x6048")
ASSERT(native_post_close == 0x60d0, "post-close hook must be at RAM 0x60d0")
ASSERT(xm1_report_stage == 0x6200, "stage reporter must be at RAM 0x6200")
ASSERT(xm1_report_buffer == 0x7000, "stage buffers must be at RAM 0x7000")
ASSERT(SIZEOF(.text) <= 0x200, "rollback text must fit below RAM 0x6200")
ASSERT(SIZEOF(.report) <= 0xe00, "stage reporter must fit below RAM 0x7000")
ASSERT(SIZEOF(.report_data) == 0x400, "stage buffers must occupy two sectors")
ASSERT(SIZEOF(.unexpected) == 0, "rollback must contain only raw text and report buffers")
ASSERT(SIZEOF(.rel.plt) == 0, "rollback must not contain runtime relocations")
