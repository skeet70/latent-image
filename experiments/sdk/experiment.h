#ifndef LATENT_IMAGE_EXPERIMENT_H
#define LATENT_IMAGE_EXPERIMENT_H

#include <stdint.h>

/* What a hook handler receives: the hooked function's r0-r12 and lr at entry.
   Changes take effect when the hooked function continues. */
struct experiment_regs {
    uint32_t r[13];
    uint32_t lr;
};

/* The same DRAM through the stock MPU's non-cacheable window. A store through
   it is visible to the other core and to `read-ram` at once. */
#define EXPERIMENT_UNCACHED(pointer) ((__typeof__(pointer))((uintptr_t)(pointer) | 0x40000000u))

#endif
