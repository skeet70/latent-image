/* The staged machines. Each owns its stock action extent in the plain image and its
   action-table rows. Sections stay in address order; the highest machine extent also takes the
   code no machine names. A machine's code is its exports and the items whose mangled path names
   its module. */
ENTRY(xm1_playback_m0_action02)
INCLUDE imports.ld
SECTIONS {
  .compiler_builtins 0 : { *compiler_builtins*(.text* .rodata* .data* .bss*) }
  .m0_actions 0x011e8ce4 : { *(.text.xm1_playback_m0_*) *(.text.*8machine0*) }
  .m1_actions 0x011ea434 : { *(.text.xm1_playback_m1_*) *(.text.*8machine1*) }
  .m2_actions 0x011ea5f0 : { *(.text.xm1_playback_m2_*) *(.text.*8machine2*) }
  .m3_actions 0x011eaa54 : { *(.text.xm1_playback_m3_*) *(.text.*8machine3*) }
  .m4_actions 0x011eacb0 : { *(.text.xm1_playback_m4_*) *(.text.*8machine4*) }
  .m5_actions 0x011eb31c : { *(.text.xm1_playback_m5_*) *(.text.*8machine5*) }
  .m6_actions 0x011eb500 : { *(.text.xm1_playback_m6_*) *(.text.*8machine6*) }
  .m7_actions 0x011eb824 : { *(.text.xm1_playback_m7_*) *(.text.*8machine7*) }
  .m8_actions 0x011ec980 : { *(.text.xm1_playback_m8_*) *(.text.*8machine8*) }
  .m30_actions 0x011f31bc : { *(.text.xm1_playback_m30_*) *(.text.*9machine30*) }
  .m34_actions 0x011f62c8 : { *(.text.xm1_playback_m34_*) *(.text.*9machine34*) }
  .m44_actions 0x011f97b4 : { *(.text.xm1_playback_m44_*) *(.text.*9machine44*) }
  .m45_actions 0x011fa1b4 : { *(.text.xm1_playback_m45_*) *(.text.*9machine45*) }
  .m46_actions 0x011fa618 : { *(.text.xm1_playback_m46_*) *(.text.*9machine46*) }
  .m47_actions 0x011facec : { *(.text.xm1_playback_m47_*) *(.text.*9machine47*) }
  .m56_actions 0x011fd988 : { *(.text.xm1_playback_m56_*) *(.text.*9machine56*) }
  .m57_actions 0x011fdc1c : { *(.text.xm1_playback_m57_*) *(.text.*9machine57*) }
  .m61_actions 0x011fe4d0 : { *(.text.xm1_playback_m61_*) *(.text.*9machine61*) }
  .m66_actions 0x011ffd3c : { *(.text.xm1_playback_m66_*) *(.text.*9machine66*) }
  .m70_actions 0x01200f58 : { *(.text.xm1_playback_m70_*) *(.text.*9machine70*) *(.text*) *(.rodata*) }
  .m61_table 0x01215c34 : { KEEP(*(.playback.m61.table)) }
  .m70_table 0x01216a18 : { KEEP(*(.playback.m70.table)) }
  .m57_table 0x01216bfc : { KEEP(*(.playback.m57.table)) }
  .m46_table 0x01216ea8 : { KEEP(*(.playback.m46.table)) }
  .m56_table 0x01217358 : { KEEP(*(.playback.m56.table)) }
  .m2_table 0x012173a4 : { KEEP(*(.playback.m2.table)) }
  .m5_table 0x012175c4 : { KEEP(*(.playback.m5.table)) }
  .m47_table 0x01217710 : { KEEP(*(.playback.m47.table)) }
  .m34_table 0x012177f4 : { KEEP(*(.playback.m34.table)) }
  .m45_table 0x01217b50 : { KEEP(*(.playback.m45.table)) }
  .m44_table 0x01217c8c : { KEEP(*(.playback.m44.table)) }
  .m1_table 0x01218d98 : { KEEP(*(.playback.m1.table)) }
  .m3_table 0x01218e9c : { KEEP(*(.playback.m3.table)) }
  .m0_table 0x01218ec8 : { KEEP(*(.playback.m0.table)) }
  .m66_table 0x01218f64 : { KEEP(*(.playback.m66.table)) }
  .m6_table 0x012193c8 : { KEEP(*(.playback.m6.table)) }
  .m30_table 0x01219890 : { KEEP(*(.playback.m30.table)) }
  .m8_table 0x0121a398 : { KEEP(*(.playback.m8.table)) }
  .m4_table 0x0121a3c8 : { KEEP(*(.playback.m4.table)) }
  .m7_table 0x0121a498 : { KEEP(*(.playback.m7.table)) }
  .data : { *(.data*) *(.sdata*) *(.got*) }
  .bss : { *(.bss*) *(.sbss*) *(COMMON) }
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.eh_frame*) *(.ARM.attributes) *(.note*) *(.comment*) *(.debug*) *(.llvm_addrsig) }
}
ASSERT(ADDR(.m0_actions) + SIZEOF(.m0_actions) <= 0x011ea434, "machine 0 actions overflow [0x011e8ce4,0x011ea434)")
ASSERT(SIZEOF(.m0_table) == 124, "machine 0 table is not 31 rows")
ASSERT(ADDR(.m1_actions) + SIZEOF(.m1_actions) <= 0x011ea5f0, "machine 1 actions overflow [0x011ea434,0x011ea5f0)")
ASSERT(SIZEOF(.m1_table) == 56, "machine 1 table is not 14 rows")
ASSERT(ADDR(.m2_actions) + SIZEOF(.m2_actions) <= 0x011eaa54, "machine 2 actions overflow [0x011ea5f0,0x011eaa54)")
ASSERT(SIZEOF(.m2_table) == 56, "machine 2 table is not 14 rows")
ASSERT(ADDR(.m3_actions) + SIZEOF(.m3_actions) <= 0x011eacb0, "machine 3 actions overflow [0x011eaa54,0x011eacb0)")
ASSERT(SIZEOF(.m3_table) == 36, "machine 3 table is not 9 rows")
ASSERT(ADDR(.m4_actions) + SIZEOF(.m4_actions) <= 0x011eb31c, "machine 4 actions overflow [0x011eacb0,0x011eb31c)")
ASSERT(SIZEOF(.m4_table) == 72, "machine 4 table is not 18 rows")
ASSERT(ADDR(.m5_actions) + SIZEOF(.m5_actions) <= 0x011eb500, "machine 5 actions overflow [0x011eb31c,0x011eb500)")
ASSERT(SIZEOF(.m5_table) == 36, "machine 5 table is not 9 rows")
ASSERT(ADDR(.m6_actions) + SIZEOF(.m6_actions) <= 0x011eb824, "machine 6 actions overflow [0x011eb500,0x011eb824)")
ASSERT(SIZEOF(.m6_table) == 40, "machine 6 table is not 10 rows")
ASSERT(ADDR(.m7_actions) + SIZEOF(.m7_actions) <= 0x011ec980, "machine 7 actions overflow [0x011eb824,0x011ec980)")
ASSERT(SIZEOF(.m7_table) == 196, "machine 7 table is not 49 rows")
ASSERT(ADDR(.m8_actions) + SIZEOF(.m8_actions) <= 0x011ecb1c, "machine 8 actions overflow [0x011ec980,0x011ecb1c)")
ASSERT(SIZEOF(.m8_table) == 40, "machine 8 table is not 10 rows")
ASSERT(ADDR(.m30_actions) + SIZEOF(.m30_actions) <= 0x011f4810, "machine 30 actions overflow [0x011f31bc,0x011f4810)")
ASSERT(SIZEOF(.m30_table) == 136, "machine 30 table is not 34 rows")
ASSERT(ADDR(.m34_actions) + SIZEOF(.m34_actions) <= 0x011f66ec, "machine 34 actions overflow [0x011f62c8,0x011f66ec)")
ASSERT(SIZEOF(.m34_table) == 52, "machine 34 table is not 13 rows")
ASSERT(ADDR(.m44_actions) + SIZEOF(.m44_actions) <= 0x011fa1b4, "machine 44 actions overflow [0x011f97b4,0x011fa1b4)")
ASSERT(SIZEOF(.m44_table) == 76, "machine 44 table is not 19 rows")
ASSERT(ADDR(.m45_actions) + SIZEOF(.m45_actions) <= 0x011fa618, "machine 45 actions overflow [0x011fa1b4,0x011fa618)")
ASSERT(SIZEOF(.m45_table) == 36, "machine 45 table is not 9 rows")
ASSERT(ADDR(.m46_actions) + SIZEOF(.m46_actions) <= 0x011facec, "machine 46 actions overflow [0x011fa618,0x011facec)")
ASSERT(SIZEOF(.m46_table) == 56, "machine 46 table is not 14 rows")
ASSERT(ADDR(.m47_actions) + SIZEOF(.m47_actions) <= 0x011fb44c, "machine 47 actions overflow [0x011facec,0x011fb44c)")
ASSERT(SIZEOF(.m47_table) == 48, "machine 47 table is not 12 rows")
ASSERT(ADDR(.m56_actions) + SIZEOF(.m56_actions) <= 0x011fdc1c, "machine 56 actions overflow [0x011fd988,0x011fdc1c)")
ASSERT(SIZEOF(.m56_table) == 68, "machine 56 table is not 17 rows")
ASSERT(ADDR(.m57_actions) + SIZEOF(.m57_actions) <= 0x011fdd38, "machine 57 actions overflow [0x011fdc1c,0x011fdd38)")
ASSERT(SIZEOF(.m57_table) == 28, "machine 57 table is not 7 rows")
ASSERT(ADDR(.m61_actions) + SIZEOF(.m61_actions) <= 0x011ff0dc, "machine 61 actions overflow [0x011fe4d0,0x011ff0dc)")
ASSERT(SIZEOF(.m61_table) == 104, "machine 61 table is not 26 rows")
ASSERT(ADDR(.m66_actions) + SIZEOF(.m66_actions) <= 0x011ffed0, "machine 66 actions overflow [0x011ffd3c,0x011ffed0)")
ASSERT(SIZEOF(.m66_table) == 28, "machine 66 table is not 7 rows")
ASSERT(ADDR(.m70_actions) + SIZEOF(.m70_actions) <= 0x01201814, "machine 70 actions overflow [0x01200f58,0x01201814)")
ASSERT(SIZEOF(.m70_table) == 84, "machine 70 table is not 21 rows")
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "playback actor has writable statics")
ASSERT(SIZEOF(.compiler_builtins) == 0, "playback actor links compiler-builtins")
