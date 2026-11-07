INCLUDE scheduler-b-placement.ld
INCLUDE scheduler-s2-b-placement.ld

MEMORY {
  f2_release (rx) : ORIGIN = 0xd4528, LENGTH = 0xcc
  f2_set_flag (rx) : ORIGIN = 0xd80fc, LENGTH = 0x260
  f2_send (rx) : ORIGIN = 0xdaa04, LENGTH = 0x304
}

xm1_rtos_objects_stock_remove_timer = 0xd4c68;

SECTIONS {
  .f2_release : {
    KEEP(*(.text.f2_release))
    __f2_release_body_end = .;
    . = ORIGIN(f2_release) + LENGTH(f2_release);
  } > f2_release =0xf000f0e7
  .f2_set_flag : {
    KEEP(*(.text.f2_set_flag))
    __f2_set_flag_body_end = .;
    *(.rodata..Lswitch.table.xm1_rtos_objects_f2_set_flag)
    . = ORIGIN(f2_set_flag) + LENGTH(f2_set_flag);
  } > f2_set_flag =0xf000f0e7
  .f2_send : {
    KEEP(*(.text.f2_send))
    __f2_send_body_end = .;
    *(.text.*objects2id6target)
    *(.text.*objects4wake4Wake13waiter_domain)
    *(.rodata.str1.1)
    *(.rodata..Lanon.*)
    . = ORIGIN(f2_send) + LENGTH(f2_send);
  } > f2_send =0xf000f0e7
  .forbidden (NOLOAD) : { *(.text .text.* .rodata .rodata.* .data .data.* .bss .bss.* COMMON) } > acquire
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}

ASSERT(xm1_rtos_objects_release == ORIGIN(f2_release), "release entry is not at its stock address")
ASSERT(__f2_release_body_end <= ORIGIN(f2_release) + LENGTH(f2_release), "release body exceeds its stock extent")
ASSERT(SIZEOF(.forbidden) == 0, "unplaced text or data remains")
ASSERT(xm1_rtos_objects_f2_set_flag == ORIGIN(f2_set_flag), "set_flag entry is not at its stock address")
ASSERT(__f2_set_flag_body_end <= ORIGIN(f2_set_flag) + LENGTH(f2_set_flag), "set_flag body exceeds its stock extent")
ASSERT(xm1_rtos_objects_f2_send == ORIGIN(f2_send), "send entry is not at its stock address")
ASSERT(__f2_send_body_end <= ORIGIN(f2_send) + LENGTH(f2_send), "send body exceeds its stock extent")
