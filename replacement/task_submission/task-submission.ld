ENTRY(xm1_task_submit_entry)

xm1_task_critical_enter = 0x7235d8;
xm1_task_critical_exit = 0x7236b0;
xm1_task_reservation = 0x725c6c;
xm1_task_fill = 0x7221c8;
xm1_task_cache = 0x42634;
xm1_task_log = 0x41a34;
xm1_task_error_callback_dispatch = 0x723398;
xm1_task_try_lock = 0x427d4;
xm1_task_unlock = 0x427fc;
xm1_task_class_table = 0x7cad9c;
xm1_task_interface_roots = 0x96bd20;
xm1_task_wrapper_base = 0x40a5b204;
xm1_task_state_base = 0x40a0fc44;
xm1_task_error_word = 0x40a0f82c;
xm1_task_release_lock = 0x409c280c;

SECTIONS
{
  . = 0x725dd8;
  __task_submission_start = .;
  .text : {
    KEEP(*(.text.entry))
    *(.text*)
    *(.rodata*)
    . = ALIGN(4);
    __task_submission_bodies_end = .;
  }
  .release 0x726098 : {
    KEEP(*(.task_release_entry))
    __task_submission_release_adapter_end = .;
  }
  .release_literal 0x726128 : {
    KEEP(*(.task_release_literal))
  }
  __task_submission_end = .;
  .data : { *(.data*) *(.sdata*) *(.got*) }
  .bss : { *(.bss*) *(.sbss*) *(COMMON) }
  /DISCARD/ : {
    *(.ARM.exidx*)
    *(.ARM.extab*)
    *(.eh_frame*)
    *(.ARM.attributes)
    *(.note*)
    *(.comment*)
    *(.debug*)
    *(.llvm_addrsig)
  }
}

ASSERT(xm1_task_submit_entry == 0x725dd8, "task submit entry moved")
ASSERT(__task_submission_bodies_end <= 0x726098, "task Rust bodies exceed the pre-release window")
ASSERT(xm1_task_release_entry == 0x726098, "task release entry moved")
ASSERT(__task_submission_release_adapter_end <= 0x726128, "task release adapter exceeds its window")
ASSERT(xm1_task_release_lock_literal == 0x726128, "task release lock literal moved")
ASSERT(__task_submission_end == 0x72612c, "task submission must overwrite all 852 bytes")
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "task submission has writable statics")
