ENTRY(xm1_s3a_exit_task)
MEMORY {
  s3a_exit (rx) : ORIGIN = 0xd53b0, LENGTH = 0x100
  s3a_rotate (rx) : ORIGIN = 0xdc24c, LENGTH = 0xe0
  s3a_irotate (rx) : ORIGIN = 0xdc32c, LENGTH = 0xc8
  s3a_crotate (rx) : ORIGIN = 0xdc3f4, LENGTH = 0xfc
  s3a_icrotate (rx) : ORIGIN = 0xdc4f0, LENGTH = 0xf0
  s3a_disable (rx) : ORIGIN = 0xdc99c, LENGTH = 0xe4
  s3a_enable (rx) : ORIGIN = 0xdca80, LENGTH = 0x118
}
s3a_stock_remove = 0xd3cdc;
s3a_stock_reset = 0xd3d8c;
s3a_stock_prepare = 0xd3f2c;
s3a_stock_rotate = 0xd40f4;
s3a_stock_notify = 0xd3fd4;
s3a_stock_save = 0xd3640;
s3a_stock_discard = 0xd3688;
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
