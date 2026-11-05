/* Loader B: owned own-first slot bodies at their stock extents (regions.json). */
ENTRY(xm1_rtos_own_first_slot_60)

MEMORY {
  slot_60 (rx) : ORIGIN = 0xd7500, LENGTH = 0x40
  slot_fc (rx) : ORIGIN = 0xdbd0c, LENGTH = 0x78
  slot_100 (rx) : ORIGIN = 0xdbd84, LENGTH = 0x78
  slot_128 (rx) : ORIGIN = 0xdc5e0, LENGTH = 0x88
  slot_12c (rx) : ORIGIN = 0xdc668, LENGTH = 0x94
  slot_130 (rx) : ORIGIN = 0xdc86c, LENGTH = 0x4c
  slot_134 (rx) : ORIGIN = 0xdc8b8, LENGTH = 0x4c
  slot_138 (rx) : ORIGIN = 0xdc904, LENGTH = 0x4c
  slot_13c (rx) : ORIGIN = 0xdc950, LENGTH = 0x4c
  slot_148 (rx) : ORIGIN = 0xdcb98, LENGTH = 0x28
  slot_14c (rx) : ORIGIN = 0xdcbc0, LENGTH = 0x1c
  slot_150 (rx) : ORIGIN = 0xdcbdc, LENGTH = 0x28
  slot_154 (rx) : ORIGIN = 0xdcc04, LENGTH = 0x50
  slot_164 (rx) : ORIGIN = 0xd1140, LENGTH = 0x24
  slot_168 (rx) : ORIGIN = 0xd1164, LENGTH = 0x28
  slot_16c (rx) : ORIGIN = 0xd118c, LENGTH = 0x40
  slot_1618 (rx) : ORIGIN = 0xd12bc, LENGTH = 0x10
}

SECTIONS {
  .slot_60 : {
    KEEP(*(.text.slot_60))
    __slot_60_body_end = .;
    . = ORIGIN(slot_60) + LENGTH(slot_60);
  } > slot_60 =0xf000f0e7
  .slot_fc : {
    KEEP(*(.text.slot_fc))
    __slot_fc_body_end = .;
    . = ORIGIN(slot_fc) + LENGTH(slot_fc);
  } > slot_fc =0xf000f0e7
  .slot_100 : {
    KEEP(*(.text.slot_100))
    __slot_100_body_end = .;
    . = ORIGIN(slot_100) + LENGTH(slot_100);
  } > slot_100 =0xf000f0e7
  .slot_128 : {
    KEEP(*(.text.slot_128))
    __slot_128_body_end = .;
    . = ORIGIN(slot_128) + LENGTH(slot_128);
  } > slot_128 =0xf000f0e7
  .slot_12c : {
    KEEP(*(.text.slot_12c))
    __slot_12c_body_end = .;
    . = ORIGIN(slot_12c) + LENGTH(slot_12c) - 0x20;
    __slot_1618_body_start = .;
    KEEP(*(.text.slot_1618_body))
    __slot_1618_body_end = .;
    . = ORIGIN(slot_12c) + LENGTH(slot_12c);
  } > slot_12c =0xf000f0e7
  .slot_130 : {
    KEEP(*(.text.slot_130))
    __slot_130_body_end = .;
    . = ORIGIN(slot_130) + LENGTH(slot_130);
  } > slot_130 =0xf000f0e7
  .slot_134 : {
    KEEP(*(.text.slot_134))
    __slot_134_body_end = .;
    . = ORIGIN(slot_134) + LENGTH(slot_134);
  } > slot_134 =0xf000f0e7
  .slot_138 : {
    KEEP(*(.text.slot_138))
    __slot_138_body_end = .;
    . = ORIGIN(slot_138) + LENGTH(slot_138);
  } > slot_138 =0xf000f0e7
  .slot_13c : {
    KEEP(*(.text.slot_13c))
    __slot_13c_body_end = .;
    . = ORIGIN(slot_13c) + LENGTH(slot_13c);
  } > slot_13c =0xf000f0e7
  .slot_148 : {
    KEEP(*(.text.slot_148))
    __slot_148_body_end = .;
    . = ORIGIN(slot_148) + LENGTH(slot_148);
  } > slot_148 =0xf000f0e7
  .slot_14c : {
    KEEP(*(.text.slot_14c))
    __slot_14c_body_end = .;
    . = ORIGIN(slot_14c) + LENGTH(slot_14c);
  } > slot_14c =0xf000f0e7
  .slot_150 : {
    KEEP(*(.text.slot_150))
    __slot_150_body_end = .;
    . = ORIGIN(slot_150) + LENGTH(slot_150);
  } > slot_150 =0xf000f0e7
  .slot_154 : {
    KEEP(*(.text.slot_154))
    __slot_154_body_end = .;
    . = ORIGIN(slot_154) + LENGTH(slot_154);
  } > slot_154 =0xf000f0e7
  .slot_164 : {
    KEEP(*(.text.slot_164))
    __slot_164_body_end = .;
    . = ORIGIN(slot_164) + LENGTH(slot_164);
  } > slot_164 =0xf000f0e7
  .slot_168 : {
    KEEP(*(.text.slot_168))
    __slot_168_body_end = .;
    . = ORIGIN(slot_168) + LENGTH(slot_168);
  } > slot_168 =0xf000f0e7
  .slot_16c : {
    KEEP(*(.text.slot_16c))
    __slot_16c_body_end = .;
    . = ORIGIN(slot_16c) + LENGTH(slot_16c);
  } > slot_16c =0xf000f0e7
  .slot_1618 : {
    KEEP(*(.text.slot_1618))
    __slot_1618_branch_end = .;
    . = ORIGIN(slot_1618) + LENGTH(slot_1618);
  } > slot_1618 =0xf000f0e7
  .forbidden (NOLOAD) : { *(.rodata .rodata.* .data .data.* .bss .bss.* COMMON) } > slot_60
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}

