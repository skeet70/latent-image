#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unicorn/arm.h>
#include <unicorn/unicorn.h>

#define BOOTSTRAP_BYTES 512u
#define ROM_STACK 0xfff00f00u
#ifndef STAGE_BYTES
#define STAGE_BYTES 0x40000u
#endif
#ifndef RELOC
#define RELOC 0x00050000u
#endif
#ifndef FAT_READS
#define FAT_READS 8u
#endif
#ifndef RAW_READS
#define RAW_READS 512u
#endif
#ifndef LOAD_PREFIX
#define LOAD_PREFIX STAGE_BYTES
#endif
#ifndef LOAD_SKIP
#define LOAD_SKIP 0u
#endif
#define FAT_OPEN 0x80001dc4u
#define FAT_READ 0x80001e88u
#define RAW_READ 0x8000148cu
#define LED_CONTROL 0xfff88084u
#define LED_DATA 0xfff893c0u
#define LED_SETUP 0xfff893c4u

typedef enum { ROUTE_FAT, ROUTE_RAW, ROUTE_REJECT, ROUTE_FAT_FAILURE, ROUTE_FAT_SHORT } Route;

typedef struct {
    const uint8_t *bootstrap;
    const uint8_t *stage;
    uint32_t call_pc[3];
    Route route;
    unsigned entry_visits;
    unsigned fat_opens;
    unsigned fat_reads;
    unsigned raw_reads;
    unsigned led_control_writes;
    unsigned led_setup_writes;
    unsigned led_data_writes;
    int relocated;
    int complete;
    int failed;
} Run;

static void checked(uc_err error) {
    if (error != UC_ERR_OK) {
        fprintf(stderr, "Unicorn: %s\n", uc_strerror(error));
        exit(EXIT_FAILURE);
    }
}

static uint32_t reg(uc_engine *uc, int id) {
    uint32_t value;
    checked(uc_reg_read(uc, id, &value));
    return value;
}

static void set_reg(uc_engine *uc, int id, uint32_t value) {
    checked(uc_reg_write(uc, id, &value));
}


static void code(uc_engine *uc, uint64_t pc, uint32_t size, void *opaque) {
    (void)size;
    Run *run = opaque;
    if (pc == 0) {
        run->entry_visits++;
        if (run->entry_visits == 2) {
            run->complete = 1;
            checked(uc_emu_stop(uc));
        }
        return;
    }
    if (pc >= RELOC && pc < RELOC + BOOTSTRAP_BYTES && !run->relocated) {
        uint8_t relocated[BOOTSTRAP_BYTES];
        checked(uc_mem_read(uc, RELOC, relocated, sizeof relocated));
        if (memcmp(relocated, run->bootstrap, sizeof relocated) != 0) run->failed = 1;
        uint32_t nop = 0xe1a00000u;
        for (unsigned i = 0; i < 3; i++) checked(uc_mem_write(uc, run->call_pc[i], &nop, 4));
        run->relocated = 1;
    }
    if (pc == run->call_pc[1]) {
        char name[13] = {0};
        checked(uc_mem_read(uc, reg(uc, UC_ARM_REG_R0), name, 12));
        if (strcmp(name, "ffrecov.bin") != 0) run->failed = 1;
        run->fat_opens++;
        set_reg(uc, UC_ARM_REG_R0, 0);
        return;
    }
    if (pc == run->call_pc[2]) {
        uint32_t destination = reg(uc, UC_ARM_REG_R0);
        uint32_t count = reg(uc, UC_ARM_REG_R1);
        uint32_t transferred = reg(uc, UC_ARM_REG_R2);
        if (run->route == ROUTE_FAT_FAILURE) {
            set_reg(uc, UC_ARM_REG_R0, 1);
            return;
        }
        uint32_t offset = run->fat_reads * 0x8000u;
        uint32_t wanted_destination = 0x40000000u + offset + (offset >= LOAD_PREFIX ? LOAD_SKIP : 0u);
        if (count != 0x8000u || run->fat_reads >= FAT_READS ||
            destination != wanted_destination) run->failed = 1;
        uint16_t actual = run->route == ROUTE_FAT_SHORT ? 0x4000u : (uint16_t)count;
        checked(uc_mem_write(uc, destination, run->stage + run->fat_reads * 0x8000u, actual));
        checked(uc_mem_write(uc, transferred, &actual, sizeof actual));
        run->fat_reads++;
        set_reg(uc, UC_ARM_REG_R0, 0);
        return;
    }
    if (pc == run->call_pc[0]) {
        uint32_t channel = reg(uc, UC_ARM_REG_R0);
        uint32_t destination = reg(uc, UC_ARM_REG_R1);
        uint32_t lba = reg(uc, UC_ARM_REG_R2);
        uint32_t offset = run->raw_reads * 512u;
        uint32_t wanted_destination = 0x40000000u + offset + (offset >= LOAD_PREFIX ? LOAD_SKIP : 0u);
        if (channel != 1 || run->raw_reads >= RAW_READS || lba != 3 + run->raw_reads ||
            destination != wanted_destination) run->failed = 1;
        checked(uc_mem_write(uc, destination, run->stage + run->raw_reads * 512u, 512));
        run->raw_reads++;
        set_reg(uc, UC_ARM_REG_R0, 0);
        return;
    }
    uint32_t word;
    checked(uc_mem_read(uc, pc, &word, sizeof word));
    if (word == 0xe2500001u) set_reg(uc, UC_ARM_REG_R0, 1);
}

