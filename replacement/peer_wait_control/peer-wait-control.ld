ENTRY(xm1_peer_wait_control_entry)
xm1_peer_wait_original_clear = 0x1ff8;
SECTIONS
{
    . = 0x100;
    .text : { KEEP(*(.text.entry)) *(.text .text.*) }
    .rodata : { *(.rodata .rodata.*) }
    .data : { *(.data .data.*) }
    .bss (NOLOAD) : { *(.bss .bss.*) *(COMMON) }
    /DISCARD/ : { *(.ARM.exidx* .ARM.extab* .comment .note* .eh_frame*) }
}
ASSERT(ADDR(.text) == 0x100, "control entry address changed")
ASSERT(SIZEOF(.text) == 104, "control veneer must be exactly 104 bytes")
ASSERT(SIZEOF(.rodata) + SIZEOF(.data) + SIZEOF(.bss) == 0, "control veneer has extra data")
ASSERT(xm1_peer_wait_control_poll == 0x114, "control poll moved")
ASSERT(xm1_peer_wait_control_failure == 0x148, "control failure moved")
ASSERT(xm1_peer_wait_control_acknowledged == 0x14c, "control acknowledgment moved")
ASSERT(xm1_peer_wait_control_end == 0x168, "control end moved")
