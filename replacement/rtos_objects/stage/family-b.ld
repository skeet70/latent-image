INCLUDE scheduler-b-placement.ld
INCLUDE scheduler-s2-b-placement.ld

MEMORY {
  family_release (rx) : ORIGIN = 0xd4528, LENGTH = 0xcc
  family_flag_create (rx) : ORIGIN = 0xdd360, LENGTH = 0x98
  family_flag_delete (rx) : ORIGIN = 0xdd3f8, LENGTH = 0x68
  family_flag_set (rx) : ORIGIN = 0xd80fc, LENGTH = 0x260
  family_flag_set_handler (rx) : ORIGIN = 0xd835c, LENGTH = 0x224
  family_flag_clear (rx) : ORIGIN = 0xd8580, LENGTH = 0x138
  family_flag_wait (rx) : ORIGIN = 0xd86b8, LENGTH = 0x2b4
  family_flag_poll (rx) : ORIGIN = 0xd896c, LENGTH = 0x184
  family_flag_timed_wait (rx) : ORIGIN = 0xd8af0, LENGTH = 0x308
  family_mailbox_create (rx) : ORIGIN = 0xdd580, LENGTH = 0x98
  family_mailbox_delete (rx) : ORIGIN = 0xdd618, LENGTH = 0x70
  family_mailbox_send (rx) : ORIGIN = 0xdaa04, LENGTH = 0x304
  family_mailbox_receive (rx) : ORIGIN = 0xdad08, LENGTH = 0x254
  family_mailbox_poll (rx) : ORIGIN = 0xdaf5c, LENGTH = 0x148
  family_mailbox_timed_receive (rx) : ORIGIN = 0xdb0a4, LENGTH = 0x2a8
}

xm1_rtos_objects_stock_insert_timer = 0xd4c20;
xm1_rtos_objects_stock_remove_timer = 0xd4c68;
xm1_rtos_objects_stock_timeout = 0xd42d8;

