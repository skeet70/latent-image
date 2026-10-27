#include "recovery-bootstrap.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

typedef struct { uint8_t bytes[2048]; } TestRam;
static bool read_ram(void *context, BbAddress address, uint8_t *output, BbByteCount count) {
    TestRam *ram = context;
    if ((uint64_t)address.value + count.value > sizeof ram->bytes) return false;
    memcpy(output, ram->bytes + address.value, count.value);
    return true;
}
static bool write_ram(void *context, enum BbRamWriter writer, BbAddress address, const uint8_t *input, BbByteCount count) {
    (void)writer;
    TestRam *ram = context;
    if ((uint64_t)address.value + count.value > sizeof ram->bytes) return false;
    memcpy(ram->bytes + address.value, input, count.value);
    return true;
}
static BbDevices *devices(TestRam *ram) {
    static const uint8_t media[2048] = {0x31, 0x42, 0x53};
    static const uint8_t card[512] = {0};
    memset(ram->bytes, 0xa5, sizeof ram->bytes);
    return bb_devices_create((BbDeviceInputs){
        .media = {media, {sizeof media}}, .card = {card, {sizeof card}},
        .ram = {ram, read_ram, write_ram}, .route = BB_ROUTE_MEDIA, .fault = BB_FAULT_NONE
    });
}
static void set_word(BbDevices *model, uint32_t address, uint32_t value) {
    assert(bb_devices_write(model, (BbAddress){address}, (BbByteCount){4}, (BbWord){value}) == BB_ACCESS_OK);
}
static void untouched(const TestRam *ram) {
    for (size_t index = 0; index < sizeof ram->bytes; index++) assert(ram->bytes[index] == 0xa5);
}

/* RX parameters and native TX control are source words; media and RAM bytes are synthetic. */
static void reject_tx_control(void) {
    TestRam ram;
    BbDevices *model = devices(&ram);
    assert(model);
    set_word(model, 0xfff52000, 1);
    set_word(model, 0xfff52018, 0x80008013);
    set_word(model, 0xfff5201c, 0x300004);
    set_word(model, 0xfff52020, 0);
    set_word(model, 0xfff52024, 0);
    set_word(model, 0xfff52884, 0x10001);
    set_word(model, 0xfff528a0, 0x401000);
    set_word(model, 0xfff528a4, 0);
    set_word(model, 0xfff528a8, 2048);
    assert(bb_devices_write(model, (BbAddress){0xfff52008}, (BbByteCount){4}, (BbWord){0x8013b3}) == BB_ACCESS_COMMAND);
    untouched(&ram);
    bb_devices_destroy(model);
}
static void reject_sd_program(void) {
    TestRam ram;
    BbDevices *model = devices(&ram);
    assert(model);
    assert(bb_devices_write(model, (BbAddress){0xfff5000e}, (BbByteCount){2}, (BbWord){0x193a}) == BB_ACCESS_PERSISTENT_WRITE);
    untouched(&ram);
    bb_devices_destroy(model);
}
int main(void) {
    reject_tx_control();
    reject_sd_program();
    puts("bootstrap devices: malformed NAND RX/TX control and SD program refuse without RAM effects");
    return 0;
}
