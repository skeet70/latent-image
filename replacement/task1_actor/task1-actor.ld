ENTRY(xm1_task1_actor)
setup = 0xbfb124;
controller_setup = 0xc22bd0;
loop_service = 0xd3cbd4;
execution = 0x63270c;
receive = 0xdb5124;
low_command = 0xd0b318;
command_service = 0xd0b438;
predicate_a = 0xd0c954;
predicate_b = 0xd35f3c;
suppression = 0xd5cf50;
held_command = 0xd3ad34;
held_clear = 0xd3ad10;
pending_first = 0xd3b5ec;
pending_second = 0xd19d18;
image4_pending = 0x636004;
pending_predicate = 0xd1734c;
pending_third = 0xd17370;
pending_priority = 0xd3b0dc;
deferred_first = 0xd3ab00;
deferred_second = 0xd3ad5c;
after_first = 0xd36ea4;
after_second = 0xd36e60;
after_third = 0xd35e14;
diagnostic = 0xd0cd10;
pump = 0xbfbd20;
allocate = 0xd36310;
register = 0xdb4b34;
registration_error = 0x633594;
setup_service = 0xd48240;
setup_zero = 0xd38130;
SECTIONS {
  . = 0xbfb124;
  .once : { KEEP(*(.text.once)) }
  . = 0xc22f60;
  .actor : { KEEP(*(.text.actor)) *(.text*) }
  .rodata : { *(.rodata*) }
  .data : { *(.data*) *(.got*) }
  .bss : { *(.bss*) *(COMMON) }
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.eh_frame*) *(.ARM.attributes) *(.note*) *(.comment*) *(.llvm_addrsig) }
}
ASSERT(ADDR(.once) == 0xbfb124 && SIZEOF(.once) <= 0x94, "once-duty exceeds its carrier")
ASSERT(ADDR(.actor) == 0xc22f60 && SIZEOF(.actor) <= 0x31c, "task1 exceeds its carrier")
ASSERT(SIZEOF(.rodata) == 0 && SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "task1 has allocated data")
