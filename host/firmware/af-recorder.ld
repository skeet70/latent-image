/* af-recorder.ld - place the recorder blob in the plain-window tail padding.
   The blob starts at 0x0144a5e0, the head of the id-6 loader's raw-copy window
   [0x01100000, 0x01460000), and claims 0x800 bytes; the write is carried by
   rebuild-update replace-plain --flash-offset 0x8ea5e0. The blob touches no
   byte of the D3 prefix [0x0061c1fc, 0x0061c400) and no byte of the recipe
   bank's gap blocks [0x011ac5a0, 0x011ad714] and [0x011dcca0, 0x011dfe10].
   The stage trampoline is pinned at base+0x3c0, the state block at base+0x700
   and the trailer at base+0x740. The runtime-verify fixture's probes and the
   emitted span are therefore fixed numbers; text must fit below the trampoline
   and the trampoline below the state block. Stock call targets are absolute
   symbols resolved through link relocations. */
ENTRY(rec_hook)

rec_open_wrap  = 0x0071c648;   /* open(cb, path, mode), stock writer's entry */
rec_write_wrap = 0x0071c75c;   /* write(cb, handle, len, src) */
rec_close_wrap = 0x0071c7bc;   /* close(cb, handle, 0, 0) */
rec_recv_impl  = 0x0071fe2c;   /* stock media receive helper */
rec_stage_impl = 0x00723a14;   /* displaced id-6 completion helper */

REC_BASE    = 0x0144a5e0;
REC_STAGE   = REC_BASE + 0x3c0;
REC_STATE   = REC_BASE + 0x700;
REC_TRAILER = REC_BASE + 0x740;

SECTIONS {
  . = REC_BASE;
  .text : { *(.text) *(.rodata) }
  . = REC_STAGE;
  .stage : { *(.stage) }
  . = REC_STATE;
  .state : { *(.state) }
  . = REC_TRAILER;
  .trailer : { *(.trailer) }
  /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.note*) *(.ARM.exidx*) }
}

ASSERT(rec_hook == REC_BASE, "hook must start at the blob base")
ASSERT(SIZEOF(.text) <= REC_STAGE - REC_BASE, "code must fit below the pinned stage trampoline")
ASSERT(rec_stage == REC_STAGE, "stage trampoline pinned at base + 0x3c0")
ASSERT(SIZEOF(.stage) <= REC_STATE - REC_STAGE, "stage trampoline must fit below the pinned state block")
ASSERT(rec_state == REC_STATE, "state block pinned at base + 0x700")
ASSERT(SIZEOF(.state) == 64, "state block is 64 bytes")
ASSERT(rec_trailer == REC_TRAILER, "trailer pinned at base + 0x740")
ASSERT(SIZEOF(.trailer) == 40, "trailer record is 40 bytes")
ASSERT(rec_trailer + SIZEOF(.trailer) <= REC_BASE + 0x800, "blob must fit the 0x800-byte region budget")
