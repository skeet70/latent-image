ENTRY(rb_value_picker_case)
SECTIONS {
  . = 0x02000000;
  .text : { KEEP(*(.text.entry)) *(.text*) }
  .rodata : { *(.rodata*) }
  .data : { *(.data*) }
  .bss : { *(.bss*) *(COMMON) }
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.comment*) }
}