static void writes(uc_engine *uc, uc_mem_type type, uint64_t address, int size,
                   int64_t value, void *opaque) {
    (void)type;
    Run *run = opaque;
    if (address == LED_CONTROL && size == 4 && (uint32_t)value == 1) {
        run->led_control_writes++;
    } else if (address == LED_SETUP && size == 4 && (uint32_t)value == 0x00050001u) {
        run->led_setup_writes++;
    } else if (address == LED_DATA && size == 4 &&
               ((uint32_t)value == 0x00010001u || (uint32_t)value == 0x00010000u)) {
        run->led_data_writes++;
        if (run->route != ROUTE_FAT && run->route != ROUTE_RAW &&
            run->led_data_writes == 4) checked(uc_emu_stop(uc));
    }
}

static void exercise(const uint8_t *bootstrap, const uint8_t *stage, Route route) {
    uc_engine *uc;
    uc_hook code_hook, write_hook;
    Run run = {.bootstrap = bootstrap, .stage = stage, .route = route};
    for (unsigned offset = 0; offset < BOOTSTRAP_BYTES; offset += 4) {
        uint32_t word;
        memcpy(&word, bootstrap + offset, 4);
        if (word == 0xe12fff33u) {
            unsigned slot = run.call_pc[0] != 0u
                ? (run.call_pc[1] != 0u ? 2u : 1u)
                : 0u;
            if (slot >= 3) exit(EXIT_FAILURE);
            run.call_pc[slot] = RELOC + offset;
        }
    }
    if (run.call_pc[0] == 0u || run.call_pc[1] == 0u || run.call_pc[2] == 0u)
        exit(EXIT_FAILURE);
    checked(uc_open(UC_ARCH_ARM, UC_MODE_ARM, &uc));
    checked(uc_ctl_set_cpu_model(uc, UC_CPU_ARM_CORTEX_R5F));
    checked(uc_mem_map(uc, 0, 0x100000, UC_PROT_ALL));
    checked(uc_mem_map(uc, 0x40000000u, STAGE_BYTES + LOAD_SKIP, UC_PROT_ALL));
    checked(uc_mem_map(uc, 0xfff00000u, 0x100000, UC_PROT_ALL));
    checked(uc_mem_write(uc, 0, bootstrap, BOOTSTRAP_BYTES));
    uint32_t context = 0xfff00404u;
    uint32_t stage_size = STAGE_BYTES;
    checked(uc_mem_write(uc, 0xfff00000u, &context, 4));
    checked(uc_mem_write(uc, context + 0x20, &stage_size, 4));
    uint32_t caller = route == ROUTE_RAW ? 0x174u : 0x15cu;
    uint32_t stack = route == ROUTE_REJECT ? ROM_STACK - 4 : ROM_STACK;
    checked(uc_mem_write(uc, ROM_STACK - 4, &caller, 4));
    checked(uc_reg_write(uc, UC_ARM_REG_SP, &stack));
    const int preserved[] = {UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3,
        UC_ARM_REG_R4, UC_ARM_REG_R5, UC_ARM_REG_R6, UC_ARM_REG_R7,
        UC_ARM_REG_R8, UC_ARM_REG_R9, UC_ARM_REG_R10, UC_ARM_REG_R11,
        UC_ARM_REG_R12, UC_ARM_REG_LR};
    for (unsigned i = 0; i < sizeof preserved / sizeof preserved[0]; i++)
        set_reg(uc, preserved[i], 0x24680000u + i);
    checked(uc_hook_add(uc, &code_hook, UC_HOOK_CODE, (void *)code, &run, 1, 0));
    checked(uc_hook_add(uc, &write_hook, UC_HOOK_MEM_WRITE, (void *)writes, &run, 1, 0));
    uc_err emulation = uc_emu_start(uc, 0, 0x100000, 1000000, 200000);
    if (emulation != UC_ERR_OK) {
        fprintf(stderr, "Unicorn at %08x lr=%08x cpsr=%08x: %s\n",
                reg(uc, UC_ARM_REG_PC), reg(uc, UC_ARM_REG_LR),
                reg(uc, UC_ARM_REG_CPSR), uc_strerror(emulation));
        exit(EXIT_FAILURE);
    }

    int accepted = route == ROUTE_FAT || route == ROUTE_RAW;
    if (accepted) {
        uint8_t observed[STAGE_BYTES];
        uint8_t relocated[BOOTSTRAP_BYTES];
        uint8_t expected_relocated[BOOTSTRAP_BYTES];
        memcpy(expected_relocated, bootstrap, sizeof expected_relocated);
        uint32_t nop = 0xe1a00000u;
        for (unsigned i = 0; i < 3; i++)
            memcpy(expected_relocated + run.call_pc[i] - RELOC, &nop, 4);
        if (LOAD_SKIP == 0) {
            checked(uc_mem_read(uc, 0x40000000u, observed, STAGE_BYTES));
        } else {
            checked(uc_mem_read(uc, 0x40000000u, observed, LOAD_PREFIX));
            checked(uc_mem_read(uc, 0x40000000u + LOAD_PREFIX + LOAD_SKIP,
                                observed + LOAD_PREFIX, STAGE_BYTES - LOAD_PREFIX));
        }
        checked(uc_mem_read(uc, RELOC, relocated, sizeof relocated));
        if (!run.complete || !run.relocated || run.failed ||
            memcmp(observed, stage, sizeof observed) != 0 ||
            memcmp(relocated, expected_relocated, sizeof relocated) != 0 ||
            run.led_control_writes != 1 || run.led_setup_writes != 1 ||
            run.led_data_writes != 2 || reg(uc, UC_ARM_REG_SP) != ROM_STACK ||
            reg(uc, UC_ARM_REG_R0) != 0 || (reg(uc, UC_ARM_REG_CPSR) & 0xf8000000u) != 0 ||
            run.fat_opens != (route == ROUTE_FAT) || run.fat_reads != (route == ROUTE_FAT ? FAT_READS : 0u) ||
            run.raw_reads != (route == ROUTE_RAW ? RAW_READS : 0u)) run.failed = 1;
        for (unsigned i = 0; i < sizeof preserved / sizeof preserved[0]; i++)
            if (reg(uc, preserved[i]) != 0x24680000u + i) run.failed = 1;
    } else if (run.complete || run.failed || run.led_setup_writes != 1 || run.led_data_writes != 4) {
        run.failed = 1;
    }
    if (run.failed) {
        fprintf(stderr, "staged bootstrap failed route=%d fat=%u raw=%u led=%u complete=%d\n",
                route, run.fat_reads, run.raw_reads, run.led_data_writes, run.complete);
        exit(EXIT_FAILURE);
    }
    printf("PASS staged route=%d fat=%u raw=%u led-writes=%u\n",
           route, run.fat_reads, run.raw_reads, run.led_data_writes);
    checked(uc_close(uc));
}

int main(int argc, char **argv) {
    if (argc != 2) return EXIT_FAILURE;
    FILE *file = fopen(argv[1], "rb");
    if (!file) { perror(argv[1]); return EXIT_FAILURE; }
    uint8_t bootstrap[BOOTSTRAP_BYTES];
    size_t count = fread(bootstrap, 1, sizeof bootstrap, file);
    int extra = fgetc(file);
    int bad = ferror(file);
    if (fclose(file) != 0 || bad || count != sizeof bootstrap || extra != EOF)
        return EXIT_FAILURE;
    uint8_t *stage = malloc(STAGE_BYTES);
    if (!stage) return EXIT_FAILURE;
    for (unsigned i = 0; i < STAGE_BYTES; i++) stage[i] = (uint8_t)(i * 37u + 11u);
    exercise(bootstrap, stage, ROUTE_FAT);
    exercise(bootstrap, stage, ROUTE_RAW);
    exercise(bootstrap, stage, ROUTE_REJECT);
    exercise(bootstrap, stage, ROUTE_FAT_FAILURE);
    exercise(bootstrap, stage, ROUTE_FAT_SHORT);
    free(stage);
    return EXIT_SUCCESS;
}
