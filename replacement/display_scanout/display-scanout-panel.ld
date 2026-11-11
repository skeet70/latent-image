ENTRY(xm1_scanout_commit)
scanout_enter_critical = 0x007235d8;
scanout_leave_critical = 0x007236b0;
scanout_yield = 0x00723ce8;
scanout_irq_configure = 0x0064e918;
scanout_irq_release = 0x0064eb20;
SECTIONS {
  . = 0x02000000;
  .text : {
    KEEP(*(.text.xm1_scanout_item_configuration))
    KEEP(*(.text.xm1_scanout_range_packet))
    KEEP(*(.text.xm1_scanout_range_enabled))
    KEEP(*(.text.xm1_scanout_group_options))
    KEEP(*(.text.xm1_scanout_blank))
    KEEP(*(.text.xm1_scanout_commit))
    KEEP(*(.text.xm1_scanout_commit_zero))
    KEEP(*(.text.xm1_scanout_commit_one))
    KEEP(*(.text.xm1_scanout_capture))
    KEEP(*(.text.xm1_scanout_scheduler_register))
    KEEP(*(.text.xm1_scanout_isr_zero))
    KEEP(*(.text.xm1_scanout_isr_one))
    KEEP(*(.text.xm1_scanout_reset))
    KEEP(*(.text.xm1_scanout_wait))
    KEEP(*(.text.xm1_scanout_timing_flush))
    KEEP(*(.text.xm1_scanout_timing_defaults))
    KEEP(*(.text.xm1_scanout_timing_immediate))
    KEEP(*(.text.xm1_scanout_timing_stage))
    KEEP(*(.text.xm1_scanout_formatter_flush))
    KEEP(*(.text.xm1_scanout_formatter_strobe))
    KEEP(*(.text.xm1_scanout_engine_initialize))
    KEEP(*(.text.xm1_scanout_operation_release))
    KEEP(*(.text.xm1_scanout_mode_window))
    KEEP(*(.text.xm1_scanout_mode_backing))
    KEEP(*(.text.xm1_scanout_mode_enabled))
    KEEP(*(.text.xm1_scanout_mode_range))
    KEEP(*(.text.xm1_scanout_mode_colour))
    KEEP(*(.text.xm1_scanout_mode_scale))
    KEEP(*(.text.xm1_scanout_mode_window_size))
    KEEP(*(.text.xm1_scanout_mode_colour_packet))
    *(.text .text.*)
  }
  .rodata : { *(.rodata .rodata.*) }
  .data : { *(.data*) *(.got .got.* .igot .igot.*) }
  .bss : { *(.bss*) *(COMMON) }
  .rel : { *(.rel .rel.*) }
  .rela : { *(.rela .rela.*) }
  .dynamic : { *(.dynamic) }
  ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "unexpected writable scanout state")
  ASSERT(SIZEOF(.rel) == 0 && SIZEOF(.rela) == 0 && SIZEOF(.dynamic) == 0, "unexpected scanout relocation")
  /DISCARD/ : {
    *(.ARM.exidx* .ARM.extab* .comment .note* .eh_frame*)
    INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.ARM.attributes .llvm_addrsig)
  }
  .unexpected : { INPUT_SECTION_FLAGS (SHF_ALLOC) *(*) }
  ASSERT(. <= 0x02040000, "scanout artifact exceeds 256 KiB")
  .symtab 0 : { INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.symtab) }
  .symtab_shndx 0 : { INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.symtab_shndx) }
  .strtab 0 : { INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.strtab) }
  .shstrtab 0 : { INPUT_SECTION_FLAGS (!SHF_ALLOC) *(.shstrtab) }
  ASSERT(SIZEOF(.unexpected) == 0, "unexpected allocated scanout data")
}
