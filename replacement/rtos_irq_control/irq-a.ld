ENTRY(xm1_rtos_irq_enable)

MEMORY {
  irq_enable (rx) : ORIGIN = 0x41108, LENGTH = 4
  irq_disable (rx) : ORIGIN = 0x41124, LENGTH = 4
  irq_define (rx) : ORIGIN = 0x4d8fc, LENGTH = 4
  irq_body (rx) : ORIGIN = 0x50000, LENGTH = 0x2000
}

SECTIONS {
  .irq_enable : {
    KEEP(*(.text.irq_enable))
  } > irq_enable
  .irq_disable : {
    KEEP(*(.text.irq_disable))
  } > irq_disable
  .irq_define : {
    KEEP(*(.text.irq_define))
  } > irq_define
  .irq_body : {
    KEEP(*(.text.irq_body_*))
    *(.text.*)
    *(.rodata .rodata.*)
    __irq_body_end = .;
    . = ORIGIN(irq_body) + LENGTH(irq_body);
  } > irq_body =0xf000f0e7
  .forbidden (NOLOAD) : { *(.data .data.* .bss .bss.* COMMON) } > irq_body
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}

ASSERT(xm1_rtos_irq_enable == 0x41108, "enable entry moved")
ASSERT(xm1_rtos_irq_disable == 0x41124, "disable entry moved")
ASSERT(xm1_rtos_irq_define == 0x4d8fc, "define entry moved")
ASSERT(SIZEOF(.irq_enable) == 4 && SIZEOF(.irq_disable) == 4 && SIZEOF(.irq_define) == 4, "entry veneer size differs")
ASSERT(__irq_body_end <= ORIGIN(irq_body) + LENGTH(irq_body), "irq body exceeds scratch")
ASSERT(SIZEOF(.forbidden) == 0, "unplaced writable data")
