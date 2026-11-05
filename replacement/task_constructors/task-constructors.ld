EXTERN(xm1_ctor_69f000_body xm1_ctor_d0a238_body xm1_ctor_d80d44_body xm1_ctor_db63e0_body xm1_ctor_dca604_body xm1_ctor_ec5350_body xm1_ctor_ed10b4_body xm1_ctor_ed2b00_body xm1_ctor_ef8c90_body xm1_ctor_1055d10_body xm1_ctor_1062090_body xm1_ctor_1066280_body xm1_ctor_10726f4_body)
xm1_create_queue = 0x724cdc;
xm1_submit_task = 0x725dd8;
xm1_usb_clear = 0xd81424;
xm1_record_initialize = 0xe66750;
xm1_record_service = 0x72d9f0;
xm1_record_finish = 0xdca5dc;
xm1_diag_initialize = 0xeea2b0;
xm1_sac_prepare = 0xeeb55c;
xm1_sac_empty = 0xee8e08;
xm1_sac_optional = 0x76fce4;
xm1_sac_capture = 0xfea408;
xm1_sac_f4c368 = 0xf4c368;
xm1_sac_10546c0 = 0x10546c0;
xm1_sac_1039500 = 0x1039500;
xm1_sac_eecbf4 = 0xeecbf4;
xm1_sac_1009974 = 0x1009974;
xm1_sac_1050efc = 0x1050efc;
xm1_sac_1051f30 = 0x1051f30;
xm1_sac_1053854 = 0x1053854;
xm1_sac_reset = 0xef7d44;
PHDRS {
 ctor69f000 PT_LOAD FLAGS(5);
 ctord0a238 PT_LOAD FLAGS(5);
 ctord80d44 PT_LOAD FLAGS(5);
 ctordb63e0 PT_LOAD FLAGS(5);
 ctordca604 PT_LOAD FLAGS(5);
 ctorec5350 PT_LOAD FLAGS(5);
 ctored10b4 PT_LOAD FLAGS(5);
 ctored2b00 PT_LOAD FLAGS(5);
 ctoref8c90 PT_LOAD FLAGS(5);
 ctor1055d10 PT_LOAD FLAGS(5);
 ctor1062090 PT_LOAD FLAGS(5);
 ctor1066280 PT_LOAD FLAGS(5);
 ctor10726f4 PT_LOAD FLAGS(5);
}
SECTIONS {
 .ctor_69f000 0x69f000 : AT(0x0) {
 __ctor_69f000_start = .;
 KEEP(*(.ctor.69f000.entry))
 KEEP(*(.ctor.69f000.body))
 *(.rodata.xm1_ctor_69f000_body*)
 __ctor_69f000_code_end = .;
 FILL(0)
 . = __ctor_69f000_start + 0x80;
 __ctor_69f000_end = .;
 } :ctor69f000
 __ctor_69f000_load = LOADADDR(.ctor_69f000);
 ASSERT(__ctor_69f000_code_end <= 0x69f080, "constructor 69f000 exceeds stock extent")
 .ctor_d0a238 0xd0a238 : AT(0x80) {
 __ctor_d0a238_start = .;
 KEEP(*(.ctor.d0a238.entry))
 KEEP(*(.ctor.d0a238.body))
 *(.rodata.xm1_ctor_d0a238_body*)
 __ctor_d0a238_code_end = .;
 FILL(0)
 . = __ctor_d0a238_start + 0x78;
 __ctor_d0a238_end = .;
 } :ctord0a238
 __ctor_d0a238_load = LOADADDR(.ctor_d0a238);
 ASSERT(__ctor_d0a238_code_end <= 0xd0a2b0, "constructor d0a238 exceeds stock extent")
 .ctor_d80d44 0xd80d44 : AT(0xf8) {
 __ctor_d80d44_start = .;
 KEEP(*(.ctor.d80d44.entry))
 KEEP(*(.ctor.d80d44.body))
 *(.rodata.xm1_ctor_d80d44_body*)
 __ctor_d80d44_code_end = .;
 FILL(0)
 . = __ctor_d80d44_start + 0x90;
 __ctor_d80d44_end = .;
 } :ctord80d44
 __ctor_d80d44_load = LOADADDR(.ctor_d80d44);
 ASSERT(__ctor_d80d44_code_end <= 0xd80dd4, "constructor d80d44 exceeds stock extent")
 .ctor_db63e0 0xdb63e0 : AT(0x188) {
 __ctor_db63e0_start = .;
 KEEP(*(.ctor.db63e0.entry))
 KEEP(*(.ctor.db63e0.body))
 *(.rodata.xm1_ctor_db63e0_body*)
 __ctor_db63e0_code_end = .;
 FILL(0)
 . = __ctor_db63e0_start + 0x78;
 __ctor_db63e0_end = .;
 } :ctordb63e0
 __ctor_db63e0_load = LOADADDR(.ctor_db63e0);
 ASSERT(__ctor_db63e0_code_end <= 0xdb6458, "constructor db63e0 exceeds stock extent")
 .ctor_dca604 0xdca604 : AT(0x200) {
 __ctor_dca604_start = .;
 KEEP(*(.ctor.dca604.entry))
 KEEP(*(.ctor.dca604.body))
 *(.rodata.xm1_ctor_dca604_body*)
 __ctor_dca604_code_end = .;
 FILL(0)
 . = __ctor_dca604_start + 0xac;
 __ctor_dca604_end = .;
 } :ctordca604
 __ctor_dca604_load = LOADADDR(.ctor_dca604);
 ASSERT(__ctor_dca604_code_end <= 0xdca6b0, "constructor dca604 exceeds stock extent")
 .ctor_ec5350 0xec5350 : AT(0x2ac) {
 __ctor_ec5350_start = .;
 KEEP(*(.ctor.ec5350.entry))
 KEEP(*(.ctor.ec5350.body))
 *(.rodata.xm1_ctor_ec5350_body*)
 __ctor_ec5350_code_end = .;
 FILL(0)
 . = __ctor_ec5350_start + 0x78;
 __ctor_ec5350_end = .;
 } :ctorec5350
 __ctor_ec5350_load = LOADADDR(.ctor_ec5350);
 ASSERT(__ctor_ec5350_code_end <= 0xec53c8, "constructor ec5350 exceeds stock extent")
 .ctor_ed10b4 0xed10b4 : AT(0x324) {
 __ctor_ed10b4_start = .;
 KEEP(*(.ctor.ed10b4.entry))
 KEEP(*(.ctor.ed10b4.body))
 *(.rodata.xm1_ctor_ed10b4_body*)
 __ctor_ed10b4_code_end = .;
 FILL(0)
 . = __ctor_ed10b4_start + 0x78;
 __ctor_ed10b4_end = .;
 } :ctored10b4
 __ctor_ed10b4_load = LOADADDR(.ctor_ed10b4);
 ASSERT(__ctor_ed10b4_code_end <= 0xed112c, "constructor ed10b4 exceeds stock extent")
 .ctor_ed2b00 0xed2b00 : AT(0x39c) {
 __ctor_ed2b00_start = .;
 KEEP(*(.ctor.ed2b00.entry))
 KEEP(*(.ctor.ed2b00.body))
 *(.rodata.xm1_ctor_ed2b00_body*)
 __ctor_ed2b00_code_end = .;
 FILL(0)
 . = __ctor_ed2b00_start + 0x78;
 __ctor_ed2b00_end = .;
 } :ctored2b00
 __ctor_ed2b00_load = LOADADDR(.ctor_ed2b00);
 ASSERT(__ctor_ed2b00_code_end <= 0xed2b78, "constructor ed2b00 exceeds stock extent")
 .ctor_ef8c90 0xef8c90 : AT(0x414) {
 __ctor_ef8c90_start = .;
 KEEP(*(.ctor.ef8c90.entry))
 KEEP(*(.ctor.ef8c90.body))
 *(.rodata.xm1_ctor_ef8c90_body*)
 __ctor_ef8c90_code_end = .;
 FILL(0)
 . = __ctor_ef8c90_start + 0x124;
 __ctor_ef8c90_end = .;
 } :ctoref8c90
 __ctor_ef8c90_load = LOADADDR(.ctor_ef8c90);
 ASSERT(__ctor_ef8c90_code_end <= 0xef8db4, "constructor ef8c90 exceeds stock extent")
 .ctor_1055d10 0x1055d10 : AT(0x538) {
 __ctor_1055d10_start = .;
 KEEP(*(.ctor.1055d10.entry))
 KEEP(*(.ctor.1055d10.body))
 *(.rodata.xm1_ctor_1055d10_body*)
 __ctor_1055d10_code_end = .;
 FILL(0)
 . = __ctor_1055d10_start + 0x74;
 __ctor_1055d10_end = .;
 } :ctor1055d10
 __ctor_1055d10_load = LOADADDR(.ctor_1055d10);
 ASSERT(__ctor_1055d10_code_end <= 0x1055d84, "constructor 1055d10 exceeds stock extent")
 .ctor_1062090 0x1062090 : AT(0x5ac) {
 __ctor_1062090_start = .;
 KEEP(*(.ctor.1062090.entry))
 KEEP(*(.ctor.1062090.body))
 *(.rodata.xm1_ctor_1062090_body*)
 __ctor_1062090_code_end = .;
 FILL(0)
 . = __ctor_1062090_start + 0x84;
 __ctor_1062090_end = .;
 } :ctor1062090
 __ctor_1062090_load = LOADADDR(.ctor_1062090);
 ASSERT(__ctor_1062090_code_end <= 0x1062114, "constructor 1062090 exceeds stock extent")
 .ctor_1066280 0x1066280 : AT(0x630) {
 __ctor_1066280_start = .;
 KEEP(*(.ctor.1066280.entry))
 KEEP(*(.ctor.1066280.body))
 *(.rodata.xm1_ctor_1066280_body*)
 __ctor_1066280_code_end = .;
 FILL(0)
 . = __ctor_1066280_start + 0x84;
 __ctor_1066280_end = .;
 } :ctor1066280
 __ctor_1066280_load = LOADADDR(.ctor_1066280);
 ASSERT(__ctor_1066280_code_end <= 0x1066304, "constructor 1066280 exceeds stock extent")
 .ctor_10726f4 0x10726f4 : AT(0x6b4) {
 __ctor_10726f4_start = .;
 KEEP(*(.ctor.10726f4.entry))
 KEEP(*(.ctor.10726f4.body))
 *(.rodata.xm1_ctor_10726f4_body*)
 __ctor_10726f4_code_end = .;
 FILL(0)
 . = __ctor_10726f4_start + 0x88;
 __ctor_10726f4_end = .;
 } :ctor10726f4
 __ctor_10726f4_load = LOADADDR(.ctor_10726f4);
 ASSERT(__ctor_10726f4_code_end <= 0x107277c, "constructor 10726f4 exceeds stock extent")
 .unexpected : { *(.text*) *(.rodata*) *(.data*) *(.bss*) *(.got*) *(COMMON) }
 .symtab 0 : { *(.symtab) }
 .strtab 0 : { *(.strtab) }
 .shstrtab 0 : { *(.shstrtab) }
 /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.eh_frame*) *(.ARM.attributes) *(.note*) *(.comment*) *(.debug*) *(.llvm_addrsig) }
}
ASSERT(SIZEOF(.unexpected) == 0, "unplaced constructor code or data")
