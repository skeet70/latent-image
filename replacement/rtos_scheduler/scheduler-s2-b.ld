ENTRY(xm1_rtos_scheduler_save)
INCLUDE s2-b-placement.ld

SECTIONS {
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
