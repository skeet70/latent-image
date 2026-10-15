#define _POSIX_C_SOURCE 200809L
#include <unicorn/unicorn.h>
#include <unicorn/arm.h>
#include <inttypes.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <sys/mman.h>
#include <unistd.h>

#define RAM_BYTES 0x02000000u
#define STACK_BASE 0xfff00000u
#define STACK_TOP 0xfff00f00u
#define FAT_OPEN 0x80001dc4u
#define FAT_READ 0x80001e88u
#define RAW_READ 0x8000148cu

typedef struct { uint32_t value; } Address;
typedef struct { uint32_t value; } Sector;
typedef struct { const uint8_t *bytes; size_t length; } Input;
enum Route { FAT, RAW };
enum Fault { NONE, READ_ERROR, SHORT_READ, BAD_STACK, BAD_RETURN, CACHE_ENABLED, CACHE_AFTER_READ };
enum Terminal { RUNNING, HANDOFF, REFUSAL, BUDGET, BRIDGE_ERROR };
enum MpuSize { MPU_4G = 0x3f, MPU_1G = 0x3b, MPU_16K = 0x1b, MPU_4K = 0x17 };
enum MpuAccess { MPU_DEVICE_XN = 0x1304, MPU_RAM_EXEC = 0x30c, MPU_RAM_XN = 0x130c, MPU_ROM_EXEC = 0x308 };
typedef struct { Address base; enum MpuSize size; enum MpuAccess access; } MpuRegion;
typedef struct {
    uc_engine *uc;
    uint8_t *ram, *stack;
    Input storage;
    enum Route route;
    enum Fault fault;
    enum Terminal terminal;
    size_t position;
    uint64_t reads;
    unsigned opens, led_writes;
    uint32_t led;
    uint32_t boot_writes[3];
    bool boot_seen[3];
    bool opened;
    bool state_error;
    const char *detail;
} Engine;

