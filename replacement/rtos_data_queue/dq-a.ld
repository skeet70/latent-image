/* Loader A: data-queue entry thunks at their stock addresses; bodies in scratch. */
INCLUDE scheduler-a-placement.ld
INCLUDE scheduler-s2-a-placement.ld

MEMORY {
  dq_create (rx) : ORIGIN = 0x4d49c, LENGTH = 0xa4
  dq_delete (rx) : ORIGIN = 0x4d540, LENGTH = 0x7c
  dq_send (rx) : ORIGIN = 0x48ff8, LENGTH = 0x310
  dq_send_poll (rx) : ORIGIN = 0x49308, LENGTH = 0x220
  dq_send_poll_handler (rx) : ORIGIN = 0x49528, LENGTH = 0x1f0
  dq_send_timeout (rx) : ORIGIN = 0x49718, LENGTH = 0x3d4
  dq_force_send (rx) : ORIGIN = 0x49aec, LENGTH = 0x240
  dq_force_send_handler (rx) : ORIGIN = 0x49d2c, LENGTH = 0x200
  dq_receive (rx) : ORIGIN = 0x49f2c, LENGTH = 0x3c4
  dq_receive_poll (rx) : ORIGIN = 0x4a2f0, LENGTH = 0x27c
  dq_receive_timeout (rx) : ORIGIN = 0x4a56c, LENGTH = 0x450
  dq_bodies (rx) : ORIGIN = 0x50000, LENGTH = 0x10000
}

xm1_rtos_data_queue_stock_insert_timer = 0x44c60;
xm1_rtos_data_queue_stock_remove_timer = 0x44ca8;

SECTIONS {
  .dq_create : { KEEP(*(.text.dq_create)) __dq_create_end = .; . = ORIGIN(dq_create) + LENGTH(dq_create); } > dq_create =0xf000f0e7
  .dq_delete : { KEEP(*(.text.dq_delete)) __dq_delete_end = .; . = ORIGIN(dq_delete) + LENGTH(dq_delete); } > dq_delete =0xf000f0e7
  .dq_send : { KEEP(*(.text.dq_send)) __dq_send_end = .; . = ORIGIN(dq_send) + LENGTH(dq_send); } > dq_send =0xf000f0e7
  .dq_send_poll : { KEEP(*(.text.dq_send_poll)) __dq_send_poll_end = .; . = ORIGIN(dq_send_poll) + LENGTH(dq_send_poll); } > dq_send_poll =0xf000f0e7
  .dq_send_poll_handler : { KEEP(*(.text.dq_send_poll_handler)) __dq_send_poll_handler_end = .; . = ORIGIN(dq_send_poll_handler) + LENGTH(dq_send_poll_handler); } > dq_send_poll_handler =0xf000f0e7
  .dq_send_timeout : { KEEP(*(.text.dq_send_timeout)) __dq_send_timeout_end = .; . = ORIGIN(dq_send_timeout) + LENGTH(dq_send_timeout); } > dq_send_timeout =0xf000f0e7
  .dq_force_send : { KEEP(*(.text.dq_force_send)) __dq_force_send_end = .; . = ORIGIN(dq_force_send) + LENGTH(dq_force_send); } > dq_force_send =0xf000f0e7
  .dq_force_send_handler : { KEEP(*(.text.dq_force_send_handler)) __dq_force_send_handler_end = .; . = ORIGIN(dq_force_send_handler) + LENGTH(dq_force_send_handler); } > dq_force_send_handler =0xf000f0e7
  .dq_receive : { KEEP(*(.text.dq_receive)) __dq_receive_end = .; . = ORIGIN(dq_receive) + LENGTH(dq_receive); } > dq_receive =0xf000f0e7
  .dq_receive_poll : { KEEP(*(.text.dq_receive_poll)) __dq_receive_poll_end = .; . = ORIGIN(dq_receive_poll) + LENGTH(dq_receive_poll); } > dq_receive_poll =0xf000f0e7
  .dq_receive_timeout : { KEEP(*(.text.dq_receive_timeout)) __dq_receive_timeout_end = .; . = ORIGIN(dq_receive_timeout) + LENGTH(dq_receive_timeout); } > dq_receive_timeout =0xf000f0e7
  .dq_bodies : {
    KEEP(*(.text.dq_body_*))
    *(.text .text.*)
    *(.rodata .rodata.*)
    __dq_bodies_end = .;
    . = ORIGIN(dq_bodies) + LENGTH(dq_bodies);
  } > dq_bodies =0xf000f0e7
  .forbidden (NOLOAD) : { *(.data .data.* .bss .bss.* COMMON) } > dq_bodies
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}

