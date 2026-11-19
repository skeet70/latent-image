ENTRY(xm1_file_service_tag_allocate)
INCLUDE imports.ld
SECTIONS {
  .compiler_builtins 0 : { *compiler_builtins*(.text* .rodata* .data* .bss*) }
  .bind 0x71ea34 : { KEEP(*(.text.xm1_file_service_map_bind)) }
  .unbind 0x71eaac : { KEEP(*(.text.xm1_file_service_map_unbind)) }
  .lookup 0x71eb28 : { KEEP(*(.text.xm1_file_service_map_lookup)) }
  .allocate 0x71ef18 : { KEEP(*(.text.xm1_file_service_tag_allocate)) *(.text._* .text) *(.rodata*) . = ALIGN(4); }
  .release 0x71eff4 : { KEEP(*(.text.xm1_file_service_tag_release)) }
  .data : { *(.data*) *(.sdata*) *(.got*) }
  .bss : { *(.bss*) *(.sbss*) *(COMMON) }
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.eh_frame*) *(.ARM.attributes) *(.note*) *(.comment*) *(.debug*) *(.llvm_addrsig) }
}
ASSERT(ADDR(.bind) + SIZEOF(.bind) <= 0x71eaac, "bind exceeds [0x71ea34,0x71eaac)")
ASSERT(ADDR(.unbind) + SIZEOF(.unbind) <= 0x71eb28, "unbind exceeds [0x71eaac,0x71eb28)")
ASSERT(ADDR(.lookup) + SIZEOF(.lookup) <= 0x71eba0, "lookup exceeds [0x71eb28,0x71eba0)")
ASSERT(ADDR(.allocate) + SIZEOF(.allocate) <= 0x71eff4, "allocate and shared code exceed [0x71ef18,0x71eff4)")
ASSERT(ADDR(.release) + SIZEOF(.release) <= 0x71f078, "release exceeds [0x71eff4,0x71f078)")
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "file service has writable statics")
ASSERT(SIZEOF(.compiler_builtins) == 0, "file service links compiler-builtins")
