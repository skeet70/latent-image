INCLUDE scheduler-a-placement.ld
INCLUDE scheduler-s2-a-placement.ld

MEMORY {
  f2_release (rx) : ORIGIN = 0x44568, LENGTH = 0xcc
  f2_set_flag (rx) : ORIGIN = 0x4813c, LENGTH = 0x260
  f2_send (rx) : ORIGIN = 0x4aa44, LENGTH = 0x304
}

xm1_rtos_objects_stock_remove_timer = 0x44ca8;

SECTIONS {
  .f2_release : {
    KEEP(*(.text.f2_release))
    __f2_release_body_end = .;
    . = ORIGIN(f2_release) + LENGTH(f2_release);
  } > f2_release =0xf000f0e7
  .f2_set_flag : {
    KEEP(*(.text.f2_set_flag))
    __f2_set_flag_body_end = .;
    KEEP(*(.rodata..Lswitch.table.xm1_rtos_objects_f2_set_flag))
    . = ORIGIN(f2_set_flag) + LENGTH(f2_set_flag);
  } > f2_set_flag =0xf000f0e7
  .f2_send : {
    KEEP(*(.text.f2_send))
    __f2_send_body_end = .;
    KEEP(*(.text._RNvNtNtCs2rtKTofzijI_27xm1_rtos_objects_f2_adapter7objects2id6target))
    KEEP(*(.text._RNvXs1_Cs2rtKTofzijI_27xm1_rtos_objects_f2_adapterNtB5_3CpuNtNtNtB5_7objects4wake4Wake13waiter_domain))
    KEEP(*(.rodata.str1.1))
    KEEP(*(.rodata..Lanon.11dcd14709f36921fe42ceb8bc821f5b.1))
    KEEP(*(.rodata..Lanon.11dcd14709f36921fe42ceb8bc821f5b.2))
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
