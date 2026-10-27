#include "recovery-bootstrap.h"
#include "../lz/fflzd-codec.h"

#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define MEDIA_CAPACITY UINT32_C(0x08000000)
#define MAX_DECODED (16u << 20)
#define MAX_RECORD (8u << 20)
#define WORDS(n) ((n) / 4)

static const char assumptions[] =
    "Unspecified admitted registers start zero; SYS selector1 and GPIO0 high pair follow route; "
    "GPIO2 input=0x200, GPIO12 input=0, CLOCK+0x80=0x10000; "
    "clock settling except clock48, PHY and media/SD/LZ completion are instantaneous after data commit; "
    "fill commits at first status poll; clock48 ready after two MMIO transactions; "
    "timer advances 1024 virtual counts per MMIO transaction before divider; "
    "128MiB type1 logical media, erased tail, clean spare/ECC, no bad blocks; "
    "SDHC immediate-ready OCR/RCA fixture; CPU1 release recorded but held without ack";

struct BbDevices {
    BbDeviceInputs inputs;
    BbDeviceStats stats;
    enum BbAccess failure;
    char detail[1024];
    uint32_t clock[WORDS(0x88)], gpio[WORDS(0x480)], dram[WORDS(0xfc)];
    uint32_t phy[WORDS(0x248)], system[WORDS(0x20c)], reset[WORDS(0x18)];
    uint32_t nand[WORDS(0x900)], fill[WORDS(0x38)], lz[WORDS(0x90)];
    uint32_t irq[2][WORDS(0xc04)];
    uint32_t enabled[2][4], pending[2][4], source[2][4];
    uint32_t timer[3][4];
    uint64_t ticks, clock48_due;
    bool phy_started, handoff, fill_armed;
    uint8_t aperture[512], sd_buffer[512], feed[4096];
    uint32_t sd[0x38];
    enum { SD_IDLE, SD_READY, SD_IDENT, SD_STANDBY, SD_TRANSFER } sd_state;
    bool sd_app;
    size_t sd_position, sd_remaining;
    FflzStream stream;
    uint8_t *decoded;
    uint32_t encoded_size, decoded_size, stream_source, stream_destination;
    size_t fed;
};

static enum BbAccess fail(BbDevices *d, enum BbAccess failure, const char *reason) {
    if (d->failure == BB_ACCESS_OK) {
        d->failure = failure;
        snprintf(d->detail, sizeof d->detail, "%s; %s", reason, assumptions);
    }
    return d->failure;
}

static uint32_t le_word(const uint8_t *p, size_t width) {
    uint32_t result = 0;
    for (size_t i = 0; i < width; ++i) result |= (uint32_t)p[i] << (8 * i);
    return result;
}

static bool member(uint32_t offset, const uint16_t *set, size_t count) {
    for (size_t i = 0; i < count; ++i) if (offset == set[i]) return true;
    return false;
}
#define IN_SET(o, set) member((o), (set), sizeof(set) / sizeof((set)[0]))

static bool family(uint32_t offset, uint32_t start, uint32_t words) {
    return offset >= start && offset < start + 4 * words && !(offset & 3);
}

static bool ram_read(BbDevices *d, uint32_t address, uint8_t *bytes, size_t length) {
    return (uint64_t)address + length <= UINT64_C(0x100000000) &&
        d->inputs.ram.read(d->inputs.ram.context, (BbAddress){address}, bytes, (BbByteCount){length});
}

static bool ram_write(BbDevices *d, enum BbRamWriter writer, uint32_t address, const uint8_t *bytes, size_t length) {
    return (uint64_t)address + length <= UINT64_C(0x100000000) &&
        d->inputs.ram.write(d->inputs.ram.context, writer, (BbAddress){address}, bytes, (BbByteCount){length});
}

static void advance(BbDevices *d) {
    ++d->ticks;
    if (d->clock48_due && d->ticks >= d->clock48_due) {
        d->clock48_due = 0;
        d->clock[0x48 / 4] |= UINT32_C(0x01000000);
    }
    for (unsigned i = 0; i < 3; ++i) {
        uint32_t *t = d->timer[i];
        if (t[2] & 1) {
            uint32_t delta = 1024u >> ((t[2] >> 4) & 7);
            if (t[2] == 0x23 && i == 0) t[0] = t[0] > delta ? t[0] - delta : 0;
            else {
                uint64_t next = (uint64_t)t[0] + delta;
                t[0] = t[1] ? (uint32_t)(next % ((uint64_t)t[1] + 1)) : (uint32_t)next;
            }
        }
    }
}

