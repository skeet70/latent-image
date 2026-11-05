ENTRY(xm1_s3a_exit_task)
MEMORY {
  s3a_exit (rx) : ORIGIN = 0x453f0, LENGTH = 0x100
  s3a_rotate (rx) : ORIGIN = 0x4c28c, LENGTH = 0xe0
  s3a_irotate (rx) : ORIGIN = 0x4c36c, LENGTH = 0xc8
  s3a_crotate (rx) : ORIGIN = 0x4c434, LENGTH = 0xfc
  s3a_icrotate (rx) : ORIGIN = 0x4c530, LENGTH = 0xf0
  s3a_disable (rx) : ORIGIN = 0x4c9dc, LENGTH = 0xe4
  s3a_enable (rx) : ORIGIN = 0x4cac0, LENGTH = 0x118
}
s3a_stock_remove = 0x43d20;
s3a_stock_reset = 0x43dd0;
s3a_stock_prepare = 0x43f70;
s3a_stock_rotate = 0x44134;
s3a_stock_notify = 0x44018;
s3a_stock_save = 0x43640;
s3a_stock_discard = 0x43688;
SECTIONS {
  .s3a_exit : { KEEP(*(.text.s3a_exit)) __s3a_exit_body_end = .; . = ORIGIN(s3a_exit) + LENGTH(s3a_exit); } > s3a_exit =0xf000f0e7
  .s3a_rotate : { KEEP(*(.text.s3a_rotate)) __s3a_rotate_body_end = .; . = ORIGIN(s3a_rotate) + LENGTH(s3a_rotate); } > s3a_rotate =0xf000f0e7
  .s3a_irotate : { KEEP(*(.text.s3a_irotate)) __s3a_irotate_body_end = .; . = ORIGIN(s3a_irotate) + LENGTH(s3a_irotate); } > s3a_irotate =0xf000f0e7
  .s3a_crotate : { KEEP(*(.text.s3a_crotate)) __s3a_crotate_body_end = .; . = ORIGIN(s3a_crotate) + LENGTH(s3a_crotate); } > s3a_crotate =0xf000f0e7
  .s3a_icrotate : { KEEP(*(.text.s3a_icrotate)) __s3a_icrotate_body_end = .; . = ORIGIN(s3a_icrotate) + LENGTH(s3a_icrotate); } > s3a_icrotate =0xf000f0e7
  .s3a_disable : { KEEP(*(.text.s3a_disable)) __s3a_disable_body_end = .; . = ORIGIN(s3a_disable) + LENGTH(s3a_disable); } > s3a_disable =0xf000f0e7
  .s3a_enable : { KEEP(*(.text.s3a_enable)) __s3a_enable_body_end = .; . = ORIGIN(s3a_enable) + LENGTH(s3a_enable); } > s3a_enable =0xf000f0e7
  .forbidden (NOLOAD) : { *(.rodata .rodata.* .data .data.* .bss .bss.* COMMON) } > s3a_exit
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}
ASSERT(xm1_s3a_exit_task == ORIGIN(s3a_exit), "exit entry moved")
ASSERT(__s3a_exit_body_end <= ORIGIN(s3a_exit) + LENGTH(s3a_exit), "exit extent exceeded")
ASSERT(xm1_s3a_rotate_ready == ORIGIN(s3a_rotate), "rotate entry moved")
ASSERT(__s3a_rotate_body_end <= ORIGIN(s3a_rotate) + LENGTH(s3a_rotate), "rotate extent exceeded")
ASSERT(xm1_s3a_interrupt_rotate_ready == ORIGIN(s3a_irotate), "irotate entry moved")
ASSERT(__s3a_irotate_body_end <= ORIGIN(s3a_irotate) + LENGTH(s3a_irotate), "irotate extent exceeded")
ASSERT(xm1_s3a_core_rotate_ready == ORIGIN(s3a_crotate), "crotate entry moved")
ASSERT(__s3a_crotate_body_end <= ORIGIN(s3a_crotate) + LENGTH(s3a_crotate), "crotate extent exceeded")
ASSERT(xm1_s3a_interrupt_core_rotate_ready == ORIGIN(s3a_icrotate), "icrotate entry moved")
ASSERT(__s3a_icrotate_body_end <= ORIGIN(s3a_icrotate) + LENGTH(s3a_icrotate), "icrotate extent exceeded")
ASSERT(xm1_s3a_disable_dispatch == ORIGIN(s3a_disable), "disable entry moved")
ASSERT(__s3a_disable_body_end <= ORIGIN(s3a_disable) + LENGTH(s3a_disable), "disable extent exceeded")
ASSERT(xm1_s3a_enable_dispatch == ORIGIN(s3a_enable), "enable entry moved")
ASSERT(__s3a_enable_body_end <= ORIGIN(s3a_enable) + LENGTH(s3a_enable), "enable extent exceeded")
ASSERT(SIZEOF(.forbidden) == 0, "data present")
