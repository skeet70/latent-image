#ifndef XM1_SAVE_ABI_H
#define XM1_SAVE_ABI_H

/* Stock ABI: r0 frame, r1 kind, r2 status-out; returns 1 success, 0 otherwise. */
int xm1_save_facade(unsigned *frame, int kind, int *status_out);

#endif