/* X-M1 ROM 0x304-0x718 and native 0x25d4-0x2724 supply the startup register census. */
static uint32_t *startup(BbDevices *d, uint32_t a) {
    static const uint16_t clock[] = {0,4,8,12,16,20,24,28,32,36,40,44,48,52,56,64,72,80,84,88,92,128,132};
    static const uint16_t dram[] = {0,4,8,12,16,20,24,28,64,68,192,200,208,216,224,228,232,236,244,248};
    static const uint16_t system[] = {0,8,0x100,0x104,0x208};
    static const uint16_t reset[] = {4,8,12,16,20};
    if (a >= 0xfff88000 && IN_SET(a - 0xfff88000, clock)) return &d->clock[(a - 0xfff88000) / 4];
    if (a >= 0xfff89000 && a < 0xfff89480 && (a & 0x3f) <= 0x1c && !(a & 3) &&
        ((a - 0xfff89000) / 0x40 < 16 || (a & 0x3f) != 0x14)) return &d->gpio[(a - 0xfff89000) / 4];
    if (a >= 0xfff99000 && IN_SET(a - 0xfff99000, dram)) return &d->dram[(a - 0xfff99000) / 4];
    if (a >= 0xfff01000 && a < 0xfff01248 && !(a & 3)) return &d->phy[(a - 0xfff01000) / 4];
    if (a >= 0xfff98000 && IN_SET(a - 0xfff98000, system)) return &d->system[(a - 0xfff98000) / 4];
    if (a >= 0xfffb2000 && IN_SET(a - 0xfffb2000, reset)) return &d->reset[(a - 0xfffb2000) / 4];
    return NULL;
}

static enum BbAccess media_source(BbDevices *d, uint32_t offset, size_t count, BbInput *source) {
    if ((uint64_t)offset + count > MEDIA_CAPACITY) return fail(d, BB_ACCESS_MEDIA_RANGE, "logical media read outside type1 capacity");
    if (d->inputs.fault == BB_FAULT_MEDIA) return fail(d, BB_ACCESS_MEDIA_RANGE, "required logical-media read refused by media fault");
    size_t available = offset < d->inputs.media.length.value ? d->inputs.media.length.value - offset : 0;
    if (available > count) available = count;
    *source = (BbInput){available ? d->inputs.media.bytes + offset : NULL, {available}};
    d->stats.media_read_bytes += count;
    return BB_ACCESS_OK;
}

static enum BbAccess media_read(BbDevices *d, uint32_t offset, uint8_t *out, size_t count) {
    BbInput source = {0};
    enum BbAccess status = media_source(d, offset, count, &source);
    if (status != BB_ACCESS_OK) return status;
    if (source.length.value) memcpy(out, source.bytes, source.length.value);
    memset(out + source.length.value, 0xff, count - source.length.value);
    return BB_ACCESS_OK;
}

static enum BbAccess media_dma(BbDevices *d, uint32_t offset, uint32_t destination, size_t count) {
    BbInput source = {0};
    enum BbAccess status = media_source(d, offset, count, &source);
    if (status != BB_ACCESS_OK) return status;
    if ((uint64_t)destination + count > UINT64_C(0x100000000))
        return fail(d, BB_ACCESS_MEMORY_RANGE, "media DMA destination wraps address space");
    size_t written = source.length.value;
    if (written && !ram_write(d, BB_RAM_MEDIA_DMA, destination, source.bytes, written)) written = 0;
    else if (written < count) {
        memset(d->feed, 0xff, sizeof d->feed);
        while (written < count) {
            size_t chunk = count - written;
            if (chunk > sizeof d->feed) chunk = sizeof d->feed;
            if (!ram_write(d, BB_RAM_MEDIA_DMA, destination + written, d->feed, chunk)) break;
            written += chunk;
        }
    }
    if (written != count) {
        char detail[160];
        snprintf(detail, sizeof detail, "media DMA destination=0x%08" PRIx32 " length=0x%zx refused by RAM model", destination, count);
        return fail(d, BB_ACCESS_MEMORY_RANGE, detail);
    }
    return BB_ACCESS_OK;
}

