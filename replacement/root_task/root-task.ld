ENTRY(xm1_root_task)
root_platform_init = 0x649624;
root_runtime_init = 0x7284ec;
root_mark = 0x72872c;
root_install_table = 0x725b54;
root_create_resource = 0x7247ac;
root_create_workers = 0x69ca24;
root_init_676054 = 0x676054;
root_init_662e48 = 0x662e48;
root_init_665224 = 0x665224;
root_init_649070 = 0x649070;
root_init_6490dc = 0x6490dc;
root_checked_init_a = 0x6a9354;
root_checked_init_b = 0x6ac2b4;
root_init_69f204 = 0x69f204;
root_init_656788 = 0x656788;
root_init_7262cc = 0x7262cc;
root_install_descriptor = 0x724cbc;
root_init_6a5634 = 0x6a5634;
root_config_write = 0x6a59b8;
root_mode = 0x69cfa4;
root_config_query = 0x6a5920;
root_init_6a564c = 0x6a564c;
root_init_6a5c08 = 0x6a5c08;
root_init_6568ec = 0x6568ec;
root_submit_later = 0x69ca44;
xm1_app_register_callback = 0x725424;
root_cleanup = 0x72612c;
root_periodic_callback = 0x69cc18;
root_periodic_context = 0x96ca4c;
SECTIONS
{
  . = 0x69d160;
  __root_task_start = .;
  .text : { KEEP(*(.text.xm1_root_task)) *(.text*) }
  .rodata : { *(.rodata*) }
  .data : { *(.data*) *(.sdata*) *(.got*) *(.tdata*) *(.preinit_array*) *(.init_array*) *(.fini_array*) *(.ctors*) *(.dtors*) }
  .bss : { *(.bss*) *(.sbss*) *(.tbss*) *(COMMON) }
  __root_task_end = .;
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.note*) *(.comment*) }
}
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "root task has writable sections")
ASSERT(SIZEOF(.rodata) == 0, "root task has external data")
ASSERT(__root_task_start == 0x69d160 && xm1_root_task == __root_task_start, "root task entry moved")
ASSERT(__root_task_end > __root_task_start && __root_task_end <= 0x69d2b4, "root task exceeds its 340-byte carrier")
ASSERT((__root_task_end & 3) == 0, "root task code extent is not A32 aligned")
