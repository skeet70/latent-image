/* The scanout exports in the owned fixed range. Retained callers reach them through one A32 branch
   published at each stock entry (`artifact publications`). */
ENTRY(xm1_scanout_commit)
EXTERN(xm1_scanout_isr_one xm1_scanout_isr_zero xm1_scanout_scheduler_register xm1_scanout_capture
  xm1_scanout_item_configuration xm1_scanout_range_packet xm1_scanout_range_enabled xm1_scanout_group_options
  xm1_scanout_commit xm1_scanout_commit_zero xm1_scanout_commit_one xm1_scanout_blank)
scanout_enter_critical = 0x007235d8;
scanout_leave_critical = 0x007236b0;
scanout_yield = 0x00723ce8;
scanout_irq_configure = 0x0064e918;
scanout_irq_release = 0x0064eb20;
MEMORY {
  INCLUDE fixed-memory.ld
}
SECTIONS {
  .text : { __fixed_body_start = .; *(.text*) KEEP(*(.rodata.XM1_REQUIRES_LENS_SAFETY_CONTRACT)) *(.rodata*) } > OWNED_FIXED
  .data : { *(.data*) . = ALIGN(4); __fixed_body_end = .; } > OWNED_FIXED
  .bss (NOLOAD) : { __fixed_bss_start = .; *(.bss*) *(COMMON) . = ALIGN(4); __fixed_bss_end = .; } > OWNED_FIXED
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.note*) *(.comment*) }
}
ASSERT(__fixed_bss_end <= ORIGIN(OWNED_FIXED) + LENGTH(OWNED_FIXED), "display scanout exceeds the fixed reservation")
ASSERT(__fixed_bss_end == __fixed_body_end, "display scanout has writable statics")