/* ROM 0xb74-0x1064; native 0x2ab8, 0x2fc8 and 0x3054. */
static enum BbAccess nand_start(BbDevices *d, uint32_t command) {
    uint32_t *r = d->nand;
    if (command == 0x802042 || command == 0x802242 || command == 0x802362 || command == 0x802062)
        return fail(d, BB_ACCESS_PERSISTENT_WRITE, "logical-media program/erase refused");
    if (r[0] != 1) return fail(d, BB_ACCESS_COMMAND, "media command requires type1 selection");
    bool reset = command == 0x800007 || command == 0x802007;
    bool spare = command == 0x800001 || command == 0x802001 || command == 0x802041;
    bool pio = command == 0x800033 || command == 0x802033 || command == 0x8020f3;
    bool rom_dma = command == 0x8013b3 || command == 0x8033b3;
    bool dma = rom_dma || command == 0x8023f3;
    if (!reset && !spare && !pio && !dma) return fail(d, BB_ACCESS_COMMAND, "media command outside ROM/native read grammar");
    uint32_t setup = r[0x1c / 4], row = r[0x24 / 4], column = r[0x20 / 4];
    uint32_t control = r[0x18 / 4];
    if ((control != 0x8003 && (control != 0x80008003 || reset || spare || rom_dma)) ||
        (dma && !rom_dma && control != 0x80008003))
        return fail(d, BB_ACCESS_COMMAND, "media command control outside source-bound read grammar");
    if (reset) {
        if (setup != 0xff007000) return fail(d, BB_ACCESS_COMMAND, "media reset parameter mismatch");
    } else {
        if (row >= MEDIA_CAPACITY / 2048) return fail(d, BB_ACCESS_MEDIA_RANGE, "media row outside type1 capacity");
        if (spare) {
            if (setup != 0x300000 || column != 0x200) return fail(d, BB_ACCESS_COMMAND, "spare read setup mismatch");
            if (d->inputs.fault == BB_FAULT_MEDIA) return fail(d, BB_ACCESS_MEDIA_RANGE, "required spare read refused by media fault");
            memset(d->aperture, 0xff, sizeof d->aperture);
        } else if (pio) {
            if ((setup != 0x300000 && setup != 0x300001) || column) return fail(d, BB_ACCESS_COMMAND, "PIO read setup mismatch");
            enum BbAccess result = media_read(d, row * 2048, d->aperture, sizeof d->aperture);
            if (result != BB_ACCESS_OK) return result;
        } else {
            uint32_t count = (setup & 0x1ff) * 512;
            uint32_t config = r[0x8a0 / 4], remaining = r[0x8a8 / 4];
            if ((setup & ~0x1ffu) != 0x300000 || !count || count > 0x20000 || column ||
                r[0x884 / 4] != 0x10001 || remaining < count ||
                (config != 0x301000 && config != 0x401000) ||
                (rom_dma && (count != 2048 || config != 0x401000)) ||
                (!rom_dma && remaining != 0x100000)) return fail(d, BB_ACCESS_COMMAND, "media DMA configuration outside ROM/native grammar");
            enum BbAccess result = media_dma(d, row * 2048, r[0x8a4 / 4], count);
            if (result != BB_ACCESS_OK) return result;
            r[0x8a4 / 4] += count;
            r[0x8a8 / 4] -= count;
        }
    }
    r[0x04 / 4] &= ~4u;
    r[0x54 / 4] |= 1;
    r[0x28 / 4] = 0;
    r[0xc8 / 4] = 0;
    ++d->stats.transactions;
    return BB_ACCESS_OK;
}

static bool nand_register(uint32_t offset) {
    static const uint16_t set[] = {0,4,8,0x10,0x18,0x1c,0x20,0x24,0x28,0x30,0x40,0x44,0x48,0x4c,0x50,0x54,0x58,0x5c,0x60,0x70,0x74,0x78,0x7c,0x98,0xc8,0xe8,0x804,0x80c,0x810,0x820,0x824,0x828,0x844,0x884,0x88c,0x890,0x8a0,0x8a4,0x8a8,0x8c4};
    return IN_SET(offset, set);
}

/* Native 0x3844-0x39d8: channel0 uniform 32-byte source, count in bytes. */
static enum BbAccess fill_complete(BbDevices *d) {
    uint32_t *r = d->fill, config = r[0x2c / 4];
    unsigned width = (config >> 12) & 15;
    uint32_t count = r[2], destination = r[1];
    if ((config & ~0xff000u) != 0x1500400 || width < 2 || width > 5 ||
        ((config >> 16) & 15) != width || !count || count > MEDIA_CAPACITY ||
        r[3] || r[4] || r[5] || r[0x30 / 4] || r[0x34 / 4])
        return fail(d, BB_ACCESS_COMMAND, "fill mode/count/stride outside native uniform-fill grammar");
    if (!ram_read(d, r[0], d->feed, 32)) return fail(d, BB_ACCESS_MEMORY_RANGE, "fill descriptor refused by RAM model");
    for (size_t i = 4; i < 32; ++i) if (d->feed[i] != d->feed[i % 4]) return fail(d, BB_ACCESS_COMMAND, "fill descriptor is not eight identical words");
    for (size_t i = 32; i < 4096; ++i) d->feed[i] = d->feed[i % 4];
    if ((uint64_t)destination + count > UINT64_C(0x100000000)) return fail(d, BB_ACCESS_MEMORY_RANGE, "fill destination wraps address space");
    for (uint32_t done = 0; done < count;) {
        uint32_t chunk = count - done;
        if (chunk > 4096) chunk = 4096;
        if (!ram_write(d, BB_RAM_FILL, destination + done, d->feed, chunk)) return fail(d, BB_ACCESS_MEMORY_RANGE, "fill destination refused by RAM model");
        done += chunk;
        d->stats.fill_bytes += chunk;
    }
    r[0x24 / 4] = 0x40;
    d->fill_armed = false;
    ++d->stats.transactions;
    return BB_ACCESS_OK;
}

