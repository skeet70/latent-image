ENTRY(xm1_rtos_kernel_start)

/* Loader B kernel startup extents: a 4-byte branch at each stock entry to its bank body,
   traps elsewhere. All bodies are in OWNED_FIXED. */
MEMORY {
  start (rx) : ORIGIN = 0xd0430, LENGTH = 0x6d0
  tick (rx) : ORIGIN = 0xd0b5c, LENGTH = 0xfc
  core (rx) : ORIGIN = 0xd3a0c, LENGTH = 0x1f8
  tasks (rx) : ORIGIN = 0xd3dd4, LENGTH = 0x158
  autostart (rx) : ORIGIN = 0xd3f58, LENGTH = 0x7c
  semaphores (rx) : ORIGIN = 0xd7540, LENGTH = 0x84
  event_flags (rx) : ORIGIN = 0xd8004, LENGTH = 0x88
  data_queues (rx) : ORIGIN = 0xd8df8, LENGTH = 0xa4
  mailboxes (rx) : ORIGIN = 0xda97c, LENGTH = 0x88
  fixed_pools (rx) : ORIGIN = 0xdb34c, LENGTH = 0x90
  objects (rx) : ORIGIN = 0xdce60, LENGTH = 0x1f4
  INCLUDE fixed-memory.ld
}

SECTIONS {
  .kernel_start : {
    KEEP(*(.text.kernel_veneer_kernel_publish_interfaces))
    . = 0xd09ac;
    KEEP(*(.text.kernel_veneer_kernel_start))
    . = 0xd0b00;
  } > start =0xf000f0e7
  .kernel_tick : {
    KEEP(*(.text.kernel_veneer_kernel_start_tick_timer))
    . = 0xd0c58;
  } > tick =0xf000f0e7
  .kernel_core : {
    KEEP(*(.text.kernel_veneer_kernel_configure_core))
    . = 0xd3a7c;
    KEEP(*(.text.kernel_veneer_kernel_initialize_owner_contexts))
    . = 0xd3c04;
  } > core =0xf000f0e7
  .kernel_tasks : {
    KEEP(*(.text.kernel_veneer_kernel_initialize_tasks))
    . = 0xd3f2c;
  } > tasks =0xf000f0e7
  .kernel_autostart : {
    KEEP(*(.text.kernel_veneer_kernel_autostart_tasks))
    . = 0xd3fd4;
  } > autostart =0xf000f0e7
  .kernel_semaphores : {
    KEEP(*(.text.kernel_veneer_kernel_initialize_semaphores))
    . = 0xd75c4;
  } > semaphores =0xf000f0e7
  .kernel_event_flags : {
    KEEP(*(.text.kernel_veneer_kernel_initialize_event_flags))
    . = 0xd808c;
  } > event_flags =0xf000f0e7
  .kernel_data_queues : {
    KEEP(*(.text.kernel_veneer_kernel_initialize_data_queues))
    . = 0xd8e9c;
  } > data_queues =0xf000f0e7
  .kernel_mailboxes : {
    KEEP(*(.text.kernel_veneer_kernel_initialize_mailboxes))
    . = 0xdaa04;
  } > mailboxes =0xf000f0e7
  .kernel_fixed_pools : {
    KEEP(*(.text.kernel_veneer_kernel_initialize_fixed_pools))
    . = 0xdb3dc;
  } > fixed_pools =0xf000f0e7
  .kernel_objects : {
    KEEP(*(.text.kernel_veneer_kernel_initialize_objects))
    . = 0xdd054;
  } > objects =0xf000f0e7
  .bank : {
    __kernel_bank_start = .;
    KEEP(*(.text.kernel_body_*))
    *(.text .text.*)
    *(.rodata .rodata.*)
    . = ALIGN(4);
    __kernel_bank_end = .;
  } > OWNED_FIXED
  .forbidden (NOLOAD) : { *(.data .data.* .bss .bss.* COMMON) } > OWNED_FIXED
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}

ASSERT(xm1_rtos_kernel_publish_interfaces == 0xd0430, "kernel_publish_interfaces veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_start == 0xd09ac, "kernel_start veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_start_tick_timer == 0xd0b5c, "kernel_start_tick_timer veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_configure_core == 0xd3a0c, "kernel_configure_core veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_initialize_owner_contexts == 0xd3a7c, "kernel_initialize_owner_contexts veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_initialize_tasks == 0xd3dd4, "kernel_initialize_tasks veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_autostart_tasks == 0xd3f58, "kernel_autostart_tasks veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_initialize_semaphores == 0xd7540, "kernel_initialize_semaphores veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_initialize_event_flags == 0xd8004, "kernel_initialize_event_flags veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_initialize_data_queues == 0xd8df8, "kernel_initialize_data_queues veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_initialize_mailboxes == 0xda97c, "kernel_initialize_mailboxes veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_initialize_fixed_pools == 0xdb34c, "kernel_initialize_fixed_pools veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_initialize_objects == 0xdce60, "kernel_initialize_objects veneer is not at its stock entry")
ASSERT(SIZEOF(.kernel_start) == 0x6d0 && SIZEOF(.kernel_tick) == 0xfc && SIZEOF(.kernel_core) == 0x1f8 && SIZEOF(.kernel_tasks) == 0x158 && SIZEOF(.kernel_autostart) == 0x7c, "retained extent size differs")
ASSERT(SIZEOF(.kernel_semaphores) == 0x84 && SIZEOF(.kernel_event_flags) == 0x88 && SIZEOF(.kernel_data_queues) == 0xa4 && SIZEOF(.kernel_mailboxes) == 0x88 && SIZEOF(.kernel_fixed_pools) == 0x90 && SIZEOF(.kernel_objects) == 0x1f4, "retained extent size differs")
ASSERT(__kernel_bank_start == ORIGIN(OWNED_FIXED), "bank body moved")
ASSERT(__kernel_bank_end <= ORIGIN(OWNED_FIXED) + LENGTH(OWNED_FIXED), "bank exceeds the fixed reservation")
ASSERT(SIZEOF(.forbidden) == 0, "writable data")
