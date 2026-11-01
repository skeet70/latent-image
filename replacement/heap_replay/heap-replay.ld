ENTRY(heap_replay_init)
MEMORY
{
    REPLAY (rwx) : ORIGIN = 0x01f00000, LENGTH = 0x40000
}
SECTIONS
{
    . = 0x01f00000;
    __image_start = .;
    .text : ALIGN(4)
    {
        __text_start = .;
        KEEP(*(.text.heap_replay_init))
        *(.text .text.*)
        . = ALIGN(4);
        __text_end = .;
    } > REPLAY
    .rodata : ALIGN(4) { *(.rodata .rodata.*) } > REPLAY
    .ARM.extab : ALIGN(4) { *(.ARM.extab*) } > REPLAY
    .ARM.exidx : ALIGN(4) { *(.ARM.exidx*) } > REPLAY
    .data : ALIGN(4) { *(.data .data.* .got .got.*) } > REPLAY
    .bss (NOLOAD) : ALIGN(16)
    {
        __bss_start = .;
        *(.bss .bss.* COMMON)
        . = ALIGN(16);
        __bss_end = .;
    } > REPLAY
    __image_end = .;
    /DISCARD/ : { *(.comment .note* .eh_frame*) }
    ASSERT(__image_start == 0x01f00000, "heap replay image base differs")
    ASSERT(__image_end <= 0x01f40000, "heap replay exceeds isolated image window")
}