static void stop(Engine *e, enum Terminal terminal, const char *detail) {
    if (e->terminal != RUNNING) return;
    e->terminal = terminal;
    e->detail = detail;
    uc_emu_stop(e->uc);
}
static bool checked(Engine *e, uc_err error) {
    if (error == UC_ERR_OK) return true;
    e->state_error = true;
    stop(e, BRIDGE_ERROR, uc_strerror(error));
    return false;
}
static uint32_t get(Engine *e, int reg) {
    uint32_t value = 0;
    checked(e, uc_reg_read(e->uc, reg, &value));
    return value;
}
static void put(Engine *e, int reg, uint32_t value) {
    checked(e, uc_reg_write(e->uc, reg, &value));
}
static uint8_t *memory(Engine *e, Address address, size_t length) {
    uint64_t end = (uint64_t)address.value + length;
    if (end <= RAM_BYTES) return e->ram + address.value;
    if (address.value >= 0x40000000u && end <= 0x42000000ull)
        return e->ram + address.value - 0x40000000u;
    if (address.value >= STACK_BASE && end <= (uint64_t)STACK_BASE + 4096)
        return e->stack + address.value - STACK_BASE;
    return NULL;
}
static void returned(Engine *e, uint32_t result) {
    put(e, UC_ARM_REG_R0, result);
    put(e, UC_ARM_REG_PC, get(e, UC_ARM_REG_LR));
}
static void read_storage(Engine *e, Address destination, size_t offset, Address count_out) {
    ++e->reads;
    uint8_t *target = memory(e, destination, 512);
    uint8_t *count = e->route == FAT ? memory(e, count_out, 2) : NULL;
    if (!target || (e->route == FAT && !count)) {
        stop(e, BRIDGE_ERROR, "ROM buffer outside mapped memory");
        return;
    }
    size_t available = offset <= e->storage.length ? e->storage.length - offset : 0;
    size_t length = available < 512 ? available : 512;
    if (e->fault == READ_ERROR) {
        if (count) count[0] = count[1] = 0;
        returned(e, 1);
        return;
    }
    if (e->fault == SHORT_READ && length == 512) length = 511;
    if (length) memcpy(target, e->storage.bytes + offset, length);
    if (count) {
        count[0] = (uint8_t)length;
        count[1] = (uint8_t)(length >> 8);
        e->position += length;
    }
    if (e->fault == CACHE_AFTER_READ) {
        uc_arm_cp_reg control = {.cp = 15, .crn = 1};
        checked(e, uc_reg_read(e->uc, UC_ARM_REG_CP_REG, &control));
        control.val |= UINT64_C(4);
        checked(e, uc_reg_write(e->uc, UC_ARM_REG_CP_REG, &control));
    }
    returned(e, e->route == RAW && length != 512 ? 1 : 0);
}
static void rom_boundary(uc_engine *uc, uint64_t address, uint32_t size, void *context) {
    (void)uc;
    (void)size;
    Engine *e = context;
    if (address == FAT_OPEN && e->route == FAT) {
        uint8_t *name = memory(e, (Address){get(e, UC_ARM_REG_R0)}, 12);
        if (!name || memcmp(name, "ffrecov.bin", 11) || name[11] != 0) {
            returned(e, 1);
            return;
        }
        ++e->opens;
        e->opened = true;
        e->position = 0;
        returned(e, 0);
    } else if (address == FAT_READ && e->route == FAT && e->opened) {
        if (get(e, UC_ARM_REG_R1) != 512) {
            stop(e, BRIDGE_ERROR, "ROM FAT read is not one sector");
            return;
        }
        read_storage(e, (Address){get(e, UC_ARM_REG_R0)}, e->position,
                  (Address){get(e, UC_ARM_REG_R2)});
    } else if (address == RAW_READ && e->route == RAW) {
        Sector source = {get(e, UC_ARM_REG_R2)};
        if (get(e, UC_ARM_REG_R0) != 1) {
            returned(e, 1);
            return;
        }
        uint64_t offset = (uint64_t)source.value * 512;
        if (offset > SIZE_MAX) { returned(e, 1); return; }
        read_storage(e, (Address){get(e, UC_ARM_REG_R1)}, (size_t)offset, (Address){0});
    } else {
        stop(e, BRIDGE_ERROR, "Uncaptured ROM boundary");
    }
}
static void handoff(uc_engine *uc, uint64_t address, uint32_t size, void *context) {
    (void)uc;
    (void)address;
    (void)size;
    stop(context, HANDOFF, "Native entry reached; continuation not executed");
}
static void wait_boundary(uc_engine *uc, uint64_t address, uint32_t size, void *context) {
    (void)uc;
    (void)address;
    (void)size;
    stop(context, REFUSAL, "Target entered bounded failure wait");
}
static uint64_t device_read(uc_engine *uc, uint64_t offset, unsigned size, void *context) {
    (void)uc;
    (void)offset;
    (void)size;
    stop(context, BRIDGE_ERROR, "Uncaptured device read");
    return 0;
}
static void device_write(uc_engine *uc, uint64_t offset, unsigned size, uint64_t value, void *context) {
    (void)uc;
    Engine *e = context;
    if (size != 4 || !((offset == 0x8084 && value == 1) ||
        (offset == 0x93c4 && value == 0x50001) ||
        (offset == 0x93c0 && (value == 0x10000 || value == 0x10001)))) {
        stop(e, BRIDGE_ERROR, "Uncaptured device write");
        return;
    }
    if (offset == 0x93c0) { e->led = (uint32_t)value; ++e->led_writes; }
    unsigned slot = offset == 0x8084 ? 0 : offset == 0x93c4 ? 1 : 2;
    e->boot_writes[slot] = (uint32_t)value;
    e->boot_seen[slot] = true;
}
static bool load(const char *path, size_t maximum, Input *input) {
    int fd = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK);
    if (fd < 0) return false;
    struct stat status;
    if (fstat(fd, &status) || !S_ISREG(status.st_mode) || status.st_size < 0 ||
        (uint64_t)status.st_size > maximum) {
        close(fd);
        return false;
    }
    size_t length = (size_t)status.st_size;
    if (length == 0) { close(fd); return true; }
    void *bytes = mmap(NULL, length, PROT_READ, MAP_PRIVATE, fd, 0);
    close(fd);
    if (bytes == MAP_FAILED) return false;
    input->bytes = bytes;
    input->length = length;
    return true;
}
static void unload(Input *input) {
    if (input->length) munmap((void *)input->bytes, input->length);
    *input = (Input){0};
}
static bool save(const char *path, const uint8_t *bytes, size_t length) {
    int fd = open(path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW, 0600);
    if (fd < 0) return false;
    FILE *file = fdopen(fd, "wb");
    if (!file) { close(fd); return false; }
    bool ok = fwrite(bytes, 1, length, file) == length;
    if (fclose(file)) ok = false;
    return ok;
}

