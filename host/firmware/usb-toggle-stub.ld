/* Placement of the USB personality toggle stub
 * (reports/usb-personality-toggle.md section 5): the unreferenced image-5
 * function extent [0x00bf4454, 0x00bf44e4). The four callees are stock
 * image-4 functions. */
ENTRY(usb_toggle)

stock_cache_byte = 0x0069f8fc;
stock_cache_store = 0x0069fc94;
stock_key_state = 0x006a0d00;
stock_system_mode = 0x0073c15c;

SECTIONS {
  .toggle 0x00bf4454 : { *(.text.toggle) }
  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) *(.text) *(.rodata*) *(.data*) *(.bss*) *(COMMON) }
}

ASSERT(usb_toggle == 0x00bf4454, "stub at the extent start")
ASSERT(SIZEOF(.toggle) <= 0x90, "stub inside [0x00bf4454, 0x00bf44e4)")
