INCLUDE scheduler-a-placement.ld
INCLUDE scheduler-s2-a-placement.ld

MEMORY {
  helper_release (rx) : ORIGIN = 0x44744, LENGTH = 0xbc
  helper_block (rx) : ORIGIN = 0x44800, LENGTH = 0xe4
  helper_block_timed (rx) : ORIGIN = 0x448e4, LENGTH = 0xdc
  helper_check (rx) : ORIGIN = 0x480cc, LENGTH = 0x70
  family_release (rx) : ORIGIN = 0x44568, LENGTH = 0xcc
  family_flag_create (rx) : ORIGIN = 0x4d39c, LENGTH = 0x98
  family_flag_delete (rx) : ORIGIN = 0x4d434, LENGTH = 0x68
  family_flag_set (rx) : ORIGIN = 0x4813c, LENGTH = 0x260
  family_flag_set_handler (rx) : ORIGIN = 0x4839c, LENGTH = 0x224
  family_flag_clear (rx) : ORIGIN = 0x485c0, LENGTH = 0x138
  family_flag_wait (rx) : ORIGIN = 0x486f8, LENGTH = 0x2b4
  family_flag_poll (rx) : ORIGIN = 0x489ac, LENGTH = 0x184
  family_flag_timed_wait (rx) : ORIGIN = 0x48b30, LENGTH = 0x308
  family_mailbox_create (rx) : ORIGIN = 0x4d5bc, LENGTH = 0x98
  family_mailbox_delete (rx) : ORIGIN = 0x4d654, LENGTH = 0x70
  family_mailbox_send (rx) : ORIGIN = 0x4aa44, LENGTH = 0x304
  family_mailbox_receive (rx) : ORIGIN = 0x4ad48, LENGTH = 0x254
  family_mailbox_poll (rx) : ORIGIN = 0x4af9c, LENGTH = 0x148
  family_mailbox_timed_receive (rx) : ORIGIN = 0x4b0e4, LENGTH = 0x2a8
}

xm1_rtos_objects_stock_insert_timer = 0x44c60;
xm1_rtos_objects_stock_remove_timer = 0x44ca8;
xm1_rtos_objects_stock_timeout = 0x44318;

