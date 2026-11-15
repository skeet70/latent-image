ENTRY(xm1_decoder_init)
xm1_decoder_current_task = 0x723cb8;
xm1_decoder_take = 0x724b68;
xm1_decoder_give = 0x724c00;
xm1_decoder_route = 0x6499d0;
xm1_decoder_clock_take = 0x649c4c;
xm1_decoder_clock_give = 0x649ddc;
xm1_decoder_irq_attach = 0x64e918;
xm1_decoder_irq_detach = 0x64eb20;
xm1_decoder_marker = 0x42188;
xm1_decoder_progress_entry = 0x6ae1dc;
xm1_decoder_complete_entry = 0x6ae310;
SECTIONS
{
  . = DEFINED(__decoder_origin) ? __decoder_origin : 0;
  __decoder_start = .;
  .text : {
    KEEP(*(.decoder.init))
    KEEP(*(.decoder.feed))
    KEEP(*(.decoder.progress))
    KEEP(*(.decoder.complete))
    KEEP(*(.decoder.finish))
    *(.text .text.*)
  }
  .rodata : { *(.rodata .rodata.*) }
  . = ALIGN(4);
  __decoder_end = .;
  .data : { *(.data .data.*) }
  .bss : { *(.bss .bss.*) *(COMMON) }
  . = ALIGN(4);
  __decoder_memory_end = .;
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.note*) *(.comment*) }
}
ASSERT(SIZEOF(.data) == 0, "decoder introduces initialized writable data")
