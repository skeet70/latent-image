ENTRY(xm1_file_service_route)
INCLUDE imports.ld
SECTIONS {
  .compiler_builtins 0 : { *compiler_builtins*(.text* .rodata* .data* .bss*) }
  .result_acquire 0x71bc2c : { KEEP(*(.text.xm1_file_service_result_acquire)) }
  .result_release 0x71bc6c : { KEEP(*(.text.xm1_file_service_result_release)) }
  .result_wait 0x71bc90 : { KEEP(*(.text.xm1_file_service_result_wait)) }
  .read 0x71be74 : { KEEP(*(.text.xm1_file_service_read)) }
  .write 0x71bfe0 : { KEEP(*(.text.xm1_file_service_write)) }
  .close 0x71c134 : { KEEP(*(.text.xm1_file_service_close)) }
  .open 0x71c2b0 : { KEEP(*(.text.xm1_file_service_open)) *(.text._R*8requests*) }
  .result_done 0x71c634 : { KEEP(*(.text.xm1_file_service_result_done)) }
  .open_async 0x71c648 : { KEEP(*(.text.xm1_file_service_open_async)) }
  .open_wait 0x71c670 : { KEEP(*(.text.xm1_file_service_open_wait)) }
  .open_unflagged 0x71c6a4 : { KEEP(*(.text.xm1_file_service_open_unflagged)) }
  .open_unflagged_wait 0x71c6c8 : { KEEP(*(.text.xm1_file_service_open_unflagged_wait)) }
  .read_async 0x71c6fc : { KEEP(*(.text.xm1_file_service_read_async)) }
  .read_wait 0x71c720 : { KEEP(*(.text.xm1_file_service_read_wait)) }
  .write_async 0x71c75c : { KEEP(*(.text.xm1_file_service_write_async)) }
  .write_wait 0x71c780 : { KEEP(*(.text.xm1_file_service_write_wait)) }
  .close_async 0x71c7bc : { KEEP(*(.text.xm1_file_service_close_async)) }
  .close_wait 0x71c7e0 : { KEEP(*(.text.xm1_file_service_close_wait)) }
  .translate 0x71d450 : { KEEP(*(.text.xm1_file_service_translate)) *(.text._R*10completion*) *(.rodata._R*10completion*) . = ALIGN(4); }
  .media_ready 0x71ddf0 : { KEEP(*(.text.xm1_file_service_media_ready)) }
  .bind 0x71ea34 : { KEEP(*(.text.xm1_file_service_map_bind)) }
  .unbind 0x71eaac : { KEEP(*(.text.xm1_file_service_map_unbind)) }
  .lookup 0x71eb28 : { KEEP(*(.text.xm1_file_service_map_lookup)) }
  .allocate 0x71ef18 : { KEEP(*(.text.xm1_file_service_tag_allocate)) *(.text._R*7handles*) *(.text._R*5stock*) }
  .release 0x71eff4 : { KEEP(*(.text.xm1_file_service_tag_release)) }
  .post 0x71fde4 : { KEEP(*(.text.xm1_file_service_post)) }
  .media_task 0x71fe7c : { KEEP(*(.text.xm1_file_service_media_task)) }
  .mediamsg 0x721108 : { *(.text._* .text) *(.rodata*) . = ALIGN(4); }
  .route 0x721578 : { KEEP(*(.text.xm1_file_service_route)) }
  .intercept 0x7217f0 : { KEEP(*(.text.xm1_file_service_open_intercept)) }
  .data : { *(.data*) *(.sdata*) *(.got*) }
  .bss : { *(.bss*) *(.sbss*) *(COMMON) }
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.eh_frame*) *(.ARM.attributes) *(.note*) *(.comment*) *(.debug*) *(.llvm_addrsig) }
}
ASSERT(ADDR(.result_acquire) + SIZEOF(.result_acquire) <= 0x71bc6c, "result_acquire overflows")
ASSERT(ADDR(.result_release) + SIZEOF(.result_release) <= 0x71bc90, "result_release overflows")
ASSERT(ADDR(.result_wait) + SIZEOF(.result_wait) <= 0x71bcc8, "result_wait overflows")
ASSERT(ADDR(.read) + SIZEOF(.read) <= 0x71bfe0, "read overflows")
ASSERT(ADDR(.write) + SIZEOF(.write) <= 0x71c134, "write overflows")
ASSERT(ADDR(.close) + SIZEOF(.close) <= 0x71c2b0, "close overflows")
ASSERT(ADDR(.open) + SIZEOF(.open) <= 0x71c504, "open and request code overflow [0x71c2b0,0x71c504)")
ASSERT(ADDR(.result_done) + SIZEOF(.result_done) <= 0x71c648, "result_done overflows")
ASSERT(ADDR(.open_async) + SIZEOF(.open_async) <= 0x71c670, "open_async overflows")
ASSERT(ADDR(.open_wait) + SIZEOF(.open_wait) <= 0x71c6a4, "open_wait overflows")
ASSERT(ADDR(.open_unflagged) + SIZEOF(.open_unflagged) <= 0x71c6c8, "open_unflagged overflows")
ASSERT(ADDR(.open_unflagged_wait) + SIZEOF(.open_unflagged_wait) <= 0x71c6fc, "open_unflagged_wait overflows")
ASSERT(ADDR(.read_async) + SIZEOF(.read_async) <= 0x71c720, "read_async overflows")
ASSERT(ADDR(.read_wait) + SIZEOF(.read_wait) <= 0x71c75c, "read_wait overflows")
ASSERT(ADDR(.write_async) + SIZEOF(.write_async) <= 0x71c780, "write_async overflows")
ASSERT(ADDR(.write_wait) + SIZEOF(.write_wait) <= 0x71c7bc, "write_wait overflows")
ASSERT(ADDR(.close_async) + SIZEOF(.close_async) <= 0x71c7e0, "close_async overflows")
ASSERT(ADDR(.close_wait) + SIZEOF(.close_wait) <= 0x71c81c, "close_wait overflows")
ASSERT(ADDR(.translate) + SIZEOF(.translate) <= 0x71d66c, "translate overflows")
ASSERT(ADDR(.media_ready) + SIZEOF(.media_ready) <= 0x71ded4, "media_ready overflows")
ASSERT(ADDR(.bind) + SIZEOF(.bind) <= 0x71eaac, "bind overflows")
ASSERT(ADDR(.unbind) + SIZEOF(.unbind) <= 0x71eb28, "unbind overflows")
ASSERT(ADDR(.lookup) + SIZEOF(.lookup) <= 0x71eba0, "lookup overflows")
ASSERT(ADDR(.allocate) + SIZEOF(.allocate) <= 0x71eff4, "allocate and handle code overflow [0x71ef18,0x71eff4)")
ASSERT(ADDR(.release) + SIZEOF(.release) <= 0x71f078, "release overflows")
ASSERT(ADDR(.post) + SIZEOF(.post) <= 0x71fe2c, "post overflows")
ASSERT(ADDR(.media_task) + SIZEOF(.media_task) <= 0x71ff3c, "media_task overflows")
ASSERT(ADDR(.mediamsg) + SIZEOF(.mediamsg) <= 0x721578, "shared code overflows [0x721108,0x721578)")
ASSERT(ADDR(.route) + SIZEOF(.route) <= 0x721744, "route overflows")
ASSERT(ADDR(.intercept) + SIZEOF(.intercept) <= 0x721834, "intercept overflows")
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "file service has writable statics")
ASSERT(SIZEOF(.compiler_builtins) == 0, "file service links compiler-builtins")