/* Native 0x3c54-0x3ea0 feeds method4 bytes after the twenty-byte RAM record header. */
static enum BbAccess lz_start(BbDevices *d) {
    uint32_t *r = d->lz;
    uint32_t count = r[0x44 / 4] ? r[0x44 / 4] : 0x80000;
    uint32_t source = r[0x40 / 4], destination = r[0x48 / 4];
    if (r[0] != 1 || r[0x80 / 4] != 0x30003000 || r[0x8c / 4] != 1 ||
        (r[0x44 / 4] & ~0x7ffffu) || r[0x88 / 4]) return fail(d, BB_ACCESS_COMMAND, "LZ start setup/method/status mismatch");
    if (!d->decoded) {
        uint8_t header[20];
        if (source < 20 || !ram_read(d, source - 20, header, sizeof header)) return fail(d, BB_ACCESS_MEMORY_RANGE, "LZ RAM header unavailable at first source minus20");
        uint32_t out = le_word(header, 4), total = le_word(header + 4, 4);
        if (!out || out > MAX_DECODED || total < 20 || total > MAX_RECORD ||
            le_word(header + 8, 4) != (total + 0x3fff) / 0x4000 ||
            le_word(header + 12, 4) != 0x4000 || le_word(header + 16, 4) != 4)
            return fail(d, BB_ACCESS_CODEC, "LZ RAM record header outside bounded method4 grammar");
        d->decoded = malloc(out);
        if (!d->decoded) return fail(d, BB_ACCESS_ALLOCATION, "LZ history allocation failed");
        d->decoded_size = out;
        d->encoded_size = total - 20;
        d->stream_source = source;
        d->stream_destination = destination;
        d->fed = 0;
        fflz_stream_init(&d->stream, (FflzOutput){d->decoded, out});
    }
    if ((uint64_t)d->stream_source + d->fed != source ||
        (uint64_t)d->stream_destination + d->stream.produced != destination ||
        count > d->encoded_size - d->fed) return fail(d, BB_ACCESS_CODEC, "LZ feed is noncontiguous or exceeds RAM header extent");
    size_t before = d->stream.produced;
    for (uint32_t copied = 0; copied < count;) {
        size_t chunk = count - copied;
        if (chunk > sizeof d->feed) chunk = sizeof d->feed;
        if (!ram_read(d, source + copied, d->feed, chunk))
            return fail(d, BB_ACCESS_MEMORY_RANGE, "LZ source refused by RAM model");
        if (fflz_stream_feed(&d->stream, (FflzInput){d->feed, chunk}) != FFLZ_OK)
            return fail(d, BB_ACCESS_CODEC, "LZ fed stream rejected by strict codec");
        copied += chunk;
    }
    d->fed += count;
    if (d->fed == d->encoded_size && (fflz_stream_finish(&d->stream) != FFLZ_OK || d->stream.produced != d->decoded_size))
        return fail(d, BB_ACCESS_CODEC, "LZ final token/output extent differs from RAM header");
    size_t produced = d->stream.produced - before;
    if (!ram_write(d, BB_RAM_LZ, destination, d->decoded + before, produced)) return fail(d, BB_ACCESS_MEMORY_RANGE, "LZ destination refused by RAM model");
    r[0x4c / 4] = (uint32_t)produced;
    r[0x88 / 4] = 1;
    d->stats.decoded_bytes += produced;
    ++d->stats.transactions;
    return BB_ACCESS_OK;
}

