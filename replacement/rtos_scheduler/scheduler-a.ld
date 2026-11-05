/* Loader A: owned scheduler helpers at their stock extents (regions.json). */
ENTRY(xm1_rtos_scheduler_acquire)

MEMORY {
  acquire (rx) : ORIGIN = 0x439d8, LENGTH = 0x3c
  release (rx) : ORIGIN = 0x43a14, LENGTH = 0x28
  find (rx) : ORIGIN = 0x43c48, LENGTH = 0x5c
  insert (rx) : ORIGIN = 0x43ca4, LENGTH = 0x7c
  remove (rx) : ORIGIN = 0x43d20, LENGTH = 0xb0
  reset (rx) : ORIGIN = 0x43dd0, LENGTH = 0x48
  prepare (rx) : ORIGIN = 0x43f70, LENGTH = 0x2c
  notify (rx) : ORIGIN = 0x44018, LENGTH = 0x2c
  requeue (rx) : ORIGIN = 0x44044, LENGTH = 0xf0
  rotate (rx) : ORIGIN = 0x44134, LENGTH = 0x80
}

xm1_rtos_scheduler_stock_raise_irq = 0x433e4;

SECTIONS {
  .acquire : {
    KEEP(*(.text.acquire))
    __acquire_body_end = .;
    . = ORIGIN(acquire) + LENGTH(acquire);
  } > acquire =0xf000f0e7
  .release : {
    KEEP(*(.text.release))
    __release_body_end = .;
    . = ORIGIN(release) + LENGTH(release);
  } > release =0xf000f0e7
  .find : {
    KEEP(*(.text.find))
    __find_body_end = .;
    . = ORIGIN(find) + LENGTH(find) - 0x30;
    __prepare_body_start = .;
    KEEP(*(.text.prepare_body))
    __prepare_body_end = .;
    . = ORIGIN(find) + LENGTH(find);
  } > find =0xf000f0e7
  .insert : {
    KEEP(*(.text.insert))
    __insert_body_end = .;
    . = ORIGIN(insert) + LENGTH(insert);
  } > insert =0xf000f0e7
  .remove : {
    KEEP(*(.text.remove))
    __remove_body_end = .;
    . = ORIGIN(remove) + LENGTH(remove);
  } > remove =0xf000f0e7
  .reset : {
    KEEP(*(.text.reset))
    __reset_body_end = .;
    . = ORIGIN(reset) + LENGTH(reset);
  } > reset =0xf000f0e7
  .prepare : {
    KEEP(*(.text.prepare))
    __prepare_branch_end = .;
    . = ORIGIN(prepare) + LENGTH(prepare);
  } > prepare =0xf000f0e7
  .notify : {
    KEEP(*(.text.notify))
    __notify_body_end = .;
    . = ORIGIN(notify) + LENGTH(notify);
  } > notify =0xf000f0e7
  .requeue : {
    KEEP(*(.text.requeue))
    __requeue_body_end = .;
    . = ORIGIN(requeue) + LENGTH(requeue);
  } > requeue =0xf000f0e7
  .rotate : {
    KEEP(*(.text.rotate))
    __rotate_body_end = .;
    . = ORIGIN(rotate) + LENGTH(rotate);
  } > rotate =0xf000f0e7
  .forbidden (NOLOAD) : { *(.rodata .rodata.* .data .data.* .bss .bss.* COMMON) } > acquire
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}

__acquire_end = ORIGIN(acquire) + LENGTH(acquire);
__release_end = ORIGIN(release) + LENGTH(release);
__find_end = ORIGIN(find) + LENGTH(find);
__insert_end = ORIGIN(insert) + LENGTH(insert);
__remove_end = ORIGIN(remove) + LENGTH(remove);
__reset_end = ORIGIN(reset) + LENGTH(reset);
__prepare_end = ORIGIN(prepare) + LENGTH(prepare);
__notify_end = ORIGIN(notify) + LENGTH(notify);
__requeue_end = ORIGIN(requeue) + LENGTH(requeue);
__rotate_end = ORIGIN(rotate) + LENGTH(rotate);

ASSERT(xm1_rtos_scheduler_acquire == ORIGIN(acquire), "acquire entry is not at its stock address")
ASSERT(__acquire_body_end <= __acquire_end, "acquire body exceeds its stock extent")
ASSERT(xm1_rtos_scheduler_release == ORIGIN(release), "release entry is not at its stock address")
ASSERT(__release_body_end <= __release_end, "release body exceeds its stock extent")
ASSERT(xm1_rtos_scheduler_find == ORIGIN(find), "find entry is not at its stock address")
ASSERT(__find_body_end <= __find_end, "find body exceeds its stock extent")
ASSERT(xm1_rtos_scheduler_insert == ORIGIN(insert), "insert entry is not at its stock address")
ASSERT(__insert_body_end <= __insert_end, "insert body exceeds its stock extent")
ASSERT(xm1_rtos_scheduler_remove == ORIGIN(remove), "remove entry is not at its stock address")
ASSERT(__remove_body_end <= __remove_end, "remove body exceeds its stock extent")
ASSERT(xm1_rtos_scheduler_reset == ORIGIN(reset), "reset entry is not at its stock address")
ASSERT(__reset_body_end <= __reset_end, "reset body exceeds its stock extent")
ASSERT(xm1_rtos_scheduler_prepare == ORIGIN(prepare), "prepare entry is not at its stock address")
ASSERT(__prepare_branch_end <= __prepare_end, "prepare branch exceeds its stock extent")
ASSERT(xm1_rtos_scheduler_notify == ORIGIN(notify), "notify entry is not at its stock address")
ASSERT(__notify_body_end <= __notify_end, "notify body exceeds its stock extent")
ASSERT(xm1_rtos_scheduler_requeue == ORIGIN(requeue), "requeue entry is not at its stock address")
ASSERT(__requeue_body_end <= __requeue_end, "requeue body exceeds its stock extent")
ASSERT(xm1_rtos_scheduler_rotate == ORIGIN(rotate), "rotate entry is not at its stock address")
ASSERT(__rotate_body_end <= __rotate_end, "rotate body exceeds its stock extent")
ASSERT(__find_body_end <= __prepare_body_start, "find body overlaps the prepare body")
ASSERT(xm1_rtos_scheduler_prepare_body == __prepare_body_start, "prepare body is not at its reserved address")
ASSERT(__prepare_body_end <= __find_end, "prepare body exceeds the find extent")
ASSERT(SIZEOF(.forbidden) == 0, "rodata, data or bss present")
