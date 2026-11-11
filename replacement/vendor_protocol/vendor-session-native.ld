ENTRY(vendor_session_probe)
SECTIONS
{
    . = 0x01000000;
    .text : {
        KEEP(*(.text.vendor_session_probe))
        *(.text .text.*)
    }
    .rodata : { *(.rodata .rodata.*) }
    .ARM.exidx : { *(.ARM.exidx .ARM.exidx.*) }
    .ARM.extab : { *(.ARM.extab .ARM.extab.*) }
    .data : { *(.data .data.*) }
    .bss (NOLOAD) : { *(.bss .bss.* COMMON) }
    . = ALIGN(4);
    .image_end : { LONG(0) }
    ASSERT(. - 0x01000000 <= 0x20000, "Session native image exceeds bounded admission")
    /DISCARD/ : { *(.comment .note.*) }
}
