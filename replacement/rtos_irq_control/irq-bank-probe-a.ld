ENTRY(xm1_rtos_irq_slot_enable)

/* Retained loader A extents: a 4-byte branch at each stock entry to its bank body,
   traps elsewhere. All bodies are in OWNED_FIXED. */
MEMORY {
  slots (rx) : ORIGIN = 0x41108, LENGTH = 0x38
  mask_cache (rx) : ORIGIN = 0x42560, LENGTH = 0x274
  controller (rx) : ORIGIN = 0x42854, LENGTH = 0xc9c
  define (rx) : ORIGIN = 0x4d8fc, LENGTH = 0x7c
  INCLUDE fixed-memory.ld
}

SECTIONS {
  .irq_slots : {
    KEEP(*(.text.irq_veneer_irq_slot_enable))
    . = 0x41124;
    KEEP(*(.text.irq_veneer_irq_slot_disable))
    . = 0x41140;
  } > slots =0xf000f0e7
  .irq_mask_cache : {
    KEEP(*(.text.irq_veneer_mask_save_irq_fiq))
    . = 0x42568;
    KEEP(*(.text.irq_veneer_mask_restore_irq_fiq))
    . = 0x42574;
    KEEP(*(.text.irq_veneer_mask_save_irq))
    . = 0x4257c;
    KEEP(*(.text.irq_veneer_mask_restore_irq))
    . = 0x42588;
    KEEP(*(.text.irq_veneer_mask_save_fiq))
    . = 0x42590;
    KEEP(*(.text.irq_veneer_mask_restore_fiq))
    . = 0x425c8;
    KEEP(*(.text.irq_veneer_cache_enable_instruction))
    . = 0x425d0;
    KEEP(*(.text.irq_veneer_cache_enable_data))
    . = 0x425d8;
    KEEP(*(.text.irq_veneer_cache_disable_instruction))
    . = 0x425e0;
    KEEP(*(.text.irq_veneer_cache_disable_data))
    . = 0x4262c;
    KEEP(*(.text.irq_veneer_cache_invalidate_instruction_range))
    . = 0x42634;
    KEEP(*(.text.irq_veneer_cache_clean_invalidate_data_range))
    . = 0x4263c;
    KEEP(*(.text.irq_veneer_cache_clean_data_range))
    . = 0x42644;
    KEEP(*(.text.irq_veneer_cache_invalidate_data_range))
    . = 0x427b8;
    KEEP(*(.text.irq_veneer_irq_identity))
    . = 0x427d4;
  } > mask_cache =0xf000f0e7
  .irq_controller : {
    KEEP(*(.text.irq_veneer_irq_core_index))
    . = 0x4286c;
    KEEP(*(.text.irq_veneer_irq_initialize))
    . = 0x429ac;
    KEEP(*(.text.irq_veneer_irq_configure))
    . = 0x42a98;
    KEEP(*(.text.irq_veneer_irq_reset))
    . = 0x42adc;
    KEEP(*(.text.irq_veneer_irq_handler))
    . = 0x42b00;
    KEEP(*(.text.irq_veneer_irq_enable))
    . = 0x42eb0;
    KEEP(*(.text.irq_veneer_irq_disable))
    . = 0x430a8;
    KEEP(*(.text.irq_veneer_irq_enter))
    . = 0x431d8;
    KEEP(*(.text.irq_veneer_irq_leave))
    . = 0x432f8;
    KEEP(*(.text.irq_veneer_irq_set_enable))
    . = 0x43358;
    KEEP(*(.text.irq_veneer_irq_clear))
    . = 0x433e4;
    KEEP(*(.text.irq_veneer_irq_raise))
    . = 0x43444;
    KEEP(*(.text.irq_veneer_irq_clear_pending))
    . = 0x434a4;
    KEEP(*(.text.irq_veneer_irq_copy_active))
    . = 0x434f0;
  } > controller =0xf000f0e7
  .irq_define : {
    KEEP(*(.text.irq_veneer_irq_define))
    . = 0x4d978;
  } > define =0xf000f0e7
  .bank : {
    __irq_bank_start = .;
    KEEP(*(.text.irq_body_*))
    *(.text .text.*)
    __irq_bank_end = .;
  } > OWNED_FIXED
  .forbidden (NOLOAD) : { *(.rodata .rodata.* .data .data.* .bss .bss.* COMMON) } > OWNED_FIXED
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}

