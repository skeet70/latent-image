ENTRY(_start)
SECTIONS {
  . = 0;
  .text 0 : { *(.text) }
  /DISCARD/ : { *(.ARM.attributes) *(.comment) }
}
ASSERT(SIZEOF(.text) == 512, "diagnostic must occupy one sector")
ASSERT(_start == 0, "entry must be RAM zero")