__slot_60_end = ORIGIN(slot_60) + LENGTH(slot_60);
__slot_fc_end = ORIGIN(slot_fc) + LENGTH(slot_fc);
__slot_100_end = ORIGIN(slot_100) + LENGTH(slot_100);
__slot_128_end = ORIGIN(slot_128) + LENGTH(slot_128);
__slot_12c_end = ORIGIN(slot_12c) + LENGTH(slot_12c);
__slot_130_end = ORIGIN(slot_130) + LENGTH(slot_130);
__slot_134_end = ORIGIN(slot_134) + LENGTH(slot_134);
__slot_138_end = ORIGIN(slot_138) + LENGTH(slot_138);
__slot_13c_end = ORIGIN(slot_13c) + LENGTH(slot_13c);
__slot_148_end = ORIGIN(slot_148) + LENGTH(slot_148);
__slot_14c_end = ORIGIN(slot_14c) + LENGTH(slot_14c);
__slot_150_end = ORIGIN(slot_150) + LENGTH(slot_150);
__slot_154_end = ORIGIN(slot_154) + LENGTH(slot_154);
__slot_164_end = ORIGIN(slot_164) + LENGTH(slot_164);
__slot_168_end = ORIGIN(slot_168) + LENGTH(slot_168);
__slot_16c_end = ORIGIN(slot_16c) + LENGTH(slot_16c);
__slot_1618_end = ORIGIN(slot_1618) + LENGTH(slot_1618);

