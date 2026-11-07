/* Loader B, F2: the owned wake bodies at their stock extents over the owned scheduler
   regions. Helper text must inline into the two entries; the scheduler script forbids
   data and rodata, so the adapter keeps no statics. */
INCLUDE scheduler-b-placement.ld
INCLUDE scheduler-s2-b-placement.ld

MEMORY {
  f2_set_flag (rx) : ORIGIN = 0xd80fc, LENGTH = 0x260
  f2_send (rx) : ORIGIN = 0xdaa04, LENGTH = 0x304
}

xm1_rtos_objects_stock_remove_timer = 0xd4c68;

SECTIONS {
  .f2_set_flag : {
    KEEP(*(.text.f2_set_flag))
    __f2_set_flag_body_end = .;
    . = ORIGIN(f2_set_flag) + LENGTH(f2_set_flag);
  } > f2_set_flag =0xf000f0e7
  .f2_send : {
    KEEP(*(.text.f2_send))
    __f2_send_body_end = .;
    . = ORIGIN(f2_send) + LENGTH(f2_send);
  } > f2_send =0xf000f0e7
  .forbidden (NOLOAD) : { *(.rodata .rodata.* .data .data.* .bss .bss.* COMMON) } > acquire
}

ASSERT(xm1_rtos_objects_f2_set_flag == ORIGIN(f2_set_flag), "set_flag entry is not at its stock address")
ASSERT(__f2_set_flag_body_end <= ORIGIN(f2_set_flag) + LENGTH(f2_set_flag), "set_flag body exceeds its stock extent")
ASSERT(xm1_rtos_objects_f2_send == ORIGIN(f2_send), "send entry is not at its stock address")
ASSERT(__f2_send_body_end <= ORIGIN(f2_send) + LENGTH(f2_send), "send body exceeds its stock extent")
