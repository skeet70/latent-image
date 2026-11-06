ENTRY(s3c_get_priority)
MEMORY {
  s3c_exception_loop (rx) : ORIGIN = 0xd4174, LENGTH = 0xd4
  s3c_exception_delivery (rx) : ORIGIN = 0xd4248, LENGTH = 0x90
  s3c_get_priority (rx) : ORIGIN = 0xd5b00, LENGTH = 0x194
  s3c_sleep (rx) : ORIGIN = 0xd5c94, LENGTH = 0x180
  s3c_suspend (rx) : ORIGIN = 0xd69c0, LENGTH = 0x280
  s3c_resume (rx) : ORIGIN = 0xd6c40, LENGTH = 0x1b8
  s3c_force_resume (rx) : ORIGIN = 0xd6df8, LENGTH = 0x10
  s3c_raise (rx) : ORIGIN = 0xd6f8c, LENGTH = 0x1e4
  s3c_interrupt_raise (rx) : ORIGIN = 0xd7170, LENGTH = 0x1a4
  s3c_disable_exception (rx) : ORIGIN = 0xd7314, LENGTH = 0xd4
  s3c_enable_exception (rx) : ORIGIN = 0xd73e8, LENGTH = 0x118
}
s3c_remove = 0xd3cdc;
s3c_insert = 0xd3c60;
s3c_notify = 0xd3fd4;
s3c_switch = 0xd3640;
SECTIONS {
  .s3c_exception_loop : { KEEP(*(.text.s3c_exception_loop)) __s3c_exception_loop_body_end = .; . = ORIGIN(s3c_exception_loop) + LENGTH(s3c_exception_loop); } > s3c_exception_loop =0xf000f0e7
  .s3c_exception_delivery : { KEEP(*(.text.s3c_exception_delivery)) __s3c_exception_delivery_body_end = .; . = ORIGIN(s3c_exception_delivery) + LENGTH(s3c_exception_delivery); } > s3c_exception_delivery =0xf000f0e7
  .s3c_get_priority : { KEEP(*(.text.s3c_get_priority)) __s3c_get_priority_body_end = .; . = ORIGIN(s3c_get_priority) + LENGTH(s3c_get_priority); } > s3c_get_priority =0xf000f0e7
  .s3c_sleep : { KEEP(*(.text.s3c_sleep)) __s3c_sleep_body_end = .; . = ORIGIN(s3c_sleep) + LENGTH(s3c_sleep); } > s3c_sleep =0xf000f0e7
  .s3c_suspend : { KEEP(*(.text.s3c_suspend)) __s3c_suspend_body_end = .; . = ORIGIN(s3c_suspend) + LENGTH(s3c_suspend); } > s3c_suspend =0xf000f0e7
  .s3c_resume : { KEEP(*(.text.s3c_resume)) __s3c_resume_body_end = .; . = ORIGIN(s3c_resume) + LENGTH(s3c_resume); } > s3c_resume =0xf000f0e7
  .s3c_force_resume : { KEEP(*(.text.s3c_force_resume)) __s3c_force_resume_body_end = .; . = ORIGIN(s3c_force_resume) + LENGTH(s3c_force_resume); } > s3c_force_resume =0xf000f0e7
  .s3c_raise : { KEEP(*(.text.s3c_raise)) __s3c_raise_body_end = .; . = ORIGIN(s3c_raise) + LENGTH(s3c_raise); } > s3c_raise =0xf000f0e7
  .s3c_interrupt_raise : { KEEP(*(.text.s3c_interrupt_raise)) __s3c_interrupt_raise_body_end = .; . = ORIGIN(s3c_interrupt_raise) + LENGTH(s3c_interrupt_raise); } > s3c_interrupt_raise =0xf000f0e7
  .s3c_disable_exception : { KEEP(*(.text.s3c_disable_exception)) __s3c_disable_exception_body_end = .; . = ORIGIN(s3c_disable_exception) + LENGTH(s3c_disable_exception); } > s3c_disable_exception =0xf000f0e7
  .s3c_enable_exception : { KEEP(*(.text.s3c_enable_exception)) __s3c_enable_exception_body_end = .; . = ORIGIN(s3c_enable_exception) + LENGTH(s3c_enable_exception); } > s3c_enable_exception =0xf000f0e7
  .forbidden (NOLOAD) : { *(.text .text.* .rodata .rodata.* .data .data.* .bss .bss.* COMMON) } > s3c_exception_loop
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}
ASSERT(s3c_exception_loop == ORIGIN(s3c_exception_loop), "exception_loop entry moved")
ASSERT(__s3c_exception_loop_body_end <= ORIGIN(s3c_exception_loop) + LENGTH(s3c_exception_loop), "exception_loop body overflow")
ASSERT(s3c_exception_delivery == ORIGIN(s3c_exception_delivery), "exception_delivery entry moved")
ASSERT(__s3c_exception_delivery_body_end <= ORIGIN(s3c_exception_delivery) + LENGTH(s3c_exception_delivery), "exception_delivery body overflow")
ASSERT(s3c_get_priority == ORIGIN(s3c_get_priority), "get_priority entry moved")
ASSERT(__s3c_get_priority_body_end <= ORIGIN(s3c_get_priority) + LENGTH(s3c_get_priority), "get_priority body overflow")
ASSERT(s3c_sleep == ORIGIN(s3c_sleep), "sleep entry moved")
ASSERT(__s3c_sleep_body_end <= ORIGIN(s3c_sleep) + LENGTH(s3c_sleep), "sleep body overflow")
ASSERT(s3c_suspend == ORIGIN(s3c_suspend), "suspend entry moved")
ASSERT(__s3c_suspend_body_end <= ORIGIN(s3c_suspend) + LENGTH(s3c_suspend), "suspend body overflow")
ASSERT(s3c_resume == ORIGIN(s3c_resume), "resume entry moved")
ASSERT(__s3c_resume_body_end <= ORIGIN(s3c_resume) + LENGTH(s3c_resume), "resume body overflow")
ASSERT(s3c_force_resume == ORIGIN(s3c_force_resume), "force_resume entry moved")
ASSERT(__s3c_force_resume_body_end <= ORIGIN(s3c_force_resume) + LENGTH(s3c_force_resume), "force_resume body overflow")
ASSERT(s3c_raise == ORIGIN(s3c_raise), "raise entry moved")
ASSERT(__s3c_raise_body_end <= ORIGIN(s3c_raise) + LENGTH(s3c_raise), "raise body overflow")
ASSERT(s3c_interrupt_raise == ORIGIN(s3c_interrupt_raise), "interrupt_raise entry moved")
ASSERT(__s3c_interrupt_raise_body_end <= ORIGIN(s3c_interrupt_raise) + LENGTH(s3c_interrupt_raise), "interrupt_raise body overflow")
ASSERT(s3c_disable_exception == ORIGIN(s3c_disable_exception), "disable_exception entry moved")
ASSERT(__s3c_disable_exception_body_end <= ORIGIN(s3c_disable_exception) + LENGTH(s3c_disable_exception), "disable_exception body overflow")
ASSERT(s3c_enable_exception == ORIGIN(s3c_enable_exception), "enable_exception entry moved")
ASSERT(__s3c_enable_exception_body_end <= ORIGIN(s3c_enable_exception) + LENGTH(s3c_enable_exception), "enable_exception body overflow")
ASSERT(SIZEOF(.forbidden) == 0, "unplaced sections")