ASSERT(xm1_rtos_own_first_slot_60 == ORIGIN(slot_60), "+0x60 entry is not at its stock address")
ASSERT(__slot_60_body_end <= __slot_60_end, "+0x60 body exceeds its stock extent")
ASSERT(xm1_rtos_own_first_slot_fc == ORIGIN(slot_fc), "+0xfc entry is not at its stock address")
ASSERT(__slot_fc_body_end <= __slot_fc_end, "+0xfc body exceeds its stock extent")
ASSERT(xm1_rtos_own_first_slot_100 == ORIGIN(slot_100), "+0x100 entry is not at its stock address")
ASSERT(__slot_100_body_end <= __slot_100_end, "+0x100 body exceeds its stock extent")
ASSERT(xm1_rtos_own_first_slot_128 == ORIGIN(slot_128), "+0x128 entry is not at its stock address")
ASSERT(__slot_128_body_end <= __slot_128_end, "+0x128 body exceeds its stock extent")
ASSERT(xm1_rtos_own_first_slot_12c == ORIGIN(slot_12c), "+0x12c entry is not at its stock address")
ASSERT(__slot_12c_body_end <= __slot_12c_end, "+0x12c body exceeds its stock extent")
ASSERT(xm1_rtos_own_first_slot_130 == ORIGIN(slot_130), "+0x130 entry is not at its stock address")
ASSERT(__slot_130_body_end <= __slot_130_end, "+0x130 body exceeds its stock extent")
ASSERT(xm1_rtos_own_first_slot_134 == ORIGIN(slot_134), "+0x134 entry is not at its stock address")
ASSERT(__slot_134_body_end <= __slot_134_end, "+0x134 body exceeds its stock extent")
ASSERT(xm1_rtos_own_first_slot_138 == ORIGIN(slot_138), "+0x138 entry is not at its stock address")
ASSERT(__slot_138_body_end <= __slot_138_end, "+0x138 body exceeds its stock extent")
ASSERT(xm1_rtos_own_first_slot_13c == ORIGIN(slot_13c), "+0x13c entry is not at its stock address")
ASSERT(__slot_13c_body_end <= __slot_13c_end, "+0x13c body exceeds its stock extent")
ASSERT(xm1_rtos_own_first_slot_148 == ORIGIN(slot_148), "+0x148 entry is not at its stock address")
ASSERT(__slot_148_body_end <= __slot_148_end, "+0x148 body exceeds its stock extent")
ASSERT(xm1_rtos_own_first_slot_14c == ORIGIN(slot_14c), "+0x14c entry is not at its stock address")
ASSERT(__slot_14c_body_end <= __slot_14c_end, "+0x14c body exceeds its stock extent")
ASSERT(xm1_rtos_own_first_slot_150 == ORIGIN(slot_150), "+0x150 entry is not at its stock address")
ASSERT(__slot_150_body_end <= __slot_150_end, "+0x150 body exceeds its stock extent")
ASSERT(xm1_rtos_own_first_slot_154 == ORIGIN(slot_154), "+0x154 entry is not at its stock address")
ASSERT(__slot_154_body_end <= __slot_154_end, "+0x154 body exceeds its stock extent")
ASSERT(xm1_rtos_own_first_slot_164 == ORIGIN(slot_164), "+0x164 entry is not at its stock address")
ASSERT(__slot_164_body_end <= __slot_164_end, "+0x164 body exceeds its stock extent")
ASSERT(xm1_rtos_own_first_slot_168 == ORIGIN(slot_168), "+0x168 entry is not at its stock address")
ASSERT(__slot_168_body_end <= __slot_168_end, "+0x168 body exceeds its stock extent")
ASSERT(xm1_rtos_own_first_slot_16c == ORIGIN(slot_16c), "+0x16c entry is not at its stock address")
ASSERT(__slot_16c_body_end <= __slot_16c_end, "+0x16c body exceeds its stock extent")
ASSERT(xm1_rtos_own_first_slot_1618 == ORIGIN(slot_1618), "+0x1618 entry is not at its stock address")
ASSERT(__slot_1618_branch_end <= __slot_1618_end, "+0x1618 branch exceeds its stock extent")
ASSERT(__slot_12c_body_end <= __slot_1618_body_start, "+0x12c body overlaps the +0x1618 body")
ASSERT(xm1_rtos_own_first_slot_1618_body == __slot_1618_body_start, "+0x1618 body is not at its reserved address")
ASSERT(__slot_1618_body_end <= __slot_12c_end, "+0x1618 body exceeds the +0x12c extent")
ASSERT(SIZEOF(.forbidden) == 0, "rodata, data or bss present")
