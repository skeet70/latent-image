#ifndef FUJI_AF_RECOVERY_BOOTSTRAP_H
#define FUJI_AF_RECOVERY_BOOTSTRAP_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct { uint32_t value; } BbAddress;
typedef struct { uint32_t value; } BbWord;
typedef struct { size_t value; } BbByteCount;
typedef struct { const uint8_t *bytes; BbByteCount length; } BbInput;
enum BbRoute { BB_ROUTE_MEDIA, BB_ROUTE_FAT_SD };
enum BbFault { BB_FAULT_NONE, BB_FAULT_TAP, BB_FAULT_MARKER, BB_FAULT_MEDIA, BB_FAULT_ROUTE };
enum BbAccess {
    BB_ACCESS_OK, BB_ACCESS_REGISTER, BB_ACCESS_COMMAND, BB_ACCESS_WIDTH,
    BB_ACCESS_MEDIA_RANGE, BB_ACCESS_MEMORY_RANGE, BB_ACCESS_PERSISTENT_WRITE,
    BB_ACCESS_CODEC, BB_ACCESS_ALLOCATION
};
enum BbRamWriter { BB_RAM_MEDIA_DMA, BB_RAM_SD_DMA, BB_RAM_FILL, BB_RAM_LZ };
typedef struct {
    void *context;
    bool (*read)(void *, BbAddress, uint8_t *, BbByteCount);
    bool (*write)(void *, enum BbRamWriter, BbAddress, const uint8_t *, BbByteCount);
} BbRamIo;
typedef struct {
    BbInput media, card;
    BbRamIo ram;
    enum BbRoute route;
    enum BbFault fault;
} BbDeviceInputs;
typedef struct {
    uint64_t media_read_bytes, sd_read_bytes, fill_bytes, decoded_bytes;
    uint64_t transactions, peer_release_requests;
} BbDeviceStats;
typedef struct BbDevices BbDevices;

BbDevices *bb_devices_create(BbDeviceInputs inputs);
void bb_devices_destroy(BbDevices *devices);
enum BbAccess bb_devices_read(BbDevices *devices, BbAddress address,
                             BbByteCount width, BbWord *result);
enum BbAccess bb_devices_write(BbDevices *devices, BbAddress address,
                              BbByteCount width, BbWord value);
bool bb_devices_handoff(const BbDevices *devices);
BbDeviceStats bb_devices_stats(const BbDevices *devices);
const char *bb_devices_detail(const BbDevices *devices);
const char *bb_devices_assumptions(void);
#endif