/* Captured ROM table 0x22e0, issue 0x10f4, PIO 0x1368 and SDMA 0x148c. */
static enum BbAccess sd_command(BbDevices *d, uint32_t word) {
    static const uint16_t grammar[] = {0x0000,0x081a,0x371a,0x2902,0x0209,0x031a,0x071b,0x101a,0x061a,0x113a};
    unsigned command = word >> 8;
    if (command == 24 || command == 25 || command == 32 || command == 33 || command == 38)
        return fail(d, BB_ACCESS_PERSISTENT_WRITE, "SD program/erase refused");
    if (!IN_SET(word, grammar)) return fail(d, BB_ACCESS_COMMAND, "SD0 command outside captured ROM grammar");
    uint32_t *r = d->sd, argument = r[8], mode = r[12], response = 0, status = 1;
    if ((r[0x2c] & 5) != 5 || (r[0x29] & 15) != 15 || (r[0x34] & 3) != 3 || r[0x30] || r[0x32] || d->sd_remaining)
        return fail(d, BB_ACCESS_COMMAND, "SD0 command before clock/power/status setup or before PIO consumption");
    bool app = d->sd_app;
    d->sd_app = false;
    if (command != 17 && mode) return fail(d, BB_ACCESS_COMMAND, "SD0 nondata command has transfer mode");
    uint32_t r1 = (uint32_t)d->sd_state << 9 | 0x100;
    switch (command) {
    case 0:
        if (argument) return fail(d, BB_ACCESS_COMMAND, "CMD0 argument mismatch");
        d->sd_state = SD_IDLE;
        break;
    case 8:
        if (d->sd_state != SD_IDLE || argument != 0x1aa) return fail(d, BB_ACCESS_COMMAND, "CMD8 state/voltage mismatch");
        response = 0x1aa;
        break;
    case 55:
        if (argument != (d->sd_state >= SD_STANDBY ? 0x5a5a0000u : 0)) return fail(d, BB_ACCESS_COMMAND, "CMD55 RCA mismatch");
        d->sd_app = true;
        response = r1 | 0x20;
        break;
    case 41:
        if (!app || d->sd_state > SD_READY || argument != 0x40100000) return fail(d, BB_ACCESS_COMMAND, "ACMD41 state/HCS mismatch");
        d->sd_state = SD_READY;
        response = 0xc0ff8000;
        break;
    case 2:
        if (d->sd_state != SD_READY || argument) return fail(d, BB_ACCESS_COMMAND, "CMD2 state/argument mismatch");
        d->sd_state = SD_IDENT;
        break;
    case 3:
        if (d->sd_state != SD_IDENT || argument) return fail(d, BB_ACCESS_COMMAND, "CMD3 state/argument mismatch");
        d->sd_state = SD_STANDBY;
        response = 0x5a5a0500;
        break;
    case 7:
        if (d->sd_state != SD_STANDBY || argument != 0x5a5a0000) return fail(d, BB_ACCESS_COMMAND, "CMD7 state/RCA mismatch");
        d->sd_state = SD_TRANSFER;
        response = 0x900;
        status |= 2;
        break;
    case 16:
        if (d->sd_state != SD_TRANSFER || argument != 512) return fail(d, BB_ACCESS_COMMAND, "CMD16 state/blocklength mismatch");
        response = r1;
        break;
    case 6:
        if (!app || d->sd_state != SD_TRANSFER || argument != 2) return fail(d, BB_ACCESS_COMMAND, "ACMD6 state/width mismatch");
        response = r1;
        break;
    case 17: {
        if (d->sd_state != SD_TRANSFER || (r[4] & 0xfff) != 512 || r[6] != 1 || (mode != 0x10 && mode != 0x11))
            return fail(d, BB_ACCESS_COMMAND, "CMD17 requires single512-byte PIO/SDMA transfer");
        uint64_t offset = (uint64_t)argument * 512;
        if (offset + 512 > d->inputs.card.length.value) return fail(d, BB_ACCESS_MEDIA_RANGE, "SD sector beyond supplied exact card extent");
        const uint8_t *block = d->inputs.card.bytes + (size_t)offset;
        if (mode == 0x11) {
            if (r[0] < 0x40000000 || r[0] >= 0x80000000 || (r[0] & 3) ||
                !ram_write(d, BB_RAM_SD_DMA, r[0], block, 512)) return fail(d, BB_ACCESS_MEMORY_RANGE, "SDMA destination outside RAM alias");
            r[0] += 512;
            status |= 2;
        } else {
            memcpy(d->sd_buffer, block, 512);
            d->sd_position = 0;
            d->sd_remaining = 512;
            status |= 0x20;
        }
        response = r1;
        d->stats.sd_read_bytes += 512;
        break;
    }
    default: return fail(d, BB_ACCESS_COMMAND, "SD command dispatcher grammar mismatch");
    }
    r[0x0e] = word;
    r[0x10] = response;
    r[0x30] |= status & r[0x34];
    ++d->stats.transactions;
    return BB_ACCESS_OK;
}

static size_t sd_width(uint32_t offset) {
    switch (offset) {
    case 0: case 8: case 0x10: case 0x20: case 0x24: return 4;
    case 4: case 6: case 0xc: case 0xe: case 0x2c: case 0x30: case 0x32: case 0x34: case 0x36: return 2;
    case 0x28: case 0x29: case 0x2e: case 0x2f: return 1;
    default: return 0;
    }
}

