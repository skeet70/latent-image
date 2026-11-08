ENTRY(s3d_terminate)
MEMORY {
  terminate_entry (rx) : ORIGIN = 0x454f0, LENGTH = 0x04
  change_priority_entry (rx) : ORIGIN = 0x4582c, LENGTH = 0x04
  timed_sleep_entry (rx) : ORIGIN = 0x45e54, LENGTH = 0x04
  release_wait_entry (rx) : ORIGIN = 0x46534, LENGTH = 0x04
  interrupt_release_wait_entry (rx) : ORIGIN = 0x467b4, LENGTH = 0x04
  delay_entry (rx) : ORIGIN = 0x46e48, LENGTH = 0x04
  delay_body (rx) : ORIGIN = 0x46e4c, LENGTH = 0x168
  bodies (rx) : ORIGIN = 0x50000, LENGTH = 0x2000
}
s3d_remove_ready = 0x43d20;
s3d_insert_ready = 0x43ca4;
s3d_reset_task = 0x43dd0;
s3d_restart_task = 0x43f70;
s3d_notify_dispatch = 0x44018;
s3d_switch = 0x43640;
s3d_timer_insert = 0x44c60;
s3d_timer_remove = 0x44ca8;
SECTIONS {
  .s3d_terminate_entry : { KEEP(*(.text.s3d_terminate_entry)) } > terminate_entry =0xf000f0e7
  .s3d_change_priority_entry : { KEEP(*(.text.s3d_change_priority_entry)) } > change_priority_entry =0xf000f0e7
  .s3d_timed_sleep_entry : { KEEP(*(.text.s3d_timed_sleep_entry)) } > timed_sleep_entry =0xf000f0e7
  .s3d_release_wait_entry : { KEEP(*(.text.s3d_release_wait_entry)) } > release_wait_entry =0xf000f0e7
  .s3d_interrupt_release_wait_entry : { KEEP(*(.text.s3d_interrupt_release_wait_entry)) } > interrupt_release_wait_entry =0xf000f0e7
  .s3d_delay_entry : { KEEP(*(.text.s3d_delay_entry)) } > delay_entry =0xf000f0e7
  .s3d_delay_body : { KEEP(*(.text.s3d_delay_body)) __s3d_delay_body_end = .; } > delay_body =0xf000f0e7
  .s3d_bodies : {
    KEEP(*(.text.s3d_terminate_body))
    __s3d_terminate_body_end = .;
    KEEP(*(.text.s3d_change_priority_body))
    __s3d_change_priority_body_end = .;
    KEEP(*(.text.s3d_timed_sleep_body))
    __s3d_timed_sleep_body_end = .;
    KEEP(*(.text.s3d_release_wait_body))
    __s3d_release_wait_body_end = .;
    KEEP(*(.text.s3d_interrupt_release_wait_body))
    __s3d_interrupt_release_wait_body_end = .;
    KEEP(*(.text.s3d_timer_active))
    __s3d_timer_active_end = .;
    . = ORIGIN(bodies) + LENGTH(bodies);
  } > bodies =0xf000f0e7
  .forbidden (NOLOAD) : { *(.text .text.* .rodata .rodata.* .data .data.* .bss .bss.* COMMON) } > bodies
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}
ASSERT(s3d_terminate == ORIGIN(terminate_entry), "terminate entry moved")
ASSERT(s3d_change_priority == ORIGIN(change_priority_entry), "priority entry moved")
ASSERT(s3d_timed_sleep == ORIGIN(timed_sleep_entry), "timed sleep entry moved")
ASSERT(s3d_release_wait == ORIGIN(release_wait_entry), "release entry moved")
ASSERT(s3d_interrupt_release_wait == ORIGIN(interrupt_release_wait_entry), "handler release entry moved")
ASSERT(s3d_delay == ORIGIN(delay_entry), "delay entry moved")
ASSERT(s3d_delay_body == ORIGIN(delay_body), "delay body is not grounded after its entry")
ASSERT(__s3d_delay_body_end <= ORIGIN(delay_body) + LENGTH(delay_body), "delay body overflow")
ASSERT(s3d_terminate_body == ORIGIN(bodies), "terminate body moved")
ASSERT(__s3d_terminate_body_end <= ORIGIN(bodies) + LENGTH(bodies), "terminate body overflow")
ASSERT(__s3d_change_priority_body_end <= ORIGIN(bodies) + LENGTH(bodies), "priority body overflow")
ASSERT(__s3d_timed_sleep_body_end <= ORIGIN(bodies) + LENGTH(bodies), "timed sleep body overflow")
ASSERT(__s3d_release_wait_body_end <= ORIGIN(bodies) + LENGTH(bodies), "release body overflow")
ASSERT(__s3d_interrupt_release_wait_body_end <= ORIGIN(bodies) + LENGTH(bodies), "handler release body overflow")
ASSERT(SIZEOF(.forbidden) == 0, "unplaced code or data")
