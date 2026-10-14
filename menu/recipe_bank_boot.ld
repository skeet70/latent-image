/* Linker script for the recipe bank's boot copy (menu/recipe_bank_boot.S).
 *
 * The stub is assembled at the four stock array runs the D3/Q retargets
 * free: the only runs whose freedom the emitted write set produces. The
 * six bracket runs stay live arrays (the combined candidate keeps the
 * bracket surfaces stock), so nothing is placed there. RB_SRC_BASE and
 * RB_SRC_LIMIT come from the build with --defsym; RB_SRC_LIMIT is the
 * package end minus the plain stage base 0x01100000, the form gate 2
 * compares against the reporter's bytes-copied cell. RB_SRC_END is passed
 * by the case generator but read by nothing here. RB_STORE_ENTRY is
 * RB_SRC_BASE + 32 + boot.store_off: the store pass is image code, linked
 * at the address it executes from (the source copy of the hashed image)
 * and emitted by the builder into the image at boot.store_off.
 *
 * Run extents, byte-read from the node maps and the pinned decodes:
 *   Q values    [0x00782348, 0x00782378) 48  freed by the Q node
 *                                          retarget (0x78b020/24)
 *   film values [0x0078319c, 0x007831cc) 48  freed by the three film-sim
 *                                          node retargets
 *   Q labels    [0x00789e2c, 0x00789e8c) 96  same Q retarget
 *   film labels [0x0078d4f8, 0x0078d558) 96  same film-sim retargets
 *
 * Measured counts after consolidation: entry 48/48, check 96/96, sum
 * 48/48, publish 96/96 (the store pass and the bulk cache pass moved into
 * the image, chosen byte source (a) in the contract), store pass 60 B in
 * its image slot. The stub fills the four runs exactly; the size
 * assertions are the budget instrument. Placement blocks are in ascending
 * VMA order: lld refuses back-steps.
 */
ENTRY(rb_boot_entry)

/* Absolute stock helper addresses. These assignments must stay in this
 * script: a numeric `bl` operand or a same-unit `.set` in the source
 * assembles a raw imm24 field and the call lands at pc+8+operand, not at
 * the helper (verified: both forms link to 0xeb18713d). With a
 * linker-script symbol lld resolves the call pc-relatively. */
rb_cache_dclean_inval = 0x0061c4f4;
rb_cache_iinval = 0x0061c4ec;

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
  . = RB_STORE_ENTRY;
  .text.rb_boot_store : { *(.text.rb_boot_store) }

  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) }
}

/* Every fragment inside its run; the entry pinned to the run the container
 * patch names; the store pass pinned to its execution address. */
ASSERT(rb_boot_entry == 0x0078319c, "entry starts the film-values run")
ASSERT(SIZEOF(.text.rb_boot_entry) <= 48, "entry fits the film-values run (measured 48)")
ASSERT(SIZEOF(.text.rb_boot_sum) <= 48, "copy/hash fragment fits the Q-values run (measured 48)")
ASSERT(SIZEOF(.text.rb_boot_publish) <= 96, "validation fragment fits the Q-labels run (measured 96)")
ASSERT(SIZEOF(.text.rb_boot_check) <= 96, "validation fragment fits the film-labels run (measured 96)")
ASSERT(rb_boot_fail > 0x00789e2c && rb_boot_fail < 0x00789e8c, "refusal path stays in its run")
ASSERT(rb_boot_done > 0x00789e2c && rb_boot_done < 0x00789e8c, "stock branch stays in its run")
ASSERT(rb_store_entry == RB_STORE_ENTRY, "store pass placed at its execution address")
ASSERT(SIZEOF(.text.rb_boot_store) <= 64, "store pass fits its image slot (measured 60)")
