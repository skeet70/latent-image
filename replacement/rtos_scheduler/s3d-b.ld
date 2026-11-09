ENTRY(s3d_terminate)
MEMORY {
  terminate_entry (rx) : ORIGIN = 0xd54b0, LENGTH = 0x04
  change_priority_entry (rx) : ORIGIN = 0xd57ec, LENGTH = 0x04
  timed_sleep_entry (rx) : ORIGIN = 0xd5e14, LENGTH = 0x04
  release_wait_entry (rx) : ORIGIN = 0xd64f4, LENGTH = 0x04
  interrupt_release_wait_entry (rx) : ORIGIN = 0xd6774, LENGTH = 0x04
  delay_entry (rx) : ORIGIN = 0xd6e08, LENGTH = 0x04
  delay_body (rx) : ORIGIN = 0xd6e0c, LENGTH = 0x180
  timeout_entry (rx) : ORIGIN = 0xd42d8, LENGTH = 0x04
  delay_completion_entry (rx) : ORIGIN = 0xd45f4, LENGTH = 0x04
  bodies (rx) : ORIGIN = 0xe0000, LENGTH = 0x4000
}
s3d_remove_ready = 0xd3cdc;
s3d_insert_ready = 0xd3c60;
s3d_reset_task = 0xd3d8c;
s3d_restart_task = 0xd3f2c;
s3d_notify_dispatch = 0xd3fd4;
s3d_switch = 0xd3640;
s3d_timer_insert = 0xd4c20;
s3d_timer_remove = 0xd4c68;
SECTIONS {
  .s3d_terminate_entry : { KEEP(*(.text.s3d_terminate_entry)) } > terminate_entry =0xf000f0e7
  .s3d_change_priority_entry : { KEEP(*(.text.s3d_change_priority_entry)) } > change_priority_entry =0xf000f0e7
  .s3d_timed_sleep_entry : { KEEP(*(.text.s3d_timed_sleep_entry)) } > timed_sleep_entry =0xf000f0e7
  .s3d_release_wait_entry : { KEEP(*(.text.s3d_release_wait_entry)) } > release_wait_entry =0xf000f0e7
  .s3d_interrupt_release_wait_entry : { KEEP(*(.text.s3d_interrupt_release_wait_entry)) } > interrupt_release_wait_entry =0xf000f0e7
  .s3d_delay_entry : { KEEP(*(.text.s3d_delay_entry)) } > delay_entry =0xf000f0e7
  .s3d_delay_body : { KEEP(*(.text.s3d_delay_body)) __s3d_delay_body_end = .; } > delay_body =0xf000f0e7
  .scheduler_timeout_entry : { KEEP(*(.text.scheduler_timeout_entry)) } > timeout_entry =0xf000f0e7
  .scheduler_delay_completion_entry : { KEEP(*(.text.scheduler_delay_completion_entry)) } > delay_completion_entry =0xf000f0e7
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
    KEEP(*(.text.*release_waiting*))
    __s3d_release_waiting_end = .;
    KEEP(*(.text.*release_queued*))
    __s3d_release_queued_end = .;
    KEEP(*(.text.*release*))
    KEEP(*(.text.s3d_release_core))
    __s3d_release_core_end = .;
    KEEP(*(.text.scheduler_timeout_body))
    __scheduler_timeout_body_end = .;
    KEEP(*(.text.scheduler_delay_completion_body))
    __scheduler_delay_completion_body_end = .;
    KEEP(*(.text.scheduler_timeout_core))
    __scheduler_timeout_core_end = .;
    KEEP(*(.text))
    KEEP(*(.rodata .rodata.*))
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
ASSERT(xm1_rtos_scheduler_timeout == ORIGIN(timeout_entry), "timeout callback entry moved")
ASSERT(xm1_rtos_scheduler_delay_completion == ORIGIN(delay_completion_entry), "delay callback entry moved")
ASSERT(s3d_delay_body == ORIGIN(delay_body), "delay body is not grounded after its entry")
ASSERT(__s3d_delay_body_end <= ORIGIN(delay_body) + LENGTH(delay_body), "delay body overflow")
ASSERT(s3d_terminate_body == ORIGIN(bodies), "terminate body moved")
ASSERT(__s3d_terminate_body_end <= ORIGIN(bodies) + LENGTH(bodies), "terminate body overflow")
ASSERT(__s3d_change_priority_body_end <= ORIGIN(bodies) + LENGTH(bodies), "priority body overflow")
ASSERT(__s3d_timed_sleep_body_end <= ORIGIN(bodies) + LENGTH(bodies), "timed sleep body overflow")
ASSERT(__s3d_release_wait_body_end <= ORIGIN(bodies) + LENGTH(bodies), "release body overflow")
ASSERT(__s3d_interrupt_release_wait_body_end <= ORIGIN(bodies) + LENGTH(bodies), "handler release body overflow")
ASSERT(__scheduler_timeout_body_end <= ORIGIN(bodies) + LENGTH(bodies), "timeout body overflow")
ASSERT(__scheduler_delay_completion_body_end <= ORIGIN(bodies) + LENGTH(bodies), "delay completion body overflow")
ASSERT(__scheduler_timeout_core_end <= ORIGIN(bodies) + LENGTH(bodies), "timeout core overflow")
ASSERT(SIZEOF(.forbidden) == 0, "unplaced code or data")
