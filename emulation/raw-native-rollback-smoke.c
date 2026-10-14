#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unicorn/arm.h>
#include <unicorn/unicorn.h>

#define TAP 0x0071feb4u
#define STOCK 0xebffffdcu
#define RECORDER 0xeb34a9c9u
#define STACK 0x0000f000u
#define EXIT 0x00008000u

enum Outcome { RUNNING, CLOSED, REFUSED };
typedef struct {
    enum Outcome outcome;
    int unexpected_write;
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

static void code(uc_engine *uc, uint64_t pc, uint32_t size, void *opaque) {
    (void)size;
    Run *run = opaque;
    uint32_t word;
    checked(uc_mem_read(uc, pc, &word, sizeof word));
    if (word == 0xee070f3au || word == 0xee070f15u) {
        // Local cache visibility is a coherent-memory fixture.
        uint32_t next = (uint32_t)pc + 4;
        checked(uc_reg_write(uc, UC_ARM_REG_PC, &next));
    } else if (word == 0xe320f003u) {
        run->outcome = REFUSED;
        checked(uc_emu_stop(uc));
    } else if (pc == 0x2044 && reg(uc, UC_ARM_REG_R0) != 0x12340000u) {
        fprintf(stderr, "Close ABI changed R0\n");
        exit(EXIT_FAILURE);
    } else if (pc == EXIT) {
        run->outcome = CLOSED;
        checked(uc_emu_stop(uc));
    }
}

static void writes(uc_engine *uc, uc_mem_type type, uint64_t address,
                   int size, int64_t value, void *opaque) {
    (void)type;
    (void)value;
    Run *run = opaque;
    if (!((address >= STACK - 24 && address + (unsigned)size <= STACK) ||
          (address == TAP && size == 4))) {
        run->unexpected_write = 1;
        checked(uc_emu_stop(uc));
    }
}

static void exercise(const uint8_t *stub, uint32_t initial_tap, unsigned discriminator) {
    uc_engine *uc;
    uc_hook code_hook, write_hook;
    Run run = {.outcome = RUNNING};
    checked(uc_open(UC_ARCH_ARM, UC_MODE_ARM, &uc));
    checked(uc_ctl_set_cpu_model(uc, UC_CPU_ARM_CORTEX_R5F));
    checked(uc_mem_map(uc, 0, 0x10000, UC_PROT_ALL));
    checked(uc_mem_write(uc, 0x6000, stub, 0xa8));
    uint32_t classifier[] = {0xe3a00000u | discriminator, 0xe12fff1eu};
    uint32_t close[] = {0xe3a00007u, 0xe12fff1eu};
    checked(uc_mem_write(uc, 0x1aa0, classifier, sizeof classifier));
    checked(uc_mem_write(uc, 0x2044, close, sizeof close));
    uint8_t expected[4096], observed[4096];
    memset(expected, 0xa5, sizeof expected);
    memcpy(expected + (TAP & 4095), &initial_tap, sizeof initial_tap);
    checked(uc_mem_map(uc, TAP & ~4095u, 4096, UC_PROT_READ | UC_PROT_WRITE));
    checked(uc_mem_write(uc, TAP & ~4095u, expected, sizeof expected));
    uint32_t flags = 0x13;
    checked(uc_reg_write(uc, UC_ARM_REG_CPSR, &flags));
    const int ids[] = {UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3,
        UC_ARM_REG_R4, UC_ARM_REG_R5, UC_ARM_REG_R6, UC_ARM_REG_R7, UC_ARM_REG_R8,
        UC_ARM_REG_R9, UC_ARM_REG_R10, UC_ARM_REG_R11, UC_ARM_REG_R12};
    for (unsigned i = 0; i < sizeof ids / sizeof ids[0]; i++) {
        uint32_t value = 0x12340000u + i;
        checked(uc_reg_write(uc, ids[i], &value));
    }
    uint32_t sp = STACK, lr = EXIT;
    checked(uc_reg_write(uc, UC_ARM_REG_SP, &sp));
    checked(uc_reg_write(uc, UC_ARM_REG_LR, &lr));
    checked(uc_hook_add(uc, &code_hook, UC_HOOK_CODE, (void *)code, &run, 1, 0));
    checked(uc_hook_add(uc, &write_hook, UC_HOOK_MEM_WRITE, (void *)writes, &run, 1, 0));
    checked(uc_emu_start(uc, 0x6040, EXIT + 4, 1000000, 256));
    int accepted = discriminator == 0 && (initial_tap == STOCK || initial_tap == RECORDER);
    if (accepted) memcpy(expected + (TAP & 4095), &(uint32_t){STOCK}, 4);
    checked(uc_mem_read(uc, TAP & ~4095u, observed, sizeof observed));
    if (run.unexpected_write || run.outcome != (accepted ? CLOSED : REFUSED) ||
        memcmp(expected, observed, sizeof expected) != 0) {
        fprintf(stderr, "Rollback failed: tap=%08x discriminator=%u outcome=%d\n",
                initial_tap, discriminator, run.outcome);
        exit(EXIT_FAILURE);
    }
    if (accepted) {
        for (unsigned i = 0; i < sizeof ids / sizeof ids[0]; i++) {
            if (reg(uc, ids[i]) != (i == 0 ? 7u : 0x12340000u + i)) {
                fprintf(stderr, "Close ABI changed register %u\n", i);
                exit(EXIT_FAILURE);
            }
        }
        if (reg(uc, UC_ARM_REG_SP) != STACK || reg(uc, UC_ARM_REG_LR) != EXIT)
            exit(EXIT_FAILURE);
    }
    printf("PASS tap=%08x discriminator=%u outcome=%s\n", initial_tap, discriminator,
           accepted ? "closed" : "refused");
    checked(uc_close(uc));
}

int main(int argc, char **argv) {
    if (argc != 2) return EXIT_FAILURE;
    FILE *file = fopen(argv[1], "rb");
    if (!file) { perror(argv[1]); return EXIT_FAILURE; }
    uint8_t stub[0xa8];
    size_t count = fread(stub, 1, sizeof stub, file);
    int extra = fgetc(file);
    int bad = ferror(file);
    if (fclose(file) != 0 || bad || count != sizeof stub || extra != EOF)
        return EXIT_FAILURE;
    exercise(stub, RECORDER, 0);
    exercise(stub, STOCK, 0);
    exercise(stub, 0xdeadbeefu, 0);
    exercise(stub, RECORDER, 1);
    exercise(stub, STOCK, 1);
    return EXIT_SUCCESS;
}
