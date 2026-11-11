ENTRY(rb_menu_value_size_arm)
__rb_menu_value_stock_arm = 0x00d51b58;
__rb_menu_value_shared_tail = 0x00d52674;

SECTIONS {
  . = 0x00a00000;
  .text : { *(.text*) }
  .rodata : { *(.rodata*) }
  . = 0x00a80000;
  .data : { *(.data*) *(.bss*) *(.bss.*) }
  /DISCARD/ : { *(.ARM.exidx*) *(.comment*) *(.note*) }
}