ASSERT(xm1_rtos_data_queue_create == ORIGIN(dq_create), "create entry is not at its stock address")
ASSERT(__dq_create_end <= ORIGIN(dq_create) + LENGTH(dq_create), "create entry exceeds its stock extent")
ASSERT(xm1_rtos_data_queue_delete == ORIGIN(dq_delete), "delete entry is not at its stock address")
ASSERT(__dq_delete_end <= ORIGIN(dq_delete) + LENGTH(dq_delete), "delete entry exceeds its stock extent")
ASSERT(xm1_rtos_data_queue_send == ORIGIN(dq_send), "send entry is not at its stock address")
ASSERT(__dq_send_end <= ORIGIN(dq_send) + LENGTH(dq_send), "send entry exceeds its stock extent")
ASSERT(xm1_rtos_data_queue_send_poll == ORIGIN(dq_send_poll), "send_poll entry is not at its stock address")
ASSERT(__dq_send_poll_end <= ORIGIN(dq_send_poll) + LENGTH(dq_send_poll), "send_poll entry exceeds its stock extent")
ASSERT(xm1_rtos_data_queue_send_poll_handler == ORIGIN(dq_send_poll_handler), "send_poll_handler entry is not at its stock address")
ASSERT(__dq_send_poll_handler_end <= ORIGIN(dq_send_poll_handler) + LENGTH(dq_send_poll_handler), "send_poll_handler entry exceeds its stock extent")
ASSERT(xm1_rtos_data_queue_send_timeout == ORIGIN(dq_send_timeout), "send_timeout entry is not at its stock address")
ASSERT(__dq_send_timeout_end <= ORIGIN(dq_send_timeout) + LENGTH(dq_send_timeout), "send_timeout entry exceeds its stock extent")
ASSERT(xm1_rtos_data_queue_force_send == ORIGIN(dq_force_send), "force_send entry is not at its stock address")
ASSERT(__dq_force_send_end <= ORIGIN(dq_force_send) + LENGTH(dq_force_send), "force_send entry exceeds its stock extent")
ASSERT(xm1_rtos_data_queue_force_send_handler == ORIGIN(dq_force_send_handler), "force_send_handler entry is not at its stock address")
ASSERT(__dq_force_send_handler_end <= ORIGIN(dq_force_send_handler) + LENGTH(dq_force_send_handler), "force_send_handler entry exceeds its stock extent")
ASSERT(xm1_rtos_data_queue_receive == ORIGIN(dq_receive), "receive entry is not at its stock address")
ASSERT(__dq_receive_end <= ORIGIN(dq_receive) + LENGTH(dq_receive), "receive entry exceeds its stock extent")
ASSERT(xm1_rtos_data_queue_receive_poll == ORIGIN(dq_receive_poll), "receive_poll entry is not at its stock address")
ASSERT(__dq_receive_poll_end <= ORIGIN(dq_receive_poll) + LENGTH(dq_receive_poll), "receive_poll entry exceeds its stock extent")
ASSERT(xm1_rtos_data_queue_receive_timeout == ORIGIN(dq_receive_timeout), "receive_timeout entry is not at its stock address")
ASSERT(__dq_receive_timeout_end <= ORIGIN(dq_receive_timeout) + LENGTH(dq_receive_timeout), "receive_timeout entry exceeds its stock extent")
ASSERT(__dq_bodies_end <= ORIGIN(dq_bodies) + LENGTH(dq_bodies), "data-queue bodies exceed their region")
ASSERT(SIZEOF(.forbidden) == 0, "data or bss present")
