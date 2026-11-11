/* Added to recipe_bank_ram.ld for the modal75,menu-engine package: the owned
 * menu engine region at RAM_PUSH_MENU_ENGINE_START (RamPushLayout
 * menuEngineRegionStart, past the installer). build-recipe-bank-ram.sh renames
 * the engine's input sections to .menu_engine.* so .text's wildcards miss them. */
__rb_menu_engine_stock_resume = 0x00bf95cc;
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
}

ASSERT(__rb_menu_engine_start == RAM_PUSH_MENU_ENGINE_START, "menu engine region placement changed")
ASSERT(__rb_menu_engine_end <= RAM_PUSH_MENU_ENGINE_END, "menu engine overflows its region")
ASSERT(RAM_PUSH_MENU_ENGINE_START >= __ram_push_end, "menu engine region overlaps the installer")
ASSERT(!DEFINED(RAM_PUSH_BANK_VARIANT) || DEFINED(RAM_PUSH_MENU_VALUE) || DEFINED(RAM_PUSH_PROPERTY_PUBLICATION) || RAM_PUSH_MENU_ENGINE_START == RAM_PUSH_BASE + 0x1dd00, "render region start differs")
ASSERT(!DEFINED(RAM_PUSH_BANK_VARIANT) || DEFINED(RAM_PUSH_MENU_VALUE) || DEFINED(RAM_PUSH_PROPERTY_PUBLICATION) || RAM_PUSH_MENU_ENGINE_END == RAM_PUSH_BASE + 0x2dcc0, "render allocation end differs")
ASSERT(!DEFINED(RAM_PUSH_MENU_VALUE) || RAM_PUSH_MENU_ENGINE_END == RAM_PUSH_BASE + 0x33d00, "menu value allocation end differs")
ASSERT(!DEFINED(RAM_PUSH_PROPERTY_PUBLICATION) || RAM_PUSH_MENU_ENGINE_START == RAM_PUSH_BASE + 0x2dcc0, "property render region start differs")
ASSERT(!DEFINED(RAM_PUSH_PROPERTY_PUBLICATION) || RAM_PUSH_MENU_ENGINE_END == RAM_PUSH_BASE + 0x3dc80, "property render allocation end differs")