static enum BbAccess irq_access(BbDevices *d, unsigned bank, uint32_t offset, bool write, uint32_t *value) {
    unsigned group = (offset & 15) / 4;
    uint32_t *word = &d->irq[bank][offset / 4];
    if (write) {
        if (family(offset, 0x80, 4)) d->enabled[bank][group] |= *value;
        else if (family(offset, 0xa0, 4)) d->enabled[bank][group] &= ~*value;
        else if (family(offset, 0xc0, 4)) {
            d->pending[bank][group] |= *value;
            if (bank == 1 && offset == 0xcc && (*value & 0x1000000)) ++d->stats.peer_release_requests;
        } else if (family(offset, 0xe0, 4) || family(offset, 0x120, 4)) d->pending[bank][group] &= ~*value;
        else if (family(offset, 0x100, 4)) d->source[bank][group] = *value;
        else if (offset == 0x1c0 || offset == 0x1c4) {
            if (*value != 0xffff) return fail(d, BB_ACCESS_COMMAND, "IRQ current-source reset mismatch");
        } else if (offset == 0x1c8 || offset == 0xc00) {
            if (*value != (offset == 0x1c8 ? 1u : 0u)) return fail(d, BB_ACCESS_COMMAND, "IRQ bank reset command mismatch");
            *word = *value;
        } else if (offset == 0x200) {
            if (*value) return fail(d, BB_ACCESS_COMMAND, "IRQ end-of-interrupt command mismatch");
        } else if (family(offset, 0x60, 4) || family(offset, 0x140, 8) || family(offset, 0x180, 8) ||
                   family(offset, 0x400, 128) || family(offset, 0x800, 128)) {
            if (family(offset, 0x800, 128) && *value > 15) return fail(d, BB_ACCESS_COMMAND, "IRQ priority outside source-bound four-bit field");
            *word = *value;
        } else return fail(d, BB_ACCESS_REGISTER, "IRQ write outside ROM/native register census");
    } else {
        if (family(offset, 0x80, 4)) *value = d->enabled[bank][group];
        else if (family(offset, 0x100, 4)) *value = d->source[bank][group];
        else if (offset == 0x1c0 || offset == 0x1c4) *value = 0xffff;
        else if (offset == 0x200 || family(offset, 0x230, 4)) *value = 0;
        else if (family(offset, 0x60, 4) || family(offset, 0x140, 8) || family(offset, 0x180, 8) ||
                 family(offset, 0x400, 128) || family(offset, 0x800, 128)) *value = *word;
        else return fail(d, BB_ACCESS_REGISTER, "IRQ read outside ROM/native register census");
    }
    return BB_ACCESS_OK;
}

BbDevices *bb_devices_create(BbDeviceInputs inputs) {
    if (!inputs.ram.read || !inputs.ram.write || !inputs.media.bytes || !inputs.media.length.value ||
        inputs.media.length.value > MEDIA_CAPACITY || !inputs.card.bytes || !inputs.card.length.value ||
        inputs.card.length.value % 512 ||
        (inputs.route != BB_ROUTE_MEDIA && inputs.route != BB_ROUTE_FAT_SD) ||
        (inputs.fault != BB_FAULT_NONE && inputs.fault != BB_FAULT_TAP && inputs.fault != BB_FAULT_MARKER &&
         inputs.fault != BB_FAULT_MEDIA && inputs.fault != BB_FAULT_ROUTE)) return NULL;
    BbDevices *d = calloc(1, sizeof *d);
    if (!d) return NULL;
    d->inputs = inputs;
    snprintf(d->detail, sizeof d->detail, "%s", assumptions);
    d->system[0] = 1;
    bool media = (inputs.route == BB_ROUTE_MEDIA) != (inputs.fault == BB_FAULT_ROUTE);
    d->gpio[0x14 / 4] = media ? 0xc00 : 0;
    d->gpio[0x94 / 4] = 0x200;
    d->gpio[0x314 / 4] = 0;
    d->clock[0x80 / 4] = 0x10000;
    memset(d->aperture, 0xff, sizeof d->aperture);
    for (unsigned i = 0x40 / 4; i <= 0x4c / 4; ++i) d->nand[i] = UINT32_MAX;
    return d;
}

void bb_devices_destroy(BbDevices *d) {
    if (d) free(d->decoded);
    free(d);
}

