/* Placement for the recipe-bank UI model: every trampoline target sits at the
 * byte-proven stock address the blob branches to, each run stays inside one
 * case region, and the model's data lands in the cells region's free tail.
 * The generator extracts one chunk per run (.text.a/.text.b/.text.c)
 * and reads the function symbols for the case's function list. */
ENTRY(ui_run)
SECTIONS {
  . = 0x00957e00;
  .data : { *(.data*) *(.bss*) *(COMMON) *(.rodata*) }
  .text.a 0x00c12aac : {
    KEEP(*(.text.ui_transition))
  }
  .text.b 0x00c72e10 : {
    KEEP(*(.text.ui_set_dispatch))
    . = 0x00c75ed4;
    KEEP(*(.text.ui_editor_confirm))
    . = 0x00c76650;
    KEEP(*(.text.ui_leave_edge))
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
ui_span_b_lo = ADDR(.text.b);
ui_span_b_hi = ADDR(.text.b) + SIZEOF(.text.b);
ui_span_c_lo = ADDR(.text.c);
ui_span_c_hi = ADDR(.text.c) + SIZEOF(.text.c);
ASSERT(ui_transition == 0x00c12aac, "transition at stock address")
ASSERT(ui_set_dispatch == 0x00c72e10, "set dispatch at stock address")
ASSERT(ui_editor_confirm == 0x00c75ed4, "editor confirm at stock address")
ASSERT(ui_leave_edge == 0x00c76650, "leave edge at stock address")
ASSERT(ui_redraw == 0x00d38d98, "redraw at stock address")
ASSERT(ui_postlude == 0x00d38f34, "postlude at stock address")
ASSERT(ui_stage_open == 0x00d3d858, "stage open at stock address")
ASSERT(ADDR(.data) + SIZEOF(.data) <= 0x00959000, "model data precedes the case stack")
ASSERT(ui_span_a_hi <= 0x00c12c00, "transition fits its case region")
ASSERT(ui_span_b_hi <= 0x00c76800, "delegates fit their case region")
ASSERT(ui_span_c_hi <= 0x00d4d604, "model text precedes the stock consumers")
ASSERT((ui_exit_boundary & 3) == 0, "exit boundary is ARM-aligned")
ASSERT(ui_exit_boundary + 4 == ui_span_c_hi, "exit boundary follows every model function")
