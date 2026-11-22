/* The staged machines. Each owns its stock action extent in the plain image and its
   action-table rows. Sections stay in address order; the highest machine extent also takes the
   code no machine names. A machine's code is its exports and the items whose mangled path names
   its module. */
ENTRY(xm1_playback_m0_action02)
INCLUDE imports.ld
SECTIONS {
  .compiler_builtins 0 : { *compiler_builtins*(.text* .rodata* .data* .bss*) }
  .m0_actions 0x011e8ce4 : { *(.text.xm1_playback_m0_*) *(.text.*8machine0*) }
  .m7_actions 0x011eb824 : { *(.text.xm1_playback_m7_*) *(.text.*8machine7*) *(.text*) *(.rodata*) }
  .m0_table 0x01218ec8 : { KEEP(*(.playback.m0.table)) }
  .m7_table 0x0121a498 : { KEEP(*(.playback.m7.table)) }
  .data : { *(.data*) *(.sdata*) *(.got*) }
  .bss : { *(.bss*) *(.sbss*) *(COMMON) }
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.eh_frame*) *(.ARM.attributes) *(.note*) *(.comment*) *(.debug*) *(.llvm_addrsig) }
}
ASSERT(ADDR(.m0_actions) + SIZEOF(.m0_actions) <= 0x011ea434, "machine 0 actions overflow [0x011e8ce4,0x011ea434)")
ASSERT(SIZEOF(.m0_table) == 124, "machine 0 table is not 31 rows")
ASSERT(ADDR(.m7_actions) + SIZEOF(.m7_actions) <= 0x011ec980, "machine 7 actions overflow [0x011eb824,0x011ec980)")
ASSERT(SIZEOF(.m7_table) == 196, "machine 7 table is not 49 rows")
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "playback actor has writable statics")
ASSERT(SIZEOF(.compiler_builtins) == 0, "playback actor links compiler-builtins")
