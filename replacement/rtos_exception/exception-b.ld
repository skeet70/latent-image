/* Loader B: owned exception entries at their stock extents. */
ENTRY(xm1_exception_irq)

MEMORY {
  undefined (rx) : ORIGIN = 0xd0100, LENGTH = 0x28
  prefetch (rx) : ORIGIN = 0xd0128, LENGTH = 0x20
  data (rx) : ORIGIN = 0xd0148, LENGTH = 0x20
  reserved (rx) : ORIGIN = 0xd0168, LENGTH = 0x1c
  fiq (rx) : ORIGIN = 0xd0184, LENGTH = 0x30
  svc (rx) : ORIGIN = 0xd01b4, LENGTH = 0x30
  svc_diag (rx) : ORIGIN = 0xd01e4, LENGTH = 0x20
  irq (rx) : ORIGIN = 0xd0204, LENGTH = 0x164
  dispatcher (rx) : ORIGIN = 0xd0cac, LENGTH = 0xa4
  wrappers (rx) : ORIGIN = 0xd0d50, LENGTH = 0x48
  literal_slack (rx) : ORIGIN = 0xd03a0, LENGTH = 0x70
  bodies (rx) : ORIGIN = 0xd30a8, LENGTH = 0x250
}

xm1_exception_stock_try = 0xd27d4;
xm1_exception_stock_release = 0xd27fc;
xm1_exception_stock_identity = 0xd27b8;
xm1_exception_vfp_owner = 0xd2420;
xm1_exception_shutdown = 0xd0df8;
xm1_exception_halt = 0xd10fc;
xm1_exception_stock_log = 0xd1668;

SECTIONS {
  .undefined : { KEEP(*(.text.exception_undefined)) . = ORIGIN(undefined) + LENGTH(undefined); } > undefined =0xf000f0e7
  .prefetch : { KEEP(*(.text.exception_prefetch)) . = ORIGIN(prefetch) + LENGTH(prefetch); } > prefetch =0xf000f0e7
  .data : { KEEP(*(.text.exception_data)) . = ORIGIN(data) + LENGTH(data); } > data =0xf000f0e7
  .reserved : { KEEP(*(.text.exception_reserved)) . = ORIGIN(reserved) + LENGTH(reserved); } > reserved =0xf000f0e7
  .fiq : { KEEP(*(.text.exception_fiq)) . = ORIGIN(fiq) + LENGTH(fiq); } > fiq =0xf000f0e7
  .svc : { KEEP(*(.text.exception_svc)) . = ORIGIN(svc) + LENGTH(svc); } > svc =0xf000f0e7
  .svc_diag : { KEEP(*(.text.exception_svc_diag)) . = ORIGIN(svc_diag) + LENGTH(svc_diag); } > svc_diag =0xf000f0e7
  .irq : { KEEP(*(.text.exception_irq)) . = ORIGIN(irq) + LENGTH(irq); } > irq =0xf000f0e7
  .dispatcher : { KEEP(*(.text.exception_dispatcher)) . = ORIGIN(dispatcher) + LENGTH(dispatcher); } > dispatcher =0xf000f0e7
  .wrappers : {
    KEEP(*(.text.exception_ack_wrapper))
    KEEP(*(.text.exception_eoi_wrapper))
    KEEP(*(.text.exception_fiq_hook))
    . = ORIGIN(wrappers) + LENGTH(wrappers);
  } > wrappers =0xf000f0e7
  .bodies : {
    KEEP(*(.text.exception_ack))
    __ack_end = .;
    KEEP(*(.text.exception_lock_acquire))
    __lock_end = .;
    KEEP(*(.text.exception_eoi))
    __eoi_end = .;
    KEEP(*(.text.exception_exit_tail))
    __exit_end = .;
    *(.text.*panicking*)
    *(.text.*rust_begin_unwind*)
    . = ORIGIN(bodies) + LENGTH(bodies);
  } > bodies =0xf000f0e7
  .literal_slack : {
    KEEP(*(.text.exception_fault_record))
    __fault_end = .;
    . = ORIGIN(literal_slack) + LENGTH(literal_slack);
  } > literal_slack =0xf000f0e7
  .forbidden (NOLOAD) : { *(.text .text.* .rodata .rodata.* .data .data.* .bss .bss.* COMMON) } > bodies
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.*) }
}

ASSERT(xm1_exception_irq == ORIGIN(irq), "IRQ entry is not at its stock address")
ASSERT(xm1_exception_ack == ORIGIN(bodies), "ack body is not at the bodies origin")
ASSERT(__lock_end <= ORIGIN(bodies) + LENGTH(bodies), "lock body exceeds the bodies extent")
ASSERT(__eoi_end <= ORIGIN(bodies) + LENGTH(bodies), "EOI body exceeds the bodies extent")
ASSERT(__exit_end <= ORIGIN(bodies) + LENGTH(bodies), "exit body exceeds the bodies extent")
ASSERT(__fault_end <= ORIGIN(literal_slack) + LENGTH(literal_slack), "fault body exceeds the literal slack")
ASSERT(SIZEOF(.forbidden) == 0, "unplaced code or writable data")