ASSERT(xm1_rtos_irq_slot_enable == 0x41108, "irq_slot_enable veneer is not at its stock entry")
ASSERT(xm1_rtos_irq_slot_disable == 0x41124, "irq_slot_disable veneer is not at its stock entry")
ASSERT(xm1_rtos_mask_save_irq_fiq == 0x42560, "mask_save_irq_fiq veneer is not at its stock entry")
ASSERT(xm1_rtos_mask_restore_irq_fiq == 0x42568, "mask_restore_irq_fiq veneer is not at its stock entry")
ASSERT(xm1_rtos_mask_save_irq == 0x42574, "mask_save_irq veneer is not at its stock entry")
ASSERT(xm1_rtos_mask_restore_irq == 0x4257c, "mask_restore_irq veneer is not at its stock entry")
ASSERT(xm1_rtos_mask_save_fiq == 0x42588, "mask_save_fiq veneer is not at its stock entry")
ASSERT(xm1_rtos_mask_restore_fiq == 0x42590, "mask_restore_fiq veneer is not at its stock entry")
ASSERT(xm1_rtos_cache_enable_instruction == 0x425c8, "cache_enable_instruction veneer is not at its stock entry")
ASSERT(xm1_rtos_cache_enable_data == 0x425d0, "cache_enable_data veneer is not at its stock entry")
ASSERT(xm1_rtos_cache_disable_instruction == 0x425d8, "cache_disable_instruction veneer is not at its stock entry")
ASSERT(xm1_rtos_cache_disable_data == 0x425e0, "cache_disable_data veneer is not at its stock entry")
ASSERT(xm1_rtos_cache_invalidate_instruction_range == 0x4262c, "cache_invalidate_instruction_range veneer is not at its stock entry")
ASSERT(xm1_rtos_cache_clean_invalidate_data_range == 0x42634, "cache_clean_invalidate_data_range veneer is not at its stock entry")
ASSERT(xm1_rtos_cache_clean_data_range == 0x4263c, "cache_clean_data_range veneer is not at its stock entry")
ASSERT(xm1_rtos_cache_invalidate_data_range == 0x42644, "cache_invalidate_data_range veneer is not at its stock entry")
ASSERT(xm1_rtos_irq_identity == 0x427b8, "irq_identity veneer is not at its stock entry")
ASSERT(xm1_rtos_irq_core_index == 0x42854, "irq_core_index veneer is not at its stock entry")
ASSERT(xm1_rtos_irq_initialize == 0x4286c, "irq_initialize veneer is not at its stock entry")
ASSERT(xm1_rtos_irq_configure == 0x429ac, "irq_configure veneer is not at its stock entry")
ASSERT(xm1_rtos_irq_reset == 0x42a98, "irq_reset veneer is not at its stock entry")
ASSERT(xm1_rtos_irq_handler == 0x42adc, "irq_handler veneer is not at its stock entry")
ASSERT(xm1_rtos_irq_enable == 0x42b00, "irq_enable veneer is not at its stock entry")
ASSERT(xm1_rtos_irq_disable == 0x42eb0, "irq_disable veneer is not at its stock entry")
ASSERT(xm1_rtos_irq_enter == 0x430a8, "irq_enter veneer is not at its stock entry")
ASSERT(xm1_rtos_irq_leave == 0x431d8, "irq_leave veneer is not at its stock entry")
ASSERT(xm1_rtos_irq_set_enable == 0x432f8, "irq_set_enable veneer is not at its stock entry")
ASSERT(xm1_rtos_irq_clear == 0x43358, "irq_clear veneer is not at its stock entry")
ASSERT(xm1_rtos_irq_raise == 0x433e4, "irq_raise veneer is not at its stock entry")
ASSERT(xm1_rtos_irq_clear_pending == 0x43444, "irq_clear_pending veneer is not at its stock entry")
ASSERT(xm1_rtos_irq_copy_active == 0x434a4, "irq_copy_active veneer is not at its stock entry")
ASSERT(xm1_rtos_irq_define == 0x4d8fc, "irq_define veneer is not at its stock entry")
ASSERT(SIZEOF(.irq_slots) == 0x38 && SIZEOF(.irq_mask_cache) == 0x274 && SIZEOF(.irq_controller) == 0xc9c && SIZEOF(.irq_define) == 0x7c, "retained extent size differs")
ASSERT(__irq_bank_start == ORIGIN(OWNED_FIXED), "bank body moved")
ASSERT(__irq_bank_end <= ORIGIN(OWNED_FIXED) + LENGTH(OWNED_FIXED), "bank exceeds the fixed reservation")
ASSERT(SIZEOF(.forbidden) == 0, "read-only or writable data")
