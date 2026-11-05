ENTRY(xm1_rtos_scheduler_save)
MEMORY {
  s2_doorbell (rx) : ORIGIN = 0xd0b00, LENGTH = 0x24
  s2_save (rx) : ORIGIN = 0xd3640, LENGTH = 0x28
  s2_resume (rx) : ORIGIN = 0xd3668, LENGTH = 0x20
  s2_select (rx) : ORIGIN = 0xd3688, LENGTH = 0xa0
  s2_first_entry (rx) : ORIGIN = 0xd3728, LENGTH = 0x34
  s2_irq_return (rx) : ORIGIN = 0xd375c, LENGTH = 0x48
  s2_irq_resume (rx) : ORIGIN = 0xd37a4, LENGTH = 0x38
  s2_pool (rx) : ORIGIN = 0xd37fc, LENGTH = 0x14
}
xm1_rtos_scheduler_s2_exit_task = 0xd53b0;
xm1_rtos_scheduler_s2_deliver_exception = 0xd4248;
xm1_rtos_scheduler_s2_clear_irq = 0xd3444;
xm1_rtos_scheduler_s2_global_unlock = 0xd27fc;
SECTIONS {
  .s2_doorbell : { KEEP(*(.text.s2_doorbell)) __s2_doorbell_body_end = .; . = ORIGIN(s2_doorbell) + LENGTH(s2_doorbell); } > s2_doorbell =0xf000f0e7
  .s2_save : { KEEP(*(.text.s2_save)) __s2_save_body_end = .; . = ORIGIN(s2_save) + LENGTH(s2_save); } > s2_save =0xf000f0e7
  .s2_resume : { KEEP(*(.text.s2_resume)) __s2_resume_body_end = .; . = ORIGIN(s2_resume) + LENGTH(s2_resume); } > s2_resume =0xf000f0e7
  .s2_select : { KEEP(*(.text.s2_select)) __s2_select_body_end = .; . = ORIGIN(s2_select) + LENGTH(s2_select); } > s2_select =0xf000f0e7
  .s2_first_entry : { KEEP(*(.text.s2_first_entry)) __s2_first_entry_body_end = .; . = ORIGIN(s2_first_entry) + LENGTH(s2_first_entry); } > s2_first_entry =0xf000f0e7
  .s2_irq_return : { KEEP(*(.text.s2_irq_return)) __s2_irq_return_body_end = .; . = ORIGIN(s2_irq_return) + LENGTH(s2_irq_return); } > s2_irq_return =0xf000f0e7
  .s2_irq_resume : { KEEP(*(.text.s2_irq_resume)) __s2_irq_resume_body_end = .; . = ORIGIN(s2_irq_resume) + LENGTH(s2_irq_resume); } > s2_irq_resume =0xf000f0e7
  .s2_pool : { KEEP(*(.rodata.s2_pool)) __s2_pool_body_end = .; . = ORIGIN(s2_pool) + LENGTH(s2_pool); } > s2_pool =0xf000f0e7
  .forbidden (NOLOAD) : { *(.rodata .rodata.* .data .data.* .bss .bss.* COMMON) } > s2_save
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}
ASSERT(__s2_doorbell_body_end <= ORIGIN(s2_doorbell) + LENGTH(s2_doorbell), "S2 doorbell exceeds stock extent")
ASSERT(__s2_save_body_end <= ORIGIN(s2_save) + LENGTH(s2_save), "S2 save exceeds stock extent")
ASSERT(__s2_resume_body_end <= ORIGIN(s2_resume) + LENGTH(s2_resume), "S2 resume exceeds stock extent")
ASSERT(__s2_select_body_end <= ORIGIN(s2_select) + LENGTH(s2_select), "S2 select exceeds stock extent")
ASSERT(__s2_first_entry_body_end <= ORIGIN(s2_first_entry) + LENGTH(s2_first_entry), "S2 first_entry exceeds stock extent")
ASSERT(__s2_irq_return_body_end <= ORIGIN(s2_irq_return) + LENGTH(s2_irq_return), "S2 irq_return exceeds stock extent")
ASSERT(__s2_irq_resume_body_end <= ORIGIN(s2_irq_resume) + LENGTH(s2_irq_resume), "S2 irq_resume exceeds stock extent")
ASSERT(__s2_pool_body_end <= ORIGIN(s2_pool) + LENGTH(s2_pool), "S2 pool exceeds stock extent")
ASSERT(xm1_rtos_scheduler_doorbell == ORIGIN(s2_doorbell), "S2 doorbell moved")
ASSERT(xm1_rtos_scheduler_save == ORIGIN(s2_save), "S2 save moved")
ASSERT(xm1_rtos_scheduler_resume == ORIGIN(s2_resume), "S2 resume moved")
ASSERT(xm1_rtos_scheduler_first_entry == ORIGIN(s2_first_entry), "S2 first_entry moved")
ASSERT(xm1_rtos_scheduler_irq_return == ORIGIN(s2_irq_return), "S2 irq_return moved")
ASSERT(xm1_rtos_scheduler_irq_resume == ORIGIN(s2_irq_resume), "S2 irq_resume moved")
ASSERT(xm1_rtos_scheduler_no_save == ORIGIN(s2_select), "S2 no-save moved")
ASSERT(SIZEOF(.forbidden) == 0, "S2 has mutable data")
