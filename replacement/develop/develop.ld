ENTRY(xm1_develop_dispatch)
xm1_stock_61df44 = 0x61df44;
xm1_stock_61d8a4 = 0x61d8a4;
xm1_stock_61d59c = 0x61d59c;
xm1_stock_61d618 = 0x61d618;
xm1_stock_61d5f0 = 0x61d5f0;
xm1_stock_61d65c = 0x61d65c;
xm1_kernel_console9 = 0x41a34;
xm1_kernel_critical_enter = 0x7235d8;
xm1_kernel_critical_exit = 0x7236b0;
PHDRS { code PT_LOAD FLAGS(5); data PT_LOAD FLAGS(6); }
SECTIONS
{
  . = 0x07000000;
  .text : { KEEP(*(.text.xm1_develop_dispatch)) KEEP(*(.text.xm1_develop_*)) KEEP(*(.text.xm1_pipeline_*)) KEEP(*(.text.xm1_buffers_*)) KEEP(*(.text.rb_media_permit_*)) KEEP(*(.text.xm1_save_*)) *(.text .text.*) } :code
  .rodata : { *(.rodata .rodata.*) } :code
  .data : { *(.data .data.*) } :data
  .bss (NOLOAD) : { *(.bss .bss.*) *(COMMON) } :data
  ASSERT(. <= 0x07200000, "develop native proof artifact exceeds reserved two MiB window")
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.comment) }
}
