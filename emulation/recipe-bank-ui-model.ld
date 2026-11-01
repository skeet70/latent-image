/* Retained primitive addresses for the recipe-bank UI guest ABI consumer.
 * Model data occupies the cells region's free tail. */
ENTRY(ui_run)
SECTIONS {
  . = 0x00957e00;
  .data : { *(.data*) *(.bss*) *(COMMON) *(.rodata*) }
  .text.a 0x00c12aac : {
    KEEP(*(.text.ui_transition))
  }
  .text.c 0x00d38d98 : {
    KEEP(*(.text.ui_redraw))
    . = 0x00d38f34;
    KEEP(*(.text.ui_postlude))
    . = 0x00d3b000;
    KEEP(*(.text.ui_run))
    . = 0x00d3d858;
    KEEP(*(.text.ui_stage_open))
    /* every remaining model function, helpers included */
    KEEP(*(.text.ui_*))
    . = ALIGN(4);
    ui_exit_boundary = .;
    LONG(0xe320f000)
  }
  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) }
}
ui_span_a_lo = ADDR(.text.a);
ui_span_a_hi = ADDR(.text.a) + SIZEOF(.text.a);
ui_span_c_lo = ADDR(.text.c);
ui_span_c_hi = ADDR(.text.c) + SIZEOF(.text.c);
ASSERT(ui_transition == 0x00c12aac, "transition at stock address")
ASSERT(ui_redraw == 0x00d38d98, "redraw at stock address")
ASSERT(ui_postlude == 0x00d38f34, "postlude at stock address")
ASSERT(ui_stage_open == 0x00d3d858, "stage open at stock address")
ASSERT(ADDR(.data) + SIZEOF(.data) <= 0x00959000, "model data precedes the case stack")
ASSERT(ui_span_a_hi <= 0x00c12c00, "transition fits its case region")
ASSERT(ui_span_c_hi <= 0x00d49adc, "model text precedes the retained editor primitive")
ASSERT((ui_exit_boundary & 3) == 0, "exit boundary is ARM-aligned")
ASSERT(ui_exit_boundary + 4 == ui_span_c_hi, "exit boundary follows every model function")