SECTIONS {
  .helper_release : {
    KEEP(*(.text.helper_release))
    __helper_release_body_end = .;
    *(.text._RINvNtNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter7objects5block12block_lockedNtB6_3CpuEB6_)
    *(.text.OUTLINED_FUNCTION_0)
    __helper_release_shared_end = .;
    . = ORIGIN(helper_release) + LENGTH(helper_release);
  } > helper_release =0xf000f0e7
  .helper_block : {
    KEEP(*(.text.helper_block))
    __helper_block_body_end = .;
    *(.text._RNvNtNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter7objects2id15resolve_flag_in)
    *(.text._RNvNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter6family14mask_lifecycle)
    *(.text._RNvNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter6family17restore_lifecycle)
    *(.text.OUTLINED_FUNCTION_3)
    __helper_block_shared_end = .;
    . = ORIGIN(helper_block) + LENGTH(helper_block);
  } > helper_block =0xf000f0e7
  .helper_block_timed : {
    KEEP(*(.text.helper_block_timed))
    __helper_block_timed_body_end = .;
    *(.rodata.cst16)
    *(.rodata..Lswitch.table._RNvNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter6family20delete_mailbox_entry)
    *(.text.OUTLINED_FUNCTION_2)
    *(.text.OUTLINED_FUNCTION_4)
    *(.text.OUTLINED_FUNCTION_8)
    __helper_block_timed_shared_end = .;
    . = ORIGIN(helper_block_timed) + LENGTH(helper_block_timed);
  } > helper_block_timed =0xf000f0e7
  .helper_check : {
    KEEP(*(.text.helper_check))
    __helper_check_body_end = .;
    *(.text.OUTLINED_FUNCTION_9)

    __helper_check_shared_end = .;
    . = ORIGIN(helper_check) + LENGTH(helper_check);
  } > helper_check =0xf000f0e7
  .family_release : {
    KEEP(*(.text.f2_release))
    __family_release_body_end = .;
    *(.text._RINvNtNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter7objects4wake13release_readyNtB6_3CpuEB6_)
    *(.text._RNvNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter6family8unmask_i)
    __family_release_shared_end = .;
    . = ORIGIN(family_release) + LENGTH(family_release);
  } > family_release =0xf000f0e7
  .family_flag_create : {
    KEEP(*(.text.family_flag_create))
    __family_flag_create_body_end = .;
    *(.text._RINvNtNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter7objects4wake12release_withNtB6_3CpuEB6_)
    __family_flag_create_shared_end = .;
    . = ORIGIN(family_flag_create) + LENGTH(family_flag_create);
  } > family_flag_create =0xf000f0e7
  .family_flag_delete : {
    KEEP(*(.text.family_flag_delete))
    __family_flag_delete_body_end = .;

    __family_flag_delete_shared_end = .;
    . = ORIGIN(family_flag_delete) + LENGTH(family_flag_delete);
  } > family_flag_delete =0xf000f0e7
  .family_flag_set : {
    KEEP(*(.text.family_flag_set))
    __family_flag_set_body_end = .;
    *(.text._RINvNtNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter7objects4wake13set_flag_modeNtB6_3CpuEB6_)
    *(.text._RINvNtNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter7objects8teardown11delete_flagNtB6_3CpuEB6_)
    __family_flag_set_shared_end = .;
    . = ORIGIN(family_flag_set) + LENGTH(family_flag_set);
  } > family_flag_set =0xf000f0e7
  .family_flag_set_handler : {
    KEEP(*(.text.family_flag_set_handler))
    __family_flag_set_handler_body_end = .;
    *(.text._RINvNtNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter7objects4wake12queue_insertNtB6_3CpuEB6_)
    *(.text._RINvNtNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter7objects6create11create_flagNtB6_3CpuEB6_)
    *(.text._RINvNtNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter7objects8teardown14delete_mailboxNtB6_3CpuEB6_)
    *(.text._RNvNtNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter7objects2id18resolve_mailbox_in)
    __family_flag_set_handler_shared_end = .;
    . = ORIGIN(family_flag_set_handler) + LENGTH(family_flag_set_handler);
  } > family_flag_set_handler =0xf000f0e7
  .family_flag_clear : {
    KEEP(*(.text.family_flag_clear))
    __family_flag_clear_body_end = .;
    *(.text._RINvNtNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter7objects6create14create_mailboxNtB6_3CpuEB6_)
    *(.text._RINvNtNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter7objects5block5blockNtB6_3CpuEB6_)
    *(.text._RINvNtNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter7objects6locked13check_controlNtB6_3CpuEB6_)
    *(.text.OUTLINED_FUNCTION_1)
    __family_flag_clear_shared_end = .;
    . = ORIGIN(family_flag_clear) + LENGTH(family_flag_clear);
  } > family_flag_clear =0xf000f0e7
  .family_flag_wait : {
    KEEP(*(.text.family_flag_wait))
    __family_flag_wait_body_end = .;
    *(.text._RNvNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter6family16receive_admitted)
    __family_flag_wait_shared_end = .;
    . = ORIGIN(family_flag_wait) + LENGTH(family_flag_wait);
  } > family_flag_wait =0xf000f0e7
  .family_flag_poll : {
    KEEP(*(.text.family_flag_poll))
    __family_flag_poll_body_end = .;
    *(.text._RNvNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter6family17create_flag_entry)
    *(.text._RNvNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter6family17delete_flag_entry)
    *(.text._RNvNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter6family20create_mailbox_entry)
    __family_flag_poll_shared_end = .;
    . = ORIGIN(family_flag_poll) + LENGTH(family_flag_poll);
  } > family_flag_poll =0xf000f0e7
  .family_flag_timed_wait : {
    KEEP(*(.text.family_flag_timed_wait))
    __family_flag_timed_wait_body_end = .;
    *(.text._RNvNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter6family11flag_router)
    __family_flag_timed_wait_shared_end = .;
    . = ORIGIN(family_flag_timed_wait) + LENGTH(family_flag_timed_wait);
  } > family_flag_timed_wait =0xf000f0e7
  .family_mailbox_create : {
    KEEP(*(.text.family_mailbox_create))
    __family_mailbox_create_body_end = .;
    *(.text.OUTLINED_FUNCTION_7)
    *(.rodata .rodata.*)

    __family_mailbox_create_shared_end = .;
    . = ORIGIN(family_mailbox_create) + LENGTH(family_mailbox_create);
  } > family_mailbox_create =0xf000f0e7
  .family_mailbox_delete : {
    KEEP(*(.text.family_mailbox_delete))
    __family_mailbox_delete_body_end = .;

    __family_mailbox_delete_shared_end = .;
    . = ORIGIN(family_mailbox_delete) + LENGTH(family_mailbox_delete);
  } > family_mailbox_delete =0xf000f0e7
  .family_mailbox_send : {
    KEEP(*(.text.family_mailbox_send))
    __family_mailbox_send_body_end = .;
    *(.text._RNvNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter6family18flag_wait_admitted)
    *(.text.OUTLINED_FUNCTION_5)
    __family_mailbox_send_shared_end = .;
    . = ORIGIN(family_mailbox_send) + LENGTH(family_mailbox_send);
  } > family_mailbox_send =0xf000f0e7
  .family_mailbox_receive : {
    KEEP(*(.text.family_mailbox_receive))
    __family_mailbox_receive_body_end = .;
    *(.text._RINvNtNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter7objects4wake4sendNtB6_3CpuEB6_)
    *(.text._RINvNtNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter7objects8teardown11release_allNtB6_3CpuEB6_)
    __family_mailbox_receive_shared_end = .;
    . = ORIGIN(family_mailbox_receive) + LENGTH(family_mailbox_receive);
  } > family_mailbox_receive =0xf000f0e7
  .family_mailbox_poll : {
    KEEP(*(.text.family_mailbox_poll))
    __family_mailbox_poll_body_end = .;
    *(.text._RINvNtNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter7objects6locked10clear_flagNtB6_3CpuEB6_)
    *(.text._RNvNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter6family14entry_snapshot)
    *(.text._RNvNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter6family20delete_mailbox_entry)
    *(.text.OUTLINED_FUNCTION_6)
    __family_mailbox_poll_shared_end = .;
    . = ORIGIN(family_mailbox_poll) + LENGTH(family_mailbox_poll);
  } > family_mailbox_poll =0xf000f0e7
  .family_mailbox_timed_receive : {
    KEEP(*(.text.family_mailbox_timed_receive))
    __family_mailbox_timed_receive_body_end = .;
    *(.text._RNvNtCs1rraWxgmT7_31xm1_rtos_objects_family_adapter6family14mailbox_router)
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
ASSERT(xm1_rtos_objects_wait_release == ORIGIN(helper_release), "helper_release entry is not at its stock address")
ASSERT(__helper_release_body_end <= ORIGIN(helper_release) + LENGTH(helper_release), "helper_release body exceeds its stock extent")
ASSERT(xm1_rtos_objects_wait_block == ORIGIN(helper_block), "helper_block entry is not at its stock address")
ASSERT(__helper_block_body_end <= ORIGIN(helper_block) + LENGTH(helper_block), "helper_block body exceeds its stock extent")
ASSERT(xm1_rtos_objects_wait_block_timed == ORIGIN(helper_block_timed), "helper_block_timed entry is not at its stock address")
ASSERT(__helper_block_timed_body_end <= ORIGIN(helper_block_timed) + LENGTH(helper_block_timed), "helper_block_timed body exceeds its stock extent")
ASSERT(xm1_rtos_objects_check == ORIGIN(helper_check), "helper_check entry is not at its stock address")
ASSERT(__helper_check_body_end <= ORIGIN(helper_check) + LENGTH(helper_check), "helper_check body exceeds its stock extent")
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
