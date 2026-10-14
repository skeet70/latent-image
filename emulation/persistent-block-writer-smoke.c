#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unicorn/arm.h>
#include <unicorn/unicorn.h>

#define ID4_BASE 0x0061c400u
#define ID4_SIZE 0x00282800u
#define WRITER 0x006a9268u
#define CHECK 0x006faf00u
#define PROGRAM 0x006faf5cu
#define RELEASE 0x006faf40u
#define WINDOW 0x0004263cu
#define READY 0x00828574u
#define REMAP 0x00828580u
#define SOURCE 0x00040000u
#define SOURCE_SIZE 0x00020000u
#define STACK_BASE 0x3b000000u
#define STACK_TOP (STACK_BASE + 0x1000u)
#define EXIT 0x3c000000u

typedef struct {
    const uint8_t *source;
    uint32_t check_status;
    uint32_t program_status;
    unsigned checks;
    unsigned programs;
    unsigned releases;
    unsigned windows;
    int bad;
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

static uint32_t word_at(uc_engine *uc, uint32_t address) {
    uint32_t value;
    checked(uc_mem_read(uc, address, &value, 4));
    return value;
}

static void return_with(uc_engine *uc, uint32_t value) {
    set_reg(uc, UC_ARM_REG_R0, value);
    set_reg(uc, UC_ARM_REG_PC, reg(uc, UC_ARM_REG_LR));
}

static void code(uc_engine *uc, uint64_t pc, uint32_t size, void *opaque) {
    (void)size;
    Run *run = opaque;
    if (pc == CHECK) {
        run->checks++;
        return_with(uc, run->check_status);
    } else if (pc == PROGRAM) {
        run->programs++;
        uint32_t descriptor = reg(uc, UC_ARM_REG_R0);
        if (word_at(uc, descriptor) != 14 || word_at(uc, descriptor + 4) != 1 ||
            word_at(uc, descriptor + 12) != (SOURCE | 0x40000000u) ||
            word_at(uc, descriptor + 16) != 1) run->bad = 1;
        checked(uc_mem_write(uc, 0x38000000u, run->source, SOURCE_SIZE));
        return_with(uc, run->program_status);
    } else if (pc == RELEASE) {
        run->releases++;
        return_with(uc, 0);
    } else if (pc == WINDOW) {
        run->windows++;
        if (reg(uc, UC_ARM_REG_R0) != SOURCE || reg(uc, UC_ARM_REG_R1) != SOURCE_SIZE)
            run->bad = 1;
        return_with(uc, 0);
    } else if (pc == EXIT) {
        checked(uc_emu_stop(uc));
    }
}

static void exercise(const uint8_t *id4, const uint8_t *source, uint32_t ready,
                     uint32_t logical, uint32_t count, uint32_t check_status,
                     uint32_t program_status, uint32_t wanted_status,
                     unsigned wanted_checks, unsigned wanted_programs,
                     unsigned wanted_releases, unsigned wanted_windows) {
    Run run = {.source = source, .check_status = check_status, .program_status = program_status};
    uc_engine *uc;
    uc_hook hook;
    checked(uc_open(UC_ARCH_ARM, UC_MODE_ARM, &uc));
    checked(uc_ctl_set_cpu_model(uc, UC_CPU_ARM_CORTEX_R5F));
    checked(uc_mem_map(uc, 0, 0x100000, UC_PROT_ALL));
    checked(uc_mem_map(uc, 0x00600000u, 0x00300000, UC_PROT_ALL));
    checked(uc_mem_map(uc, 0x38000000u, SOURCE_SIZE, UC_PROT_ALL));
    checked(uc_mem_map(uc, STACK_BASE, 0x1000, UC_PROT_ALL));
    checked(uc_mem_map(uc, EXIT, 0x1000, UC_PROT_ALL));
    checked(uc_mem_write(uc, ID4_BASE, id4, ID4_SIZE));
    checked(uc_mem_write(uc, SOURCE, source, SOURCE_SIZE));
    checked(uc_mem_write(uc, READY, &ready, 4));
    uint16_t physical = 14;
    checked(uc_mem_write(uc, REMAP + 14 * 2, &physical, 2));
    set_reg(uc, UC_ARM_REG_R0, logical);
    set_reg(uc, UC_ARM_REG_R1, count);
    set_reg(uc, UC_ARM_REG_R2, SOURCE);
    set_reg(uc, UC_ARM_REG_SP, STACK_TOP);
    set_reg(uc, UC_ARM_REG_LR, EXIT);
    checked(uc_hook_add(uc, &hook, UC_HOOK_CODE, (void *)code, &run, 1, 0));
    checked(uc_emu_start(uc, WRITER, EXIT + 4, 0, 100000));
    uint32_t status = reg(uc, UC_ARM_REG_R0);
    if (status != wanted_status || run.checks != wanted_checks ||
        run.programs != wanted_programs || run.releases != wanted_releases ||
        run.windows != wanted_windows || run.bad) {
        fprintf(stderr, "writer ready=%u logical=%u count=%u status=%08x/%08x calls=%u/%u/%u/%u bad=%d pc=%08x\n",
                ready, logical, count, status, wanted_status, run.checks,
                run.programs, run.releases, run.windows, run.bad, reg(uc, UC_ARM_REG_PC));
        exit(EXIT_FAILURE);
    }
    if (wanted_programs != 0) {
        uint8_t *medium = malloc(SOURCE_SIZE);
        if (!medium) exit(EXIT_FAILURE);
        checked(uc_mem_read(uc, 0x38000000u, medium, SOURCE_SIZE));
        if (memcmp(medium, source, SOURCE_SIZE) != 0) exit(EXIT_FAILURE);
        free(medium);
    }
    checked(uc_close(uc));
}

static uint8_t *read_exact(const char *path, size_t wanted) {
    FILE *file = fopen(path, "rb");
    if (!file) { perror(path); exit(EXIT_FAILURE); }
    uint8_t *bytes = malloc(wanted);
    if (!bytes) exit(EXIT_FAILURE);
    size_t got = fread(bytes, 1, wanted, file);
    int extra = fgetc(file);
    if (fclose(file) != 0 || got != wanted || extra != EOF) exit(EXIT_FAILURE);
    return bytes;
}

int main(int argc, char **argv) {
    if (argc != 3) return EXIT_FAILURE;
    uint8_t *id4 = read_exact(argv[1], ID4_SIZE);
    uint8_t *source = read_exact(argv[2], SOURCE_SIZE);
    exercise(id4, source, 1, 14, 1, 0, 0, 0, 1, 1, 1, 1);
    exercise(id4, source, 0, 14, 1, 0, 0, 0xffffff01u, 0, 0, 0, 0);
    exercise(id4, source, 1, 0x400, 1, 0, 0, 0xffffffe2u, 0, 0, 0, 0);
    exercise(id4, source, 1, 14, 1, 7, 0, 7, 1, 0, 0, 0);
    exercise(id4, source, 1, 14, 1, 0, 9, 9, 1, 1, 1, 1);
    free(source);
    free(id4);
    puts("PASS exact 0x006a9268 one-block writer success and refusal paths");
    return EXIT_SUCCESS;
}
