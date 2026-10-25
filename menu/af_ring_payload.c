#include <stdint.h>
#include <stddef.h>

#define AF_RING_CAPACITY 256u
#define AF_RING_CORE 0x4003u

struct af_ring_event {
    uint32_t sequence;
    uint32_t kind;
    uint32_t argument[4];
    uint32_t search_state;
    uint32_t statistic;
    uint32_t core;
};

struct af_ring_storage {
    uint32_t magic;
    uint32_t version;
    uint32_t header_bytes;
    uint32_t event_bytes;
    uint32_t capacity;
    uint32_t committed;
    uint32_t overwritten;
    uint32_t active;
    uint32_t core;
    uint32_t contention_loss;
    uint32_t reserved[6];
    struct af_ring_event events[AF_RING_CAPACITY];
};

_Static_assert(sizeof(struct af_ring_event) == 36, "event wire size");
_Static_assert(offsetof(struct af_ring_storage, events) == 64, "header wire size");
_Static_assert(sizeof(struct af_ring_storage) == 0x2440, "bulk window size");

__attribute__((section(".af_ring.storage"), aligned(8), used))
volatile struct af_ring_storage af_ring_storage = {
    .magic = 0x31524641u,
    .version = 1,
    .header_bytes = 64,
    .event_bytes = 36,
    .capacity = AF_RING_CAPACITY,
    .core = AF_RING_CORE,
};

/* The alias is the stock MPU's non-cacheable DRAM window. */
static volatile struct af_ring_storage *ring(void) {
    return (volatile struct af_ring_storage *)((uintptr_t)&af_ring_storage | 0x40000000u);
}

__attribute__((noinline, used))
void af_ring_record(uint32_t kind, const uint32_t *arguments) {
    uint32_t core;
    __asm__ volatile("mrc p14, 0, %0, c2, c0, 0" : "=r"(core));
    if (core != AF_RING_CORE)
        return;

    volatile struct af_ring_storage *storage = ring();
    uint32_t observed;
    uint32_t failed;
    __asm__ volatile(
        "ldrex %0, [%2]\n\t"
        "cmp %0, #0\n\t"
        "strexeq %1, %3, [%2]\n\t"
        "movne %1, #1\n\t"
        "clrex"
        : "=&r"(observed), "=&r"(failed)
        : "r"(&storage->active), "r"(1u)
        : "cc", "memory");
    if (failed != 0) {
        storage->contention_loss = 1;
        __asm__ volatile("dmb sy" : : : "memory");
        return;
    }
    __asm__ volatile("dmb sy" : : : "memory");
    uint32_t sequence = storage->committed;
    if (sequence != UINT32_MAX) {
        volatile struct af_ring_event *event = &storage->events[sequence & (AF_RING_CAPACITY - 1)];
        event->sequence = 0;
        event->kind = kind;
        event->argument[0] = arguments[0];
        event->argument[1] = arguments[1];
        event->argument[2] = arguments[2];
        event->argument[3] = arguments[3];
        event->search_state = *(volatile const uint32_t *)(uintptr_t)0x016c0fc0u;
        event->statistic = *(volatile const uint32_t *)(uintptr_t)0x016c1160u;
        event->core = core;
        event->sequence = sequence + 1;
        if (sequence >= AF_RING_CAPACITY)
            storage->overwritten = sequence + 1 - AF_RING_CAPACITY;
        __asm__ volatile("dmb sy" : : : "memory");
        storage->committed = sequence + 1;
    }
    __asm__ volatile("dmb sy" : : : "memory");
    storage->active = 0;
}

/* The query callback supplies its output cells in r1 and r2. */
int32_t af_ring_synthetic(uint32_t count, uintptr_t *output, uint32_t *length) {
    if (count == 0 || count > 1024)
        return -1;
    uint32_t arguments[4] = {count, 0x53594e54u, 0, 0};
    for (uint32_t index = 0; index != count; ++index) {
        arguments[2] = index;
        af_ring_record(0x80000001u, arguments);
    }
    *output = (uintptr_t)ring();
    *length = sizeof(struct af_ring_storage);
    return 0;
}
