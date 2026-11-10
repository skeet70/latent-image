ENTRY(xm1_af_reserve)
xm1_af_critical_enter = 0x007235d8;
xm1_af_critical_leave = 0x007236b0;
xm1_af_secondary_mode = 0x0103e988;
MEMORY {
  selector (rx) : ORIGIN = 0x0103f190, LENGTH = 108
  rotate (rx) : ORIGIN = 0x0103f1fc, LENGTH = 352
  reserve (rx) : ORIGIN = 0x0103fb50, LENGTH = 488
  release (rx) : ORIGIN = 0x0103fd38, LENGTH = 80
  secondary (rx) : ORIGIN = 0x0103fd88, LENGTH = 740
}
SECTIONS {
  .af_select : {
    KEEP(*(.text.af_select))
    KEEP(*(.text.af_select_core))
    __af_select_end = .;
    . = ORIGIN(selector) + LENGTH(selector);
  } > selector =0xf000f0e7
  .af_rotate : {
    KEEP(*(.text.af_rotate))
    KEEP(*(.text.af_rotate_core))
    __af_rotate_end = .;
    . = ORIGIN(rotate) + LENGTH(rotate);
  } > rotate =0xf000f0e7
  .af_release : {
    KEEP(*(.text.af_release))
    __af_release_end = .;
    . = ORIGIN(release) + LENGTH(release);
  } > release =0xf000f0e7
  .af_reserve : {
    KEEP(*(.text.af_reserve))
    *(.text.*)
    __af_reserve_end = .;
    . = ORIGIN(reserve) + LENGTH(reserve);
  } > reserve =0xf000f0e7
  .forbidden (NOLOAD) : {
    *(.text .rodata .rodata.* .data .data.* .bss .bss.* COMMON)
  } > reserve
  .af_secondary : {
    KEEP(*(.af_secondary_entry))
    KEEP(*(.af_secondary_core))
    __af_secondary_end = .;
    . = ORIGIN(secondary) + LENGTH(secondary);
  } > secondary =0xf000f0e7
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}
ASSERT(xm1_af_select == 0x0103f190, "AF selector entry moved")
ASSERT(xm1_af_rotate == 0x0103f1fc, "AF rotate entry moved")
ASSERT(xm1_af_reserve == 0x0103fb50, "AF reserve entry moved")
ASSERT(xm1_af_release == 0x0103fd38, "AF release entry moved")
ASSERT(xm1_af_secondary_consumer == 0x0103fd88, "AF secondary consumer entry moved")
ASSERT(__af_select_end <= 0x0103f1fc, "AF selector overflow")
ASSERT(__af_rotate_end <= 0x0103f35c, "AF rotate overflow")
ASSERT(__af_reserve_end <= 0x0103fd38, "AF reserve overflow")
ASSERT(__af_release_end <= 0x0103fd88, "AF release overflow")
ASSERT(__af_secondary_end <= 0x0104006c, "AF secondary consumer overflow")
ASSERT(SIZEOF(.forbidden) == 0, "AF unplaced code or writable data")
