ENTRY(xm1_rtos_kernel_start)

/* Loader A kernel startup extents: a 4-byte branch at each stock entry to its bank body,
   traps elsewhere. All bodies are in OWNED_FIXED. */
MEMORY {
  start (rx) : ORIGIN = 0x40430, LENGTH = 0x6d0
  tick (rx) : ORIGIN = 0x40b5c, LENGTH = 0xfc
  core (rx) : ORIGIN = 0x43a58, LENGTH = 0x1f0
  tasks (rx) : ORIGIN = 0x43e18, LENGTH = 0x158
  autostart (rx) : ORIGIN = 0x43f9c, LENGTH = 0x7c
  semaphores (rx) : ORIGIN = 0x47580, LENGTH = 0x84
  event_flags (rx) : ORIGIN = 0x48044, LENGTH = 0x88
  data_queues (rx) : ORIGIN = 0x48e38, LENGTH = 0xa4
  mailboxes (rx) : ORIGIN = 0x4a9bc, LENGTH = 0x88
  fixed_pools (rx) : ORIGIN = 0x4b38c, LENGTH = 0x90
  objects (rx) : ORIGIN = 0x4cea0, LENGTH = 0x1f4
  INCLUDE fixed-memory.ld
}

SECTIONS {
  .kernel_start : {
    KEEP(*(.text.kernel_veneer_kernel_publish_interfaces))
    . = 0x409ac;
    KEEP(*(.text.kernel_veneer_kernel_start))
    . = 0x40b00;
  } > start =0xf000f0e7
  .kernel_tick : {
    KEEP(*(.text.kernel_veneer_kernel_start_tick_timer))
    . = 0x40c58;
  } > tick =0xf000f0e7
  .kernel_core : {
    KEEP(*(.text.kernel_veneer_kernel_configure_core))
    . = 0x43ac0;
    KEEP(*(.text.kernel_veneer_kernel_initialize_owner_contexts))
    . = 0x43c48;
  } > core =0xf000f0e7
  .kernel_tasks : {
    KEEP(*(.text.kernel_veneer_kernel_initialize_tasks))
    . = 0x43f70;
  } > tasks =0xf000f0e7
  .kernel_autostart : {
    KEEP(*(.text.kernel_veneer_kernel_autostart_tasks))
    . = 0x44018;
  } > autostart =0xf000f0e7
  .kernel_semaphores : {
    KEEP(*(.text.kernel_veneer_kernel_initialize_semaphores))
    . = 0x47604;
  } > semaphores =0xf000f0e7
  .kernel_event_flags : {
    KEEP(*(.text.kernel_veneer_kernel_initialize_event_flags))
    . = 0x480cc;
  } > event_flags =0xf000f0e7
  .kernel_data_queues : {
    KEEP(*(.text.kernel_veneer_kernel_initialize_data_queues))
    . = 0x48edc;
  } > data_queues =0xf000f0e7
  .kernel_mailboxes : {
    KEEP(*(.text.kernel_veneer_kernel_initialize_mailboxes))
    . = 0x4aa44;
  } > mailboxes =0xf000f0e7
  .kernel_fixed_pools : {
    KEEP(*(.text.kernel_veneer_kernel_initialize_fixed_pools))
    . = 0x4b41c;
  } > fixed_pools =0xf000f0e7
  .kernel_objects : {
    KEEP(*(.text.kernel_veneer_kernel_initialize_objects))
    . = 0x4d094;
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

ASSERT(xm1_rtos_kernel_publish_interfaces == 0x40430, "kernel_publish_interfaces veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_start == 0x409ac, "kernel_start veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_start_tick_timer == 0x40b5c, "kernel_start_tick_timer veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_configure_core == 0x43a58, "kernel_configure_core veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_initialize_owner_contexts == 0x43ac0, "kernel_initialize_owner_contexts veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_initialize_tasks == 0x43e18, "kernel_initialize_tasks veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_autostart_tasks == 0x43f9c, "kernel_autostart_tasks veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_initialize_semaphores == 0x47580, "kernel_initialize_semaphores veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_initialize_event_flags == 0x48044, "kernel_initialize_event_flags veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_initialize_data_queues == 0x48e38, "kernel_initialize_data_queues veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_initialize_mailboxes == 0x4a9bc, "kernel_initialize_mailboxes veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_initialize_fixed_pools == 0x4b38c, "kernel_initialize_fixed_pools veneer is not at its stock entry")
ASSERT(xm1_rtos_kernel_initialize_objects == 0x4cea0, "kernel_initialize_objects veneer is not at its stock entry")
ASSERT(SIZEOF(.kernel_start) == 0x6d0 && SIZEOF(.kernel_tick) == 0xfc && SIZEOF(.kernel_core) == 0x1f0 && SIZEOF(.kernel_tasks) == 0x158 && SIZEOF(.kernel_autostart) == 0x7c, "retained extent size differs")
ASSERT(SIZEOF(.kernel_semaphores) == 0x84 && SIZEOF(.kernel_event_flags) == 0x88 && SIZEOF(.kernel_data_queues) == 0xa4 && SIZEOF(.kernel_mailboxes) == 0x88 && SIZEOF(.kernel_fixed_pools) == 0x90 && SIZEOF(.kernel_objects) == 0x1f4, "retained extent size differs")
ASSERT(__kernel_bank_start == ORIGIN(OWNED_FIXED), "bank body moved")
ASSERT(__kernel_bank_end <= ORIGIN(OWNED_FIXED) + LENGTH(OWNED_FIXED), "bank exceeds the fixed reservation")
ASSERT(SIZEOF(.forbidden) == 0, "writable data")
