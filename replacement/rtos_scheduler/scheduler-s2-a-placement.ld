MEMORY {
  s2_doorbell (rx) : ORIGIN = 0x40b00, LENGTH = 0x24
  s2_save (rx) : ORIGIN = 0x43640, LENGTH = 0x28
  s2_resume (rx) : ORIGIN = 0x43668, LENGTH = 0x20
  s2_select (rx) : ORIGIN = 0x43688, LENGTH = 0xa0
  s2_first_entry (rx) : ORIGIN = 0x43728, LENGTH = 0x34
  s2_irq_return (rx) : ORIGIN = 0x4375c, LENGTH = 0x48
  s2_irq_resume (rx) : ORIGIN = 0x437a4, LENGTH = 0x38
  s2_pool (rx) : ORIGIN = 0x437fc, LENGTH = 0x14
}
xm1_rtos_scheduler_s2_exit_task = 0x453f0;
xm1_rtos_scheduler_s2_deliver_exception = 0x44288;
xm1_rtos_scheduler_s2_clear_irq = 0x43444;
xm1_rtos_scheduler_s2_global_unlock = 0x427fc;
SECTIONS {
  .s2_doorbell : { KEEP(*(.text.s2_doorbell)) __s2_doorbell_body_end = .; . = ORIGIN(s2_doorbell) + LENGTH(s2_doorbell); } > s2_doorbell =0xf000f0e7
  .s2_save : { KEEP(*(.text.s2_save)) __s2_save_body_end = .; . = ORIGIN(s2_save) + LENGTH(s2_save); } > s2_save =0xf000f0e7
  .s2_resume : { KEEP(*(.text.s2_resume)) __s2_resume_body_end = .; . = ORIGIN(s2_resume) + LENGTH(s2_resume); } > s2_resume =0xf000f0e7
  .s2_select : { KEEP(*(.text.s2_select)) __s2_select_body_end = .; . = ORIGIN(s2_select) + LENGTH(s2_select); } > s2_select =0xf000f0e7
  .s2_first_entry : { KEEP(*(.text.s2_first_entry)) __s2_first_entry_body_end = .; . = ORIGIN(s2_first_entry) + LENGTH(s2_first_entry); } > s2_first_entry =0xf000f0e7
  .s2_irq_return : { KEEP(*(.text.s2_irq_return)) __s2_irq_return_body_end = .; . = ORIGIN(s2_irq_return) + LENGTH(s2_irq_return); } > s2_irq_return =0xf000f0e7
  .s2_irq_resume : { KEEP(*(.text.s2_irq_resume)) __s2_irq_resume_body_end = .; . = ORIGIN(s2_irq_resume) + LENGTH(s2_irq_resume); } > s2_irq_resume =0xf000f0e7
  .s2_pool : { KEEP(*(.rodata.s2_pool)) __s2_pool_body_end = .; . = ORIGIN(s2_pool) + LENGTH(s2_pool); } > s2_pool =0xf000f0e7
}
