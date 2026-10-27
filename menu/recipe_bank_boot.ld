/* Trusted fragments occupy the four stock arrays freed by D3/Q retargets.
 * Checked source-image code validates the table, publishes and maintains caches.
 * RB_SRC_LIMIT is the package end relative to the plain-stage base.
 */
ENTRY(rb_boot_entry)
rb_cache_dclean_inval = 0x0061c4f4;
rb_cache_iinval = 0x0061c4ec;
rb_plain_load_wait = 0x00724b68;
rb_plain_load_release = 0x00724c00;

SECTIONS
{
  . = 0x00782348;
  .text.rb_boot_sum : { *(.text.rb_boot_sum) }
  . = 0x0078319c;
  .text.rb_boot_entry : { *(.text.rb_boot_entry) }
  . = 0x00789e2c;
  .text.rb_boot_publish : { *(.text.rb_boot_publish) }
  . = 0x0078d4f8;
  .text.rb_boot_check : { *(.text.rb_boot_check) }
  . = RB_VALIDATE_ENTRY;
  .text.rb_boot_validate : { *(.text.rb_boot_validate) }
  . = RB_STORE_ENTRY;
  .text.rb_boot_store : { *(.text.rb_boot_store) }
  . = RB_COPY_ENTRY;
  .text.rb_boot_copy : { *(.text.rb_boot_copy) }
  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) }
}

ASSERT(rb_boot_entry == 0x0078319c, "entry starts the film-values run")
ASSERT(SIZEOF(.text.rb_boot_entry) == 48, "entry fills the film-values run")
ASSERT(SIZEOF(.text.rb_boot_sum) == 48, "CRC fragment fills the Q-values run")
ASSERT(SIZEOF(.text.rb_boot_publish) == 96, "CRC and exit fill the Q-labels run")
ASSERT(SIZEOF(.text.rb_boot_check) == 96, "header gate fills the film-labels run")
ASSERT(rb_boot_fail > 0x00789e2c && rb_boot_done < 0x00789e8c, "refusal and exit stay in their run")
ASSERT(rb_validate_entry == RB_VALIDATE_ENTRY, "validator placed at its execution address")
ASSERT(SIZEOF(.text.rb_boot_validate) <= 128, "validator fits before publication table")
ASSERT(rb_store_entry == RB_STORE_ENTRY, "store pass placed at its execution address")
ASSERT(SIZEOF(.text.rb_boot_store) == 60, "store/cache pass is sixty bytes")
ASSERT(rb_copy_entry == RB_COPY_ENTRY, "copy placed at its execution address")
ASSERT(SIZEOF(.text.rb_boot_copy) == 32, "copy fits before executable kernel")
ASSERT(RB_SRC_BASE == 0x011ad800, "source uses the provisional gap1 carrier")
ASSERT(RB_VALIDATE_ENTRY == RB_SRC_BASE + 32, "validator starts the image")
ASSERT(RB_STORE_ENTRY == RB_SRC_BASE + 32 + 0x480, "store follows publication table")
ASSERT(RB_COPY_ENTRY == RB_SRC_BASE + 32 + 0x4bc, "copy follows store/cache pass")
ASSERT(RB_SRC_LIMIT == RB_SRC_BASE + 32 + 0xa120 - 0x01100000, "source gate covers the exact image")
