ENTRY(xm1_saclac_qualification_encode)

SECTIONS
{
    . = 0x02000000;
    __xm1_qualification_start = .;

    .text : {
        KEEP(*(.text.xm1_saclac_qualification_encode))
        KEEP(*(.text.xm1_saclac_qualification_roundtrip))
        *(.text .text.*)
    }

    .rodata : {
        KEEP(*(.rodata.XM1_SACLAC_QUALIFICATION_FIELDS))
        KEEP(*(.rodata.XM1_SACLAC_QUALIFICATION_FIELD_COUNT))
        KEEP(*(.data.rel.ro.XM1_SACLAC_QUALIFICATION_FIELDS))
        KEEP(*(.data.rel.ro.XM1_SACLAC_QUALIFICATION_FIELD_COUNT))
        *(.rodata .rodata.*)
        *(.data.rel.ro .data.rel.ro.*)
        *(.got .got.*)
    }

    . = ALIGN(4096);
    __xm1_qualification_text_end = .;

    .data : {
        KEEP(*(.data.XM1_SACLAC_QUALIFICATION_FIELDS))
        KEEP(*(.data.XM1_SACLAC_QUALIFICATION_FIELD_COUNT))
        *(.data .data.*)
    }

    __xm1_qualification_image_end = .;

    .bss : {
        *(.bss .bss.*)
        *(COMMON)
    }

    . = ALIGN(4);
    __xm1_qualification_end = .;

    ASSERT(__xm1_qualification_start == 0x02000000, "qualification image must start at 0x02000000")
    ASSERT(__xm1_qualification_text_end <= 0x02100000, "qualification text exceeds its 1 MiB bank")
    ASSERT(__xm1_qualification_end <= 0x02100000, "qualification image exceeds its 1 MiB bank")

    /DISCARD/ : {
        *(.ARM.exidx*)
        *(.ARM.extab*)
        *(.eh_frame*)
        *(.comment)
        *(.note*)
    }
}
