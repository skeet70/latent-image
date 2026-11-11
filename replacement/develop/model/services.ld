ENTRY(receive)
PHDRS { low PT_LOAD FLAGS(5); storage PT_LOAD FLAGS(5); helper PT_LOAD FLAGS(5); kind PT_LOAD FLAGS(5); }
SECTIONS
{
  . = 0x00721bdc;
  .append : { KEEP(*(.text.append)) } :low
  ASSERT(. <= 0x00722054, "append model overlaps copy")
  . = 0x00722054;
  .copy : { KEEP(*(.text.copy)) } :low
  ASSERT(. <= 0x007221c8, "copy model overlaps fill")
  . = 0x007221c8;
  .fill : { KEEP(*(.text.fill)) } :low
  ASSERT(. <= 0x00722218, "fill model overlaps decimal")
  . = 0x00722218;
  .decimal : { KEEP(*(.text.decimal)) } :low
  ASSERT(. <= 0x00725318, "decimal model overlaps receive")
  . = 0x00725318;
  .receive : { KEEP(*(.text.receive)) } :low
  . = 0x011d5600;
  .storage_direct : { KEEP(*(.text.storage_direct)) } :storage
  ASSERT(. <= 0x011d5688, "storage model overlaps alternate")
  . = 0x011d5688;
  .storage_alternate : { KEEP(*(.text.storage_alternate)) } :storage
  ASSERT(. <= 0x011d5714, "alternate model overlaps release")
  . = 0x011d5714;
  .storage_release : { KEEP(*(.text.storage_release)) } :storage
  . = 0x00e75490;
  .frame_kind : { KEEP(*(.text.frame_kind)) } :kind
  ASSERT(. <= 0x00e755d0, "frame kind model overlaps next retained service")
  . = 0x00820000;
  .text : { *(.text .text.*) } :helper
  .rodata : { *(.rodata .rodata.*) }
  .data : { *(.data .data.*) }
  .bss (NOLOAD) : { *(.bss .bss.*) *(COMMON) }
  ASSERT(. <= 0x00830000, "service model exceeds footprint")
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.comment) }
}