static enum BbAccess access_device(BbDevices *d, uint32_t a, size_t width, bool write, uint32_t *value) {
    if (d->failure != BB_ACCESS_OK) return d->failure;
    if (width != 1 && width != 2 && width != 4) return fail(d, BB_ACCESS_WIDTH, "device access width is not1/2/4");
    if (a & (width - 1)) return fail(d, BB_ACCESS_WIDTH, "unaligned device access");
    if (write && width < 4) *value &= (1u << (width * 8)) - 1;
    advance(d);
    if (a >= 0xfff50000 && a < 0xfff50038) {
        uint32_t o = a - 0xfff50000;
        if (width != sd_width(o)) return fail(d, BB_ACCESS_WIDTH, "SD0 offset/access width outside ROM census");
        if (write) {
            if (o == 0x10 || o == 0x20 || o == 0x24) return fail(d, BB_ACCESS_REGISTER, "SD0 response/data/present-state write refused");
            if (o == 0x30 || o == 0x32) d->sd[o] &= ~*value;
            else if (o == 0x2f) {
                if (*value & ~1u) return fail(d, BB_ACCESS_COMMAND, "SD0 reset outside ROM reset-all grammar");
                if (*value & 1) {
                    memset(d->sd, 0, sizeof d->sd);
                    d->sd_remaining = 0;
                    d->sd_position = 0;
                }
            } else if (o == 0xe) return sd_command(d, *value);
            else d->sd[o] = *value;
        } else if (o == 0x20) {
            if (d->sd_remaining < 4) return fail(d, BB_ACCESS_COMMAND, "SD0 PIO read without ready sector");
            *value = le_word(d->sd_buffer + d->sd_position, 4);
            d->sd_position += 4;
            d->sd_remaining -= 4;
            if (!d->sd_remaining) d->sd[0x30] |= 2 & d->sd[0x34];
        } else if (o == 0x24) *value = 0x30000;
        else if (o == 0x2c) *value = d->sd[o] | ((d->sd[o] & 1) ? 2u : 0u);
        else *value = d->sd[o];
        return BB_ACCESS_OK;
    }
    if (a >= 0xfff52400 && a < 0xfff52600 && (uint64_t)a + width <= 0xfff52600) {
        if (write) return fail(d, BB_ACCESS_PERSISTENT_WRITE, "media data-window write refused");
        *value = le_word(d->aperture + a - 0xfff52400, width);
        return BB_ACCESS_OK;
    }
    if (width != 4) return fail(d, BB_ACCESS_WIDTH, "device register requires aligned32-bit access");
    uint32_t *s = startup(d, a);
    if (s) {
        if (write) {
            if (a == 0xfff88080 || (a >= 0xfff89000 && a < 0xfff89480 && (a & 0x3f) == 0x14))
                return fail(d, BB_ACCESS_REGISTER, "startup fixture status is read-only");
            if (a == 0xfff98008) {
                if (*value != 1 && *value != 2) return fail(d, BB_ACCESS_COMMAND, "unsupported native mapping request");
                d->handoff = true;
            }
            if (a == 0xfff88048) {
                if ((*value & ~0x11000000u) || ((*value & 0x1000000) && !(*s & 0x1000000)))
                    return fail(d, BB_ACCESS_COMMAND, "clock48 unknown field or firmware-created ready bit");
                if ((*value ^ *s) & 0x10000000) d->clock48_due = (*value & 0x10000000) ? d->ticks + 2 : 0;
                *s = *value & ((*value & 0x10000000) ? UINT32_MAX : ~0x1000000u);
            } else {
                *s = *value;
                if (a >= 0xfff88000 && a <= 0xfff88020) *s &= ~0x10000000u;
                if (a == 0xfff01000) d->phy_started = (*value & 1) != 0;
            }
        } else {
            *value = *s;
            if ((a == 0xfff88038 || a == 0xfff88040) && (*s & 0x10000000)) *value |= 0x1000000;
            if (a == 0xfff01090 && d->phy_started) *value |= 0x10000000;
        }
        return BB_ACCESS_OK;
    }
    if (a >= 0xfff52000 && a < 0xfff52900) {
        uint32_t o = a - 0xfff52000;
        if (!nand_register(o)) return fail(d, BB_ACCESS_REGISTER, "media offset outside ROM/native register census");
        uint32_t *r = &d->nand[o / 4];
        if (!write) *value = *r;
        else if (o == 0x810 || o == 0x890 || o == 0xc8) return fail(d, BB_ACCESS_REGISTER, "media DMA/ECC status is read-only");
        else if (o == 4) *r &= ~(*value & ~4u);
        else if (o == 0x54) {
            if (*value & ~0xe0001u) return fail(d, BB_ACCESS_COMMAND, "media completion control outside ROM polling grammar");
            *r = (*r & 1 & ~*value) | (*value & 0xe0000);
        } else if (o == 0x80c || o == 0x88c) {
            if (*value & ~1u) return fail(d, BB_ACCESS_COMMAND, "media DMA acknowledge field mismatch");
            *r &= ~*value;
        } else if ((o == 0x844 || o == 0x8c4) && *value) return fail(d, BB_ACCESS_COMMAND, "media DMA unknown status-field control");
        else {
            *r = *value;
            if (o == 8 && (*value & 0x800000)) return nand_start(d, *value);
            if (o == 8 && *value && *value != 0x2000 && *value != 1 && *value != 7 && *value != 0x33 && *value != 0x13b3 && *value != 0x33b3 &&
                *value != 0x2001 && *value != 0x2033 && *value != 0x2041 && *value != 0x2007 && *value != 0x23f3)
                return fail(d, BB_ACCESS_COMMAND, "media command preparation outside ROM/native grammar");
        }
        return BB_ACCESS_OK;
    }
    if (a >= 0xfff02000 && a < 0xfff02038) {
        uint32_t o = a - 0xfff02000;
        if (o == 0x18 || o == 0x1c || o == 0x20) return fail(d, BB_ACCESS_REGISTER, "fill offset outside native census");
        if (!write) {
            if (o == 0x24 && d->fill_armed) {
                enum BbAccess result = fill_complete(d);
                if (result != BB_ACCESS_OK) return result;
            }
            *value = d->fill[o / 4];
        } else if (o == 0x24) return fail(d, BB_ACCESS_REGISTER, "fill completion status is read-only");
        else {
            if (o == 0x28) {
                if (*value != 8 && *value != 5) return fail(d, BB_ACCESS_COMMAND, "fill control outside native grammar");
                d->fill_armed = *value == 5;
                d->fill[0x24 / 4] = *value == 5 ? 1 : 0;
            }
            d->fill[o / 4] = *value;
        }
        return BB_ACCESS_OK;
    }
    if (a >= 0xfff43000 && a < 0xfff43090) {
        uint32_t o = a - 0xfff43000;
        static const uint16_t set[] = {0,4,0x40,0x44,0x48,0x4c,0x80,0x88,0x8c};
        if (!IN_SET(o, set)) return fail(d, BB_ACCESS_REGISTER, "LZ offset outside native census");
        uint32_t *r = &d->lz[o / 4];
        if (!write) *value = *r;
        else if (o == 0x4c) return fail(d, BB_ACCESS_REGISTER, "LZ output count is read-only");
        else if (o == 0x88) {
            if (*value & ~0xf01u) return fail(d, BB_ACCESS_COMMAND, "LZ acknowledgement field mismatch");
            *r &= ~*value;
        } else if (o == 0) {
            if (*value) return fail(d, BB_ACCESS_COMMAND, "LZ reset requires zero");
            free(d->decoded);
            d->decoded = NULL;
            memset(d->lz, 0, sizeof d->lz);
            d->lz[0] = 1;
        } else {
            if (o == 0x8c && *value > 1) return fail(d, BB_ACCESS_COMMAND, "LZ interrupt-enable field mismatch");
            *r = *value;
            if (o == 4) {
                if (*value != 2) return fail(d, BB_ACCESS_COMMAND, "LZ trigger requires method4 start2");
                return lz_start(d);
            }
        }
        return BB_ACCESS_OK;
    }
    if (a >= 0xfff91000 && a < 0xfff91030) {
        uint32_t o = a - 0xfff91000;
        unsigned channel = o / 16, reg = (o & 15) / 4;
        uint32_t *t = d->timer[channel];
        if (!write) {
            if (reg == 3) return fail(d, BB_ACCESS_REGISTER, "timer acknowledgement has no modeled getter");
            *value = t[reg];
        } else {
            if ((reg == 0 && *value) || (reg == 2 && *value != 0 && *value != 0x23 && *value != 0x53 && *value != 0x63) ||
                (reg == 3 && (channel == 0 || *value != 1 || t[1] || t[2])))
                return fail(d, BB_ACCESS_COMMAND, "timer operation outside native poll grammar");
            t[reg] = *value;
            if (reg == 3 || (channel && reg == 2 && *value)) t[0] = 0;
        }
        return BB_ACCESS_OK;
    }
    for (unsigned bank = 0; bank < 2; ++bank) {
        uint32_t base = bank ? 0xfffb4000 : 0xfffb3000;
        if (a >= base && a < base + 0xc04) return irq_access(d, bank, a - base, write, value);
    }
    return fail(d, BB_ACCESS_REGISTER, "device address outside bounded ROM/native census");
}

enum BbAccess bb_devices_read(BbDevices *d, BbAddress address, BbByteCount width, BbWord *result) {
    if (!d || !result) return BB_ACCESS_REGISTER;
    uint32_t value = 0;
    enum BbAccess status = access_device(d, address.value, width.value, false, &value);
    if (status == BB_ACCESS_OK) result->value = value;
    return status;
}

enum BbAccess bb_devices_write(BbDevices *d, BbAddress address, BbByteCount width, BbWord value) {
    if (!d) return BB_ACCESS_REGISTER;
    return access_device(d, address.value, width.value, true, &value.value);
}

bool bb_devices_handoff(const BbDevices *d) { return d && d->handoff; }
BbDeviceStats bb_devices_stats(const BbDevices *d) { return d ? d->stats : (BbDeviceStats){0}; }
const char *bb_devices_detail(const BbDevices *d) { return d ? d->detail : "device allocation/input rejected"; }
const char *bb_devices_assumptions(void) { return assumptions; }
