ENTRY(xm1_s3b_activate)
MEMORY {
 activate (rx) : ORIGIN = 0x44f2c, LENGTH = 0x1c4
 interrupt_activate (rx) : ORIGIN = 0x450f0, LENGTH = 0x180
 cancel_activation (rx) : ORIGIN = 0x45270, LENGTH = 0x180
 wake (rx) : ORIGIN = 0x46004, LENGTH = 0x1fc
 interrupt_wake (rx) : ORIGIN = 0x46200, LENGTH = 0x198
 cancel_wake (rx) : ORIGIN = 0x46398, LENGTH = 0x19c
}
s3b_prepare = 0x43f70;
s3b_release_wait = 0x44568;
s3b_notify = 0x44018;
s3b_switch = 0x43640;
SECTIONS {
 .activate : { KEEP(*(.text.s3b_activate)) __s3b_activate_body_end = .; . = ORIGIN(activate) + LENGTH(activate); } > activate =0xf000f0e7
 .interrupt_activate : { KEEP(*(.text.s3b_interrupt_activate)) __s3b_interrupt_activate_body_end = .; . = ORIGIN(interrupt_activate) + LENGTH(interrupt_activate); } > interrupt_activate =0xf000f0e7
 .cancel_activation : { KEEP(*(.text.s3b_cancel_activation)) __s3b_cancel_activation_body_end = .; . = ORIGIN(cancel_activation) + LENGTH(cancel_activation); } > cancel_activation =0xf000f0e7
 .wake : { KEEP(*(.text.s3b_wake)) __s3b_wake_body_end = .; . = ORIGIN(wake) + LENGTH(wake); } > wake =0xf000f0e7
 .interrupt_wake : { KEEP(*(.text.s3b_interrupt_wake)) __s3b_interrupt_wake_body_end = .; . = ORIGIN(interrupt_wake) + LENGTH(interrupt_wake); } > interrupt_wake =0xf000f0e7
 .cancel_wake : { KEEP(*(.text.s3b_cancel_wake)) __s3b_cancel_wake_body_end = .; . = ORIGIN(cancel_wake) + LENGTH(cancel_wake); } > cancel_wake =0xf000f0e7
 .forbidden (NOLOAD) : { *(.rodata .rodata.* .data .data.* .bss .bss.* COMMON) } > activate
 .ARM.attributes 0 : { *(.ARM.attributes) }
 .symtab 0 : { *(.symtab) }
 .shstrtab 0 : { *(.shstrtab) }
 .strtab 0 : { *(.strtab) }
 /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}
ASSERT(SIZEOF(.forbidden) == 0, "unexpected data")
ASSERT(xm1_s3b_activate == ORIGIN(activate), "activation entry moved")
ASSERT(xm1_s3b_interrupt_activate == ORIGIN(interrupt_activate), "handler activation entry moved")
ASSERT(xm1_s3b_cancel_activation == ORIGIN(cancel_activation), "cancel activation entry moved")
ASSERT(xm1_s3b_wake == ORIGIN(wake), "wake entry moved")
ASSERT(xm1_s3b_interrupt_wake == ORIGIN(interrupt_wake), "handler wake entry moved")
ASSERT(xm1_s3b_cancel_wake == ORIGIN(cancel_wake), "cancel wake entry moved")
