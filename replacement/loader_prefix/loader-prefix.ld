/* The owned record-2 image with the loader-B prefix linked into the same
   record-2 padding. */
INCLUDE secondary-stage.ld

/* Loader B: segment id 1, image [0xd0000,0xddaa0). The header word +0xc is
   published by the stage builder only over this stock entry. */
xm1_loader_b_image_start = 0xd0000;
xm1_loader_b_image_end = 0xddaa0;
xm1_loader_b_stock_entry = 0xd0020;

xm1_loader_b_startup = 0xd09ac;
xm1_loader_b_frame_return = 0xd00c0;
xm1_loader_b_stack_top = 0x400f3420;

/* Candidate forbidden ranges: stock loader-B code the owned prefix replaces
   (fetch) and the literal pool only that code reads (read). */
xm1_loader_b_forbid_fetch_entry_start = 0xd0020;
xm1_loader_b_forbid_fetch_entry_end = 0xd00c8;
xm1_loader_b_forbid_read_entry_pool_start = 0xd00c8;
xm1_loader_b_forbid_read_entry_pool_end = 0xd00fc;
