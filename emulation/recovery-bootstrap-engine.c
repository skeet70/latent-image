#define _POSIX_C_SOURCE 200809L
#include "recovery-bootstrap.h"
#include <unicorn/unicorn.h>
#include <unicorn/arm.h>
#include <errno.h>
#include <inttypes.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <sys/stat.h>
#include <unistd.h>

#define RAM_SIZE 0x03660000u
#define ROM_SIZE 0x4000u
#define ROM_WINDOW 0xc000u
#define TAP_ADDRESS 0x0071feb4u
#define PREFIX_ADDRESS 0x0061c1fcu
#define PREFIX_END 0x0061c400u
#define PREFIX_EVENT_CAPACITY 512u

enum Terminal { RUNNING, COMPLETE, UNSUPPORTED, NATIVE_HALT, BUDGET, ENGINE_ERROR, ROUTE_MISMATCH };
enum Phase { RESET, ROM_LOAD, NATIVE_RESET, NATIVE_LOAD, ROLLBACK, CLOSE };
static const char *const phase_names[] = {"reset", "rom_load", "native_reset", "native_load", "rollback", "close"};
enum ModelProfile { X_M1_PRIMARY_V1 };
enum RomMode { ROM_MODE_0, ROM_MODE_1, ROM_MODE_2, ROM_MODE_3, ROM_MODE_4, ROM_MODE_5, ROM_MODE_6, ROM_MODE_7, ROM_MODE_UNSELECTED };
enum SnapshotName { STAGING, KERNEL0, KERNEL1, ID4_BEFORE, ID4_BSS, ID4_AFTER, SEGMENTS, SNAPSHOT_COUNT };
typedef struct { BbAddress address; BbByteCount bytes; bool present; } SnapshotExtent;
enum WriteOrigin { CPU_ATTEMPT, MEDIA_DMA, SD_DMA, FILL, LZ };
enum LoadStage { BEFORE_LOAD, PRE_DECODE, IMAGE_DECODE, AFTER_LOAD };
typedef struct {
    enum WriteOrigin origin;
    enum Phase phase;
    enum LoadStage stage;
    BbAddress pc, address, overlap_address;
    BbByteCount bytes, overlap_bytes;
    bool overlap_unchanged;
} PrefixWrite;
typedef struct {
    PrefixWrite events[PREFIX_EVENT_CAPACITY];
    size_t count;
    uint64_t cpu_attempts, device_commits;
    bool incomplete, load_entered, decode_entered, load_returned;
} PrefixWrites;
typedef struct { BbAddress address; BbByteCount bytes; bool applied; } BssPoison;
typedef struct {
    bool entered, callback_entered, restore_entered, returned, armed, restore_cleared;
    BbWord open_before, open_after, return_r0;
    BbAddress sp_before, sp_after;
    BbWord frame;
    BbAddress expected_sp;
    size_t step;
} CloseEvidence;
typedef struct { uint8_t *bytes; size_t length; } InputFile;
typedef struct {
    uint32_t address, width, value, pc, opcode, caller;
    bool write, present;
} Access;
typedef struct {
    uc_engine *uc;
    BbDevices *devices;
    uint8_t *ram, *rom, *sram, *tcm, *vectors;
    const char *output;
    enum BbRoute route;
    enum RomMode selected_rom_mode;
    enum BbFault fault;
    enum Terminal terminal;
    enum Phase phase;
    Access access;
    enum ModelProfile profile;
    InputFile media;
    SnapshotExtent snapshots[SNAPSHOT_COUNT];
    BssPoison bss_poison;
    CloseEvidence close;
    PrefixWrites prefix_writes;
    uint32_t previous_pc, previous_opcode, helper_return;
    bool helper_verified, helper_active;
    uint32_t pc, opcode, id4_address, id4_size;
    uint64_t instructions, limit;
    BbWord pmu_control, pmu_enable, pmu_cycles;
    unsigned wall_seconds;
    struct timespec started;
    bool dram, remap, icache, bootstrap, staged, loaded, file_error;
    uint32_t tap_before, tap_after;
    char detail[512];
} Engine;