static uint64_t cp_value(Engine *e, unsigned crn, unsigned crm, unsigned opc2) {
    uc_arm_cp_reg reg = {.cp = 15, .crn = crn, .crm = crm, .opc2 = opc2};
    checked(e, uc_reg_read(e->uc, UC_ARM_REG_CP_REG, &reg));
    return reg.val;
}

static uint32_t ram_crc32(const uint8_t *bytes, size_t length) {
    uint32_t table[256];
    for (unsigned i = 0; i < 256; ++i) {
        uint32_t value = i;
        for (unsigned bit = 0; bit < 8; ++bit)
            value = (value >> 1) ^ (0xedb88320u & (0u - (value & 1u)));
        table[i] = value;
    }
    uint32_t crc = UINT32_MAX;
    for (size_t i = 0; i < length; ++i)
        crc = table[(crc ^ bytes[i]) & 255u] ^ (crc >> 8);
    return ~crc;
}

static bool save_state(Engine *e, const char *ram_path) {
    size_t length = strlen(ram_path) + sizeof ".state.json";
    char *path = malloc(length);
    if (!path) return false;
    snprintf(path, length, "%s.state.json", ram_path);
    int fd = open(path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW, 0600);
    free(path);
    if (fd < 0) return false;
    FILE *file = fdopen(fd, "wb");
    if (!file) { close(fd); return false; }
    uint32_t cpsr = get(e, UC_ARM_REG_CPSR);
    static const int registers[] = {UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2,
        UC_ARM_REG_R3, UC_ARM_REG_R4, UC_ARM_REG_R5, UC_ARM_REG_R6, UC_ARM_REG_R7,
        UC_ARM_REG_R8, UC_ARM_REG_R9, UC_ARM_REG_R10, UC_ARM_REG_R11, UC_ARM_REG_R12,
        UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_CPSR};
    fputs("{\"schema\":1,\"kind\":\"ram-rescue-terminal-cpu0\",\"status\":", file);
    fprintf(file, "\"%s\",\"route\":\"%s\",\"ram_bytes\":%u,\"ram_crc32\":%u,",
            e->terminal == HANDOFF ? "handoff" : "stopped",
            e->route == FAT ? "fat" : "raw", RAM_BYTES, ram_crc32(e->ram, RAM_BYTES));
    fputs("\"registers\":[", file);
    for (unsigned i = 0; i < sizeof registers / sizeof *registers; ++i)
        fprintf(file, "%s%u", i ? "," : "", get(e, registers[i]));
    fputs("],\"banked\":[", file);
    static const unsigned modes[] = {0x10, 0x11, 0x12, 0x13, 0x17, 0x1b};
    for (unsigned i = 0; i < sizeof modes / sizeof *modes; ++i) {
        put(e, UC_ARM_REG_CPSR, (cpsr & ~0x1fu) | modes[i]);
        fprintf(file, "%s{\"mode\":%u,\"sp\":%u,\"lr\":%u,\"spsr\":%u,\"r8_r12\":[",
                i ? "," : "", modes[i], get(e, UC_ARM_REG_SP), get(e, UC_ARM_REG_LR),
                modes[i] == 0x10 ? 0 : get(e, UC_ARM_REG_SPSR));
        for (unsigned n = 8; n <= 12; ++n)
            fprintf(file, "%s%u", n == 8 ? "" : ",", get(e, registers[n]));
        fputs("]}", file);
    }
    put(e, UC_ARM_REG_CPSR, cpsr);
    fprintf(file, "],\"vfp\":{\"fpexc\":%u,\"fpscr\":%u,\"d0_d15\":[",
            get(e, UC_ARM_REG_FPEXC), get(e, UC_ARM_REG_FPSCR));
    for (unsigned i = 0; i < 16; ++i) {
        uint64_t value = 0;
        checked(e, uc_reg_read(e->uc, UC_ARM_REG_D0 + (int)i, &value));
        fprintf(file, "%s%" PRIu64, i ? "," : "", value);
    }
    fputs("]},\"cp15\":[", file);
    static const unsigned controls[][3] = {{1,0,0}, {1,0,1}, {1,0,2},
        {3,0,0}, {9,1,0}, {9,1,1}};
    for (unsigned i = 0; i < sizeof controls / sizeof *controls; ++i)
        fprintf(file, "%s[%u,%u,%u,%" PRIu64 "]", i ? "," : "",
                controls[i][0], controls[i][1], controls[i][2],
                cp_value(e, controls[i][0], controls[i][1], controls[i][2]));
    uint64_t selector = cp_value(e, 6, 2, 0);
    fputs("],\"mpu\":[", file);
    for (unsigned i = 0; i < 8; ++i) {
        uc_arm_cp_reg select = {.cp = 15, .crn = 6, .crm = 2, .val = i};
        checked(e, uc_reg_write(e->uc, UC_ARM_REG_CP_REG, &select));
        fprintf(file, "%s[%" PRIu64 ",%" PRIu64 ",%" PRIu64 "]",
                i ? "," : "", cp_value(e, 6, 1, 0), cp_value(e, 6, 1, 2), cp_value(e, 6, 1, 4));
    }
    uc_arm_cp_reg select = {.cp = 15, .crn = 6, .crm = 2, .val = selector};
    checked(e, uc_reg_write(e->uc, UC_ARM_REG_CP_REG, &select));
    fprintf(file, "],\"mpu_selector\":%" PRIu64 ",\"sram_hex\":\"", selector);
    for (unsigned i = 0; i < 4096; ++i) fprintf(file, "%02x", e->stack[i]);
    fputs("\",\"device_writes\":[", file);
    static const unsigned addresses[] = {0xfff88084u, 0xfff893c4u, 0xfff893c0u};
    bool emitted = false;
    for (unsigned i = 0; i < 3; ++i) {
        if (!e->boot_seen[i]) continue;
        fprintf(file, "%s[%u,%u]", emitted ? "," : "", addresses[i], e->boot_writes[i]);
        emitted = true;
    }
    fputs("],\"provenance\":\"Observed CPU0 at the engine stop; ROM calls are file-backed boundary substitutions; exported device writes are the last observed bootstrap transactions only; CPU1 and ROM peripheral state are not exported\"}\n", file);
    bool ok = !ferror(file) && !e->state_error && e->terminal != BRIDGE_ERROR;
    if (fclose(file)) ok = false;
    return ok;
}
static bool fault(const char *text, enum Fault *out) {
    static const char *names[] = {"none", "read-error", "short-read", "bad-stack", "bad-return", "cache-enabled", "cache-after-read"};
    for (unsigned i = 0; i < sizeof names / sizeof *names; ++i)
        if (!strcmp(text, names[i])) { *out = (enum Fault)i; return true; }
    return false;
}
static void rom_mpu(Engine *e) {
    // Captured ROM0x28..0xe0 installs these regions before SD entry.
    static const MpuRegion regions[] = {
        {{0}, MPU_4G, MPU_DEVICE_XN},
        {{0}, MPU_1G, MPU_RAM_EXEC},
        {{0x40000000u}, MPU_1G, MPU_RAM_XN},
        {{0x80000000u}, MPU_16K, MPU_ROM_EXEC},
        {{STACK_BASE}, MPU_4K, MPU_RAM_EXEC},
    };
    for (unsigned i = 0; i < sizeof regions / sizeof *regions; ++i) {
        uc_arm_cp_reg selector = {.cp = 15, .crn = 6, .crm = 2, .val = i};
        uc_arm_cp_reg base = {.cp = 15, .crn = 6, .crm = 1, .val = regions[i].base.value};
        uc_arm_cp_reg size = {.cp = 15, .crn = 6, .crm = 1, .opc2 = 2, .val = regions[i].size};
        uc_arm_cp_reg access = {.cp = 15, .crn = 6, .crm = 1, .opc2 = 4, .val = regions[i].access};
        checked(e, uc_reg_write(e->uc, UC_ARM_REG_CP_REG, &selector));
        checked(e, uc_reg_write(e->uc, UC_ARM_REG_CP_REG, &base));
        checked(e, uc_reg_write(e->uc, UC_ARM_REG_CP_REG, &size));
        checked(e, uc_reg_write(e->uc, UC_ARM_REG_CP_REG, &access));
    }
}
int main(int argc, char **argv) {
    Engine e = {.terminal = RUNNING, .detail = ""};
    Input entry = {0};
    if (argc != 6 || !fault(argv[5], &e.fault) ||
        (strcmp(argv[3], "fat") && strcmp(argv[3], "raw"))) {
        fprintf(stderr, "Usage: %s ENTRY STORAGE fat|raw NEW_OUT_RAM none|read-error|short-read|bad-stack|bad-return|cache-enabled|cache-after-read\nFAT STORAGE is FFRECOV.BIN; raw STORAGE is the complete SD image. Inputs must remain unchanged during execution.\n", argv[0]);
        return 2;
    }
    e.route = !strcmp(argv[3], "fat") ? FAT : RAW;
    if (!load(argv[1], 4096, &entry) || entry.length != 4096 ||
        !load(argv[2], e.route == RAW ? 128*1024*1024u : RAM_BYTES, &e.storage)) {
        fprintf(stderr, "Cannot map bounded regular entry/storage files\n");
        unload(&entry); unload(&e.storage); return 2;
    }
    if (posix_memalign((void **)&e.ram, 4096, RAM_BYTES) ||
        posix_memalign((void **)&e.stack, 4096, 4096)) {
        unload(&entry); unload(&e.storage); free(e.ram); free(e.stack); return 2;
    }
    memset(e.ram, 0xa5, RAM_BYTES);
    memset(e.stack, 0, 4096);
    memcpy(e.ram, entry.bytes, entry.length);
    uint32_t witness = e.route == FAT ? 0x8000015cu : 0x80000174u;
    if (e.fault == BAD_RETURN) witness = 0x80000160u;
    memcpy(e.stack + STACK_TOP - STACK_BASE - 4, &witness, 4);
    uc_err error = uc_open(UC_ARCH_ARM, UC_MODE_ARM | UC_MODE_LITTLE_ENDIAN, &e.uc);
    if (error != UC_ERR_OK) {
        fprintf(stderr, "%s\n", uc_strerror(error));
        unload(&entry); unload(&e.storage); free(e.ram); free(e.stack); return 2;
    }
    checked(&e, uc_ctl_set_cpu_model(e.uc, UC_CPU_ARM_CORTEX_R5F));
    rom_mpu(&e);
    uc_arm_cp_reg control = {.cp = 15, .crn = 1};
    checked(&e, uc_reg_read(e.uc, UC_ARM_REG_CP_REG, &control));
    control.val = ((control.val | UINT64_C(1)) & ~UINT64_C(4)) | (e.fault == CACHE_ENABLED ? 4 : 0);
    checked(&e, uc_reg_write(e.uc, UC_ARM_REG_CP_REG, &control));
    checked(&e, uc_mem_map_ptr(e.uc, 0, RAM_BYTES, UC_PROT_ALL, e.ram));
    checked(&e, uc_mem_map_ptr(e.uc, 0x40000000u, RAM_BYTES, UC_PROT_ALL, e.ram));
    checked(&e, uc_mem_map_ptr(e.uc, STACK_BASE, 4096, UC_PROT_READ | UC_PROT_WRITE, e.stack));
    checked(&e, uc_mem_map(e.uc, 0x80000000u, 0x4000, UC_PROT_READ | UC_PROT_EXEC));
    static const uint32_t rom_return = 0xe12fff1eu;
    static const uint64_t rom_addresses[] = {FAT_OPEN, FAT_READ, RAW_READ};
    for (unsigned i = 0; i < sizeof rom_addresses / sizeof *rom_addresses; ++i)
        checked(&e, uc_mem_write(e.uc, rom_addresses[i], &rom_return, sizeof rom_return));
    checked(&e, uc_mmio_map(e.uc, 0xfff80000u, 0x10000, device_read, &e, device_write, &e));
    uc_hook rom_hook, entry_hook, wait_hook;
    checked(&e, uc_hook_add(e.uc, &rom_hook, UC_HOOK_CODE, (void *)rom_boundary, &e, 0x80000000u, 0x80003fffu));
    checked(&e, uc_hook_add(e.uc, &entry_hook, UC_HOOK_CODE, (void *)handoff, &e, 0x1800, 0x1800));
    for (size_t offset = 0; offset < 0x800; offset += 4) {
        uint32_t instruction;
        memcpy(&instruction, entry.bytes + offset, 4);
        if (instruction != 0xe320f003u) continue;
        static const uint32_t aliases[] = {0, 0x40000000u, 0xc0000u, 0x400c0000u};
        for (unsigned i = 0; i < sizeof aliases / sizeof *aliases; ++i) {
            uint64_t address = aliases[i] + offset;
            checked(&e, uc_hook_add(e.uc, &wait_hook, UC_HOOK_CODE,
                    (void *)wait_boundary, &e, address, address));
        }
    }
    put(&e, UC_ARM_REG_CPSR, 0x600001d3u);
    static const int regs[] = {UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3, UC_ARM_REG_R4,
        UC_ARM_REG_R5, UC_ARM_REG_R6, UC_ARM_REG_R7, UC_ARM_REG_R8, UC_ARM_REG_R9,
        UC_ARM_REG_R10, UC_ARM_REG_R11, UC_ARM_REG_R12};
    for (unsigned i = 0; i < sizeof regs / sizeof *regs; ++i) put(&e, regs[i], 0x11000001u + i);
    put(&e, UC_ARM_REG_LR, witness);
    put(&e, UC_ARM_REG_SP, STACK_TOP - (e.fault == BAD_STACK ? 8 : 0));
    if (e.terminal == RUNNING) {
        error = uc_emu_start(e.uc, 0, UINT64_MAX, 120000000, 800000000);
        if (error != UC_ERR_OK) checked(&e, error);
        if (e.terminal == RUNNING) stop(&e, BUDGET, "Finite instruction or wall budget exhausted");
    }
    if (!save(argv[4], e.ram, RAM_BYTES)) {
        e.terminal = BRIDGE_ERROR;
        e.detail = "Cannot write complete RAM output";
    }
    if (!save_state(&e, argv[4])) {
        e.terminal = BRIDGE_ERROR;
        e.detail = "Cannot write observed CPU0 state companion";
    }
    static const char *status[] = {"running", "handoff", "refusal", "budget", "bridge-error"};
    printf("status=%s\ndetail=%s\nreads=%" PRIu64 "\nopens=%u\nled_writes=%u\nled=0x%08" PRIx32 "\n",
           status[e.terminal], e.detail, e.reads, e.opens, e.led_writes, e.led);
    printf("r0=0x%08" PRIx32 "\n", get(&e, UC_ARM_REG_R0));
    for (unsigned i = 0; i < sizeof regs / sizeof *regs; ++i)
        printf("r%u=0x%08" PRIx32 "\n", i + 1, get(&e, regs[i]));
    printf("sp=0x%08" PRIx32 "\nlr=0x%08" PRIx32 "\npc=0x%08" PRIx32 "\ncpsr=0x%08" PRIx32 "\n",
           get(&e, UC_ARM_REG_SP), get(&e, UC_ARM_REG_LR), get(&e, UC_ARM_REG_PC), get(&e, UC_ARM_REG_CPSR));
    memcpy(&witness, e.stack + STACK_TOP - STACK_BASE - 4, 4);
    printf("stack_witness=0x%08" PRIx32 "\n", witness);
    puts("model=captured ROM MPU regions; shared coherent RAM aliases; ROM reads copy real file bytes; CP15 cache operations and barriers have no hardware timing; LED writes only; no NAND, CPU1 or native continuation model");
    int result = e.terminal == HANDOFF ? 0 : e.terminal == BRIDGE_ERROR ? 2 : 1;
    uc_close(e.uc);
    unload(&entry); unload(&e.storage); free(e.ram); free(e.stack);
    return result;
}
