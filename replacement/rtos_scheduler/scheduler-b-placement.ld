/* Loader B: owned scheduler helpers at their stock extents (regions.json). */
ENTRY(xm1_rtos_scheduler_acquire)

MEMORY {
  acquire (rx) : ORIGIN = 0xd398c, LENGTH = 0x3c
  release (rx) : ORIGIN = 0xd39c8, LENGTH = 0x28
  find (rx) : ORIGIN = 0xd3c04, LENGTH = 0x5c
  insert (rx) : ORIGIN = 0xd3c60, LENGTH = 0x7c
  remove (rx) : ORIGIN = 0xd3cdc, LENGTH = 0xb0
  reset (rx) : ORIGIN = 0xd3d8c, LENGTH = 0x48
  prepare (rx) : ORIGIN = 0xd3f2c, LENGTH = 0x2c
  notify (rx) : ORIGIN = 0xd3fd4, LENGTH = 0x30
  requeue (rx) : ORIGIN = 0xd4004, LENGTH = 0xf0
  rotate (rx) : ORIGIN = 0xd40f4, LENGTH = 0x80
}

xm1_rtos_scheduler_stock_raise_irq = 0xd33e4;

SECTIONS {
  .acquire : {
    KEEP(*(.text.acquire))
    __acquire_body_end = .;
    . = ORIGIN(acquire) + LENGTH(acquire);
  } > acquire =0xf000f0e7
  .release : {
    KEEP(*(.text.release))
    __release_body_end = .;
    . = ORIGIN(release) + LENGTH(release);
  } > release =0xf000f0e7
  .find : {
    KEEP(*(.text.find))
    __find_body_end = .;
    . = ORIGIN(find) + LENGTH(find) - 0x30;
    __prepare_body_start = .;
    KEEP(*(.text.prepare_body))
    __prepare_body_end = .;
    . = ORIGIN(find) + LENGTH(find);
  } > find =0xf000f0e7
  .insert : {
    KEEP(*(.text.insert))
    __insert_body_end = .;
    . = ORIGIN(insert) + LENGTH(insert);
  } > insert =0xf000f0e7
  .remove : {
    KEEP(*(.text.remove))
    __remove_body_end = .;
    . = ORIGIN(remove) + LENGTH(remove);
  } > remove =0xf000f0e7
  .reset : {
    KEEP(*(.text.reset))
    __reset_body_end = .;
    . = ORIGIN(reset) + LENGTH(reset);
  } > reset =0xf000f0e7
  .prepare : {
    KEEP(*(.text.prepare))
    __prepare_branch_end = .;
    . = ORIGIN(prepare) + LENGTH(prepare);
  } > prepare =0xf000f0e7
  .notify : {
    KEEP(*(.text.notify))
    __notify_body_end = .;
    . = ORIGIN(notify) + LENGTH(notify);
  } > notify =0xf000f0e7
  .requeue : {
    KEEP(*(.text.requeue))
    __requeue_body_end = .;
    . = ORIGIN(requeue) + LENGTH(requeue);
  } > requeue =0xf000f0e7
  .rotate : {
    KEEP(*(.text.rotate))
    __rotate_body_end = .;
    . = ORIGIN(rotate) + LENGTH(rotate);
  } > rotate =0xf000f0e7
}
