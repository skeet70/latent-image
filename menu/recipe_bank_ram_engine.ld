/* Added to recipe_bank_ram.ld for the modal75,menu-engine package: the owned
 * menu engine region at RAM_PUSH_MENU_ENGINE_START (RamPushLayout
 * menuEngineRegionStart, past the installer). build-recipe-bank-ram.sh renames
 * the engine's code input sections to .menu_engine.* and its writable input
 * sections to .menu_engine_data.* so .text's wildcards miss them and no
 * loadable segment is both writable and executable. */
__rb_menu_engine_stock_resume = 0x00bf95cc;
xm1_bank_current_selection = 0x006439e0;
xm1_bank_custom_refresh = 0x006a86c4;
xm1_stock_61df44 = 0x0061df44;
xm1_stock_61d8a4 = 0x0061d8a4;
xm1_stock_61d59c = 0x0061d59c;
xm1_stock_61d618 = 0x0061d618;
xm1_stock_61d5f0 = 0x0061d5f0;
xm1_stock_61d65c = 0x0061d65c;
xm1_kernel_console9 = 0x00041a34;
xm1_kernel_critical_enter = 0x007235d8;
xm1_kernel_critical_exit = 0x007236b0;
xm1_backup_diagnostic726c74 = 0x00726c74;
xm1_backup_write6a59b8 = 0x006a59b8;
xm1_backup_report633594 = 0x00633594;
__rb_menu_value_stock_arm = 0x00d51b58;
__rb_menu_value_shared_tail = 0x00d52674;
PROVIDE(__rb_render_tree_stock_resume = 0x01164e20);
PROVIDE(__rb_render_picker_stock_resume = 0x01163f30);
PROVIDE(__rb_render_pane_stock_resume = 0x011662a4);

SECTIONS {
  .menu_engine RAM_PUSH_MENU_ENGINE_START : {
    __rb_menu_engine_start = .;
    *(.menu_engine.*)
    . = ALIGN(4);
    __rb_menu_engine_end = .;
  }
  .menu_engine_data : {
    . = ALIGN(8);
    __rb_menu_engine_data_start = .;
    *(.menu_engine_data.*)
    . = ALIGN(4);
    __rb_menu_engine_data_end = .;
  }
}

ASSERT(__rb_menu_engine_start == RAM_PUSH_MENU_ENGINE_START, "menu engine region placement changed")
ASSERT(__rb_menu_engine_end <= RAM_PUSH_MENU_ENGINE_END, "menu engine overflows its region")
ASSERT(__rb_menu_engine_data_start >= __rb_menu_engine_end, "menu engine data overlaps its code")
ASSERT(__rb_menu_engine_data_end <= RAM_PUSH_MENU_ENGINE_END, "menu engine data overflows its region")
ASSERT(RAM_PUSH_MENU_ENGINE_START >= __ram_push_end, "menu engine region overlaps the installer")
ASSERT(!DEFINED(RAM_PUSH_BANK_VARIANT) || DEFINED(RAM_PUSH_MENU_VALUE) || DEFINED(RAM_PUSH_PROPERTY_PUBLICATION) || RAM_PUSH_MENU_ENGINE_START == RAM_PUSH_BASE + 0x1dd00, "render region start differs")
ASSERT(!DEFINED(RAM_PUSH_BANK_VARIANT) || DEFINED(RAM_PUSH_MENU_VALUE) || DEFINED(RAM_PUSH_PROPERTY_PUBLICATION) || RAM_PUSH_MENU_ENGINE_END == RAM_PUSH_BASE + 0x3dd00, "render allocation end differs")
ASSERT(!DEFINED(RAM_PUSH_MENU_VALUE) || RAM_PUSH_MENU_ENGINE_END == RAM_PUSH_BASE + 0x33d00, "menu value allocation end differs")
ASSERT(!DEFINED(RAM_PUSH_PROPERTY_PUBLICATION) || RAM_PUSH_MENU_ENGINE_START == RAM_PUSH_BASE + 0x2dcc0, "property render region start differs")
ASSERT(!DEFINED(RAM_PUSH_PROPERTY_PUBLICATION) || RAM_PUSH_MENU_ENGINE_END == RAM_PUSH_BASE + 0x3dc80, "property render allocation end differs")