SECTIONS {
  .family_release : {
    KEEP(*(.text.f2_release))
    __family_release_body_end = .;
    . = ORIGIN(family_release) + LENGTH(family_release);
  } > family_release =0xf000f0e7
  .family_flag_create : {
    KEEP(*(.text.family_flag_create))
    __family_flag_create_body_end = .;
    . = ORIGIN(family_flag_create) + LENGTH(family_flag_create);
  } > family_flag_create =0xf000f0e7
  .family_flag_delete : {
    KEEP(*(.text.family_flag_delete))
    __family_flag_delete_body_end = .;
    . = ORIGIN(family_flag_delete) + LENGTH(family_flag_delete);
  } > family_flag_delete =0xf000f0e7
  .family_flag_set : {
    KEEP(*(.text.family_flag_set))
    __family_flag_set_body_end = .;
    *(.text.*objects6create*)
    __family_flag_set_shared_end = .;
    . = ORIGIN(family_flag_set) + LENGTH(family_flag_set);
  } > family_flag_set =0xf000f0e7
  .family_flag_set_handler : {
    KEEP(*(.text.family_flag_set_handler))
    __family_flag_set_handler_body_end = .;
    . = ORIGIN(family_flag_set_handler) + LENGTH(family_flag_set_handler);
  } > family_flag_set_handler =0xf000f0e7
  .family_flag_clear : {
    KEEP(*(.text.family_flag_clear))
    __family_flag_clear_body_end = .;
    *(.rodata.str1.1)
    *(.rodata..Lanon.*)
    *(.rodata.*)
    __family_flag_clear_shared_end = .;
    . = ORIGIN(family_flag_clear) + LENGTH(family_flag_clear);
  } > family_flag_clear =0xf000f0e7
  .family_flag_wait : {
    KEEP(*(.text.family_flag_wait))
    __family_flag_wait_body_end = .;
    *(.text.*objects5block*)
    __family_flag_wait_shared_end = .;
    . = ORIGIN(family_flag_wait) + LENGTH(family_flag_wait);
  } > family_flag_wait =0xf000f0e7
  .family_flag_poll : {
    KEEP(*(.text.family_flag_poll))
    __family_flag_poll_body_end = .;
    *(.text.*family_adapter*Cpu*)
    __family_flag_poll_shared_end = .;
    . = ORIGIN(family_flag_poll) + LENGTH(family_flag_poll);
  } > family_flag_poll =0xf000f0e7
  .family_flag_timed_wait : {
    KEEP(*(.text.family_flag_timed_wait))
    __family_flag_timed_wait_body_end = .;
    *(.text.*objects9admission*)
    *(.text.*objects2id*)
    *(.text.*objects6status*)
    *(.text.*objects7matching*)
    *(.text.*objects7message*)
    *(.text.*objects7timeout*)
    __family_flag_timed_wait_shared_end = .;
    . = ORIGIN(family_flag_timed_wait) + LENGTH(family_flag_timed_wait);
  } > family_flag_timed_wait =0xf000f0e7
  .family_mailbox_create : {
    KEEP(*(.text.family_mailbox_create))
    __family_mailbox_create_body_end = .;
    . = ORIGIN(family_mailbox_create) + LENGTH(family_mailbox_create);
  } > family_mailbox_create =0xf000f0e7
  .family_mailbox_delete : {
    KEEP(*(.text.family_mailbox_delete))
    __family_mailbox_delete_body_end = .;
    . = ORIGIN(family_mailbox_delete) + LENGTH(family_mailbox_delete);
  } > family_mailbox_delete =0xf000f0e7
  .family_mailbox_send : {
    KEEP(*(.text.family_mailbox_send))
    __family_mailbox_send_body_end = .;
    *(.text.*objects4wake*)
    __family_mailbox_send_shared_end = .;
    . = ORIGIN(family_mailbox_send) + LENGTH(family_mailbox_send);
  } > family_mailbox_send =0xf000f0e7
  .family_mailbox_receive : {
    KEEP(*(.text.family_mailbox_receive))
    __family_mailbox_receive_body_end = .;
    *(.text.*objects8teardown*)
    __family_mailbox_receive_shared_end = .;
    . = ORIGIN(family_mailbox_receive) + LENGTH(family_mailbox_receive);
  } > family_mailbox_receive =0xf000f0e7
  .family_mailbox_poll : {
    KEEP(*(.text.family_mailbox_poll))
    __family_mailbox_poll_body_end = .;
    . = ORIGIN(family_mailbox_poll) + LENGTH(family_mailbox_poll);
  } > family_mailbox_poll =0xf000f0e7
  .family_mailbox_timed_receive : {
    KEEP(*(.text.family_mailbox_timed_receive))
    __family_mailbox_timed_receive_body_end = .;
    *(.text.*objects6locked*)
    *(.text.*)
    __family_mailbox_timed_receive_shared_end = .;
    . = ORIGIN(family_mailbox_timed_receive) + LENGTH(family_mailbox_timed_receive);
  } > family_mailbox_timed_receive =0xf000f0e7
  .forbidden (NOLOAD) : { *(.text .text.* .rodata .rodata.* .data .data.* .bss .bss.* COMMON) } > acquire
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}
ASSERT(xm1_rtos_objects_release == ORIGIN(family_release), "family_release entry is not at its stock address")
ASSERT(__family_release_body_end <= ORIGIN(family_release) + LENGTH(family_release), "family_release body exceeds its stock extent")
ASSERT(xm1_rtos_objects_family_flag_create == ORIGIN(family_flag_create), "family_flag_create entry is not at its stock address")
ASSERT(__family_flag_create_body_end <= ORIGIN(family_flag_create) + LENGTH(family_flag_create), "family_flag_create body exceeds its stock extent")
ASSERT(xm1_rtos_objects_family_flag_delete == ORIGIN(family_flag_delete), "family_flag_delete entry is not at its stock address")
ASSERT(__family_flag_delete_body_end <= ORIGIN(family_flag_delete) + LENGTH(family_flag_delete), "family_flag_delete body exceeds its stock extent")
ASSERT(xm1_rtos_objects_family_flag_set == ORIGIN(family_flag_set), "family_flag_set entry is not at its stock address")
ASSERT(__family_flag_set_body_end <= ORIGIN(family_flag_set) + LENGTH(family_flag_set), "family_flag_set body exceeds its stock extent")
ASSERT(__family_flag_set_shared_end <= ORIGIN(family_flag_set) + LENGTH(family_flag_set), "family_flag_set shared text exceeds its stock extent")
ASSERT(xm1_rtos_objects_family_flag_set_handler == ORIGIN(family_flag_set_handler), "family_flag_set_handler entry is not at its stock address")
ASSERT(__family_flag_set_handler_body_end <= ORIGIN(family_flag_set_handler) + LENGTH(family_flag_set_handler), "family_flag_set_handler body exceeds its stock extent")
ASSERT(xm1_rtos_objects_family_flag_clear == ORIGIN(family_flag_clear), "family_flag_clear entry is not at its stock address")
ASSERT(__family_flag_clear_body_end <= ORIGIN(family_flag_clear) + LENGTH(family_flag_clear), "family_flag_clear body exceeds its stock extent")
ASSERT(__family_flag_clear_shared_end <= ORIGIN(family_flag_clear) + LENGTH(family_flag_clear), "family_flag_clear shared data exceeds its stock extent")
ASSERT(xm1_rtos_objects_family_flag_wait == ORIGIN(family_flag_wait), "family_flag_wait entry is not at its stock address")
ASSERT(__family_flag_wait_body_end <= ORIGIN(family_flag_wait) + LENGTH(family_flag_wait), "family_flag_wait body exceeds its stock extent")
ASSERT(__family_flag_wait_shared_end <= ORIGIN(family_flag_wait) + LENGTH(family_flag_wait), "family_flag_wait shared text exceeds its stock extent")
ASSERT(xm1_rtos_objects_family_flag_poll == ORIGIN(family_flag_poll), "family_flag_poll entry is not at its stock address")
ASSERT(__family_flag_poll_body_end <= ORIGIN(family_flag_poll) + LENGTH(family_flag_poll), "family_flag_poll body exceeds its stock extent")
ASSERT(__family_flag_poll_shared_end <= ORIGIN(family_flag_poll) + LENGTH(family_flag_poll), "family_flag_poll shared text exceeds its stock extent")
ASSERT(xm1_rtos_objects_family_flag_timed_wait == ORIGIN(family_flag_timed_wait), "family_flag_timed_wait entry is not at its stock address")
ASSERT(__family_flag_timed_wait_body_end <= ORIGIN(family_flag_timed_wait) + LENGTH(family_flag_timed_wait), "family_flag_timed_wait body exceeds its stock extent")
ASSERT(__family_flag_timed_wait_shared_end <= ORIGIN(family_flag_timed_wait) + LENGTH(family_flag_timed_wait), "family_flag_timed_wait shared text exceeds its stock extent")
ASSERT(xm1_rtos_objects_family_mailbox_create == ORIGIN(family_mailbox_create), "family_mailbox_create entry is not at its stock address")
ASSERT(__family_mailbox_create_body_end <= ORIGIN(family_mailbox_create) + LENGTH(family_mailbox_create), "family_mailbox_create body exceeds its stock extent")
ASSERT(xm1_rtos_objects_family_mailbox_delete == ORIGIN(family_mailbox_delete), "family_mailbox_delete entry is not at its stock address")
ASSERT(__family_mailbox_delete_body_end <= ORIGIN(family_mailbox_delete) + LENGTH(family_mailbox_delete), "family_mailbox_delete body exceeds its stock extent")
ASSERT(xm1_rtos_objects_family_mailbox_send == ORIGIN(family_mailbox_send), "family_mailbox_send entry is not at its stock address")
ASSERT(__family_mailbox_send_body_end <= ORIGIN(family_mailbox_send) + LENGTH(family_mailbox_send), "family_mailbox_send body exceeds its stock extent")
ASSERT(__family_mailbox_send_shared_end <= ORIGIN(family_mailbox_send) + LENGTH(family_mailbox_send), "family_mailbox_send shared text exceeds its stock extent")
ASSERT(xm1_rtos_objects_family_mailbox_receive == ORIGIN(family_mailbox_receive), "family_mailbox_receive entry is not at its stock address")
ASSERT(__family_mailbox_receive_body_end <= ORIGIN(family_mailbox_receive) + LENGTH(family_mailbox_receive), "family_mailbox_receive body exceeds its stock extent")
ASSERT(__family_mailbox_receive_shared_end <= ORIGIN(family_mailbox_receive) + LENGTH(family_mailbox_receive), "family_mailbox_receive shared text exceeds its stock extent")
ASSERT(xm1_rtos_objects_family_mailbox_poll == ORIGIN(family_mailbox_poll), "family_mailbox_poll entry is not at its stock address")
ASSERT(__family_mailbox_poll_body_end <= ORIGIN(family_mailbox_poll) + LENGTH(family_mailbox_poll), "family_mailbox_poll body exceeds its stock extent")
ASSERT(xm1_rtos_objects_family_mailbox_timed_receive == ORIGIN(family_mailbox_timed_receive), "family_mailbox_timed_receive entry is not at its stock address")
ASSERT(__family_mailbox_timed_receive_body_end <= ORIGIN(family_mailbox_timed_receive) + LENGTH(family_mailbox_timed_receive), "family_mailbox_timed_receive body exceeds its stock extent")
ASSERT(__family_mailbox_timed_receive_shared_end <= ORIGIN(family_mailbox_timed_receive) + LENGTH(family_mailbox_timed_receive), "family_mailbox_timed_receive shared text exceeds its stock extent")
ASSERT(SIZEOF(.forbidden) == 0, "unplaced text or data remains")
