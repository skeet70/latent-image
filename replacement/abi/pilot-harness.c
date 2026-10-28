#include <stdint.h>

typedef struct {
    uint32_t first;
    uint32_t second;
} Pair;

extern uint32_t xm1_pilot_add_u32(uint32_t, uint32_t);
extern uint64_t xm1_pilot_add_u64(uint64_t, uint64_t);
extern uint64_t xm1_pilot_mixed(uint32_t, uint64_t, uint32_t);
extern uint32_t xm1_pilot_many_args(uint32_t, uint32_t, uint32_t, uint32_t,
                                    uint32_t, uint32_t, uint32_t, uint32_t);
extern Pair xm1_pilot_pair(uint32_t, uint32_t);
extern uint32_t xm1_pilot_pair_sum(Pair);
extern Pair xm1_pilot_pair_swap(Pair);
extern uint32_t xm1_pilot_checksum(const uint8_t *, uint32_t);
extern void xm1_pilot_store(uint32_t *, uint32_t);
extern uint32_t xm1_pilot_preserve_pressure(uint32_t, uint32_t, uint32_t);

static volatile uint32_t *const results = (volatile uint32_t *)0x3000u;
static const uint8_t checksum_bytes[4] = {0x01u, 0x23u, 0x45u, 0x67u};

__attribute__((noinline, used)) uint32_t pilot_run(void)
{
    uint32_t status = 0;
    uint32_t value32;
    uint64_t value64;
    Pair pair;

    value32 = xm1_pilot_add_u32(0xfffffffeu, 5u);
    results[1] = value32;
    status |= value32 != 3u;

    value64 = xm1_pilot_add_u64(0x0000000200000001ull, 0x00000001ffffffffull);
    results[2] = (uint32_t)value64;
    results[3] = (uint32_t)(value64 >> 32);
    status |= value64 != 0x0000000400000000ull;

    value64 = xm1_pilot_mixed(0x10u, 0x0000000200000003ull, 0x20u);
    results[4] = (uint32_t)value64;
    results[5] = (uint32_t)(value64 >> 32);
    status |= value64 != 0x0000000200000033ull;

    value32 = xm1_pilot_many_args(1u, 2u, 3u, 4u, 5u, 6u, 7u, 8u);
    results[6] = value32;
    status |= value32 != 8u;

    pair = xm1_pilot_pair(0x11111111u, 0x22222222u);
    results[7] = pair.first;
    results[8] = pair.second;
    status |= pair.first != 0x33333333u || pair.second != 0x22222229u;

    pair = xm1_pilot_pair_swap((Pair){0x00010002u, 0x00030004u});
    results[10] = pair.first;
    results[11] = pair.second;
    status |= pair.first != 0x00030004u || pair.second != 0x00010002u;
    value32 = xm1_pilot_pair_sum((Pair){0x00010002u, 0x00030004u});
    results[9] = value32;
    status |= value32 != 0x00040006u;

    value32 = xm1_pilot_checksum(checksum_bytes, 4u);
    results[12] = value32;
    status |= value32 != 0x7d49785du;

    xm1_pilot_store((uint32_t *)(results + 13), 0xdeadbeefu);
    status |= results[13] != 0xdeadbeefu;

    value32 = xm1_pilot_preserve_pressure(0x12345678u, 0x9abcdef0u, 0x0f0f0f0fu);
    results[14] = value32;
    status |= value32 != 0x2240f6bdu;

    results[0] = status;
    return status;
}
