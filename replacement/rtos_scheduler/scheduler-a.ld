/* Loader A: owned scheduler helpers at their stock extents (regions.json). */
ENTRY(xm1_rtos_scheduler_acquire)

INCLUDE scheduler-a-placement.ld

SECTIONS {
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
