/* Placement of the Classic Chrome stubs (reports/classic-chrome-patch.md
 * sections 6.1 and 6.2). Each blob starts at its converter's entry and stays
 * inside that function's stock extent. The data block is the first 100 bytes
 * of the image-4 decode prefix at load 0x0061c200. */
ENTRY(cc_convert_capture)

cc_data = 0x0061c200;
cc_matrix = cc_data + 0x04;
cc_chroma_fields = cc_data + 0x16;
cc_tone_head = cc_data + 0x20;

stock_colour_read = 0x00d670a0;
stock_chroma_read = 0x00d60cc4;
stock_tone_read = 0x00690924;

SECTIONS {
  .settings 0x00736a74 : { *(.text.settings) }
  .capture 0x00738fe8 : { *(.text.capture) }
  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) *(.text) *(.rodata*) *(.data*) *(.bss*) *(COMMON) }
}

ASSERT(cc_convert_capture == 0x00738fe8, "capture converter at FUN_00738fe8")
ASSERT(cc_convert_settings == 0x00736a74, "settings converter at FUN_00736a74")
ASSERT(SIZEOF(.capture) <= 0x64, "capture blob inside [0x00738fe8, 0x0073904c)")
ASSERT(SIZEOF(.settings) <= 0x68, "settings blob inside [0x00736a74, 0x00736adc)")
