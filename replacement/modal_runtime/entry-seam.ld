ENTRY(rb_modal_entry)
__rb_modal_stock_resume = 0x00bfb02c;
rb_modal_dispatch = 0x00a879b0;
SECTIONS
{
    . = 0x00977f00;
    .text : { KEEP(*(.text.rb_modal_entry)) }
    /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) }
}
ASSERT(SIZEOF(.text) == 56, "modal entry must contain fourteen A32 instructions")
