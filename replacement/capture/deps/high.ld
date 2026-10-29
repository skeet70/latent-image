ENTRY(capture_plan_lookup)

SECTIONS
{
  . = 0x00e91810;
  .text :
  {
    KEEP(*(.text.capture_plan_lookup))
    . = 0x00e93664;
    KEEP(*(.text.capture_clear))
    . = 0x00e9390c;
    KEEP(*(.text.capture_advance))
    . = 0x00e93bdc;
    KEEP(*(.text.capture_check))
    . = 0x00e9db80;
    KEEP(*(.text.capture_log))
    . = 0x00eb0ea0;
    KEEP(*(.text.capture_cancel_mask))
  }

  /DISCARD/ :
  {
    *(.ARM.exidx*)
    *(.ARM.extab*)
    *(.comment)
    *(.note*)
    *(.eh_frame*)
    *(.data*)
    *(.bss*)
  }
}

ASSERT(ADDR(.text) == 0x00e91810, "high dependency text base")
ASSERT(ADDR(.text.capture_plan_lookup) == 0x00e91810, "plan lookup address")
ASSERT(ADDR(.text.capture_clear) == 0x00e93664, "clear address")
ASSERT(ADDR(.text.capture_advance) == 0x00e9390c, "advance address")
ASSERT(ADDR(.text.capture_check) == 0x00e93bdc, "check address")
ASSERT(ADDR(.text.capture_log) == 0x00e9db80, "log address")
ASSERT(ADDR(.text.capture_cancel_mask) == 0x00eb0ea0, "cancel mask address")
ASSERT(SIZEOF(.text.capture_log) <= 0x00e9e038 - 0x00e9db80, "log overlaps the stock wait entry")
