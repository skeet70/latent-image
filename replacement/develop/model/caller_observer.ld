ENTRY(xm1_develop_proof_caller_callback)
SECTIONS
{
    . = 0x07200000;
    .text : { KEEP(*(.text.xm1_develop_proof_caller_callback)) *(.text .text.*) }
    .rodata : { *(.rodata .rodata.*) }
    /DISCARD/ : { *(.ARM.exidx* .ARM.extab* .eh_frame*) }
    ASSERT(. <= 0x07200400, "caller observer exceeds native fixture reservation")
}
