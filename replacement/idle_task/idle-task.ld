ENTRY(xm1_idle_task)
SECTIONS
{
  . = 0x69cbf0;
  __idle_task_start = .;
  .text : { KEEP(*(.text.xm1_idle_task)) *(.text*) }
  .rodata : { *(.rodata*) }
  .data : { *(.data*) *(.sdata*) *(.got*) *(.tdata*) *(.preinit_array*) *(.init_array*) *(.fini_array*) *(.ctors*) *(.dtors*) }
  .bss : { *(.bss*) *(.sbss*) *(.tbss*) *(COMMON) }
  __idle_task_end = .;
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.note*) *(.comment*) }
}
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "idle task has writable sections")
ASSERT(SIZEOF(.rodata) == 0, "idle task has external data")
ASSERT(__idle_task_start == 0x69cbf0 && xm1_idle_task == __idle_task_start, "idle task entry moved")
ASSERT(__idle_task_end > __idle_task_start && __idle_task_end <= 0x69cc00, "idle task exceeds its 16-byte carrier")
ASSERT((__idle_task_end & 3) == 0, "idle task code extent is not A32 aligned")
