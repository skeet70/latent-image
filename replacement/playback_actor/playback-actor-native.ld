/* The staged machine 0 and the native-only machines 7, 30 and 61, laid out as in
   playback-actor.ld; the highest machine extent takes the code no machine names. */
ENTRY(xm1_playback_m0_action02)
INCLUDE imports.ld
INCLUDE imports-native.ld
SECTIONS {
  .compiler_builtins 0 : { *compiler_builtins*(.text* .rodata* .data* .bss*) }
  .m0_actions 0x011e8ce4 : { *(.text.xm1_playback_m0_*) *(.text.*8machine0*) }
  .m7_actions 0x011eb824 : { *(.text.xm1_playback_m7_*) *(.text.*8machine7*) }
  .m30_actions 0x011f31bc : { *(.text.xm1_playback_m30_*) *(.text.*9machine30*) }
  .m61_actions 0x011fe4d0 : { *(.text.xm1_playback_m61_*) *(.text.*9machine61*) *(.text*) *(.rodata*) }
  .m61_table 0x01215c34 : { KEEP(*(.playback.m61.table)) }
  .m0_table 0x01218ec8 : { KEEP(*(.playback.m0.table)) }
  .m30_table 0x01219890 : { KEEP(*(.playback.m30.table)) }
  .m7_table 0x0121a498 : { KEEP(*(.playback.m7.table)) }
  .data : { *(.data*) *(.sdata*) *(.got*) }
  .bss : { *(.bss*) *(.sbss*) *(COMMON) }
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.eh_frame*) *(.ARM.attributes) *(.note*) *(.comment*) *(.debug*) *(.llvm_addrsig) }
}
ASSERT(ADDR(.m0_actions) + SIZEOF(.m0_actions) <= 0x011ea434, "machine 0 actions overflow [0x011e8ce4,0x011ea434)")
ASSERT(SIZEOF(.m0_table) == 124, "machine 0 table is not 31 rows")
ASSERT(ADDR(.m7_actions) + SIZEOF(.m7_actions) <= 0x011ec980, "machine 7 actions overflow [0x011eb824,0x011ec980)")
ASSERT(SIZEOF(.m7_table) == 196, "machine 7 table is not 49 rows")
ASSERT(ADDR(.m30_actions) + SIZEOF(.m30_actions) <= 0x011f4810, "machine 30 actions overflow [0x011f31bc,0x011f4810)")
ASSERT(SIZEOF(.m30_table) == 136, "machine 30 table is not 34 rows")
ASSERT(ADDR(.m61_actions) + SIZEOF(.m61_actions) <= 0x011ff0dc, "machine 61 actions overflow [0x011fe4d0,0x011ff0dc)")
ASSERT(SIZEOF(.m61_table) == 104, "machine 61 table is not 26 rows")
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "playback actor has writable statics")
ASSERT(SIZEOF(.compiler_builtins) == 0, "playback actor links compiler-builtins")
