ENTRY(rb_menu_value_size_arm)
__rb_menu_value_stock_arm = 0x00d51b58;
__rb_menu_value_shared_tail = 0x00d52674;

/* The property service's retained stock: default leaves, the profile
   selector, the custom refresh, kernel and backup services. */
xm1_stock_61df44 = 0x0061df44;
xm1_stock_61d8a4 = 0x0061d8a4;
xm1_stock_61d59c = 0x0061d59c;
xm1_stock_61d618 = 0x0061d618;
xm1_stock_61d5f0 = 0x0061d5f0;
xm1_stock_61d65c = 0x0061d65c;
xm1_bank_current_selection = 0x006439e0;
xm1_bank_custom_refresh = 0x006a86c4;
xm1_kernel_console9 = 0x00041a34;
xm1_kernel_critical_enter = 0x007235d8;
xm1_kernel_critical_exit = 0x007236b0;
xm1_backup_diagnostic726c74 = 0x00726c74;
xm1_backup_write6a59b8 = 0x006a59b8;
xm1_backup_report633594 = 0x00633594;

SECTIONS {
  . = 0x00a00000;
  .text : { *(.text*) }
  .rodata : { *(.rodata*) *(.menu_engine.rodata*) }
  . = 0x00a80000;
  .data : { *(.data*) *(.bss*) *(.bss.*) }
  /DISCARD/ : { *(.ARM.exidx*) *(.comment*) *(.note*) }
}
