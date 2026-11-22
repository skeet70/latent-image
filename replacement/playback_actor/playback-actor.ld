/* Each machine owns its stock action extent in the plain image and its action-table rows.
   Sections stay in address order; the highest machine extent also takes the code no machine
   names. */
ENTRY(xm1_playback_m0_action02)
INCLUDE imports.ld
SECTIONS {
  .compiler_builtins 0 : { *compiler_builtins*(.text* .rodata* .data* .bss*) }
  .m0_actions 0x011e8ce4 : { *(.text.xm1_playback_m0_*) *(.text.*machine0*) }
  .m30_actions 0x011f31bc : { *(.text.xm1_playback_m30_*) *(.text.*machine30*) *(.text*) *(.rodata*) }
  .m0_table 0x01218ec8 : { KEEP(*(.playback.m0.table)) }
  .m30_table 0x01219890 : { KEEP(*(.playback.m30.table)) }
  .data : { *(.data*) *(.sdata*) *(.got*) }
  .bss : { *(.bss*) *(.sbss*) *(COMMON) }
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.eh_frame*) *(.ARM.attributes) *(.note*) *(.comment*) *(.debug*) *(.llvm_addrsig) }
}
ASSERT(ADDR(.m0_actions) + SIZEOF(.m0_actions) <= 0x011ea434, "machine 0 actions overflow [0x011e8ce4,0x011ea434)")
ASSERT(SIZEOF(.m0_table) == 124, "machine 0 table is not 31 rows")
ASSERT(ADDR(.m30_actions) + SIZEOF(.m30_actions) <= 0x011f4810, "machine 30 actions overflow [0x011f31bc,0x011f4810)")
ASSERT(SIZEOF(.m30_table) == 136, "machine 30 table is not 34 rows")
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "playback actor has writable statics")
ASSERT(SIZEOF(.compiler_builtins) == 0, "playback actor links compiler-builtins")