static const int registers[] = {
    UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3,
    UC_ARM_REG_R4, UC_ARM_REG_R5, UC_ARM_REG_R6, UC_ARM_REG_R7,
    UC_ARM_REG_R8, UC_ARM_REG_R9, UC_ARM_REG_R10, UC_ARM_REG_R11,
    UC_ARM_REG_R12, UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC
};
static uint32_t word(const uint8_t *p) {
    return (uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24;
}
static void set_word(uint8_t *p, uint32_t value) {
    for (unsigned i = 0; i < 4; ++i) p[i] = (uint8_t)(value >> (8 * i));
}
static void stop(Engine *e, enum Terminal terminal, const char *detail) {
    if (e->terminal != RUNNING) return;
    if (terminal == UNSUPPORTED && !e->access.present) {
        uint32_t caller = 0;
        if (e->uc) uc_reg_read(e->uc, UC_ARM_REG_LR, &caller);
        e->access = (Access){e->pc, 4, e->opcode, e->pc, e->opcode, caller, false, true};
    }
    e->terminal = terminal;
    snprintf(e->detail, sizeof e->detail, "%s", detail);
    if (e->uc) uc_emu_stop(e->uc);
}
static bool checked(Engine *e, uc_err error) {
    if (error == UC_ERR_OK) return true;
    stop(e, ENGINE_ERROR, uc_strerror(error));
    return false;
}
static bool ram_offset(uint32_t address, size_t length, uint32_t *offset) {
    if (length > RAM_SIZE) return false;
    uint64_t end = (uint64_t)address + length;
    if (end <= RAM_SIZE) { *offset = address; return true; }
    if (address >= 0x40000000u && end <= 0x40000000ull + RAM_SIZE) {
        *offset = address - 0x40000000u;
        return true;
    }
    return false;
}
static uint8_t *memory(Engine *e, uint32_t address, size_t length) {
    uint32_t offset;
    if (ram_offset(address, length, &offset)) return e->ram + offset;
    uint64_t end = (uint64_t)address + length;
    if (address >= 0xfff00000u && end <= 0xfff01000ull) return e->sram + address - 0xfff00000u;
    if (address >= 0x70000000u && end <= 0x70002000ull) return e->tcm + address - 0x70000000u;
    if (address >= 0x3fff0000u && end <= 0x40000000ull) return e->vectors + address - 0x3fff0000u;
    if (address >= 0x7fff0000u && end <= 0x80000000ull) return e->vectors + address - 0x7fff0000u;
    return NULL;
}
static bool ram_read(void *context, BbAddress address, uint8_t *bytes, BbByteCount length) {
    uint8_t *source = memory(context, address.value, length.value);
    if (!source) return false;
    memcpy(bytes, source, length.value);
    return true;
}
static bool prefix_overlap(uint32_t address, size_t length, uint32_t *start, size_t *count) {
    uint32_t offset;
    if (!ram_offset(address, length, &offset) || offset >= PREFIX_END ||
        (uint64_t)offset + length <= PREFIX_ADDRESS) return false;
    *start = offset < PREFIX_ADDRESS ? PREFIX_ADDRESS : offset;
    uint64_t end = (uint64_t)offset + length;
    if (end > PREFIX_END) end = PREFIX_END;
    *count = end - *start;
    return true;
}
static bool prefix_capacity(Engine *e) {
    if (e->prefix_writes.count < PREFIX_EVENT_CAPACITY) return true;
    e->prefix_writes.incomplete = true;
    stop(e, ENGINE_ERROR, "Prefix write evidence capacity exceeded");
    return false;
}
static void record_prefix_write(Engine *e, enum WriteOrigin origin, uint32_t address,
                                size_t length, uint32_t start, size_t count, bool overlap_unchanged) {
    PrefixWrites *audit = &e->prefix_writes;
    enum LoadStage stage = audit->load_returned ? AFTER_LOAD :
        audit->decode_entered ? IMAGE_DECODE : audit->load_entered ? PRE_DECODE : BEFORE_LOAD;
    audit->events[audit->count++] = (PrefixWrite){
        origin, e->phase, stage, {e->pc}, {address}, {start}, {length}, {count}, overlap_unchanged
    };
    if (origin != CPU_ATTEMPT) ++audit->device_commits;
}
static void cpu_write(uc_engine *uc, uc_mem_type type, uint64_t address, int width, int64_t value, void *context) {
    (void)uc;
    (void)type;
    Engine *e = context;
    uint32_t start;
    size_t count;
    if (address > UINT32_MAX || width <= 0 ||
        !prefix_overlap((uint32_t)address, (size_t)width, &start, &count)) return;
    ++e->prefix_writes.cpu_attempts;
    if (width > 8) {
        e->prefix_writes.incomplete = true;
        stop(e, ENGINE_ERROR, "Prefix CPU write width exceeds hook value");
        return;
    }
    if (!prefix_capacity(e)) return;
    uint8_t bytes[8];
    for (int i = 0; i < width; ++i) bytes[i] = (uint8_t)((uint64_t)value >> (i * 8));
    uint32_t offset;
    if (!ram_offset((uint32_t)address, (size_t)width, &offset)) return;
    bool overlap_unchanged = memcmp(e->ram + start, bytes + start - offset, count) == 0;
    record_prefix_write(e, CPU_ATTEMPT, (uint32_t)address, (size_t)width, start, count, overlap_unchanged);
}
static bool ram_write(void *context, enum BbRamWriter writer, BbAddress address, const uint8_t *bytes, BbByteCount length) {
    Engine *e = context;
    uint8_t *destination = memory(e, address.value, length.value);
    if (!destination) return false;
    enum WriteOrigin origin;
    switch (writer) {
        case BB_RAM_MEDIA_DMA: origin = MEDIA_DMA; break;
        case BB_RAM_SD_DMA: origin = SD_DMA; break;
        case BB_RAM_FILL: origin = FILL; break;
        case BB_RAM_LZ: origin = LZ; break;
        default:
            e->prefix_writes.incomplete = true;
            stop(e, ENGINE_ERROR, "Unknown device RAM writer");
            return false;
    }
    uint32_t start;
    size_t count;
    bool overlaps = prefix_overlap(address.value, length.value, &start, &count);
    bool overlap_unchanged = false;
    if (overlaps) {
        if (!prefix_capacity(e)) return false;
        uint32_t offset;
        if (!ram_offset(address.value, length.value, &offset)) return false;
        overlap_unchanged = memcmp(e->ram + start, bytes + start - offset, count) == 0;
    }
    memcpy(destination, bytes, length.value);
    if (overlaps) record_prefix_write(e, origin, address.value, length.value, start, count, overlap_unchanged);
    return true;
}
static void failed_access(Engine *e, uint32_t address, unsigned width, bool write, uint32_t value, const char *detail) {
    uint32_t caller = 0;
    uc_reg_read(e->uc, UC_ARM_REG_LR, &caller);
    e->access = (Access){address, width, value, e->pc, e->opcode, caller, write, true};
    stop(e, UNSUPPORTED, detail);
}
static uint64_t mmio_read(uc_engine *uc, uint64_t offset, unsigned width, void *context) {
    (void)uc;
    Engine *e = context;
    BbWord result = {0};
    uint32_t address = 0xfff01000u + (uint32_t)offset;
    enum BbAccess access = bb_devices_read(e->devices, (BbAddress){address}, (BbByteCount){width}, &result);
    if (access == BB_ACCESS_ALLOCATION) stop(e, ENGINE_ERROR, bb_devices_detail(e->devices));
    else if (access != BB_ACCESS_OK) failed_access(e, address, width, false, result.value, bb_devices_detail(e->devices));
    return result.value;
}
static void mmio_write(uc_engine *uc, uint64_t offset, unsigned width, uint64_t value, void *context) {
    (void)uc;
    Engine *e = context;
    uint32_t address = 0xfff01000u + (uint32_t)offset;
    enum BbAccess access = bb_devices_write(e->devices, (BbAddress){address}, (BbByteCount){width}, (BbWord){(uint32_t)value});
    if (access == BB_ACCESS_ALLOCATION) stop(e, ENGINE_ERROR, bb_devices_detail(e->devices));
    else if (access != BB_ACCESS_OK) failed_access(e, address, width, true, (uint32_t)value, bb_devices_detail(e->devices));
}
static bool invalid_memory(uc_engine *uc, uc_mem_type type, uint64_t address, int width, int64_t value, void *context) {
    (void)uc;
    Engine *e = context;
    failed_access(e, (uint32_t)address, (unsigned)width,
                  type == UC_MEM_WRITE_UNMAPPED || type == UC_MEM_WRITE_PROT,
                  (uint32_t)value, "Unmapped or protected memory access");
    return false;
}
static const char *snapshot_name(enum SnapshotName name) {
    static const char *names[] = {
        "staging.bin", "kernel0.bin", "kernel1.bin", "id4-before.bin",
        "id4-bss.bin", "id4-after.bin", "segments.bin"
    };
    return names[name];
}
static bool output_path(Engine *e, const char *name, char *path, size_t size) {
    int length = snprintf(path, size, "%s/%s", e->output, name);
    if (length < 0 || (size_t)length >= size) {
        e->file_error = true;
        stop(e, ENGINE_ERROR, "Output path too long");
        return false;
    }
    return true;
}
static bool snapshot(Engine *e, enum SnapshotName name, uint32_t address, uint32_t size) {
    uint8_t *bytes = memory(e, address, size);
    if (!bytes || size == 0) {
        failed_access(e, address, size, false, 0, "Snapshot extent outside modeled RAM");
        return false;
    }
    if (e->snapshots[name].present) {
        stop(e, UNSUPPORTED, "Duplicate snapshot capture");
        return false;
    }
    char path[4096];
    if (!output_path(e, snapshot_name(name), path, sizeof path)) return false;
    FILE *file = fopen(path, "wbx");
    if (!file) {
        e->file_error = true;
        stop(e, ENGINE_ERROR, "Cannot create private snapshot");
        return false;
    }
    bool written = fwrite(bytes, 1, size, file) == size;
    if (fclose(file) != 0) written = false;
    if (!written) {
        e->file_error = true;
        stop(e, ENGINE_ERROR, "Cannot write private snapshot");
    }
    if (written) e->snapshots[name] = (SnapshotExtent){{address}, {size}, true};
    return written;
}
static bool loaded_snapshots(Engine *e) {
    const uint8_t *table = e->ram + 0x61c000;
    for (unsigned id = 0; id < 2; ++id) {
        const uint8_t *entry = table + 0x40 + id * 36;
        enum SnapshotName name = id == 0 ? KERNEL0 : KERNEL1;
        if (word(entry) != id) {
            failed_access(e, 0x61c040 + id * 36, 4, false, word(entry), "Invalid loaded kernel segment identifier");
            return false;
        }
        if (!snapshot(e, name, word(entry + 4), word(entry + 8))) return false;
    }
    const uint8_t *entry = table + 0x40 + 4 * 36;
    uint32_t size = word(entry + 8);
    if (word(entry) != 4 || size <= 0x28) {
        failed_access(e, 0x61c0d0, 4, false, size, "Invalid loaded id4 segment extent");
        return false;
    }
    e->id4_address = word(entry + 4);
    e->id4_size = size - 0x28;
    if (!snapshot(e, ID4_BEFORE, e->id4_address, e->id4_size) ||
        !snapshot(e, ID4_BSS, word(entry + 12), word(entry + 16)) ||
        !snapshot(e, SEGMENTS, 0x61c040, 8 * 36 + 4)) return false;
    e->tap_before = word(e->ram + TAP_ADDRESS);
    if (e->fault == BB_FAULT_TAP) set_word(e->ram + TAP_ADDRESS, 0xe7f0def0u);
    return true;
}
static bool condition(unsigned condition, uint32_t flags) {
    bool n = (flags >> 31) != 0, z = ((flags >> 30) & 1) != 0;
    bool c = ((flags >> 29) & 1) != 0, v = ((flags >> 28) & 1) != 0;
    switch (condition) {
    case 0: return z; case 1: return !z; case 2: return c; case 3: return !c;
    case 4: return n; case 5: return !n; case 6: return v; case 7: return !v;
    case 8: return c && !z; case 9: return !c || z; case 10: return n == v;
    case 11: return n != v; case 12: return !z && n == v; case 13: return z || n != v;
    default: return true;
    }
}
static bool cp15_bound(uint32_t pc, uint32_t instruction) {
    uint32_t canonical = pc | 0x80000000u;
    if (canonical >= 0x80000040u && canonical <= 0x800000ccu) {
        uint32_t form = instruction & 0xffff0fffu;
        return form == 0xee060f12u || form == 0xee060f11u ||
               form == 0xee060f51u || form == 0xee060f91u;
    }
    if ((canonical == 0x800000d0u && instruction == 0xee110f10u) ||
        (canonical == 0x800000dcu && instruction == 0xee010f10u)) return true;
    switch (pc) {
    case 0x18c4: return instruction == 0xee091f31u;
    case 0x1a60: return instruction == 0xee112f10u;
    case 0x1a70: return instruction == 0xee0f1f15u;
    case 0x1a74: return instruction == 0xee071f15u;
    case 0x1a78: return instruction == 0xee012f10u;
    case 0x1a84: return instruction == 0xee061f12u;
    case 0x1a88: return instruction == 0xee062f91u;
    case 0x1a90: return instruction == 0xee063f11u;
    case 0x1a98: return instruction == 0xee064f51u;
    case 0x6080: case 0x6100: return instruction == 0xee070f3au;
    case 0x608c: case 0x610c: return instruction == 0xee070f15u;
    default: return false;
    }
}
static bool rom_pmu(Engine *e) {
    enum PmuOperation { CONTROL_WRITE, ENABLE_WRITE, DISABLE_WRITE, CONTROL_READ, CYCLES_READ };
    enum PmuOperation operation;
    uint32_t expected;
    switch (e->pc & ~0x80000000u) {
    case 0x548: case 0x638: case 0x1268: case 0x15c0:
        operation = CONTROL_WRITE; expected = 0xee090f1cu; break;
    case 0x584:
        operation = CONTROL_WRITE; expected = 0xee092f1cu; break;
    case 0x568: case 0x5a4: case 0x658: case 0x1294: case 0x15e0:
        operation = CONTROL_WRITE; expected = 0xee091f1cu; break;
    case 0x54c: case 0x63c:
        operation = ENABLE_WRITE; expected = 0xee092f3cu; break;
    case 0x588:
        operation = ENABLE_WRITE; expected = 0xee090f3cu; break;
    case 0x126c: case 0x15c4:
        operation = ENABLE_WRITE; expected = 0xee091f3cu; break;
    case 0x564: case 0x5a0: case 0x654: case 0x1290: case 0x15dc:
        operation = DISABLE_WRITE; expected = 0xee090f5cu; break;
    case 0x1254:
        operation = CONTROL_READ; expected = 0xee190f1cu; break;
    case 0x550: case 0x58c: case 0x640: case 0x12dc: case 0x15c8:
        operation = CYCLES_READ; expected = 0xee190f1du; break;
    default: return false;
    }
    if (e->opcode != expected) return false;
    int target = registers[(e->opcode >> 12) & 15];
    uint32_t value = 0;
    if (operation == CONTROL_READ) value = e->pmu_control.value;
    else if (operation == CYCLES_READ) value = e->pmu_cycles.value;
    else if (!checked(e, uc_reg_read(e->uc, target, &value))) return true;
    switch (operation) {
    case CONTROL_WRITE:
        if (value != 6 && value != 7) {
            stop(e, UNSUPPORTED, "ROM PMCR write outside reset/enable profile"); return true;
        }
        e->pmu_control.value = value & 1;
        e->pmu_cycles.value = 0;
        break;
    case ENABLE_WRITE: case DISABLE_WRITE:
        if (value != 0x80000001u) {
            stop(e, UNSUPPORTED, "ROM PMU counter bitmap outside profile"); return true;
        }
        e->pmu_enable.value = operation == ENABLE_WRITE ? value : 0;
        break;
    case CONTROL_READ: case CYCLES_READ:
        if (!checked(e, uc_reg_write(e->uc, target, &value))) return true;
        break;
    }
    uint32_t next = e->pc + 4;
    checked(e, uc_reg_write(e->uc, UC_ARM_REG_PC, &next));
    return true;
}
static const uint8_t *media_span(Engine *e, uint64_t offset, size_t length) {
    if (offset > e->media.length || length > e->media.length - offset) return NULL;
    return e->media.bytes + offset;
}
static bool native_word(Engine *e, uint32_t address, uint32_t opcode) {
    const uint8_t *source = media_span(e, 0x40000ull + address, 4);
    if (!source || word(source) != opcode || word(e->ram + address) != opcode) {
        stop(e, UNSUPPORTED, "Native source opcode differs from captured bootstrap ABI");
        return false;
    }
    return true;
}
static bool poison_bss(Engine *e) {
    const uint8_t *entry = e->ram + 0x61c0d0;
    const uint8_t *source = media_span(e, 0x1400d0, 36);
    uint32_t address = word(entry + 12), bytes = word(entry + 16);
    uint32_t lr;
    if (!checked(e, uc_reg_read(e->uc, UC_ARM_REG_LR, &lr))) return false;
    if (e->bss_poison.applied || e->phase != NATIVE_LOAD ||
        e->previous_pc != 0x1d5c || lr != 0x1d60 ||
        !source || memcmp(entry, source, 36) || word(entry) != 4 ||
        !bytes || address >= RAM_SIZE || bytes > RAM_SIZE - address) {
        stop(e, UNSUPPORTED, "Native id4 BSS poison fixture has invalid live segment or entry state");
        return false;
    }
    if (!native_word(e, 0x1d5c, 0xeb00013bu) ||
        !native_word(e, 0x2250, 0xe92d4870u)) return false;
    memset(e->ram + address, 0xa5, bytes);
    e->bss_poison = (BssPoison){{address}, {bytes}, true};
    return true;
}
static bool integer_helper(Engine *e) {
    static const uint32_t captured[] = {
        0xe2512001, 0x012fff1e, 0x3a000036, 0xe1500001, 0x9a000022,
        0xe1110002, 0x0a000023, 0xe311020e, 0x01a01181, 0x03a03008,
        0x13a03001, 0xe3510201, 0x31510000, 0x31a01201, 0x31a03203,
        0x3afffffa, 0xe3510102, 0x31510000, 0x31a01081, 0x31a03083,
        0x3afffffa, 0xe3a02000, 0xe1500001, 0x20400001, 0x21822003,
        0xe15000a1, 0x204000a1, 0x218220a3, 0xe1500121, 0x20400121,
        0x21822123, 0xe15001a1, 0x204001a1, 0x218221a3, 0xe3500000,
        0x11b03223, 0x11a01221, 0x1affffef, 0xe1a00002, 0xe12fff1e,
        0x03a00001, 0x13a00000, 0xe12fff1e, 0xe3510801, 0x21a01821,
        0x23a02010, 0x33a02000, 0xe3510c01, 0x21a01421, 0x22822008,
        0xe3510010, 0x21a01221, 0x22822004, 0xe3510004, 0x82822003,
        0x908220a1, 0xe1a00230, 0xe12fff1e, 0xe3500000, 0x13e00000,
        0xeaffd6e9, 0xe3510000, 0x0afffffa, 0xe92d4003, 0xebffffbe,
        0xe8bd4006, 0xe0030092, 0xe0411003, 0xe12fff1e
    };
    bool inside = e->pc >= 0x4d978 && e->pc < 0x4da8c;
    if (!inside) {
        if (!e->helper_active) return true;
        if (e->pc != e->helper_return ||
            (e->previous_opcode & 0x0fffffffu) != 0x012fff1eu) {
            stop(e, UNSUPPORTED, "Loaded integer helper left its captured return closure");
            return false;
        }
        e->helper_active = false;
        return true;
    }
    if (e->phase != NATIVE_LOAD || !e->staged || (e->pc & 3)) {
        stop(e, UNSUPPORTED, "Loaded integer helper outside native load");
        return false;
    }
    if (!e->helper_active) {
        uint32_t caller = e->pc == 0x4da6c ? 0x3f80 : 0x3f98;
        uint32_t opcode = caller == 0x3f80 ? 0xeb0126b9u : 0xeb012676u;
        uint32_t lr;
        if (!checked(e, uc_reg_read(e->uc, UC_ARM_REG_LR, &lr))) return false;
        if ((e->pc != 0x4da6c && e->pc != 0x4d978) ||
            e->previous_pc != caller || lr != caller + 4 ||
            !native_word(e, caller, opcode)) {
            stop(e, UNSUPPORTED, "Loaded integer helper entered outside captured native call");
            return false;
        }
        e->helper_return = lr;
        e->helper_active = true;
    }
    if (!e->helper_verified) {
        const uint8_t *entry = e->ram + 0x61c040;
        const uint8_t *source_entry = media_span(e, 0x140040, 36);
        uint32_t load = word(entry + 4), bytes = word(entry + 8);
        if (!source_entry || memcmp(entry, source_entry, 36) || word(entry) ||
            load > 0x4d978 || bytes < 0x4da8c - load) {
            stop(e, UNSUPPORTED, "Integer helper is not in selected loaded kernel0 segment");
            return false;
        }
        const uint8_t *source = media_span(e, (uint64_t)word(entry + 20) + 0x4d978 - load, sizeof captured);
        if (!source || memcmp(e->ram + 0x4d978, source, sizeof captured)) {
            stop(e, UNSUPPORTED, "Loaded integer helper differs from selected logical media");
            return false;
        }
        for (size_t i = 0; i < sizeof captured / sizeof captured[0]; ++i) {
            if (word(source + i * 4) != captured[i]) {
                stop(e, UNSUPPORTED, "Selected kernel0 integer helper differs from captured source closure");
                return false;
            }
        }
        e->helper_verified = true;
    }
    if (e->opcode != captured[(e->pc - 0x4d978) / 4]) {
        stop(e, UNSUPPORTED, "Loaded integer helper opcode changed after source binding");
        return false;
    }
    return true;
}
static bool native_close(Engine *e) {
    if (!e->staged) return true;
    typedef struct { uint32_t pc, opcode; int stack_delta; } CloseInstruction;
    static const CloseInstruction trace[] = {
        {0x2044,0xe92d4800,-8}, {0x2048,0xe28db004,0}, {0x204c,0xeb00041a,0},
        {0x30bc,0xe92d4800,-8}, {0x30c0,0xe28db004,0}, {0x30c4,0xeb00004b,0},
        {0x31f8,0xe52db004,-4}, {0x31fc,0xe28db000,0}, {0x3200,0xe3043800,0},
        {0x3204,0xe3403000,0}, {0x3208,0xe5930000,0}, {0x320c,0xe28bd000,0},
        {0x3210,0xe8bd0800,4}, {0x3214,0xe12fff1e,0}, {0x30c8,0xe3500001,0},
        {0x30cc,0x18bd8800,0}, {0x30d0,0xeb0000c2,0},
        {0x33e0,0xe92d4800,-8}, {0x33e4,0xe28db004,0}, {0x33e8,0xe3050258,0},
        {0x33ec,0xe3400000,0}, {0x33f0,0xebfffce3,0},
        {0x2784,0xe52db004,-4}, {0x2788,0xe28db000,0}, {0x278c,0xe5902000,0},
        {0x2790,0xe3e03aad,0}, {0x2794,0xe5032fff,0}, {0x2798,0xe5902004,0},
        {0x279c,0xe5032faf,0}, {0x27a0,0xe5902008,0}, {0x27a4,0xe5032fa7,0},
        {0x27a8,0xe590200c,0}, {0x27ac,0xe5032f67,0}, {0x27b0,0xe5902010,0},
        {0x27b4,0xe5032f87,0}, {0x27b8,0xe28bd000,0}, {0x27bc,0xe8bd0800,4},
        {0x27c0,0xe12fff1e,0}, {0x33f4,0xe3043800,0}, {0x33f8,0xe3403000,0},
        {0x33fc,0xe3a02000,0}, {0x3400,0xe5832000,0}, {0x3404,0xe8bd8800,8},
        {0x30d4,0xe8bd8800,8}, {0x2050,0xe3a00000,0}, {0x2054,0xe8bd8800,8},
        {0x1fe0,0xe3003000,0}
    };
    CloseEvidence *close = &e->close;
    if (!close->entered && e->pc != 0x2044) {
        if ((e->pc == 0x1fe0 && e->staged) ||
            (close->armed && (e->pc == 0x30bc || e->pc == 0x31f8 || e->pc == 0x33e0))) {
            stop(e, UNSUPPORTED, "Native close continuation entered without original close");
            return false;
        }
        return true;
    }
    if (close->returned) {
        static const uint32_t continuation[] = {
            0xe344300a, 0xe3a02000, 0xe58320c4, 0xe3003000, 0xe3443002, 0xe5832000
        };
        size_t index = close->step - sizeof trace / sizeof trace[0];
        uint32_t sp, frame;
        if (index >= sizeof continuation / sizeof continuation[0] ||
            e->pc != 0x1fe4 + index * 4 ||
            !native_word(e, e->pc, continuation[index])) {
            stop(e, UNSUPPORTED, "Native post-close continuation differs from captured source");
            return false;
        }
        if (!checked(e, uc_reg_read(e->uc, UC_ARM_REG_SP, &sp)) ||
            !checked(e, uc_reg_read(e->uc, UC_ARM_REG_R11, &frame))) return false;
        if (sp != close->sp_after.value || frame != close->frame.value || word(e->ram + 0x4800)) {
            stop(e, UNSUPPORTED, "Native post-close continuation changed original frame or state");
            return false;
        }
        ++close->step;
        return true;
    }
    uint32_t sp, lr, frame, r0;
    if (!checked(e, uc_reg_read(e->uc, UC_ARM_REG_SP, &sp)) ||
        !checked(e, uc_reg_read(e->uc, UC_ARM_REG_LR, &lr)) ||
        !checked(e, uc_reg_read(e->uc, UC_ARM_REG_R11, &frame)) ||
        !checked(e, uc_reg_read(e->uc, UC_ARM_REG_R0, &r0))) return false;
    if (!close->entered) {
        bool caller = e->route == BB_ROUTE_MEDIA ? e->previous_pc == 0x1fdc :
                      e->bootstrap ? e->previous_pc == 0x611c && e->previous_opcode == 0xeaffefc8u :
                      e->previous_pc == 0x609c && e->previous_opcode == 0xeaffefe8u;
        if (!e->loaded || !close->armed || !caller || lr != 0x1fe0 ||
            sp != close->expected_sp.value || (sp & 7) || sp < 28 || !memory(e, sp - 28, 28)) {
            stop(e, UNSUPPORTED, "Native close entry violates captured caller or stack ABI");
            return false;
        }
        close->entered = true;
        close->sp_before.value = sp;
        close->open_before.value = word(e->ram + 0x4800);
        close->frame.value = frame;
        e->phase = CLOSE;
    }
    const CloseInstruction *expected = &trace[close->step];
    if (e->pc != expected->pc || sp != close->expected_sp.value ||
        !native_word(e, expected->pc, expected->opcode)) {
        stop(e, UNSUPPORTED, "Native close source or stack continuation differs from captured closure");
        return false;
    }
    uint32_t expected_lr = e->pc == 0x30bc ? 0x2050 :
                           e->pc == 0x31f8 ? 0x30c8 :
                           e->pc == 0x33e0 ? 0x30d4 :
                           e->pc == 0x2784 ? 0x33f4 : 0;
    if (e->pc == 0x3404) close->restore_cleared = true;
    if ((expected_lr && lr != expected_lr) ||
        (e->pc == 0x2784 && r0 != 0x5258) ||
        (e->pc == 0x30c8 && r0 != 1) || close->open_before.value != 1 ||
        word(e->ram + 0x4800) != (close->restore_cleared ? 0u : 1u)) {
        stop(e, UNSUPPORTED, "Native close callback or open state violates captured ABI");
        return false;
    }
    if (e->pc == 0x31f8) close->callback_entered = true;
    if (e->pc == 0x33e0) close->restore_entered = true;
    if (e->pc == 0x1fe0) {
        close->sp_after.value = sp;
        close->open_after.value = word(e->ram + 0x4800);
        close->return_r0.value = r0;
        close->returned = true;
        if (sp != close->sp_before.value || frame != close->frame.value ||
            close->open_after.value || r0 || !close->callback_entered || !close->restore_entered) {
            stop(e, UNSUPPORTED, "Native close return violates original wrapper frame or state");
            return false;
        }
    }
    close->expected_sp.value = (uint32_t)((int64_t)sp + expected->stack_delta);
    ++close->step;
    return true;
}
static bool rom_thumb(Engine *e, uint32_t size) {
    static const struct {
        BbAddress pc;
        BbWord opcode;
        BbByteCount width;
        BbAddress return_pc;
    } instructions[] = {
        {{0x22cc}, {0xf0f1fbb0}, {4}, {0x1d50}},
        {{0x22d0}, {0x4770}, {2}, {0x1d50}},
        {{0x22d2}, {0xf2f1fbb0}, {4}, {0x1ef8}},
        {{0x22d6}, {0x0111fb02}, {4}, {0x1ef8}},
        {{0x22da}, {0x4610}, {2}, {0x1ef8}},
        {{0x22dc}, {0x4770}, {2}, {0x1ef8}}
    };
    uint32_t canonical = e->pc & ~0x80000000u;
    uint32_t mask = size == 2 ? 0xffffu : UINT32_MAX;
    uint32_t lr;
    if (e->phase == ROM_LOAD &&
        checked(e, uc_reg_read(e->uc, UC_ARM_REG_LR, &lr))) {
        for (size_t i = 0; i < sizeof instructions / sizeof instructions[0]; ++i) {
            if (canonical == instructions[i].pc.value && size == instructions[i].width.value &&
                (e->opcode & mask) == instructions[i].opcode.value &&
                (word(e->rom + canonical) & mask) == instructions[i].opcode.value &&
                lr == (instructions[i].return_pc.value | (e->pc & 0x80000000u))) {
                ++e->instructions;
                if (e->pmu_control.value && (e->pmu_enable.value & 0x80000000u)) e->pmu_cycles.value += 1024;
                return true;
            }
        }
    }
    stop(e, UNSUPPORTED, "Thumb execution outside captured ROM arithmetic closure");
    return false;
}
static void instruction(uc_engine *uc, uint64_t address, uint32_t size, void *context) {
    Engine *e = context;
    e->previous_pc = e->pc;
    e->previous_opcode = e->opcode;
    e->pc = (uint32_t)address;
    if (e->terminal != RUNNING) { uc_emu_stop(uc); return; }
    if (!e->dram && bb_devices_handoff(e->devices)) {
        e->remap = true;
        uc_emu_stop(uc);
        return;
    }
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) { stop(e, ENGINE_ERROR, "Monotonic clock unavailable"); return; }
    if (e->instructions >= e->limit ||
        (double)(now.tv_sec - e->started.tv_sec) + (double)(now.tv_nsec - e->started.tv_nsec) / 1e9 >= e->wall_seconds) {
        stop(e, BUDGET, "Instruction or wall-clock bound reached before endpoint"); return;
    }
    uint32_t flags;
    if (!checked(e, uc_reg_read(uc, UC_ARM_REG_CPSR, &flags)) ||
        !checked(e, uc_mem_read(uc, address, &e->opcode, sizeof e->opcode))) return;
    if (flags & 0x20) { rom_thumb(e, size); return; }
    if (size != 4) { stop(e, UNSUPPORTED, "ARM instruction width outside captured bootstrap profile"); return; }
    if (!((address < 0x40000u) ||
          (e->phase == ROM_LOAD && address >= 0x50000u && address < 0x50200u) ||
          (e->phase == NATIVE_LOAD && address >= 0x4d978u && address < 0x4da8cu) ||
          (address >= 0x80000000u && address < 0x80000000ull + ROM_WINDOW) ||
          (address >= 0xfff00000u && address < 0xfff01000u))) {
        stop(e, UNSUPPORTED, "Execution outside ROM, staging, or ROM SRAM code"); return;
    }
    if (!integer_helper(e)) return;
    if (!e->dram && (e->pc | 0x80000000u) == 0x80000134u) {
        uint32_t selected;
        if (!checked(e, uc_reg_read(uc, UC_ARM_REG_R0, &selected))) return;
        if (selected > ROM_MODE_7) {
            stop(e, UNSUPPORTED, "Captured ROM selected a value outside its eight-mode dispatcher");
            return;
        }
        e->selected_rom_mode = (enum RomMode)selected;
        uint32_t expected = e->route == BB_ROUTE_MEDIA ? 4 : 1;
        if (selected != expected) {
            stop(e, ROUTE_MISMATCH, "Captured ROM strap dispatch selected a different route");
            return;
        }
    }
    if (e->dram && address == 0x1800 && !e->staged) {
        e->phase = NATIVE_RESET;
        if (!snapshot(e, STAGING, 0, 0x40000)) return;
        e->staged = true;
    }
    if (e->staged && address == 0x1e4c) {
        if (e->phase != NATIVE_RESET || !native_word(e, 0x1e4c, 0xe92d4830u)) {
            stop(e, UNSUPPORTED, "Native load entry violates selected source or phase"); return;
        }
        e->phase = NATIVE_LOAD;
        e->prefix_writes.load_entered = true;
    }
    if (e->staged && address == 0x2250) {
        if (!poison_bss(e)) return;
        e->prefix_writes.decode_entered = true;
    }
    if (e->staged && address == 0x1fdc && !e->loaded) {
        uint32_t sp, r0;
        uint32_t call = e->bootstrap ? 0xeb001037u :
                        e->route == BB_ROUTE_MEDIA ? 0xeb000018u : 0xeb001017u;
        if (e->opcode != call || !e->bss_poison.applied ||
            !checked(e, uc_reg_read(uc, UC_ARM_REG_SP, &sp)) ||
            !checked(e, uc_reg_read(uc, UC_ARM_REG_R0, &r0))) {
            stop(e, UNSUPPORTED, "Native loaded continuation violates selected source call or BSS fixture");
            return;
        }
        if (r0 || (sp & 7) || sp < 28 || !memory(e, sp - 28, 28)) {
            stop(e, UNSUPPORTED, "Native loaded continuation violates successful load ABI");
            return;
        }
        e->close.armed = true;
        e->close.expected_sp.value = sp;
        if (!loaded_snapshots(e)) return;
        e->loaded = true;
        e->prefix_writes.load_returned = true;
        e->phase = e->route == BB_ROUTE_FAT_SD ? ROLLBACK : CLOSE;
    }
    if (!native_close(e)) return;
    if (e->staged && address == 0x1ff8) {
        BbDeviceStats stats = bb_devices_stats(e->devices);
        if (!e->loaded || !e->close.returned || !e->close.callback_entered ||
            !e->close.restore_entered || !e->bss_poison.applied ||
            e->previous_pc != 0x1ff4 ||
            stats.decoded_bytes != e->id4_size || stats.fill_bytes < e->bss_poison.bytes.value ||
            !e->instructions || !stats.transactions || !stats.media_read_bytes || !stats.peer_release_requests) {
            stop(e, UNSUPPORTED, "Endpoint reached without captured native load and close continuation"); return;
        }
        const PrefixWrites *audit = &e->prefix_writes;
        if (audit->incomplete || !audit->load_entered || !audit->decode_entered ||
            !audit->load_returned || !audit->device_commits) {
            stop(e, ENGINE_ERROR, "Endpoint lacks complete prefix write evidence"); return;
        }
        if (!snapshot(e, ID4_AFTER, e->id4_address, e->id4_size)) return;
        e->tap_after = word(e->ram + TAP_ADDRESS);
        stop(e, COMPLETE, "Stopped before primary rendezvous store after native close; CPU1 held"); return;
    }
    ++e->instructions;
    if (e->pmu_control.value && (e->pmu_enable.value & 0x80000000u)) e->pmu_cycles.value += 1024;
    if (!condition(e->opcode >> 28, flags)) return;
    if ((e->opcode & 0x0fd0f0f0u) == 0x0710f010u ||
        (e->opcode & 0x0ff00fffu) == 0x01900f9fu ||
        (e->opcode & 0x0ff00ff0u) == 0x01800f90u) {
        stop(e, UNSUPPORTED, "R5-only divide or multiprocessing instruction"); return;
    }
    if ((e->opcode & 0x0f000f10u) == 0x0e000e10u) {
        bool bound = (address == 0x1aa0 && e->opcode == 0xee121e10u) ||
                     (address == 0x41e0 && e->opcode == 0xee120e10u);
        if (!bound) { stop(e, UNSUPPORTED, "Unknown CP14 instruction or discriminator source"); return; }
        uint32_t identity = 0x4003, next = e->pc + 4;
        checked(e, uc_reg_write(uc, registers[(e->opcode >> 12) & 15], &identity));
        checked(e, uc_reg_write(uc, UC_ARM_REG_PC, &next));
        return;
    }
    if ((e->opcode & 0x0f000f10u) == 0x0e000f10u) {
        if (e->phase == ROM_LOAD && address == 0x50128u && e->opcode == 0xee070f15u) {
            e->pc += 4;
            e->bootstrap = true;
            e->icache = true;
            checked(e, uc_emu_stop(uc));
            return;
        }
        if (!e->dram && rom_pmu(e)) return;
        if (!cp15_bound(e->pc, e->opcode)) { stop(e, UNSUPPORTED, "Unknown CP15 instruction or register read"); return; }
        if (address == 0x18c4 || address == 0x1a70 || address == 0x1a74 ||
            address == 0x6080 || address == 0x608c || address == 0x6100 || address == 0x610c) {
            uint32_t next = e->pc + 4;
            checked(e, uc_reg_write(uc, UC_ARM_REG_PC, &next));
        }
        return;
    }
    if ((e->opcode & 0x0c000000u) == 0x0c000000u &&
        (e->opcode & 0x0f000000u) != 0x0f000000u) {
        stop(e, UNSUPPORTED, "Unsupported coprocessor instruction"); return;
    }
    uint32_t canonical = e->pc | 0x80000000u;
    if (e->opcode == 0xeafffffeu &&
        ((!e->dram && canonical == 0x80000024u) ||
         (e->staged && (address == 0x1f18 || address == 0x1f58 || address == 0x1f90 || address == 0x1fd8)))) {
        stop(e, NATIVE_HALT, "Captured error path entered terminal self-branch"); return;
    }
    if (e->opcode == 0xe320f003u &&
        (address == 0x602c || address == 0x60a0 || address == 0x60b8 || address == 0x6120)) {
        uint32_t next = 0;
        if (checked(e, uc_mem_read(uc, address + 4, &next, 4)) && next == 0xeafffffdu)
            stop(e, NATIVE_HALT, "Recovery payload entered WFI and terminal back-branch");
    }
    if (!e->dram && canonical >= 0x80000130u) e->phase = ROM_LOAD;
}
static void interrupt(uc_engine *uc, uint32_t number, void *context) {
    (void)uc;
    Engine *e = context;
    char detail[128];
    snprintf(detail, sizeof detail, "Unsupported CPU exception %" PRIu32, number);
    stop(e, UNSUPPORTED, detail);
}
static bool load_file(const char *directory, const char *name, InputFile *input) {
    char path[4096];
    int length = snprintf(path, sizeof path, "%s/%s", directory, name);
    if (length < 0 || (size_t)length >= sizeof path) return false;
    int descriptor = open(path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC);
    if (descriptor < 0) return false;
    struct stat status;
    if (fstat(descriptor, &status) || !S_ISREG(status.st_mode) ||
        status.st_size <= 0 || status.st_size > 0x10000000) {
        close(descriptor);
        return false;
    }
    input->length = (size_t)status.st_size;
    input->bytes = malloc(input->length);
    if (!input->bytes) { close(descriptor); return false; }
    size_t consumed = 0;
    while (consumed < input->length) {
        ssize_t count = read(descriptor, input->bytes + consumed, input->length - consumed);
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) break;
        consumed += (size_t)count;
    }
    bool okay = consumed == input->length;
    if (close(descriptor) != 0) okay = false;
    return okay;
}
static bool bound(const char *text, uint64_t maximum, uint64_t *result) {
    if (!text[0] || text[0] < '0' || text[0] > '9') return false;
    errno = 0;
    char *end;
    unsigned long long value = strtoull(text, &end, 10);
    if (errno || *end || value == 0 || value > maximum) return false;
    *result = value;
    return true;
}
static void json_string(FILE *file, const char *text) {
    fputc('"', file);
    for (const unsigned char *p = (const unsigned char *)text; *p; ++p) {
        if (*p == '"' || *p == '\\') fprintf(file, "\\%c", *p);
        else if (*p < 0x20) fprintf(file, "\\u%04x", *p);
        else fputc(*p, file);
    }
    fputc('"', file);
}
static void json_observed_word(FILE *file, bool observed, uint32_t value) {
    if (observed) fprintf(file, "%" PRIu32, value);
    else fputs("null", file);
}
static void report_prefix_writes(FILE *file, const Engine *e) {
    static const char *const origins[] = {"cpu_attempt", "media_dma", "sd_dma", "fill", "lz"};
    static const char *const stages[] = {"before_load", "pre_decode", "image_decode", "after_load"};
    const PrefixWrites *audit = &e->prefix_writes;
    fprintf(file, ",\"prefix_writes\":{\"address\":%u,\"bytes\":%u,\"complete\":%s,"
            "\"load_entered\":%s,\"decode_entered\":%s,\"load_returned\":%s,"
            "\"cpu_attempts\":%" PRIu64 ",\"device_commits\":%" PRIu64 ",\"events\":[",
            PREFIX_ADDRESS, PREFIX_END - PREFIX_ADDRESS, audit->incomplete ? "false" : "true",
            audit->load_entered ? "true" : "false", audit->decode_entered ? "true" : "false",
            audit->load_returned ? "true" : "false", audit->cpu_attempts, audit->device_commits);
    for (size_t i = 0; i < audit->count; ++i) {
        const PrefixWrite *event = &audit->events[i];
        fprintf(file, "%s{\"origin\":\"%s\",\"phase\":\"%s\",\"stage\":\"%s\","
                "\"pc\":%" PRIu32 ",\"address\":%" PRIu32 ",\"bytes\":%zu,"
                "\"overlap_address\":%" PRIu32 ",\"overlap_bytes\":%zu,\"overlap_unchanged\":%s}",
                i ? "," : "", origins[event->origin], phase_names[event->phase], stages[event->stage],
                event->pc.value, event->address.value, event->bytes.value, event->overlap_address.value,
                event->overlap_bytes.value, event->overlap_unchanged ? "true" : "false");
    }
    fputs("]}", file);
}
static void report_evidence(FILE *file, const Engine *e) {
    static const char *profiles[] = {"x-m1-primary-v1"};
    fputs(",\"profile\":", file);
    json_string(file, profiles[e->profile]);
    fputs(",\"snapshots\":[", file);
    bool comma = false;
    for (enum SnapshotName name = STAGING; name < SNAPSHOT_COUNT; ++name) {
        const SnapshotExtent *extent = &e->snapshots[name];
        if (!extent->present) continue;
        if (comma) fputc(',', file);
        fprintf(file, "{\"file\":\"%s\",\"address\":%" PRIu32 ",\"bytes\":%zu}",
                snapshot_name(name), extent->address.value, extent->bytes.value);
        comma = true;
    }
    fputs("],\"bss_poison\":", file);
    if (!e->bss_poison.applied) fputs("null", file);
    else fprintf(file, "{\"address\":%" PRIu32 ",\"bytes\":%zu,\"value\":165}",
                 e->bss_poison.address.value, e->bss_poison.bytes.value);
    const CloseEvidence *close = &e->close;
    fprintf(file, ",\"close\":{\"entered\":%s,\"callback_entered\":%s,\"restore_entered\":%s,\"returned\":%s,\"open_before\":",
            close->entered ? "true" : "false", close->callback_entered ? "true" : "false",
            close->restore_entered ? "true" : "false", close->returned ? "true" : "false");
    json_observed_word(file, close->entered, close->open_before.value);
    fputs(",\"open_after\":", file);
    json_observed_word(file, close->returned, close->open_after.value);
    fputs(",\"sp_before\":", file);
    json_observed_word(file, close->entered, close->sp_before.value);
    fputs(",\"sp_after\":", file);
    json_observed_word(file, close->returned, close->sp_after.value);
    fputs(",\"return_r0\":", file);
    json_observed_word(file, close->returned, close->return_r0.value);
    fputc('}', file);
    report_prefix_writes(file, e);
}
static bool report(Engine *e) {
    static const char *statuses[] = {"engine_error", "complete", "unsupported", "native_halt", "budget", "engine_error", "route_mismatch"};
    static const char *faults[] = {"none", "tap", "marker", "media", "route"};
    char path[4096];
    if (!output_path(e, "report.json", path, sizeof path)) return false;
    FILE *file = fopen(path, "wx");
    if (!file) return false;
    if (e->loaded) e->tap_after = word(e->ram + TAP_ADDRESS);
    fprintf(file, "{\"schema_version\":2,\"status\":\"%s\",\"phase\":\"%s\",\"requested_route\":\"%s\",\"selected_rom_mode\":",
            statuses[e->terminal], phase_names[e->phase], e->route == BB_ROUTE_MEDIA ? "media" : "fat-sd");
    json_observed_word(file, e->selected_rom_mode != ROM_MODE_UNSELECTED, (uint32_t)e->selected_rom_mode);
    fprintf(file, ",\"fault\":\"%s\",\"pc\":%" PRIu32 ",\"instructions\":%" PRIu64 ",\"access\":",
            faults[e->fault], e->pc, e->instructions);
    if (!e->access.present) fputs("null", file);
    else fprintf(file, "{\"address\":%" PRIu32 ",\"width\":%" PRIu32 ",\"write\":%s,\"value\":%" PRIu32 ",\"pc\":%" PRIu32 ",\"opcode\":%" PRIu32 ",\"caller_pc\":%" PRIu32 "}",
                 e->access.address, e->access.width, e->access.write ? "true" : "false", e->access.value, e->access.pc, e->access.opcode, e->access.caller);
    fputs(",\"detail\":", file);
    json_string(file, e->detail);
    report_evidence(file, e);
    BbDeviceStats stats = bb_devices_stats(e->devices);
    fprintf(file, ",\"statistics\":{\"media_read_bytes\":%" PRIu64 ",\"sd_read_bytes\":%" PRIu64 ",\"fill_bytes\":%" PRIu64 ",\"decoded_bytes\":%" PRIu64 ",\"transactions\":%" PRIu64 ",\"peer_release_requests\":%" PRIu64 "},\"tap\":{\"before\":%" PRIu32 ",\"after\":%" PRIu32 "},\"assumptions\":[",
            stats.media_read_bytes, stats.sd_read_bytes, stats.fill_bytes, stats.decoded_bytes, stats.transactions, stats.peer_release_requests, e->tap_before, e->tap_after);
    const char *assumptions[] = {
        "X-M1 captured 16KiB ROM repeated over observed blocks; reset low ROM alias is assumed",
        "Unicorn Cortex-R5F is an R4F-class ISA proxy with source-bound CP15 accesses and no R5 identity reads",
        "Source-bound CP14 discriminator at 0x1aa0 and 0x41e0 returns 0x4003 for primary CPU",
        "Captured ROM PMU reset/enable/cycle reads use 1024 virtual cycle ticks per instruction, not wall time",
        "DRAM window ends at 0x03660000 to cover source staging 0x02e40000 plus admitted 8MiB record and 128KiB rounding; aliases share backing",
        "Native TCM setup at 0x18c4 selects modeled 0x70000000 SRAM; cache maintenance is coherent and immediate",
        "SYSTEM+8 native mapping request replaces reset low ROM alias with DRAM",
        "CPU1 release requests are recorded but CPU1 is held; no peer acknowledgment is synthesized",
        "Device completion, clean logical media and SD laws are explicit model assumptions, not hardware recovery proof",
        "Tap fault inserts named unknown word 0xe7f0def0 after loaded image snapshot before rollback",
        "Host fixture poisons the live id4 BSS extent with 0xa5 once at native 0x2250 before source zero-fill; this is not failed-body memory",
        "Only selected-media-bound loaded kernel0 integer helper 0x4d978..0x4da8c executes during native load; its zero-divisor target and kernel startup are excluded",
        bb_devices_assumptions()
    };
    for (size_t i = 0; i < sizeof assumptions / sizeof assumptions[0]; ++i) {
        if (i) fputc(',', file);
        json_string(file, assumptions[i]);
    }
    fputs("]}\n", file);
    bool okay = !ferror(file);
    if (fclose(file) != 0) okay = false;
    return okay;
}
int main(int argc, char **argv) {
    if (argc != 7) {
        fputs("Usage: recovery-bootstrap-engine INPUT_DIRECTORY ROUTE FAULT OUTPUT_DIRECTORY INSTRUCTION_LIMIT WALL_SECONDS\n", stderr);
        return 1;
    }
    Engine e = {.output = argv[4], .terminal = RUNNING, .phase = RESET, .profile = X_M1_PRIMARY_V1, .selected_rom_mode = ROM_MODE_UNSELECTED};
    if (!strcmp(argv[2], "media")) e.route = BB_ROUTE_MEDIA;
    else if (!strcmp(argv[2], "fat-sd")) e.route = BB_ROUTE_FAT_SD;
    else return 1;
    static const char *faults[] = {"none", "tap", "marker", "media", "route"};
    unsigned fault;
    for (fault = 0; fault < 5 && strcmp(argv[3], faults[fault]); ++fault) {}
    if (fault == 5) return 1;
    e.fault = (enum BbFault)fault;
    uint64_t seconds;
    if (!bound(argv[5], 120000000, &e.limit) || !bound(argv[6], 120, &seconds)) return 1;
    e.wall_seconds = (unsigned)seconds;
    InputFile rom = {0}, media = {0}, card = {0};
    if (!load_file(argv[1], "rom.bin", &rom) || rom.length != ROM_SIZE ||
        !load_file(argv[1], "media.bin", &media) || !load_file(argv[1], "card.img", &card)) {
        free(rom.bytes); free(media.bytes); free(card.bytes);
        fputs("Invalid bootstrap input files\n", stderr);
        return 1;
    }
    e.ram = calloc(1, RAM_SIZE); e.rom = malloc(ROM_WINDOW);
    e.sram = calloc(1, 0x1000); e.tcm = calloc(1, 0x2000); e.vectors = calloc(1, 0x10000);
    if (!e.ram || !e.rom || !e.sram || !e.tcm || !e.vectors) {
        free(e.ram); free(e.rom); free(e.sram); free(e.tcm); free(e.vectors);
        free(rom.bytes); free(media.bytes); free(card.bytes);
        return 1;
    }
    for (unsigned i = 0; i < ROM_WINDOW; i += ROM_SIZE) memcpy(e.rom + i, rom.bytes, ROM_SIZE);
    e.media = media;
    BbDeviceInputs inputs = {
        .media = {media.bytes, {media.length}}, .card = {card.bytes, {card.length}},
        .ram = {&e, ram_read, ram_write}, .route = e.route, .fault = e.fault
    };
    e.devices = bb_devices_create(inputs);
    if (!e.devices) { e.file_error = true; stop(&e, ENGINE_ERROR, "Cannot allocate device model"); }
    if (e.devices && checked(&e, uc_open(UC_ARCH_ARM, UC_MODE_ARM | UC_MODE_LITTLE_ENDIAN, &e.uc)) &&
        checked(&e, uc_ctl_set_cpu_model(e.uc, UC_CPU_ARM_CORTEX_R5F))) {
        checked(&e, uc_mem_map_ptr(e.uc, 0, ROM_WINDOW, UC_PROT_READ | UC_PROT_EXEC, e.rom));
        checked(&e, uc_mem_map_ptr(e.uc, ROM_WINDOW, RAM_SIZE - ROM_WINDOW, UC_PROT_ALL, e.ram + ROM_WINDOW));
        checked(&e, uc_mem_map_ptr(e.uc, 0x40000000, RAM_SIZE, UC_PROT_ALL, e.ram));
        checked(&e, uc_mem_map_ptr(e.uc, 0x80000000, ROM_WINDOW, UC_PROT_READ | UC_PROT_EXEC, e.rom));
        checked(&e, uc_mem_map_ptr(e.uc, 0xfff00000, 0x1000, UC_PROT_ALL, e.sram));
        checked(&e, uc_mem_map_ptr(e.uc, 0x70000000, 0x2000, UC_PROT_ALL, e.tcm));
        checked(&e, uc_mem_map_ptr(e.uc, 0x3fff0000, 0x10000, UC_PROT_ALL, e.vectors));
        checked(&e, uc_mem_map_ptr(e.uc, 0x7fff0000, 0x10000, UC_PROT_ALL, e.vectors));
        checked(&e, uc_mmio_map(e.uc, 0xfff01000, 0xff000, mmio_read, &e, mmio_write, &e));
        uc_hook code_hook, memory_hook, interrupt_hook, write_hook;
        checked(&e, uc_hook_add(e.uc, &code_hook, UC_HOOK_CODE, (void *)instruction, &e, 1, 0));
        checked(&e, uc_hook_add(e.uc, &memory_hook, UC_HOOK_MEM_INVALID, (void *)invalid_memory, &e, 1, 0));
        checked(&e, uc_hook_add(e.uc, &write_hook, UC_HOOK_MEM_WRITE, (void *)cpu_write, &e, 1, 0));
        checked(&e, uc_hook_add(e.uc, &interrupt_hook, UC_HOOK_INTR, (void *)interrupt, &e, 1, 0));
        uint32_t flags = 0xd3;
        checked(&e, uc_reg_write(e.uc, UC_ARM_REG_CPSR, &flags));
        if (clock_gettime(CLOCK_MONOTONIC, &e.started) != 0) stop(&e, ENGINE_ERROR, "Monotonic clock unavailable");
        while (e.terminal == RUNNING) {
            uc_err execution = uc_emu_start(e.uc, e.pc, 0xffffffffu, 0, 0);
            if (e.terminal != RUNNING) break;
            if (e.remap) {
                e.remap = false;
                if (!checked(&e, uc_mem_unmap(e.uc, 0, ROM_WINDOW)) ||
                    !checked(&e, uc_mem_map_ptr(e.uc, 0, ROM_WINDOW, UC_PROT_ALL, e.ram))) break;
                e.dram = true;
                continue;
            }
            if (e.icache) {
                e.icache = false;
                if (!checked(&e, uc_ctl_flush_tb(e.uc))) break;
                continue;
            }
            if (execution != UC_ERR_OK) {
                stop(&e, execution == UC_ERR_INSN_INVALID || execution == UC_ERR_EXCEPTION ? UNSUPPORTED : ENGINE_ERROR,
                     uc_strerror(execution));
            } else stop(&e, ENGINE_ERROR, "Engine returned without a declared terminal");
        }
    }
    bool reported = e.devices && report(&e);
    int result = !reported || e.file_error ? 1 : e.terminal == COMPLETE ? 0 : 2;
    if (e.uc) uc_close(e.uc);
    if (e.devices) bb_devices_destroy(e.devices);
    free(e.ram); free(e.rom); free(e.sram); free(e.tcm); free(e.vectors);
    free(rom.bytes); free(media.bytes); free(card.bytes);
    return result;
}
