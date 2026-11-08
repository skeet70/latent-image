ENTRY(xm1_rtos_semaphore_signal)
MEMORY {
  sem_service (rx) : ORIGIN = 0x47604, LENGTH = 0xa40
  sem_config (rx) : ORIGIN = 0x4d2a8, LENGTH = 0xf4
}
xm1_semaphore_stock_acquire = 0x439d8;
xm1_semaphore_stock_release = 0x43a14;
xm1_semaphore_stock_notify = 0x44018;
xm1_semaphore_stock_dispatch = 0x43640;
xm1_semaphore_stock_mask = 0x42560;
xm1_semaphore_stock_restore = 0x42568;
xm1_semaphore_stock_wait_release = 0x44568;
xm1_semaphore_stock_wait_cancel = 0x44744;
xm1_semaphore_stock_wait_enqueue = 0x44800;
xm1_semaphore_stock_wait_enqueue_timed = 0x448e4;
SECTIONS {
  .sem_signal : {
    KEEP(*(.text.sem_signal))
    KEEP(*(.text.sem_signal_body))
    __sem_signal_end = .;
    . = ORIGIN(sem_service) + 0x240;
  } > sem_service =0xf000f0e7
  .sem_isignal : {
    KEEP(*(.text.sem_isignal))
    KEEP(*(.text.sem_admit))
    __sem_isignal_end = .;
    . = ORIGIN(sem_service) + 0x450;
  } > sem_service =0xf000f0e7
  .sem_wait : {
    KEEP(*(.text.sem_wait))
    KEEP(*(.text.sem_delete_body))
    KEEP(*(.text.sem_create_body))
    __sem_wait_end = .;
    . = ORIGIN(sem_service) + 0x690;
  } > sem_service =0xf000f0e7
  .sem_poll : {
    KEEP(*(.text.sem_poll))
    __sem_poll_end = .;
    . = ORIGIN(sem_service) + 0x7cc;
  } > sem_service =0xf000f0e7
  .sem_timed_wait : {
    KEEP(*(.text.sem_timed_wait))
    KEEP(*(.text.sem_block_body))
    __sem_timed_wait_end = .;
    . = ORIGIN(sem_service) + 0xa40;
  } > sem_service =0xf000f0e7
  .sem_create : { KEEP(*(.text.sem_create)) __sem_create_end = .; . = ORIGIN(sem_config) + 0x90; } > sem_config =0xf000f0e7
  .sem_delete : { KEEP(*(.text.sem_delete)) __sem_delete_end = .; . = ORIGIN(sem_config) + 0xf4; } > sem_config =0xf000f0e7
  .forbidden (NOLOAD) : { *(.text .text.* .rodata .rodata.* .data .data.* .bss .bss.* COMMON) } > sem_service
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}
ASSERT(xm1_rtos_semaphore_signal == ORIGIN(sem_service), "signal entry is not at its stock address")
ASSERT(__sem_signal_end <= ORIGIN(sem_service) + 0x240, "signal entry or body exceeds its stock extent")
ASSERT(xm1_rtos_semaphore_interrupt_signal == ORIGIN(sem_service) + 0x240, "interrupt signal entry moved")
ASSERT(__sem_isignal_end <= ORIGIN(sem_service) + 0x450, "interrupt signal entry, admit or body exceeds its extent")
ASSERT(xm1_rtos_semaphore_wait == ORIGIN(sem_service) + 0x450, "wait entry moved")
ASSERT(__sem_wait_end <= ORIGIN(sem_service) + 0x690, "wait entry, create or delete body exceeds its extent")
ASSERT(xm1_rtos_semaphore_poll == ORIGIN(sem_service) + 0x690, "poll entry moved")
ASSERT(__sem_poll_end <= ORIGIN(sem_service) + 0x7cc, "poll entry exceeds its stock extent")
ASSERT(xm1_rtos_semaphore_timed_wait == ORIGIN(sem_service) + 0x7cc, "timed wait entry moved")
ASSERT(__sem_timed_wait_end <= ORIGIN(sem_service) + 0xa40, "timed wait entry or block body exceeds its extent")
ASSERT(xm1_rtos_semaphore_create == ORIGIN(sem_config), "create entry is not at its stock address")
ASSERT(__sem_create_end <= ORIGIN(sem_config) + 0x90, "create body exceeds its stock extent")
ASSERT(xm1_rtos_semaphore_delete == ORIGIN(sem_config) + 0x90, "delete entry moved")
ASSERT(__sem_delete_end <= ORIGIN(sem_config) + 0xf4, "delete entry exceeds its stock extent")
ASSERT(SIZEOF(.forbidden) == 0, "unplaced text or data remains")
